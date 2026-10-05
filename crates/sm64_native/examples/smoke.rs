//! Requires a locally built native DLL and legally extracted SM64 assets.
use sm64_native::{NativeClient, NativePlayerProxy};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).expect("usage: smoke <sm64-port root>");
    let mut client = NativeClient::launch(root, "bob", 1, 1)?;
    let mut player = NativePlayerProxy {
        pos: [-6558.0, 1000.0, 6464.0], health: 100, ..Default::default()
    };
    let mut snapshot = client.step(player)?;
    for _ in 0..5 { snapshot = client.step(player)?; }
    let hud = snapshot.render_triangles.iter().filter(|t| t.screen_space).count();
    assert!(hud > 0, "native HUD must be captured");
    assert!(snapshot.render_triangles.iter().any(|t| !t.screen_space && t.texture_id.is_some()));
    println!("native HUD: {hud} triangles; textured world present");
    let buddy = snapshot.objects.iter().find(|o| o.model_id == 0xC3).expect("Bob-omb Buddy");
    player.pos = [buddy.pos[0], buddy.pos[1], buddy.pos[2] + 100.0];
    player.yaw = i16::MIN;
    for i in 0..120 {
        player.attack_flags = if i % 30 == 0 { 2 } else { 0 };
        snapshot = client.step(player)?;
        if snapshot.dialog_id >= 0 { break; }
    }
    assert!(snapshot.dialog_id >= 0, "Use must open native NPC dialog");
    println!("dialog opened: {}", snapshot.dialog_id);
    for i in 0..240 {
        player.attack_flags = if i % 40 == 39 { 2 } else { 0 };
        snapshot = client.step(player)?;
        if snapshot.dialog_id < 0 { break; }
    }
    assert!(snapshot.dialog_id < 0, "Use must advance and close native dialog");
    println!("dialog advanced and closed");
    player.attack_flags = 0;
    for _ in 0..60 { snapshot = client.step(player)?; }
    let goomba = snapshot.objects.iter().find(|o| o.model_id == 0xC0).expect("Bob-omb Battlefield Goomba");
    player.pos = goomba.pos;
    let initial = snapshot.mario_health;
    let mut minimum = initial;
    for _ in 0..90 {
        snapshot = client.step(player)?;
        minimum = minimum.min(snapshot.mario_health);
    }
    assert!(minimum < initial, "enemy contact must reduce native health");
    println!("Goomba contact: health {initial} -> {minimum}");
    Ok(())
}
