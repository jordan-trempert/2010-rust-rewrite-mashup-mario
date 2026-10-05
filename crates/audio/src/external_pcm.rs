use std::num::NonZero;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{
    AddAudioSource, AudioPlayer, Decodable, PlaybackSettings,
};
use bevy::prelude::*;
use frame::{ClientSet, MatchTornDown};
use rodio::mixer::{Mixer, MixerSource};
use rodio::{ChannelCount, Player, SampleRate, Source};

use crate::pcm::PcmAudio;

const OUTPUT_CHANNELS: ChannelCount = NonZero::new(2).unwrap();
const OUTPUT_RATE: SampleRate = NonZero::new(48_000).unwrap();

/// A short interleaved PCM block produced by an external gameplay runtime.
///
/// The SM64 bridge uses this instead of opening its own WASAPI device. Keeping
/// all sound behind Bevy/rodio means COD effects and SM64 music/SFX share one
/// process mixer and cannot steal/reset each other's Windows output device.
#[derive(Message, Clone, Debug)]
pub struct ExternalPcmChunk {
    pub stream_id: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<[i16]>,
}

#[derive(Asset, TypePath)]
struct ExternalPcmAudio {
    source: Arc<Mutex<Option<MixerSource>>>,
}

struct ExternalPcmDecoder {
    source: Option<MixerSource>,
    available: Arc<Mutex<Option<MixerSource>>>,
    right: bool,
}

impl Iterator for ExternalPcmDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.source.is_none() && !self.right {
            self.source = self.available.lock().ok()?.take();
        }
        let sample = self.source.as_mut().and_then(Iterator::next).unwrap_or(0.0);
        self.right = !self.right;
        Some(sample)
    }
}

impl Drop for ExternalPcmDecoder {
    fn drop(&mut self) {
        if let Some(mut source) = self.source.take()
            && let Ok(mut available) = self.available.lock()
        {
            if self.right {
                source.next();
            }
            *available = Some(source);
        }
    }
}

impl Source for ExternalPcmDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        OUTPUT_CHANNELS
    }

    fn sample_rate(&self) -> SampleRate {
        OUTPUT_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for ExternalPcmAudio {
    type Decoder = ExternalPcmDecoder;

    fn decoder(&self) -> Self::Decoder {
        ExternalPcmDecoder {
            source: self.source.lock().ok().and_then(|mut source| source.take()),
            available: Arc::clone(&self.source),
            right: false,
        }
    }
}

#[derive(Resource, Default)]
struct ExternalPcmState {
    stream_id: u64,
    entity: Option<Entity>,
    mixer: Option<Mixer>,
    player: Option<Player>,
}

fn reset_state(commands: &mut Commands, state: &mut ExternalPcmState) {
    if let Some(entity) = state.entity.take() {
        commands.entity(entity).try_despawn();
    }
    *state = ExternalPcmState::default();
}

fn play_external_pcm(
    mut chunks: MessageReader<ExternalPcmChunk>,
    mut commands: Commands,
    mut assets: ResMut<Assets<ExternalPcmAudio>>,
    mut state: ResMut<ExternalPcmState>,
) {
    for chunk in chunks.read() {
        if chunk.samples.is_empty()
            || chunk.channels == 0
            || chunk.channels > 2
            || !(8_000..=192_000).contains(&chunk.sample_rate)
        {
            continue;
        }

        if state.stream_id != 0 && state.stream_id != chunk.stream_id {
            reset_state(&mut commands, &mut state);
        }

        if state.player.is_none() {
            let (mixer, source) = rodio::mixer::mixer(OUTPUT_CHANNELS, OUTPUT_RATE);
            let output = assets.add(ExternalPcmAudio {
                source: Arc::new(Mutex::new(Some(source))),
            });
            let entity = commands
                .spawn((AudioPlayer(output), PlaybackSettings::ONCE))
                .id();
            let (player, player_output) = Player::new();
            mixer.add(player_output);
            state.stream_id = chunk.stream_id;
            state.entity = Some(entity);
            state.mixer = Some(mixer);
            state.player = Some(player);
            diag::info!(
                Audio,
                "audio: external PCM stream {} attached to host mixer",
                chunk.stream_id
            );
        }

        let samples: Arc<[f32]> = chunk
            .samples
            .iter()
            .map(|sample| f32::from(*sample) / 32768.0)
            .collect::<Vec<_>>()
            .into();
        let Some(pcm) = PcmAudio::from_prepared(samples, chunk.channels, chunk.sample_rate) else {
            continue;
        };
        if let Some(player) = state.player.as_ref() {
            player.append(pcm.decoder());
        }
    }
}

fn reset_external_pcm(
    mut torn: MessageReader<MatchTornDown>,
    mut commands: Commands,
    mut state: ResMut<ExternalPcmState>,
) {
    if torn.read().count() == 0 {
        return;
    }
    reset_state(&mut commands, &mut state);
}

pub(crate) fn register(app: &mut App) {
    app.add_audio_source::<ExternalPcmAudio>()
        .add_message::<ExternalPcmChunk>()
        .init_resource::<ExternalPcmState>()
        .add_systems(Update, play_external_pcm.in_set(ClientSet::Effects))
        .add_systems(Update, reset_external_pcm.in_set(ClientSet::Load));
}
