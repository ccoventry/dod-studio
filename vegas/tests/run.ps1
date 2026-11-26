# Builds and runs the VEGAS scripts' tests: the nameplate script's joining
# rule and the relink script's matching (#1514). No VEGAS needed to run them;
# its API DLL is needed to compile the scripts they test. Each script defines
# its own EntryPoint, so each is built into its own test program.
param([string]$Vegas = "C:\Program Files\VEGAS\VEGAS Pro 21.0")
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$csc = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$out = Join-Path ([IO.Path]::GetTempPath()) "dodstudio-vegas-tests"
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item (Join-Path $Vegas "ScriptPortal.Vegas.dll") $out -Force
$suites = @(
    @{ Script = "DoD Studio Nameplates.cs"; Test = "NameplateMergeTest" },
    @{ Script = "DoD Studio Relink Clips.cs"; Test = "RelinkMatchTest" }
)
$failed = 0
foreach ($suite in $suites) {
    $exe = Join-Path $out "$($suite.Test).exe"
    & $csc /nologo /out:$exe "/reference:$(Join-Path $Vegas 'ScriptPortal.Vegas.dll')" /reference:System.Windows.Forms.dll `
        (Join-Path $here "..\$($suite.Script)") (Join-Path $here "$($suite.Test).cs")
    if ($LASTEXITCODE -ne 0) { throw "csc failed for $($suite.Test) (exit $LASTEXITCODE)" }
    Write-Output "== $($suite.Test)"
    & $exe
    if ($LASTEXITCODE -ne 0) { $failed++ }
}
exit $failed
