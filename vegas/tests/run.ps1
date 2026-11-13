# Builds and runs the nameplate script's joining-rule tests (no VEGAS needed to
# run them; its API DLL is needed to compile the script they test).
param([string]$Vegas = "C:\Program Files\VEGAS\VEGAS Pro 21.0")
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$csc = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$out = Join-Path ([IO.Path]::GetTempPath()) "nameplate-merge-test"
New-Item -ItemType Directory -Force $out | Out-Null
$exe = Join-Path $out "NameplateMergeTest.exe"
$script = Join-Path $here "..\DoD Studio Nameplates.cs"
& $csc /nologo /out:$exe "/reference:$(Join-Path $Vegas 'ScriptPortal.Vegas.dll')" /reference:System.Windows.Forms.dll /reference:System.Core.dll `
    $script (Join-Path $here "NameplateMergeTest.cs")
if ($LASTEXITCODE -ne 0) { throw "csc failed (exit $LASTEXITCODE)" }
Copy-Item (Join-Path $Vegas "ScriptPortal.Vegas.dll") $out -Force
& $exe
exit $LASTEXITCODE
