# status-bridge-task.ps1
# Thin compatibility wrapper for lantern-runtime.ps1 status.
param(
    [string]$TaskName = "LanternKeeper-Bridge",
    [string]$PostRepo = "D:\Projects\lantern-post",
    [switch]$Json
)

$ErrorActionPreference = 'Continue'
$runtimeScript = Join-Path $PSScriptRoot "lantern-runtime.ps1"

& $runtimeScript status -BridgeTaskName $TaskName -PostRepo $PostRepo -Json:$Json
