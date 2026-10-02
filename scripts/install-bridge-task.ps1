# install-bridge-task.ps1
# Registers a scheduled task in Windows Task Scheduler to run Lantern GitHub Bridge periodically.
param(
    [string]$TaskName = "LanternGitHubBridge",
    [int]$IntervalMinutes = 2,
    [string]$PostRepo = "D:\Projects\lantern-post",
    [string]$GitRepo = "D:\Projects\lantern-git",
    [string]$ServiceUrl = "http://127.0.0.1:4317",
    [string]$AllowAgents = "chatgpt-lucy,pi"
)

$ErrorActionPreference = 'Stop'
$sourceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$binaryPath = Join-Path $sourceRoot "target\debug\lighting.exe"

if (-not (Test-Path $binaryPath)) {
    $binaryPath = Join-Path $sourceRoot "target\release\lighting.exe"
}
if (-not (Test-Path $binaryPath)) {
    throw "lighting.exe not found. Please run 'cargo build -p lighting' first."
}

$argumentList = "bridge github once --service-url `"$ServiceUrl`" --post-repo `"$PostRepo`" --git-repo `"$GitRepo`""

Write-Host "Registering Task Scheduler task: $TaskName"
Write-Host "Binary: $binaryPath"
Write-Host "Arguments: $argumentList"
Write-Host "Repeat interval: Every $IntervalMinutes minutes"

$action = New-ScheduledTaskAction -Execute $binaryPath -Argument $argumentList -WorkingDirectory $sourceRoot
$trigger = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes $IntervalMinutes) -RepetitionDuration ([TimeSpan]::MaxValue)
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable

# Set environment variables for the task if needed via a wrapper or register task
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Settings $settings -Force

Write-Host "Task $TaskName registered successfully."
