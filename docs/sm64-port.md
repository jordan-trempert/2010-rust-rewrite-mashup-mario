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


## COD mashup: full native SM64 gameplay

The `sm64cod:*` maps can run the original host-native SM64 object/behavior runtime
instead of the small Rust behavior subset. This is the path used for coins,
Goombas/Bob-ombs, cannons, moving platforms, stars, switches, warps, hazards,
and course-specific behavior code while IW4 remains responsible for the visible
player, first-person camera, weapons, HUD, and ordinary movement.

Keep `SM64_DECOMP_ROOT` pointed at the checkout used for extracted level/model
assets. For the native gameplay process, use a host-native
[sm64-port](https://github.com/sm64-port/sm64-port) checkout and point
`SM64_NATIVE_ROOT` at it. This can be a separate checkout:

```powershell
$env:SM64_DECOMP_ROOT = "D:\Projects\sm64"
$env:SM64_NATIVE_ROOT = "D:\Projects\sm64-port"
```

The native checkout must already contain the legally obtained/generated SM64
assets needed by sm64-port. From an MSYS2/MinGW-capable shell, build the
headless gameplay bridge once:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\sm64_native_bridge\build.ps1
```

That produces:

```text
<SM64_NATIVE_ROOT>\build\us_bridge\iw4l-sm64-bridge.exe
```

The launcher auto-detects that executable. You can override its path with
`SM64_NATIVE_BRIDGE`.

At runtime the bridge executes the decomp's original level script and object
processor at 30 Hz. Mario is retained only as an invisible gameplay proxy
because the original behaviors reference `gMarioState` and `gMarioObject`.
Each tick IW4 writes the COD player's position, velocity, facing, and interaction
input into that proxy. The resulting SM64 objects are sent back to Bevy for
presentation. SM64 object/cutscene actions that must own the player (for
example cannon/warp/grab states) can temporarily drive the COD player through
the external-motion handoff.

If the bridge is missing or fails to launch, the game logs a warning and falls
back to the incomplete Rust behavior port. That fallback is useful for asset
debugging, but it is not the full-gameplay path.
