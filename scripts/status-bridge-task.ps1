# status-bridge-task.ps1
# Queries Windows Task Scheduler and Bridge Doctor to report bridge status.
param(
    [string]$TaskName = "LanternGitHubBridge",
    [string]$PostRepo = "D:\Projects\lantern-post"
)

$ErrorActionPreference = 'Continue'
Write-Host "=== Task Scheduler Status ==="
$task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
if ($null -eq $task) {
    Write-Host "Task '$TaskName' is NOT installed in Task Scheduler."
} else {
    $info = Get-ScheduledTaskInfo -TaskName $TaskName
    Write-Host "Task Name:        $($task.TaskName)"
    Write-Host "Task State:       $($task.State)"
    Write-Host "Last Run Time:    $($info.LastRunTime)"
    Write-Host "Last Result Code: $($info.LastTaskResult)"
    Write-Host "Next Run Time:    $($info.NextRunTime)"
}

Write-Host "`n=== Bridge Doctor Report ==="
$sourceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$binaryPath = Join-Path $sourceRoot "target\debug\lighting.exe"
if (-not (Test-Path $binaryPath)) {
    $binaryPath = Join-Path $sourceRoot "target\release\lighting.exe"
}

if (Test-Path $binaryPath) {
    & $binaryPath bridge github doctor --post-repo $PostRepo
} else {
    Write-Host "lighting.exe binary not found. Build lighting before running doctor."
}
