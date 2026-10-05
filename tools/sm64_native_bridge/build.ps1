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
$builtExe = Join-Path $build "sm64.us.exe"
$output = Join-Path $build "iw4l-sm64-bridge.exe"
$staleBackupSource = Join-Path $root "src\pc\pc_main.iw4l-backup.c"
$staleBackupObject = Join-Path $build "src\pc\pc_main.iw4l-backup.o"

# Older versions of this script created the backup inside src/pc with a .c
# extension. sm64-port recursively globs src/pc/*.c, so that backup was
# compiled and linked alongside the bridge, causing duplicate WinMain and
# global symbol definitions. Remove both stale artifacts before this build.
Remove-Item $staleBackupSource -Force -ErrorAction SilentlyContinue
Remove-Item $staleBackupObject -Force -ErrorAction SilentlyContinue

Write-Host "Building native SM64 gameplay bridge from $root"
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
                set_play_mode(PLAY_MODE_NORMAL);
                result = 0;
            }
            break;
"@
if (-not $levelUpdateText.Contains($updateNeedle)) {
    throw "Could not locate lvl_init_or_update update case in $levelUpdateSource"
}
$levelUpdateText = $levelUpdateText.Replace($updateNeedle, $updateReplacement)
Set-Content -Path $levelUpdateSource -Value $levelUpdateText -Encoding UTF8

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

        $makeArgs = @(
            "BUILD_DIR=$buildRel",
            "ENABLE_DX11=0",
            "ENABLE_DX12=0",
            "ENABLE_OPENGL=0",
            "GFX_CFLAGS=-DENABLE_GFX_DUMMY -DWIDESCREEN",
            "OPT_FLAGS=-O0 -g3",
            "-j$Jobs"
        )
        & make @makeArgs
        if ($LASTEXITCODE -ne 0) {
            throw "sm64-port bridge build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path $builtExe)) {
        throw "Expected bridge executable was not produced at $builtExe"
    }

    # Windows locks a running executable. A previous launcher run can leave
    # iw4l-sm64-bridge.exe alive if the game or bridge terminated abnormally.
    # Stop only the bridge helper before replacing it; never touch the game.
    Get-Process -Name "iw4l-sm64-bridge" -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue

    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    while ((Test-Path $output) -and ([DateTime]::UtcNow -lt $deadline)) {
        try {
            $stream = [System.IO.File]::Open(
                $output,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::ReadWrite,
                [System.IO.FileShare]::None
            )
            $stream.Dispose()
            break
        }
        catch {
            Start-Sleep -Milliseconds 100
        }
    }

    Copy-Item $builtExe $output -Force

    # Generate a self-contained sorted symbol table while MinGW tools are
    # definitely available. The Rust launcher can use this file later even
    # when addr2line/nm are not on the runtime PATH.
    $symbolMap = Join-Path $build "iw4l-sm64-bridge.sym"
    $nm = Get-Command "x86_64-w64-mingw32-nm" -ErrorAction SilentlyContinue
    if (-not $nm) {
        $nm = Get-Command "nm" -ErrorAction SilentlyContinue
    }
    if ($nm) {
        & $nm.Source -n $output | Set-Content -Path $symbolMap -Encoding ASCII
        Write-Host "Native symbol map: $symbolMap"
    }
    else {
        Write-Warning "nm was not found; crash symbol map was not generated."
    }

    Write-Host ""
    Write-Host "Native SM64 gameplay bridge built:"
    Write-Host "  $output"
    Write-Host ""
    Write-Host "IW4L auto-detects this when SM64_NATIVE_ROOT points at this checkout."
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
}
