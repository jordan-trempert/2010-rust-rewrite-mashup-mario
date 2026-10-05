param(
    [string]$Sm64PortRoot = $env:SM64_NATIVE_ROOT,
    [int]$Jobs = [Math]::Max(1, [Environment]::ProcessorCount)
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Sm64PortRoot)) {
    $Sm64PortRoot = $env:SM64_DECOMP_ROOT
}
if ([string]::IsNullOrWhiteSpace($Sm64PortRoot)) {
    throw "Set SM64_NATIVE_ROOT (preferred) or SM64_DECOMP_ROOT to a host-native sm64-port checkout."
}

$root = (Resolve-Path $Sm64PortRoot).Path
$pcMain = Join-Path $root "src\pc\pc_main.c"
if (-not (Test-Path $pcMain)) {
    throw @"
$root is not a host-native sm64-port checkout.
Full SM64 gameplay needs the PC-port form of the decomp so the original C
object/behavior runtime can execute on Windows. Point SM64_NATIVE_ROOT at
https://github.com/sm64-port/sm64-port (with your legally obtained assets).
"@
}

if (-not (Get-Command make -ErrorAction SilentlyContinue)) {
    throw "make is not on PATH. Run this from an MSYS2/MinGW environment that can build sm64-port."
}

$buildRel = "build/us_bridge"
$build = Join-Path $root "build\us_bridge"
$bridgeSource = Join-Path $PSScriptRoot "bridge.c"
$levelScriptSource = Join-Path $root "src\engine\level_script.c"
$levelScriptBackup = Join-Path $root "level_script.iw4l-backup.txt"
$levelUpdateSource = Join-Path $root "src\game\level_update.c"
$levelUpdateBackup = Join-Path $root "level_update.iw4l-backup.txt"
$objectProcessorSource = Join-Path $root "src\game\object_list_processor.c"
$objectProcessorBackup = Join-Path $root "object_list_processor.iw4l-backup.txt"
$armipsSource = Join-Path $root "tools\armips.cpp"
if (Test-Path $armipsSource) {
    $armipsText = Get-Content $armipsSource -Raw
    if ($armipsText -notmatch '#include\s+<cstdint>') {
        $needle = '#include <clocale>'
        if ($armipsText.Contains($needle)) {
            $armipsText = $armipsText.Replace(
                $needle,
                $needle + [Environment]::NewLine + "#include <cstdint>"
            )
            Set-Content -Path $armipsSource -Value $armipsText -Encoding UTF8
            Write-Host "Patched tools/armips.cpp for modern GCC (<cstdint>)."
        }
    }
}
$backup = Join-Path $root "pc_main.iw4l-backup.txt"
$builtModule = Join-Path $build "iw4l-sm64-native.dll"
$output = $builtModule
$legacyBridgeExe = Join-Path $build "iw4l-sm64-bridge.exe"
$staleBackupSource = Join-Path $root "src\pc\pc_main.iw4l-backup.c"
$staleBackupObject = Join-Path $build "src\pc\pc_main.iw4l-backup.o"
$pcMainObject = Join-Path $build "src\pc\pc_main.o"

# Older versions of this script created the backup inside src/pc with a .c
# extension. sm64-port recursively globs src/pc/*.c, so that backup was
# compiled and linked alongside the bridge, causing duplicate WinMain and
# global symbol definitions. Remove both stale artifacts before this build.
Remove-Item $staleBackupSource -Force -ErrorAction SilentlyContinue
Remove-Item $staleBackupObject -Force -ErrorAction SilentlyContinue
Remove-Item $pcMainObject -Force -ErrorAction SilentlyContinue
Remove-Item $legacyBridgeExe -Force -ErrorAction SilentlyContinue

Write-Host "Building embedded native SM64 gameplay module from $root"
Copy-Item $pcMain $backup -Force
Copy-Item $bridgeSource $pcMain -Force

# The native helper is headless: Bevy renders the SM64 geometry itself. The
# vanilla level-script executor always runs the full SM64 render pipeline after
# gameplay, which is unnecessary here and can execute graphics-only callbacks
# against state the bridge intentionally does not maintain. Patch only this
# temporary build checkout and restore the user's sm64-port source in finally.
Copy-Item $levelScriptSource $levelScriptBackup -Force
$levelScriptText = Get-Content $levelScriptSource -Raw
$renderBlock = @"
    profiler_log_thread5_time(LEVEL_SCRIPT_EXECUTE);
    init_rcp();
    render_game();
    end_master_display_list();
    alloc_display_list(0);
"@
$headlessBlock = @"
    profiler_log_thread5_time(LEVEL_SCRIPT_EXECUTE);
    /* IW4L headless gameplay bridge: host renderer consumes object state. */
"@
if (-not $levelScriptText.Contains($renderBlock)) {
    throw "Could not locate level_script_execute render tail in $levelScriptSource"
}
$levelScriptText = $levelScriptText.Replace($renderBlock, $headlessBlock)
Set-Content -Path $levelScriptSource -Value $levelScriptText -Encoding UTF8

# COD owns level/session transitions. If native SM64 completes a death/star exit
# or another level-changing warp, vanilla lvl_init_or_update would let the
# course script fall through CLEAR_LEVEL/EXIT and unwind the main-pool stack.
# The bridge intentionally stays inside the selected course, so consume that
# transition result and resume normal play instead.
Copy-Item $levelUpdateSource $levelUpdateBackup -Force
$levelUpdateText = Get-Content $levelUpdateSource -Raw
$updateNeedle = @"
        case 1:
            result = update_level();
            break;
"@
$updateReplacement = @"
        case 1:
            result = update_level();
            if (result != 0) {
                /*
                 * IW4L owns map/session transitions. Vanilla update_level()
                 * returns the destination level after a delayed cross-level
                 * warp, but leaving sWarpDest populated while suppressing that
                 * return makes the next play_mode_normal() call treat the stale
                 * destination as an in-area warp. That reaches
                 * init_mario_after_warp() with a node that does not exist in
                 * the current area.
                 */
                sWarpDest.type = WARP_TYPE_NOT_WARPING;
                sDelayedWarpOp = WARP_OP_NONE;
                sTransitionTimer = 0;
                sTransitionUpdate = NULL;
                set_play_mode(PLAY_MODE_NORMAL);
                result = 0;
            }
            break;
"@
if (-not $levelUpdateText.Contains($updateNeedle)) {
    throw "Could not locate lvl_init_or_update update case in $levelUpdateSource"
}
$levelUpdateText = $levelUpdateText.Replace($updateNeedle, $updateReplacement)
# Defensive guard: a malformed/unsupported native warp must never crash the
# helper. This is especially important while COD is authoritative for map
# transitions and the selected SM64 course remains loaded.
$warpNeedle = @"
void init_mario_after_warp(void) {
    struct ObjectWarpNode *spawnNode = area_get_warp_node(sWarpDest.nodeId);
    u32 marioSpawnType = get_mario_spawn_type(spawnNode->object);
"@
$warpReplacement = @"
void init_mario_after_warp(void) {
    struct ObjectWarpNode *spawnNode = area_get_warp_node(sWarpDest.nodeId);
    if (spawnNode == NULL || spawnNode->object == NULL) {
        sWarpDest.type = WARP_TYPE_NOT_WARPING;
        sDelayedWarpOp = WARP_OP_NONE;
        sTransitionTimer = 0;
        sTransitionUpdate = NULL;
        set_play_mode(PLAY_MODE_NORMAL);
        return;
    }
    u32 marioSpawnType = get_mario_spawn_type(spawnNode->object);
"@
if (-not $levelUpdateText.Contains($warpNeedle)) {
    throw "Could not locate init_mario_after_warp in $levelUpdateSource"
}
$levelUpdateText = $levelUpdateText.Replace($warpNeedle, $warpReplacement)
Set-Content -Path $levelUpdateSource -Value $levelUpdateText -Encoding UTF8

# Mario remains allocated because the original SM64 behaviors, interaction
# system, warps, cannons and cutscenes all reference gMarioState/gMarioObject.
# During ordinary gameplay, however, COD is the player controller. Replace the
# native player behavior with a compatibility shim that only mirrors the
# externally supplied MarioState into gMarioObject. Native action execution is
# temporarily re-enabled by bridge.c for cannons/warps/cutscenes/etc.
Copy-Item $objectProcessorSource $objectProcessorBackup -Force
$objectProcessorText = Get-Content $objectProcessorSource -Raw
$marioUpdateNeedle = @"
void bhv_mario_update(void) {
    u32 particleFlags = 0;
    s32 i;

    particleFlags = execute_mario_action(gCurrentObject);
    gCurrentObject->oMarioParticleFlags = particleFlags;

    // Mario code updates MarioState's versions of position etc, so we need
    // to sync it with the Mario object
    copy_mario_state_to_object();

    i = 0;
    while (sParticleTypes[i].particleFlag != 0) {
        if (particleFlags & sParticleTypes[i].particleFlag) {
            spawn_particle(sParticleTypes[i].activeParticleFlag, sParticleTypes[i].model,
                           sParticleTypes[i].behavior);
        }

        i++;
    }
}
"@
$marioUpdateReplacement = @"
extern int gIw4lRunMarioAction;

void bhv_mario_update(void) {
    u32 particleFlags = 0;
    s32 i;

    if (gIw4lRunMarioAction) {
        particleFlags = execute_mario_action(gCurrentObject);
        gCurrentObject->oMarioParticleFlags = particleFlags;
    } else {
        /*
         * IW4L embedded mode: COD owns ordinary locomotion, but the original
         * Mario interaction pipeline must still run. This preserves native
         * coins/stars/cannons/NPCs/warps/damage without executing Mario's
         * stationary/moving/airborne movement actions.
         */
        mario_reset_bodystate(gMarioState);
        update_mario_inputs(gMarioState);
        mario_handle_special_floors(gMarioState);
        mario_process_interactions(gMarioState);
        update_mario_health(gMarioState);
        mario_update_hitbox_and_cap_model(gMarioState);
        gCurrentObject->oMarioParticleFlags = 0;
    }

    copy_mario_state_to_object();

    if (gIw4lRunMarioAction) {
        i = 0;
        while (sParticleTypes[i].particleFlag != 0) {
            if (particleFlags & sParticleTypes[i].particleFlag) {
                spawn_particle(sParticleTypes[i].activeParticleFlag,
                               sParticleTypes[i].model,
                               sParticleTypes[i].behavior);
            }
            i++;
        }
    }
}
"@
if (-not $objectProcessorText.Contains($marioUpdateNeedle)) {
    throw "Could not locate bhv_mario_update in $objectProcessorSource"
}
$objectProcessorText = $objectProcessorText.Replace($marioUpdateNeedle, $marioUpdateReplacement)
Set-Content -Path $objectProcessorSource -Value $objectProcessorText -Encoding UTF8

try {
    Push-Location $root
    try {
        # The normal sm64-port build uses all-except-recomp. Do not build
        # ido5.3_recomp here: that optional ROM-matching tool requires Capstone
        # and is unrelated to the host-native gameplay bridge.
        & make -C tools all-except-recomp -j1
        if ($LASTEXITCODE -ne 0) {
            throw "sm64-port host tools build failed with exit code $LASTEXITCODE"
        }

        $moduleRel = "$buildRel/iw4l-sm64-native.dll"
        $makeArgs = @(
            "BUILD_DIR=$buildRel",
            "EXE=$moduleRel",
            "ENABLE_DX11=0",
            "ENABLE_DX12=0",
            "ENABLE_OPENGL=0",
            "GFX_CFLAGS=-DENABLE_GFX_DUMMY -DWIDESCREEN -DIW4L_SM64_EMBEDDED",
            "LDFLAGS=-lm -lxinput9_1_0 -lole32 -lgdi32 -mwindows -shared",
            "OPT_FLAGS=-O2 -g1",
            "-j$Jobs"
        )
        & make @makeArgs
        if ($LASTEXITCODE -ne 0) {
            throw "sm64-port embedded module build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path $builtModule)) {
        throw "Expected embedded SM64 module was not produced at $builtModule"
    }

    # Generate a self-contained sorted symbol table while MinGW tools are
    # definitely available. The Rust launcher can use this file later even
    # when addr2line/nm are not on the runtime PATH.
    $symbolMap = Join-Path $build "iw4l-sm64-native.sym"
    $nm = Get-Command "x86_64-w64-mingw32-nm" -ErrorAction SilentlyContinue
    if (-not $nm) {
        $nm = Get-Command "nm" -ErrorAction SilentlyContinue
    }
    if ($nm) {
        & $nm.Source -n $builtModule | Set-Content -Path $symbolMap -Encoding ASCII
        Write-Host "Embedded native symbol map: $symbolMap"
    }
    else {
        Write-Warning "nm was not found; crash symbol map was not generated."
    }

    Write-Host ""
    Write-Host "Embedded SM64 gameplay module built:"
    Write-Host "  $output"
    Write-Host ""
    Write-Host "IW4L loads this DLL in-process when SM64_NATIVE_ROOT points at this checkout."
}
finally {
    if (Test-Path $backup) {
        Copy-Item $backup $pcMain -Force
        Remove-Item $backup -Force
    }
    if (Test-Path $levelScriptBackup) {
        Copy-Item $levelScriptBackup $levelScriptSource -Force
        Remove-Item $levelScriptBackup -Force
    }
    if (Test-Path $levelUpdateBackup) {
        Copy-Item $levelUpdateBackup $levelUpdateSource -Force
        Remove-Item $levelUpdateBackup -Force
    }
    if (Test-Path $objectProcessorBackup) {
        Copy-Item $objectProcessorBackup $objectProcessorSource -Force
        Remove-Item $objectProcessorBackup -Force
    }
}
