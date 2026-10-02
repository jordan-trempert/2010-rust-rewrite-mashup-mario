# SM64 Rust port — current debug test

The SM64 port currently loads collision and level metadata from a local
[n64decomp/sm64](https://github.com/n64decomp/sm64) checkout. Nintendo-owned
assets are not stored in this repository.

## Environment

On Windows cmd.exe:

```bat
set SM64_DECOMP_ROOT=C:\path\to\sm64
set SM64_ENABLED=1
set SM64_DEBUG_VIEW=1
set SM64_LEVEL=bob
set SM64_AREA=1
cargo run -p launcher
```

`SM64_DEBUG_VIEW` defaults to enabled while the decomp root is configured.
Set it to `0` to run the SM64 simulation without the temporary Bevy PBR
presentation.

## Debug controls

- W/A/S/D — SM64 analog stick
- Space — A
- B — B
- Z — Z trigger
- Enter — Start (captured in the input carrier; menu behavior is not ported yet)

The temporary renderer shows the imported SM64 collision triangles rather than
the original display-list geometry. Mario is represented by a red cuboid. The
camera follows Mario from behind.

## Runtime split

SM64 runs at its original 30 Hz simulation cadence in `sm64_sim`, independently
of IW4L's simulation step. Rendering follows the latest SM64 snapshot at the
outer Bevy frame rate.

## Implemented foundation

- MarioState/action constants/input carrier
- original trigonometry lookup data
- floor/ceiling/wall collision queries
- ground and air quarter stepping
- gravity/action setup
- slopes/sliding/moving sand/horizontal wind
- ledge-grab collision test
- collision-source and level-script import
- water/environment boxes
- water plunge and core swimming actions
- temporary Bevy debug collision/Mario presentation

The debug renderer is deliberately disposable. The target remains the original
SM64 display-list/material/model data translated into the mashup renderer.
