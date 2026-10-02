# install-bridge-task.ps1
# Thin compatibility wrapper for lantern-runtime.ps1 install.
param(
    [string]$TaskName = "LanternKeeper-Bridge",
    [int]$IntervalMinutes = 2,
    [string]$PostRepo = "D:\Projects\lantern-post",
    [string]$GitRepo = "D:\Projects\lantern-git",
    [string]$ServiceUrl = "http://127.0.0.1:4317",
    [string]$AllowAgents = "chatgpt-lucy,pi",
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$runtimeScript = Join-Path $PSScriptRoot "lantern-runtime.ps1"

Write-Host "Forwarding to unified runtime installer: $runtimeScript" -ForegroundColor Cyan
& $runtimeScript install -BridgeTaskName $TaskName -BridgeIntervalMinutes $IntervalMinutes -PostRepo $PostRepo -GitRepo $GitRepo -ServiceUrl $ServiceUrl -AllowAgents $AllowAgents -SkipBuild:$SkipBuild
