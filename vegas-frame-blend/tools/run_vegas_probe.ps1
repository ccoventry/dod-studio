# Runs vegas_probe.cs in VEGAS Pro 21 through its -SCRIPT: switch, with
# OFX_PLUGIN_PATH set for this process tree only (nothing persistent changes).
# Closes VEGAS if the script never starts, or if neither log grows for 90 s.
# VEGAS reopens the last project first; the script discards it unsaved.
#
#   run_vegas_probe.ps1 -Clip <300 fps clip> -Out <empty folder> [-Mode event|media|subclip]
#                       [-Pixel 8bit|float] [-Order "1,2,3,4,5"] [-PluginDir <folder holding the bundle>]
#
# Results: <Out>\script.log (what the script did), <Out>\plugin.log (what VEGAS
# passed the plugin, with a hash per fetched frame), <Out>\E*.mp4.
param(
    [Parameter(Mandatory = $true)][string]$Clip,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Mode = "event",
    [string]$Pixel = "8bit",
    [string]$Order = "1,2,3,4,5",
    [string]$PluginDir = "",
    [string]$Vegas = "C:\Program Files\VEGAS\VEGAS Pro 21.0\vegas210.exe"
)
$ErrorActionPreference = "Continue"
# PowerShell 5.1 leaves $PSScriptRoot empty inside param defaults.
if (-not $PluginDir) { $PluginDir = Join-Path (Split-Path $PSScriptRoot) "build" }
New-Item -ItemType Directory -Force $Out | Out-Null
$report = Join-Path $Out "run_report.txt"
"start $(Get-Date -Format o)" | Set-Content $report
if (Get-Process vegas210 -ErrorAction SilentlyContinue) { "VEGAS already running: not launching" | Add-Content $report; exit 1 }

$env:OFX_PLUGIN_PATH = $PluginDir
$env:DODSTUDIO_FRAME_BLEND_LOG = Join-Path $Out "plugin.log"
$env:FRAME_BLEND_PROBE_OUT = $Out
$env:FRAME_BLEND_PROBE_CLIP = $Clip
$env:FRAME_BLEND_PROBE_PIXEL = $Pixel
$env:FRAME_BLEND_PROBE_MODE = $Mode
$env:FRAME_BLEND_PROBE_ORDER = $Order
$script = Join-Path $PSScriptRoot "vegas_probe.cs"
$p = Start-Process -FilePath $Vegas -ArgumentList "-SCRIPT:`"$script`"" -PassThru
"pid $($p.Id)" | Add-Content $report
$scriptLog = Join-Path $Out "script.log"

$startDeadline = (Get-Date).AddSeconds(120)
while (-not $p.HasExited -and (Get-Date) -lt $startDeadline -and -not (Test-Path $scriptLog)) { Start-Sleep -Milliseconds 500 }
"script log present: $(Test-Path $scriptLog) at $(Get-Date -Format o)" | Add-Content $report
if (Test-Path $scriptLog) {
    $runDeadline = (Get-Date).AddMinutes(12)
    $pluginLog = $env:DODSTUDIO_FRAME_BLEND_LOG
    $lastSize = -1; $lastChange = Get-Date
    while (-not $p.HasExited -and (Get-Date) -lt $runDeadline) {
        Start-Sleep -Milliseconds 1000
        $size = (Get-Item $scriptLog).Length + $(if (Test-Path $pluginLog) { (Get-Item $pluginLog).Length } else { 0 })
        if ($size -ne $lastSize) { $lastSize = $size; $lastChange = Get-Date }
        elseif (((Get-Date) - $lastChange).TotalSeconds -gt 90) { "stalled: no log growth for 90 s at $(Get-Date -Format o)" | Add-Content $report; break }
    }
}
if (-not $p.HasExited) {
    $p.CloseMainWindow() | Out-Null
    $p.WaitForExit(60000) | Out-Null
}
if (-not $p.HasExited) {
    "still running after 60 s: stopping pid $($p.Id)" | Add-Content $report
    Stop-Process -Id $p.Id -Force
} else {
    "exited, code $($p.ExitCode)" | Add-Content $report
}
"end $(Get-Date -Format o)" | Add-Content $report
