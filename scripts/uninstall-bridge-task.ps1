# uninstall-bridge-task.ps1
# Thin compatibility wrapper for lantern-runtime.ps1 uninstall.
param(
    [string]$TaskName = "LanternKeeper-Bridge"
)

$ErrorActionPreference = 'Stop'
$runtimeScript = Join-Path $PSScriptRoot "lantern-runtime.ps1"

Write-Host "Forwarding to unified runtime uninstaller: $runtimeScript" -ForegroundColor Cyan
& $runtimeScript uninstall -BridgeTaskName $TaskName
