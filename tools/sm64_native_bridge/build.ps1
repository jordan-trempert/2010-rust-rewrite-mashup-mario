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
$backup = Join-Path $root "src\pc\pc_main.iw4l-backup.c"
$builtExe = Join-Path $build "sm64.us.exe"
$output = Join-Path $build "iw4l-sm64-bridge.exe"

Write-Host "Building native SM64 gameplay bridge from $root"
Copy-Item $pcMain $backup -Force
Copy-Item $bridgeSource $pcMain -Force

try {
    Push-Location $root
    try {
        $makeArgs = @(
            "BUILD_DIR=$buildRel",
            "ENABLE_DX11=0",
            "ENABLE_DX12=0",
            "ENABLE_OPENGL=0",
            "GFX_CFLAGS=-DENABLE_GFX_DUMMY -DWIDESCREEN",
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
    Copy-Item $builtExe $output -Force

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
}
