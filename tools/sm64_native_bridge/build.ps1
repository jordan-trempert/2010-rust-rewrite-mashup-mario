param(
    [string]$Sm64PortRoot = $env:SM64_DECOMP_ROOT,
    [int]$Jobs = [Math]::Max(1, [Environment]::ProcessorCount)
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Sm64PortRoot)) {
    throw "Set SM64_DECOMP_ROOT to a host-native sm64-port checkout, or pass -Sm64PortRoot."
}

$root = (Resolve-Path $Sm64PortRoot).Path
if (-not (Test-Path (Join-Path $root "src\pc\pc_main.c"))) {
    throw @"
$root is not a host-native sm64-port checkout.
Full SM64 gameplay needs the PC-port form of the decomp so its original C
object/behavior runtime can execute on Windows. Point SM64_DECOMP_ROOT at
https://github.com/sm64-port/sm64-port (with your own extracted SM64 assets).
"@
}

foreach ($tool in @("make", "gcc")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool is not on PATH. Run this from an MSYS2/MinGW environment that can build sm64-port."
    }
}

$buildRel = "build/us_bridge"
$build = Join-Path $root "build\us_bridge"
$bridgeSource = Join-Path $PSScriptRoot "bridge.c"
$tempSource = Join-Path $root "src\pc\iw4l_bridge.c"

Write-Host "Building the native SM64 gameplay runtime from $root"
Copy-Item $bridgeSource $tempSource -Force
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
            throw "sm64-port build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }

    $objects = Get-ChildItem $build -Recurse -Filter *.o | ForEach-Object { $_.FullName }
    if ($objects.Count -eq 0) {
        throw "No sm64-port object files were produced under $build"
    }

    $response = Join-Path $build "iw4l-sm64-bridge.rsp"
    $objects | ForEach-Object { '"{0}"' -f $_ } | Set-Content -Encoding ascii $response

    $output = Join-Path $build "iw4l-sm64-bridge.exe"
    & gcc "-o" $output "@$response" "-lm" "-lxinput9_1_0" "-lole32" "-no-pie"
    if ($LASTEXITCODE -ne 0) {
        throw "native bridge link failed with exit code $LASTEXITCODE"
    }

    Write-Host ""
    Write-Host "Native SM64 gameplay bridge built:"
    Write-Host "  $output"
    Write-Host ""
    Write-Host "IW4L will auto-detect this file from SM64_DECOMP_ROOT."
}
finally {
    Remove-Item $tempSource -Force -ErrorAction SilentlyContinue
}
