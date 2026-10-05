//! Requires a locally built native DLL and legally extracted SM64 assets.
use sm64_native::{NativeClient, NativePlayerProxy};

fn aim_at(player: &mut NativePlayerProxy, target: [f32; 3], target_height: f32) {
    let dx = target[0] - player.pos[0];
    let dy = target[1] + target_height - player.pos[1] - 150.0;
    let dz = target[2] - player.pos[2];
    let yaw = dx.atan2(dz);
    let pitch = dy.atan2((dx * dx + dz * dz).sqrt());
    player.yaw = (yaw / std::f32::consts::TAU * 65536.0) as i16;
    player.pitch = (pitch / std::f32::consts::TAU * 65536.0) as i16;
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .expect("usage: smoke <sm64-port root>");
    let mut client = NativeClient::launch(&root, "bob", 1, 1)?;
    let mut player = NativePlayerProxy {
        pos: [-6558.0, 1000.0, 6464.0],
        health: 100,
        ..Default::default()
    };
    let mut snapshot = client.step(player)?;
    for _ in 0..5 {
        snapshot = client.step(player)?;
    }
    let hud = snapshot
        .render_triangles
        .iter()
        .filter(|t| t.screen_space)
        .count();
    assert!(hud > 0, "native HUD must be captured");
    assert!(
        snapshot
            .render_triangles
            .iter()
            .any(|t| !t.screen_space && t.texture_id.is_some())
    );
    println!("native HUD: {hud} triangles; textured world present");
    let buddy = snapshot
        .objects
        .iter()
        .find(|o| o.model_id == 0xC3)
        .expect("Bob-omb Buddy");
    player.pos = [buddy.pos[0], buddy.pos[1], buddy.pos[2] + 100.0];
    player.yaw = i16::MIN;
    for i in 0..120 {
        player.attack_flags = if i % 30 == 0 { 2 } else { 0 };
        snapshot = client.step(player)?;
        if snapshot.dialog_id >= 0 {
            break;
        }
    }
    assert!(snapshot.dialog_id >= 0, "Use must open native NPC dialog");
    println!("dialog opened: {}", snapshot.dialog_id);
    for i in 0..240 {
        player.attack_flags = if i % 40 == 39 { 2 } else { 0 };
        snapshot = client.step(player)?;
        if snapshot.dialog_id < 0 {
            break;
        }
    }
    assert!(
        snapshot.dialog_id < 0,
        "Use must advance and close native dialog"
    );
    println!("dialog advanced and closed");
    player.attack_flags = 0;
    for _ in 0..60 {
        snapshot = client.step(player)?;
    }
    let goomba = snapshot
        .objects
        .iter()
        .find(|o| o.model_id == 0xC0)
        .expect("Bob-omb Battlefield Goomba");
    player.pos = goomba.pos;
    let initial = snapshot.mario_health;
    let mut minimum = initial;
    for _ in 0..90 {
        snapshot = client.step(player)?;
        minimum = minimum.min(snapshot.mario_health);
    }
    assert!(minimum < initial, "enemy contact must reduce native health");
    println!("Goomba contact: health {initial} -> {minimum}");

    drop(client);
    let mut client = NativeClient::launch(&root, "bob", 1, 1)?;
    let mut player = NativePlayerProxy {
        pos: [-6558.0, 1000.0, 6464.0],
        health: 100,
        ..Default::default()
    };
    let mut snapshot = client.step(player)?;
    for _ in 0..10 {
        snapshot = client.step(player)?;
    }

    let carry_object = snapshot
        .objects
        .iter()
        .find(|object| object.model_id == 0xBC)
        .expect("grabbable Bob-omb")
        .clone();
    player.pos = [
        carry_object.pos[0],
        carry_object.pos[1],
        carry_object.pos[2] - 70.0,
    ];
    aim_at(&mut player, carry_object.pos, 50.0);
    for tick in 0..90 {
        player.attack_flags = if tick % 8 == 0 { 2 } else { 0 };
        snapshot = client.step(player)?;
        if let Some(object) = snapshot
            .objects
            .iter()
            .find(|object| object.id == carry_object.id)
        {
            player.pos = [object.pos[0], object.pos[1], object.pos[2] - 70.0];
            aim_at(&mut player, object.pos, 50.0);
        }
        if snapshot.holding_object {
            break;
        }
    }
    assert!(
        snapshot.holding_object,
        "Use must pick up native grabbable objects"
    );
    println!("native object picked up");
    player.attack_flags = 2;
    snapshot = client.step(player)?;
    player.attack_flags = 0;
    for _ in 0..45 {
        snapshot = client.step(player)?;
    }
    assert!(
        !snapshot.holding_object,
        "a second Use must throw the held object"
    );
    println!("native object thrown");

    drop(client);
    let mut client = NativeClient::launch(&root, "bob", 1, 1)?;
    let mut player = NativePlayerProxy {
        pos: [-6558.0, 1000.0, 6464.0],
        health: 100,
        ..Default::default()
    };
    let mut snapshot = client.step(player)?;
    for _ in 0..10 {
        snapshot = client.step(player)?;
    }
    let target = snapshot
        .objects
        .iter()
        .filter(|object| object.model_id == 0x82)
        .min_by(|left, right| left.pos[2].abs().total_cmp(&right.pos[2].abs()))
        .expect("small breakable shooting box")
        .clone();
    player.pos = [target.pos[0], target.pos[1], target.pos[2] - 500.0];
    aim_at(&mut player, target.pos, 30.0);
    player.attack_flags = 1 | 4;
    snapshot = client.step(player)?;
    player.attack_flags = 0;
    for _ in 0..20 {
        snapshot = client.step(player)?;
    }
    assert!(
        snapshot.objects.iter().all(|object| object.id != target.id),
        "shooting must break the targeted native box"
    );
    println!("native box broken by shooting");

    drop(client);
    let parsed = sm64_assets::load_level_collision(&root, "castle_inside", 1)?;
    let painting = parsed
        .world
        .surfaces
        .iter()
        .find(|surface| surface.surface_type as i32 == sm64_core::SURFACE_PAINTING_WARP_D3)
        .expect("BOB painting warp floor");
    let x = (painting.vertex1[0] as f32 + painting.vertex2[0] as f32 + painting.vertex3[0] as f32)
        / 3.0;
    let z = (painting.vertex1[2] as f32 + painting.vertex2[2] as f32 + painting.vertex3[2] as f32)
        / 3.0;
    let y = painting.height_at(x, z).expect("painting floor height");
    let mut client = NativeClient::launch(&root, "castle_inside", 1, 1)?;
    let player = NativePlayerProxy {
        pos: [x, y + 10.0, z],
        vel: [0.0, 0.0, 1.0],
        health: 100,
        ..Default::default()
    };
    let mut destination = None;
    for _ in 0..180 {
        let snapshot = client.step(player)?;
        if snapshot.transition.is_some() {
            destination = snapshot.transition;
            break;
        }
    }
    let destination =
        destination.expect("painting must produce a native transition without crashing");
    assert_eq!(
        destination.level, "bob",
        "BOB painting must route to Bob-omb Battlefield"
    );
    assert_eq!(destination.area, 1);
    println!(
        "castle painting transition: {} area {} node {}",
        destination.level, destination.area, destination.node
    );

    drop(client);
    let mut client = NativeClient::launch(&root, "bob", 1, 2)?;
    let mut player = NativePlayerProxy {
        pos: [-6558.0, 1000.0, 6464.0],
        health: 100,
        ..Default::default()
    };
    let mut snapshot = client.step(player)?;
    for _ in 0..10 {
        snapshot = client.step(player)?;
    }
    let is_course_cannon = |object: &&sm64_native::NativeObject| {
        object.model_id == 0x80
            && (object.pos[0] + 5694.0).abs() < 300.0
            && (object.pos[2] - 5600.0).abs() < 300.0
    };
    if snapshot.objects.iter().all(|object| !is_course_cannon(&object)) {
        let buddy = snapshot
            .objects
            .iter()
            .filter(|object| object.model_id == 0xC3)
            .min_by(|left, right| {
                let left_distance =
                    (left.pos[0] + 5723.0).powi(2) + (left.pos[2] - 6017.0).powi(2);
                let right_distance =
                    (right.pos[0] + 5723.0).powi(2) + (right.pos[2] - 6017.0).powi(2);
                left_distance.total_cmp(&right_distance)
            })
            .expect("cannon-opening Bob-omb Buddy")
            .clone();
        player.pos = [buddy.pos[0], buddy.pos[1], buddy.pos[2] + 100.0];
        player.yaw = i16::MIN;
        for tick in 0..1200 {
            player.attack_flags = if tick == 0 || tick % 40 == 39 { 2 } else { 0 };
            snapshot = client.step(player)?;
        }
    }
    let cannon = snapshot
        .objects
        .iter()
        .find(is_course_cannon)
        .expect("Bob-omb Buddy must open the native cannon")
        .clone();
    player.attack_flags = 0;
    player.pos = cannon.pos;
    for _ in 0..180 {
        snapshot = client.step(player)?;
        if snapshot.mario_action == sm64_core::ACT_IN_CANNON {
            break;
        }
    }
    println!(
        "cannon entry probe: action=0x{:08X} dialog={} cannon={:?}",
        snapshot.mario_action, snapshot.dialog_id, cannon.pos
    );
    assert_eq!(
        snapshot.mario_action,
        sm64_core::ACT_IN_CANNON,
        "walking into an open cannon must enter it"
    );
    for tick in 0..240 {
        if snapshot.mario_action == sm64_core::ACT_SHOT_FROM_CANNON {
            break;
        }
        player.attack_flags = if tick % 20 == 19 { 4 } else { 0 };
        snapshot = client.step(player)?;
    }
    assert_eq!(
        snapshot.mario_action,
        sm64_core::ACT_SHOT_FROM_CANNON,
        "Fire must launch Mario from a native cannon"
    );
    println!("native cannon entered and fired");

    drop(client);
    let mut client = NativeClient::launch(&root, "totwc", 1, 1)?;
    let mut player = NativePlayerProxy {
        pos: [0.0, -1900.0, 10.0],
        health: 100,
        ..Default::default()
    };
    let mut snapshot = client.step(player)?;
    for _ in 0..10 {
        snapshot = client.step(player)?;
    }
    let switch = snapshot
        .objects
        .iter()
        .find(|object| object.model_id == 0x55)
        .expect("Wing Cap switch")
        .clone();
    player.pos = [switch.pos[0], switch.pos[1] + 176.5, switch.pos[2]];
    for tick in 0..360 {
        player.attack_flags = if tick % 40 == 39 { 2 } else { 0 };
        snapshot = client.step(player)?;
    }
    let pressed_switch = snapshot
        .objects
        .iter()
        .find(|object| object.id == switch.id)
        .expect("pressed Wing Cap switch remains loaded");
    assert!(
        pressed_switch.scale[1] <= 0.2,
        "standing on the native cap switch must press it"
    );

    let box_object = snapshot
        .objects
        .iter()
        .filter(|object| object.model_id == 0x89)
        .min_by(|left, right| {
            let left_distance = left.pos[0].powi(2) + (left.pos[2] + 600.0).powi(2);
            let right_distance = right.pos[0].powi(2) + (right.pos[2] + 600.0).powi(2);
            left_distance.total_cmp(&right_distance)
        })
        .expect("unlocked Wing Cap box")
        .clone();
    player.pos = [box_object.pos[0], box_object.pos[1], box_object.pos[2] - 500.0];
    aim_at(&mut player, box_object.pos, 30.0);
    player.attack_flags = 1 | 4;
    snapshot = client.step(player)?;
    player.attack_flags = 0;
    for _ in 0..45 {
        snapshot = client.step(player)?;
    }
    let cap = snapshot
        .objects
        .iter()
        .find(|object| object.model_id == 0x87)
        .expect("shooting the unlocked box must spawn a Wing Cap")
        .clone();
    player.pos = cap.pos;
    for _ in 0..90 {
        snapshot = client.step(player)?;
        if snapshot.mario_flags & sm64_core::MARIO_WING_CAP != 0 {
            break;
        }
    }
    assert_ne!(snapshot.mario_flags & sm64_core::MARIO_WING_CAP, 0);
    assert!(snapshot.cap_timer > 0, "Wing Cap must start its native timer");
    println!("native Wing Cap switch, box, pickup, and timer verified");
    Ok(())
}
