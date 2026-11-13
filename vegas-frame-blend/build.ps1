# Builds the Frame Blend OpenFX bundle and the test harness with MSVC (x64).
#
#   powershell -ExecutionPolicy Bypass -File vegas-frame-blend\build.ps1 [-Test] [-OutDir <folder>]
#
# -Test    runs the harness against the fresh bundle (exit code 1 on a failed check).
# -OutDir  copies FrameBlend.ofx.bundle there as well (for example the folder named in OFX_PLUGIN_PATH).
#
# On a PC that enforces WDAC, sign with the LocalRustDev code-signing certificate
# when the current user has one, the same step build-and-launch.ps1 runs.
param(
    [switch]$Test,
    [string]$OutDir = ""
)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$build = Join-Path $root "build"
$bundle = Join-Path $build "FrameBlend.ofx.bundle"
$win64 = Join-Path $bundle "Contents\Win64"
New-Item -ItemType Directory -Force $win64 | Out-Null

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = $null
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if (-not $vsPath) { $vsPath = "C:\Program Files\Microsoft Visual Studio\2022\Community" }
$vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) { throw "vcvars64.bat not found at ${vcvars}: install the MSVC x64 build tools" }

$inc = Join-Path $root "third_party\openfx\include"
$flags = "/nologo /std:c++17 /O2 /EHsc /MT /W3 /DNOMINMAX /DWIN64 /I`"$inc`""
$bat = Join-Path $build "build.bat"
@"
@echo off
call "$vcvars" >nul 2>nul || exit /b 1
cd /d "$build" || exit /b 1
cl $flags /LD "$root\src\frame_blend.cpp" /Fo"$build\\" /Fe"$win64\FrameBlend.ofx" /link /DLL /IMPLIB:"$build\FrameBlend.lib" || exit /b 1
cl $flags "$root\harness\ofx_host_harness.cpp" /Fo"$build\\" /Fe"$build\ofx_host_harness.exe" || exit /b 1
"@ | Set-Content -Encoding ascii $bat
cmd /c "`"$bat`""
if ($LASTEXITCODE -ne 0) { throw "build failed (exit $LASTEXITCODE)" }

$ofx = Join-Path $win64 "FrameBlend.ofx"
$harness = Join-Path $build "ofx_host_harness.exe"
$cert = Get-Item Cert:\CurrentUser\My\* | Where-Object { $_.Subject -match 'LocalRustDev' } | Select-Object -First 1
if ($cert) {
    foreach ($f in @($ofx, $harness)) {
        $sig = Set-AuthenticodeSignature -Certificate $cert -FilePath $f
        Write-Host "signed $(Split-Path $f -Leaf): $($sig.Status)"
    }
} else {
    Write-Host "no LocalRustDev certificate in Cert:\CurrentUser\My: left unsigned"
}
Write-Host "built $ofx"

if ($OutDir) {
    $dest = Join-Path $OutDir "FrameBlend.ofx.bundle"
    New-Item -ItemType Directory -Force (Join-Path $dest "Contents\Win64") | Out-Null
    Copy-Item $ofx (Join-Path $dest "Contents\Win64\FrameBlend.ofx") -Force
    Write-Host "copied to $dest"
}

if ($Test) {
    $env:DODSTUDIO_FRAME_BLEND_LOG = Join-Path $build "harness_plugin.log"
    if (Test-Path $env:DODSTUDIO_FRAME_BLEND_LOG) { Remove-Item $env:DODSTUDIO_FRAME_BLEND_LOG }
    & $harness $ofx
    exit $LASTEXITCODE
}
