# Builds the VEGAS bridge extension (DodStudioVegasBridge.dll) with the C#
# compiler that ships with .NET Framework 4.x, against VEGAS's own
# ScriptPortal.Vegas.dll. No SDK or NuGet needed.
#
#   powershell -ExecutionPolicy Bypass -File vegas-mcp\build.ps1 [-Install] [-Uninstall] [-Vegas <VEGAS folder>]
#
# -Install    copies the DLL into the per-user extension folder
#             %APPDATA%\VEGAS Pro\21.0\Application Extensions (no admin).
#             VEGAS loads it on its next start.
# -Uninstall  removes it from there.
#
# Signs with the LocalRustDev code-signing certificate when the current user
# has one (WDAC), as build-and-launch.ps1 does.
param(
    [switch]$Install,
    [switch]$Uninstall,
    [string]$Vegas = "C:\Program Files\VEGAS\VEGAS Pro 21.0"
)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$extDir = Join-Path $env:APPDATA "VEGAS Pro\21.0\Application Extensions"
$name = "DodStudioVegasBridge.dll"

if ($Uninstall) {
    $installed = Join-Path $extDir $name
    if (Test-Path $installed) { Remove-Item $installed; Write-Host "removed $installed" } else { Write-Host "not installed" }
    exit 0
}

$csc = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path $csc)) { throw "csc.exe not found at ${csc}: .NET Framework 4.x is needed" }
$api = Join-Path $Vegas "ScriptPortal.Vegas.dll"
if (-not (Test-Path $api)) { throw "ScriptPortal.Vegas.dll not found in ${Vegas}: pass -Vegas <VEGAS install folder>" }

$build = Join-Path $root "build"
New-Item -ItemType Directory -Force $build | Out-Null
$out = Join-Path $build $name
$sources = Get-ChildItem (Join-Path $root "extension") -Filter *.cs | ForEach-Object { $_.FullName }
& $csc /nologo /target:library /optimize+ /platform:x64 "/out:$out" "/reference:$api" `
    /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll /reference:System.Core.dll $sources
if ($LASTEXITCODE -ne 0) { throw "csc failed (exit $LASTEXITCODE)" }

$cert = Get-Item Cert:\CurrentUser\My\* | Where-Object { $_.Subject -match 'LocalRustDev' } | Select-Object -First 1
if ($cert) {
    $sig = Set-AuthenticodeSignature -Certificate $cert -FilePath $out
    Write-Host "signed: $($sig.Status)"
} else {
    Write-Host "no LocalRustDev certificate in Cert:\CurrentUser\My: left unsigned"
}
Write-Host "built $out"

if ($Install) {
    New-Item -ItemType Directory -Force $extDir | Out-Null
    Copy-Item $out (Join-Path $extDir $name) -Force
    Write-Host "installed to $extDir (restart VEGAS to load it)"
}
