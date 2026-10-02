# uninstall-bridge-task.ps1
# Unregisters the Lantern GitHub Bridge scheduled task from Windows Task Scheduler.
param(
    [string]$TaskName = "LanternGitHubBridge"
)

$ErrorActionPreference = 'Stop'
$task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue

if ($null -eq $task) {
    Write-Host "Task '$TaskName' is not currently registered."
} else {
    Write-Host "Unregistering task: $TaskName"
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    Write-Host "Task '$TaskName' removed successfully."
}
