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

# sm64-port recipes execute under MSYS /bin/sh. PowerShell's
# Get-Command can see Windows App Execution Aliases such as python3.exe even
# when MSYS cannot resolve the bare command name. Pass an absolute executable
# path to make instead of trusting the PowerShell command name.
$pythonMake = $null
$pythonDisplay = $null

$pythonCandidates = @()
foreach ($name in @("python", "python3")) {
    $commands = @(Get-Command $name -All -ErrorAction SilentlyContinue)
    foreach ($command in $commands) {
        if ($command.CommandType -eq "Application" -and -not [string]::IsNullOrWhiteSpace($command.Source)) {
            $pythonCandidates += $command.Source
        }
    }
}

# Prefer a real installation over the Microsoft Store/WindowsApps alias.
$pythonCandidates = @(
    $pythonCandidates |
        Sort-Object -Unique |
        Sort-Object @{ Expression = { if ($_ -match "\\WindowsApps\\") { 1 } else { 0 } } }
)

foreach ($candidate in $pythonCandidates) {
    try {
        & $candidate --version *> $null
        if ($LASTEXITCODE -eq 0) {
            $normalized = $candidate -replace "\\", "/"
            $pythonMake = '"' + $normalized + '"'
            $pythonDisplay = $candidate
            break
        }
    }
    catch {
        # Try the next executable candidate.
    }
}

if ([string]::IsNullOrWhiteSpace($pythonMake)) {
    $py = Get-Command py -ErrorAction SilentlyContinue
    if ($py -and $py.CommandType -eq "Application") {
        try {
            & $py.Source -3 --version *> $null
            if ($LASTEXITCODE -eq 0) {
                $normalized = $py.Source -replace "\\", "/"
                $pythonMake = '"' + $normalized + '" -3'
                $pythonDisplay = "$($py.Source) -3"
            }
        }
        catch {
        }
    }
}

if ([string]::IsNullOrWhiteSpace($pythonMake)) {
    throw @"
Python 3 was not found as a usable Windows executable.
Install Python 3 from python.org (recommended), make sure "Add Python to PATH"
is enabled, then rerun this script.
"@
}

Write-Host "Using Python for sm64-port: $pythonDisplay"

$buildRel = "build/us_bridge"
$build = Join-Path $root "build\us_bridge"
$bridgeSource = Join-Path $PSScriptRoot "bridge.c"
$levelScriptSource = Join-Path $root "src\engine\level_script.c"
$levelScriptBackup = Join-Path $root "level_script.iw4l-backup.txt"
$levelUpdateSource = Join-Path $root "src\game\level_update.c"
$levelUpdateBackup = Join-Path $root "level_update.iw4l-backup.txt"
$gameInitSource = Join-Path $root "src\game\game_init.c"
$gameInitBackup = Join-Path $root "game_init.iw4l-backup.txt"
$areaSource = Join-Path $root "src\game\area.c"
$areaBackup = Join-Path $root "area.iw4l-backup.txt"
$objectProcessorSource = Join-Path $root "src\game\object_list_processor.c"
$objectProcessorBackup = Join-Path $root "object_list_processor.iw4l-backup.txt"
$renderGraphSource = Join-Path $root "src\game\rendering_graph_node.c"
$renderGraphBackup = Join-Path $root "rendering_graph_node.iw4l-backup.txt"
$gfxPcSource = Join-Path $root "src\pc\gfx\gfx_pc.c"
$gfxPcBackup = Join-Path $root "gfx_pc.iw4l-backup.txt"
$gfxDummySource = Join-Path $root "src\pc\gfx\gfx_dummy.c"
$gfxDummyBackup = Join-Path $root "gfx_dummy.iw4l-backup.txt"
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
Remove-Item (Join-Path $build "src\game\game_init.o") -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $build "src\game\area.o") -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $build "src\game\rendering_graph_node.o") -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $build "src\pc\gfx\gfx_pc.o") -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $build "src\pc\gfx\gfx_dummy.o") -Force -ErrorAction SilentlyContinue
Remove-Item $legacyBridgeExe -Force -ErrorAction SilentlyContinue

# Recover automatically if an older version of this script aborted before its
# cleanup block. This is especially important for the native-render migration:
# a failed patch must never leave the user's sm64-port checkout half-modified.
$recoveryPairs = @(
    @($backup, $pcMain),
    @($levelScriptBackup, $levelScriptSource),
    @($levelUpdateBackup, $levelUpdateSource),
    @($gameInitBackup, $gameInitSource),
    @($areaBackup, $areaSource),
    @($objectProcessorBackup, $objectProcessorSource),
    @($renderGraphBackup, $renderGraphSource),
    @($gfxPcBackup, $gfxPcSource),
    @($gfxDummyBackup, $gfxDummySource)
)
foreach ($pair in $recoveryPairs) {
    $backupPath = $pair[0]
    $sourcePath = $pair[1]
    if (Test-Path $backupPath) {
        Write-Warning "Recovering source left patched by an interrupted previous build: $sourcePath"
        Copy-Item $backupPath $sourcePath -Force
        Remove-Item $backupPath -Force
    }
}

try {
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
    /*
     * IW4L embedded mode keeps SM64's original scene-graph/display-list
     * renderer. The dummy PC backend is patched below into a capture backend
     * that exports world-space native triangles/textures to Rust instead of
     * opening its own window.
     */
    init_rcp();
    render_game();
    end_master_display_list();
    iw4l_sm64_capture_begin_frame();
    gfx_start_frame();
    gfx_run((Gfx *) gGfxSPTask->task.t.data_ptr);
    alloc_display_list(0);
"@
if (-not $levelScriptText.Contains($renderBlock)) {
    throw "Could not locate level_script_execute render tail in $levelScriptSource"
}
$levelScriptText = $levelScriptText.Replace($renderBlock, $headlessBlock)
$levelPrototypeNeedle = '#include "surface_load.h"'
$levelPrototypeReplacement = @"
#include "surface_load.h"

#ifdef IW4L_SM64_EMBEDDED
extern void iw4l_sm64_capture_begin_frame(void);
extern void gfx_start_frame(void);
extern void gfx_run(Gfx *commands);
#endif
"@
if (-not $levelScriptText.Contains($levelPrototypeNeedle)) {
    throw "Could not locate level_script include insertion point in $levelScriptSource"
}
$levelScriptText = $levelScriptText.Replace($levelPrototypeNeedle, $levelPrototypeReplacement)
Set-Content -Path $levelScriptSource -Value $levelScriptText -Encoding UTF8

# COD owns the outer level/session transition, but native SM64 still decides
# the destination. Capture the exact vanilla destination (level/area/node/arg)
# before preventing this embedded course script from unwinding its main-pool
# stack. Rust then relaunches the destination level and resumes at that warp
# node, preserving star exits, paintings, doors, pipes, and other level warps.
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
                 * Preserve the original SM64 destination before the embedded
                 * runtime suppresses its own outer level-script unwind. The
                 * host consumes this once and relaunches the destination
                 * course/castle at the exact warp node.
                 */
                if (sWarpDest.type == WARP_TYPE_CHANGE_LEVEL) {
                    extern void iw4l_sm64_capture_level_warp(
                        s32 level_num,
                        u32 area,
                        u32 node,
                        u32 arg
                    );
                    iw4l_sm64_capture_level_warp(
                        result,
                        sWarpDest.areaIdx,
                        sWarpDest.nodeId,
                        sWarpDest.arg
                    );
                }

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

# read_controller_inputs() runs after bridge.c supplies the COD input for this
# native tick. The stock port polls its dummy controller and overwrites those
# values before dialogs/actions can see them. Merge the bridge latch at the end
# of the native controller read so Use/Fire survive until gameplay/render code.
Copy-Item $gameInitSource $gameInitBackup -Force
$gameInitText = Get-Content $gameInitSource -Raw
$controllerNeedle = @"
    gPlayer3Controller->buttonPressed = gPlayer1Controller->buttonPressed;
    gPlayer3Controller->buttonDown = gPlayer1Controller->buttonDown;
}
"@
$controllerReplacement = @"
    gPlayer3Controller->buttonPressed = gPlayer1Controller->buttonPressed;
    gPlayer3Controller->buttonDown = gPlayer1Controller->buttonDown;
#ifdef IW4L_SM64_EMBEDDED
    {
        extern u16 gIw4lBridgeButtonDown;
        extern u16 gIw4lBridgeButtonPressed;

        gPlayer1Controller->buttonDown |= gIw4lBridgeButtonDown;
        gPlayer1Controller->buttonPressed |= gIw4lBridgeButtonPressed;
        gPlayer3Controller->buttonDown |= gIw4lBridgeButtonDown;
        gPlayer3Controller->buttonPressed |= gIw4lBridgeButtonPressed;
    }
#endif
}
"@
if (-not $gameInitText.Contains($controllerNeedle)) {
    throw "Could not locate read_controller_inputs tail in $gameInitSource"
}
$gameInitText = $gameInitText.Replace($controllerNeedle, $controllerReplacement)
Set-Content -Path $gameInitSource -Value $gameInitText -Encoding UTF8

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

/*
 * These helpers are defined in mario.c but are not exported by mario.h in the
 * upstream port. The embedded COD-player shim intentionally reuses the native
 * interaction/health/hitbox half of Mario's update without running locomotion.
 */
void mario_reset_bodystate(struct MarioState *m);
void update_mario_inputs(struct MarioState *m);
void update_mario_health(struct MarioState *m);
void mario_update_hitbox_and_cap_model(struct MarioState *m);

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

        /*
         * The interaction pass tests Mario's current hitbox/hurtbox against
         * enemy interact objects. Update it BEFORE mario_process_interactions;
         * doing this afterwards left COD's proxy with stale bounds and made
         * Goombas/Bob-ombs unable to damage the player reliably.
         */
        mario_update_hitbox_and_cap_model(gMarioState);
        mario_process_interactions(gMarioState);
        update_mario_health(gMarioState);
        gMarioState->marioObj->oInteractStatus = 0;
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

# Native rendering integration -------------------------------------------------
# Keep the original HUD and dialog state machine as well as the 3D scene.
Copy-Item $areaSource $areaBackup -Force
$areaText = Get-Content $areaSource -Raw
$renderGameNeedle = @"
void render_game(void) {
    if (gCurrentArea != NULL && !gWarpTransition.pauseRendering) {
"@
$renderGameReplacement = @"
void render_game(void) {
#ifdef IW4L_SM64_EMBEDDED
    if (gCurrentArea != NULL && !gWarpTransition.pauseRendering) {
        geo_process_root(gCurrentArea->unk04, D_8032CE74, D_8032CE78, gFBSetColor);
        gSPViewport(gDisplayListHead++, VIRTUAL_TO_PHYSICAL(&D_8032CF00));
        gDPSetScissor(gDisplayListHead++, G_SC_NON_INTERLACE, 0, 0, SCREEN_WIDTH, SCREEN_HEIGHT);
        render_hud();
        render_text_labels();
        do_cutscene_handler();
        print_displaying_credits_entry();
        gMenuOptSelectIndex = render_menus_and_dialogs();
        if (gMenuOptSelectIndex != MENU_OPT_NONE) {
            gSaveOptSelectIndex = gMenuOptSelectIndex;
        }
    }
    D_8032CE74 = NULL;
    D_8032CE78 = NULL;
    return;
#endif
    if (gCurrentArea != NULL && !gWarpTransition.pauseRendering) {
"@
if (-not $areaText.Contains($renderGameNeedle)) {
    throw "Could not locate render_game in $areaSource"
}
$areaText = $areaText.Replace($renderGameNeedle, $renderGameReplacement)
Set-Content -Path $areaSource -Value $areaText -Encoding UTF8

# Preserve SM64's own GeoLayout traversal, animation channels, switch cases,
# billboard decisions, scaling, lighting and Fast3D display-list execution.
# The temporary patches below only redirect the final native result into an
# in-memory capture backend suitable for the COD/Bevy camera.

Copy-Item $renderGraphSource $renderGraphBackup -Force
$renderGraphText = Get-Content $renderGraphSource -Raw

$orthoNeedle = @"
static void geo_process_ortho_projection(struct GraphNodeOrthoProjection *node) {
    if (node->node.children != NULL) {
"@
$orthoReplacement = @"
static void geo_process_ortho_projection(struct GraphNodeOrthoProjection *node) {
#ifdef IW4L_SM64_EMBEDDED
    /*
     * Keep the COD skybox/HUD path. Native capture exports the 3D perspective
     * scene only; SM64's orthographic skybox/HUD/dialog passes would otherwise
     * become camera-space geometry inside the COD world.
     */
    return;
#endif
    if (node->node.children != NULL) {
"@
if (-not $renderGraphText.Contains($orthoNeedle)) {
    throw "Could not locate geo_process_ortho_projection in $renderGraphSource"
}
$renderGraphText = $renderGraphText.Replace($orthoNeedle, $orthoReplacement)

$cameraNeedle = @"
    mtxf_lookat(cameraTransform, node->pos, node->focus, node->roll);
    mtxf_mul(gMatStack[gMatStackIndex + 1], cameraTransform, gMatStack[gMatStackIndex]);
"@
$cameraReplacement = @"
#ifdef IW4L_SM64_EMBEDDED
    {
        extern int gIw4lUseExternalCamera;
        extern float gIw4lRenderCameraPos[3];
        extern float gIw4lRenderCameraFocus[3];
        extern float gIw4lNativeCameraMatrix[4][4];
        if (gIw4lUseExternalCamera) {
            mtxf_lookat(cameraTransform, gIw4lRenderCameraPos, gIw4lRenderCameraFocus, 0);
        } else {
            mtxf_lookat(cameraTransform, node->pos, node->focus, node->roll);
        }
        mtxf_copy(gIw4lNativeCameraMatrix, cameraTransform);
    }
#else
    mtxf_lookat(cameraTransform, node->pos, node->focus, node->roll);
#endif
    mtxf_mul(gMatStack[gMatStackIndex + 1], cameraTransform, gMatStack[gMatStackIndex]);
"@
if (-not $renderGraphText.Contains($cameraNeedle)) {
    throw "Could not locate native camera transform in $renderGraphSource"
}
$renderGraphText = $renderGraphText.Replace($cameraNeedle, $cameraReplacement)

$objectNeedle = @"
static void geo_process_object(struct Object *node) {
    Mat4 mtxf;
"@
$objectReplacement = @"
static void geo_process_object(struct Object *node) {
#ifdef IW4L_SM64_EMBEDDED
    /*
     * COD owns the first-person player presentation. Preserve gMarioObject for
     * native gameplay/interaction state but never render Mario or his child
     * shadow in sm64cod:*.
     */
    extern struct Object *gMarioObject;
    if (node == gMarioObject) {
        return;
    }
#endif
    Mat4 mtxf;
"@
if (-not $renderGraphText.Contains($objectNeedle)) {
    throw "Could not locate geo_process_object in $renderGraphSource"
}
$renderGraphText = $renderGraphText.Replace($objectNeedle, $objectReplacement)

$objCullNeedle = @"
static s32 obj_is_in_view(struct GraphNodeObject *node, Mat4 matrix) {
    s16 cullingRadius;
"@
$objCullReplacement = @"
static s32 obj_is_in_view(struct GraphNodeObject *node, Mat4 matrix) {
#ifdef IW4L_SM64_EMBEDDED
    /*
     * COD/Bevy owns the final frustum. Do not discard native objects using
     * SM64's original 4:3/45-degree camera bounds before capture.
     */
    if (node->node.flags & GRAPH_RENDER_INVISIBLE) {
        return FALSE;
    }
    return TRUE;
#endif
    s16 cullingRadius;
"@
if (-not $renderGraphText.Contains($objCullNeedle)) {
    throw "Could not locate obj_is_in_view in $renderGraphSource"
}
$renderGraphText = $renderGraphText.Replace($objCullNeedle, $objCullReplacement)
Set-Content -Path $renderGraphSource -Value $renderGraphText -Encoding UTF8

Copy-Item $gfxPcSource $gfxPcBackup -Force
$gfxPcText = Get-Content $gfxPcSource -Raw

$loadedVertexNeedle = @"
struct LoadedVertex {
    float x, y, z, w;
    float u, v;
    struct RGBA color;
    uint8_t clip_rej;
};
"@
$loadedVertexReplacement = @"
struct LoadedVertex {
    float x, y, z, w;
#ifdef IW4L_SM64_EMBEDDED
    /* World-space result after native animation/model transforms, with the
       native camera view transform removed again for COD's Camera3d. */
    float world_x, world_y, world_z;
    bool screen_space;
#endif
    float u, v;
    struct RGBA color;
    uint8_t clip_rej;
};
"@
if (-not $gfxPcText.Contains($loadedVertexNeedle)) {
    throw "Could not locate LoadedVertex in $gfxPcSource"
}
$gfxPcText = $gfxPcText.Replace($loadedVertexNeedle, $loadedVertexReplacement)

$vertexTransformNeedle = @"
        float x = v->ob[0] * rsp.MP_matrix[0][0] + v->ob[1] * rsp.MP_matrix[1][0] + v->ob[2] * rsp.MP_matrix[2][0] + rsp.MP_matrix[3][0];
        float y = v->ob[0] * rsp.MP_matrix[0][1] + v->ob[1] * rsp.MP_matrix[1][1] + v->ob[2] * rsp.MP_matrix[2][1] + rsp.MP_matrix[3][1];
        float z = v->ob[0] * rsp.MP_matrix[0][2] + v->ob[1] * rsp.MP_matrix[1][2] + v->ob[2] * rsp.MP_matrix[2][2] + rsp.MP_matrix[3][2];
        float w = v->ob[0] * rsp.MP_matrix[0][3] + v->ob[1] * rsp.MP_matrix[1][3] + v->ob[2] * rsp.MP_matrix[2][3] + rsp.MP_matrix[3][3];
        
        x = gfx_adjust_x_for_aspect_ratio(x);
"@
$vertexTransformReplacement = @"
        float x = v->ob[0] * rsp.MP_matrix[0][0] + v->ob[1] * rsp.MP_matrix[1][0] + v->ob[2] * rsp.MP_matrix[2][0] + rsp.MP_matrix[3][0];
        float y = v->ob[0] * rsp.MP_matrix[0][1] + v->ob[1] * rsp.MP_matrix[1][1] + v->ob[2] * rsp.MP_matrix[2][1] + rsp.MP_matrix[3][1];
        float z = v->ob[0] * rsp.MP_matrix[0][2] + v->ob[1] * rsp.MP_matrix[1][2] + v->ob[2] * rsp.MP_matrix[2][2] + rsp.MP_matrix[3][2];
        float w = v->ob[0] * rsp.MP_matrix[0][3] + v->ob[1] * rsp.MP_matrix[1][3] + v->ob[2] * rsp.MP_matrix[2][3] + rsp.MP_matrix[3][3];

#ifdef IW4L_SM64_EMBEDDED
        {
            extern float gIw4lNativeCameraMatrix[4][4];
            extern float gIw4lRenderCameraPos[3];
            const float (*mv)[4] = rsp.modelview_matrix_stack[rsp.modelview_matrix_stack_size - 1];
            d->screen_space = rsp.P_matrix[3][3] != 0.0f;

            float cx = v->ob[0] * mv[0][0] + v->ob[1] * mv[1][0]
                     + v->ob[2] * mv[2][0] + mv[3][0];
            float cy = v->ob[0] * mv[0][1] + v->ob[1] * mv[1][1]
                     + v->ob[2] * mv[2][1] + mv[3][1];
            float cz = v->ob[0] * mv[0][2] + v->ob[1] * mv[1][2]
                     + v->ob[2] * mv[2][2] + mv[3][2];

            /*
             * Exact inverse of the SAME rigid mtxf_lookat() used for this
             * native frame. This produces stable world-space vertices while
             * keeping billboard/animation results baked by SM64.
             */
            d->world_x = gIw4lRenderCameraPos[0]
                       + cx * gIw4lNativeCameraMatrix[0][0]
                       + cy * gIw4lNativeCameraMatrix[0][1]
                       + cz * gIw4lNativeCameraMatrix[0][2];
            d->world_y = gIw4lRenderCameraPos[1]
                       + cx * gIw4lNativeCameraMatrix[1][0]
                       + cy * gIw4lNativeCameraMatrix[1][1]
                       + cz * gIw4lNativeCameraMatrix[1][2];
            d->world_z = gIw4lRenderCameraPos[2]
                       + cx * gIw4lNativeCameraMatrix[2][0]
                       + cy * gIw4lNativeCameraMatrix[2][1]
                       + cz * gIw4lNativeCameraMatrix[2][2];
        }
#endif
        
        x = gfx_adjust_x_for_aspect_ratio(x);
"@
if (-not $gfxPcText.Contains($vertexTransformNeedle)) {
    throw "Could not locate gfx_sp_vertex transform in $gfxPcSource"
}
$gfxPcText = $gfxPcText.Replace($vertexTransformNeedle, $vertexTransformReplacement)

$shaderInfoNeedle = @"
    uint8_t num_inputs;
    bool used_textures[2];
    gfx_rapi->shader_get_info(prg, &num_inputs, used_textures);
"@
$shaderInfoReplacement = @"
    uint8_t num_inputs;
    bool used_textures[2];
    /*
     * IMPORTANT: ask the renderer backend about prg, not gfx_cc_get_features
     * on the raw N64 cc_id. gfx_generate_cc() first rewrites the N64 combiner
     * into a generated shader_id (SHADER_TEXEL0/1, numbered color inputs,
     * alpha/fog flags). Passing cc_id directly misclassifies nearly every
     * textured SM64 draw as untextured.
     *
     * The embedded dummy backend keeps shader_id in its ShaderProgram and its
     * shader_get_info callback calls gfx_cc_get_features(shader_id), matching
     * the normal sm64-port renderer contract.
     */
    gfx_rapi->shader_get_info(prg, &num_inputs, used_textures);
"@
if (-not $gfxPcText.Contains($shaderInfoNeedle)) {
    throw "Could not locate shader info in $gfxPcSource"
}
$gfxPcText = $gfxPcText.Replace($shaderInfoNeedle, $shaderInfoReplacement)

$clipNeedle = @"
    if (v1->clip_rej & v2->clip_rej & v3->clip_rej) {
        // The whole triangle lies outside the visible area
        return;
    }
"@
$clipReplacement = @"
#ifndef IW4L_SM64_EMBEDDED
    if (v1->clip_rej & v2->clip_rej & v3->clip_rej) {
        // The whole triangle lies outside the visible area
        return;
    }
#endif
"@
if (-not $gfxPcText.Contains($clipNeedle)) {
    throw "Could not locate gfx triangle clip rejection in $gfxPcSource"
}
$gfxPcText = $gfxPcText.Replace($clipNeedle, $clipReplacement)

$captureNeedle = @"
    bool z_is_from_0_to_1 = gfx_rapi->z_is_from_0_to_1();
    
    for (int i = 0; i < 3; i++) {
"@
$captureReplacement = @"
    bool z_is_from_0_to_1 = gfx_rapi->z_is_from_0_to_1();

#ifdef IW4L_SM64_EMBEDDED
    {
        extern void iw4l_sm64_capture_triangle(
            const float *pos9,
            const float *uv6,
            const uint8_t *rgba12,
            uint32_t texture_id,
            int textured,
            int alpha,
            uint8_t wrap_s,
            uint8_t wrap_t
        );
        float native_pos[9];
        float native_uv[6];
        uint8_t native_rgba[12];
        uint32_t native_texture_id = 0xFFFFFFFFu;
        int native_texture_unit = -1;
        bool native_textured = false;
        bool screen_space = v1->screen_space;
        bool linear_filter = (rdp.other_mode_h & (3U << G_MDSFT_TEXTFILT)) != G_TF_POINT;

        /*
         * Do not trust the dummy backend's current texture slot blindly.
         * Ask the original SM64 color combiner which texture unit this draw
         * actually consumes, force any dirty texture through the native import
         * path, then capture the exact texture-cache ID selected by gfx_pc.
         */
        if (used_textures[0]) {
            native_texture_unit = 0;
        } else if (used_textures[1]) {
            native_texture_unit = 1;
        }

        if (native_texture_unit >= 0) {
            if (rdp.textures_changed[native_texture_unit]) {
                gfx_flush();
                import_texture(native_texture_unit);
                rdp.textures_changed[native_texture_unit] = false;
            }
            if (rendering_state.textures[native_texture_unit] != NULL) {
                native_texture_id = rendering_state.textures[native_texture_unit]->texture_id;
                native_textured = true;
            }
        }

        for (int native_i = 0; native_i < 3; native_i++) {
            float u = 0.0f;
            float v = 0.0f;
            native_pos[native_i * 3 + 0] = v_arr[native_i]->world_x;
            native_pos[native_i * 3 + 1] = v_arr[native_i]->world_y;
            native_pos[native_i * 3 + 2] = v_arr[native_i]->world_z;
            if (screen_space) {
                native_pos[native_i * 3 + 0] = v_arr[native_i]->x / v_arr[native_i]->w;
                native_pos[native_i * 3 + 1] = v_arr[native_i]->y / v_arr[native_i]->w;
                native_pos[native_i * 3 + 2] = 0.0f;
            }
            if (native_textured && tex_width != 0 && tex_height != 0) {
                u = (v_arr[native_i]->u - rdp.texture_tile.uls * 8) / 32.0f;
                v = (v_arr[native_i]->v - rdp.texture_tile.ult * 8) / 32.0f;
                if (linear_filter) {
                    u += 0.5f;
                    v += 0.5f;
                }
                u /= tex_width;
                v /= tex_height;
            }
            native_uv[native_i * 2 + 0] = u;
            native_uv[native_i * 2 + 1] = v;
            native_rgba[native_i * 4 + 0] = v_arr[native_i]->color.r;
            native_rgba[native_i * 4 + 1] = v_arr[native_i]->color.g;
            native_rgba[native_i * 4 + 2] = v_arr[native_i]->color.b;
            /*
             * v_arr[].color.a is a color-combiner input, not the final surface
             * opacity. Treating it as Bevy vertex alpha makes ordinary SM64
             * terrain translucent. For textured draws, let the decoded texture
             * alpha provide cutouts/transparency; keep vertex alpha opaque.
             *
             * Preserve native vertex alpha only for genuinely untextured alpha
             * draws where there is no texture alpha channel to carry opacity.
             */
            native_rgba[native_i * 4 + 3] =
                (!native_textured && use_alpha) ? v_arr[native_i]->color.a : 255;
            if (screen_space) {
                // Use the native combiner's color inputs: HUD icons use pure
                // texture, fonts modulate environment, boxes use shade/env.
                struct RGBA color = {255, 255, 255, 255};
                if (num_inputs != 0) {
                    switch (comb->shader_input_mapping[0][0]) {
                        case CC_PRIM: color = rdp.prim_color; break;
                        case CC_SHADE: color = v_arr[native_i]->color; break;
                        case CC_ENV: color = rdp.env_color; break;
                    }
                    if (use_alpha) {
                        switch (comb->shader_input_mapping[1][0]) {
                            case CC_PRIM: color.a = rdp.prim_color.a; break;
                            case CC_SHADE: color.a = v_arr[native_i]->color.a; break;
                            case CC_ENV: color.a = rdp.env_color.a; break;
                            default: color.a = 255; break;
                        }
                    } else { color.a = 255; }
                }
                memcpy(&native_rgba[native_i * 4], &color, 4);
            }
        }

        iw4l_sm64_capture_triangle(
            native_pos,
            native_uv,
            native_rgba,
            native_texture_id,
            native_textured,
            (use_alpha ? 1 : 0) | (screen_space ? 2 : 0),
            (uint8_t)rdp.texture_tile.cms,
            (uint8_t)rdp.texture_tile.cmt
        );
    }
#endif
    
    for (int i = 0; i < 3; i++) {
"@
if (-not $gfxPcText.Contains($captureNeedle)) {
    throw "Could not locate native triangle capture point in $gfxPcSource"
}
$gfxPcText = $gfxPcText.Replace($captureNeedle, $captureReplacement)
$rectangleNeedle = '    ul->x = ulxf;'
if (-not $gfxPcText.Contains($rectangleNeedle)) {
    throw "Could not locate rectangle vertices in $gfxPcSource"
}
$gfxPcText = $gfxPcText.Replace($rectangleNeedle, @"
#ifdef IW4L_SM64_EMBEDDED
    ul->screen_space = ll->screen_space = lr->screen_space = ur->screen_space = true;
#endif
    ul->x = ulxf;
"@)
Set-Content -Path $gfxPcSource -Value $gfxPcText -Encoding UTF8

Copy-Item $gfxDummySource $gfxDummyBackup -Force
$gfxDummyText = Get-Content $gfxDummySource -Raw

$dummyIncludeNeedle = '#include "gfx_rendering_api.h"'
$dummyIncludeReplacement = @"
#include "gfx_rendering_api.h"
#include "gfx_cc.h"

struct ShaderProgram {
    uint32_t shader_id;
};

static struct ShaderProgram gIw4lShaderPool[128];
static uint32_t gIw4lShaderCount = 0;
static uint32_t gIw4lNextTextureId = 1;
static uint32_t gIw4lSelectedTextures[2] = {0,0};
static int gIw4lLastSelectedTile = 0;

extern void iw4l_sm64_capture_texture(
    uint32_t id,
    const uint8_t *rgba,
    uint32_t width,
    uint32_t height
);
"@
if (-not $gfxDummyText.Contains($dummyIncludeNeedle)) {
    throw "Could not locate dummy renderer include point in $gfxDummySource"
}
$gfxDummyText = $gfxDummyText.Replace($dummyIncludeNeedle, $dummyIncludeReplacement)

$createShaderNeedle = @"
static struct ShaderProgram *gfx_dummy_renderer_create_and_load_new_shader(uint32_t shader_id) {
    return NULL;
}

static struct ShaderProgram *gfx_dummy_renderer_lookup_shader(uint32_t shader_id) {
    return NULL;
}

static void gfx_dummy_renderer_shader_get_info(struct ShaderProgram *prg, uint8_t *num_inputs, bool used_textures[2]) {
    *num_inputs = 0;
    used_textures[0] = false;
    used_textures[1] = false;
}

static uint32_t gfx_dummy_renderer_new_texture(void) {
    return 0;
}

static void gfx_dummy_renderer_select_texture(int tile, uint32_t texture_id) {
}

static void gfx_dummy_renderer_upload_texture(const uint8_t *rgba32_buf, int width, int height) {
}
"@
$createShaderReplacement = @"
static struct ShaderProgram *gfx_dummy_renderer_create_and_load_new_shader(uint32_t shader_id) {
    uint32_t i;
    for (i=0;i<gIw4lShaderCount;i++) {
        if (gIw4lShaderPool[i].shader_id == shader_id) {
            return &gIw4lShaderPool[i];
        }
    }
    if (gIw4lShaderCount >= 128) {
        return &gIw4lShaderPool[0];
    }
    gIw4lShaderPool[gIw4lShaderCount].shader_id=shader_id;
    return &gIw4lShaderPool[gIw4lShaderCount++];
}

static struct ShaderProgram *gfx_dummy_renderer_lookup_shader(uint32_t shader_id) {
    uint32_t i;
    for (i=0;i<gIw4lShaderCount;i++) {
        if (gIw4lShaderPool[i].shader_id == shader_id) {
            return &gIw4lShaderPool[i];
        }
    }
    return NULL;
}

static void gfx_dummy_renderer_shader_get_info(struct ShaderProgram *prg, uint8_t *num_inputs, bool used_textures[2]) {
    struct CCFeatures features;
    if (prg == NULL) {
        *num_inputs=0;
        used_textures[0]=false;
        used_textures[1]=false;
        return;
    }
    gfx_cc_get_features(prg->shader_id,&features);
    *num_inputs=(uint8_t)features.num_inputs;
    used_textures[0]=features.used_textures[0];
    used_textures[1]=features.used_textures[1];
}

static uint32_t gfx_dummy_renderer_new_texture(void) {
    return gIw4lNextTextureId++;
}

static void gfx_dummy_renderer_select_texture(int tile, uint32_t texture_id) {
    if (tile >= 0 && tile < 2) {
        gIw4lSelectedTextures[tile]=texture_id;
        gIw4lLastSelectedTile=tile;
    }
}

static void gfx_dummy_renderer_upload_texture(const uint8_t *rgba32_buf, int width, int height) {
    uint32_t id=gIw4lSelectedTextures[gIw4lLastSelectedTile];
    if (id != 0 && rgba32_buf != NULL && width > 0 && height > 0) {
        iw4l_sm64_capture_texture(id,rgba32_buf,(uint32_t)width,(uint32_t)height);
    }
}
"@
if (-not $gfxDummyText.Contains($createShaderNeedle)) {
    throw "Could not locate dummy renderer shader/texture callbacks in $gfxDummySource"
}
$gfxDummyText = $gfxDummyText.Replace($createShaderNeedle, $createShaderReplacement)
Set-Content -Path $gfxDummySource -Value $gfxDummyText -Encoding UTF8

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
            "PYTHON=$pythonMake",
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
    if (Test-Path $gameInitBackup) {
        Copy-Item $gameInitBackup $gameInitSource -Force
        Remove-Item $gameInitBackup -Force
    }
    if (Test-Path $areaBackup) {
        Copy-Item $areaBackup $areaSource -Force
        Remove-Item $areaBackup -Force
    }
    if (Test-Path $objectProcessorBackup) {
        Copy-Item $objectProcessorBackup $objectProcessorSource -Force
        Remove-Item $objectProcessorBackup -Force
    }
    if (Test-Path $renderGraphBackup) {
        Copy-Item $renderGraphBackup $renderGraphSource -Force
        Remove-Item $renderGraphBackup -Force
    }
    if (Test-Path $gfxPcBackup) {
        Copy-Item $gfxPcBackup $gfxPcSource -Force
        Remove-Item $gfxPcBackup -Force
    }
    if (Test-Path $gfxDummyBackup) {
        Copy-Item $gfxDummyBackup $gfxDummySource -Force
        Remove-Item $gfxDummyBackup -Force
    }
}
