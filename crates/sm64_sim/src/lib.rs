use sm64_core::{CollisionWorld, MarioState};

pub const SM64_TICK_HZ: u32 = 30;
pub const SM64_TICK_SECONDS: f64 = 1.0 / SM64_TICK_HZ as f64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sm64Input {
    pub stick_x: i8,
    pub stick_y: i8,
    pub button_a: bool,
    pub button_b: bool,
    pub button_z: bool,
    pub button_start: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sm64Snapshot {
    pub tick: u64,
    pub mario: MarioState,
}

#[derive(Clone, Debug, Default)]
pub struct Sm64World {
    pub tick: u64,
    pub mario: MarioState,
    pub collision: CollisionWorld,
}

impl Sm64World {
    pub fn new() -> Self { Self::default() }

    pub fn snapshot(&self) -> Sm64Snapshot {
        Sm64Snapshot { tick: self.tick, mario: self.mario.clone() }
    }

    pub fn step(&mut self, input: Sm64Input) -> Sm64Snapshot {
        self.tick = self.tick.wrapping_add(1);
        self.apply_controller(input);
        self.advance_common_timers();
        // Action-group execution is intentionally centralized here. Individual
        // action files are being translated from the decomp onto this boundary.
        self.mario.action_timer = self.mario.action_timer.wrapping_add(1);
        self.snapshot()
    }

    fn apply_controller(&mut self, input: Sm64Input) {
        use sm64_core::*;
        let m=&mut self.mario;
        m.input &= !(INPUT_NONZERO_ANALOG | INPUT_A_PRESSED | INPUT_A_DOWN | INPUT_B_PRESSED | INPUT_Z_DOWN | INPUT_Z_PRESSED);
        let sx=input.stick_x as f32;
        let sy=input.stick_y as f32;
        let mag=(sx*sx+sy*sy).sqrt().min(64.0);
        m.intended_mag=mag;
        if mag > 0.0 {
            m.input |= INPUT_NONZERO_ANALOG as u16;
            m.intended_yaw=sm64_core::math::atan2s(-sy,sx);
        }
        if input.button_a { m.input |= INPUT_A_DOWN as u16; }
        if input.button_b { m.input |= INPUT_B_PRESSED as u16; }
        if input.button_z { m.input |= INPUT_Z_DOWN as u16; }
    }

    fn advance_common_timers(&mut self) {
        let m=&mut self.mario;
        if m.invinc_timer > 0 { m.invinc_timer -= 1; }
        if m.wall_kick_timer > 0 { m.wall_kick_timer -= 1; }
        if m.double_jump_timer > 0 { m.double_jump_timer -= 1; }
        m.frames_since_a = m.frames_since_a.saturating_add(1);
        m.frames_since_b = m.frames_since_b.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simulation_ticks_at_a_stable_integer_boundary() {
        let mut w=Sm64World::new();
        assert_eq!(w.step(Sm64Input::default()).tick,1);
        assert_eq!(w.step(Sm64Input::default()).tick,2);
    }
}
