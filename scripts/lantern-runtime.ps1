#Requires -Version 5.1
<#
.SYNOPSIS
    Lantern Keeper Unified Runtime Manager.
    Manages the lifecycle of the Lighting service and GitHub bridge on Windows.

.DESCRIPTION
    Consolidates Lighting service and GitHub bridge installation, lifecycle,
    health inspection, atomic updates, and uninstallation into a single operator surface.
    Executes from an isolated runtime directory (.lighting-runtime/bin) rather than Cargo
    target directories, eliminating build file locks during active operation.

.PARAMETER Action
    Lifecycle action to perform: install, start, stop, restart, status, doctor, update, uninstall.
#>

[CmdletBinding()]
param(
    [Parameter(Position = 0, Mandatory = $true)]
    [ValidateSet('install', 'start', 'stop', 'restart', 'status', 'doctor', 'update', 'uninstall')]
    [string]$Action,

    [string]$ServiceTaskName = "LanternKeeper-Service",
    [string]$BridgeTaskName = "LanternKeeper-Bridge",
    [string]$ServiceUrl = "http://127.0.0.1:4317",
    [int]$BridgeIntervalMinutes = 2,
    [string]$AllowAgents = "chatgpt-lucy,pi",
    [string]$AskAgents = "",
    [string]$DataDir = "",
    [string]$PostRepo = "D:\Projects\lantern-post",
    [string]$GitRepo = "D:\Projects\lantern-git",
    [string]$RuntimeDir = "",
    [switch]$Push,
    [switch]$SkipBuild,
    [switch]$NoStart,
    [switch]$Json
)

$ErrorActionPreference = 'Stop'

# Project root resolution
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path

# Setup runtime directory defaults
if ([string]::IsNullOrWhiteSpace($RuntimeDir)) {
    if (Test-Path "D:\Projects\lantern-keeper\.lighting-runtime") {
        $RuntimeDir = "D:\Projects\lantern-keeper\.lighting-runtime"
    } else {
        $RuntimeDir = Join-Path $ProjectRoot ".lighting-runtime"
    }
}
$BinDir = Join-Path $RuntimeDir "bin"
$LogsDir = Join-Path $RuntimeDir "logs"
$StateDir = Join-Path $RuntimeDir "state"
$ConfigDir = Join-Path $RuntimeDir "config"

$InstalledBinary = Join-Path $BinDir "lighting.exe"
$ServiceScript = Join-Path $BinDir "run-service.cmd"
$BridgeScript = Join-Path $BinDir "run-bridge.cmd"
$ServiceLog = Join-Path $LogsDir "service.log"
$ServiceErrLog = Join-Path $LogsDir "service.err.log"
$BridgeLog = Join-Path $LogsDir "bridge.log"
$BridgeErrLog = Join-Path $LogsDir "bridge.err.log"

if ([string]::IsNullOrWhiteSpace($DataDir)) {
    # Check if D:\Projects\lantern-keeper\.lighting-data\surrealkv exists (canonical datastore)
    if (Test-Path "D:\Projects\lantern-keeper\.lighting-data\surrealkv") {
        $DataDir = "D:\Projects\lantern-keeper\.lighting-data\surrealkv"
    } else {
        $DataDir = Join-Path $ProjectRoot ".lighting-data\surrealkv"
    }
}

function Ensure-Directories {
    foreach ($dir in @($RuntimeDir, $BinDir, $LogsDir, $StateDir, $ConfigDir)) {
        if (-not (Test-Path $dir)) {
            New-Item -ItemType Directory -Path $dir -Force | Out-Null
        }
    }
}

function Build-Release-Binary {
    Write-Host "==> Building Lighting release binary via Cargo..." -ForegroundColor Cyan
    Push-Location $ProjectRoot
    try {
        & cargo build --release -p lighting
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo release build failed with code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

function Install-Runtime-Files {
    Ensure-Directories

    $releaseBinary = Join-Path $ProjectRoot "target\release\lighting.exe"
    if (-not (Test-Path $releaseBinary)) {
        if ($SkipBuild) {
            throw "Release binary not found at $releaseBinary and -SkipBuild was specified."
        }
        Build-Release-Binary
    }

    $needCopy = $true
    if (Test-Path $InstalledBinary) {
        try {
            $h1 = (Get-FileHash $releaseBinary).Hash
            $h2 = (Get-FileHash $InstalledBinary).Hash
            if ($h1 -eq $h2) {
                $needCopy = $false
            }
        } catch { }
    }
    if ($needCopy) {
        Write-Host "==> Installing runtime binary to $InstalledBinary..." -ForegroundColor Cyan
        Copy-Item -Path $releaseBinary -Destination $InstalledBinary -Force
    } else {
        Write-Host "==> Runtime binary at $InstalledBinary is already up to date." -ForegroundColor Green
    }

    # Generate run-service.cmd
    # Sets storage and path environment variables, then launches lighting.exe serve
    $serviceContent = @"
@echo off
set LIGHTING_STORAGE=embedded-surrealkv
set LIGHTING_SURREAL_PATH=$DataDir
set LIGHTING_SERVICE_URL=$ServiceUrl
"$InstalledBinary" serve >> "$ServiceLog" 2>> "$ServiceErrLog"
"@
    Set-Content -Path $ServiceScript -Value $serviceContent -Encoding ASCII -Force

    # Generate run-bridge.cmd
    # Enforces explicit actor authority environment and structurally correct CLI syntax
    $pushFlag = if ($Push) { " --push" } else { "" }
    $bridgeContent = @"
@echo off
set LANTERN_BRIDGE_ALLOW_AGENTS=$AllowAgents
set LANTERN_BRIDGE_ASK_AGENTS=$AskAgents
set LIGHTING_SERVICE_URL=$ServiceUrl
set LANTERN_BRIDGE_STATE_PATH=$RuntimeDir\bridge-state.json
"$InstalledBinary" --service-url $ServiceUrl bridge github once --post-repo "$PostRepo" --git-repo "$GitRepo" --state-path "$RuntimeDir\bridge-state.json"$pushFlag >> "$BridgeLog" 2>> "$BridgeErrLog"
"@
    Set-Content -Path $BridgeScript -Value $bridgeContent -Encoding ASCII -Force
}

function Register-Service-Task {
    Write-Host "==> Registering Windows Scheduled Task: $ServiceTaskName..." -ForegroundColor Cyan
    
    # Clean up legacy task names if present
    Get-ScheduledTask -TaskName "LightingService" -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false

    $currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    $action = New-ScheduledTaskAction -Execute "cmd.exe" -Argument "/c `"$ServiceScript`"" -WorkingDirectory $ProjectRoot
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $currentUser
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)

    Register-ScheduledTask -TaskName $ServiceTaskName -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null
    Write-Host "Service task $ServiceTaskName registered successfully." -ForegroundColor Green
}

function Register-Bridge-Task {
    Write-Host "==> Registering Windows Scheduled Task: $BridgeTaskName..." -ForegroundColor Cyan

    # Clean up legacy task name if present
    Get-ScheduledTask -TaskName "LanternGitHubBridge" -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false

    $action = New-ScheduledTaskAction -Execute "cmd.exe" -Argument "/c `"$BridgeScript`"" -WorkingDirectory $ProjectRoot
    $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes $BridgeIntervalMinutes)
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable

    Register-ScheduledTask -TaskName $BridgeTaskName -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null
    Write-Host "Bridge task $BridgeTaskName registered successfully (every $BridgeIntervalMinutes min)." -ForegroundColor Green
}

function Wait-For-Service-Healthy {
    param([int]$TimeoutSeconds = 15)
    $readyUrl = "$($ServiceUrl.TrimEnd('/'))/health/ready"
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)

    Write-Host "Waiting for Lighting service at $readyUrl..." -NoNewline
    while ((Get-Date) -lt $deadline) {
        try {
            $resp = Invoke-RestMethod -Uri $readyUrl -Method Get -TimeoutSec 2 -ErrorAction Stop
            if ($resp.ready -eq $true) {
                Write-Host " READY." -ForegroundColor Green
                return $true
            }
        } catch {
            Start-Sleep -Milliseconds 500
            Write-Host "." -NoNewline
        }
    }
    Write-Host " TIMEOUT." -ForegroundColor Red
    return $false
}

function Start-Runtime {
    Write-Host "==> Starting Lighting service ($ServiceTaskName)..." -ForegroundColor Cyan
    $task = Get-ScheduledTask -TaskName $ServiceTaskName -ErrorAction SilentlyContinue
    if ($task) {
        Start-ScheduledTask -TaskName $ServiceTaskName
    } else {
        Write-Warning "Task $ServiceTaskName is not registered. Starting via background process..."
        Start-Process -FilePath "cmd.exe" -ArgumentList "/c `"$ServiceScript`"" -WindowStyle Hidden
    }

    $healthy = Wait-For-Service-Healthy -TimeoutSeconds 15
    if (-not $healthy) {
        Write-Warning "Lighting service did not report ready within timeout. Check $ServiceErrLog"
    }

    Write-Host "==> Enabling and triggering Bridge task ($BridgeTaskName)..." -ForegroundColor Cyan
    $bridgeTask = Get-ScheduledTask -TaskName $BridgeTaskName -ErrorAction SilentlyContinue
    if ($bridgeTask) {
        Enable-ScheduledTask -TaskName $BridgeTaskName -ErrorAction SilentlyContinue | Out-Null
        Start-ScheduledTask -TaskName $BridgeTaskName
    }
}

function Stop-Runtime {
    Write-Host "==> Stopping Bridge task ($BridgeTaskName)..." -ForegroundColor Cyan
    Get-ScheduledTask -TaskName $BridgeTaskName -ErrorAction SilentlyContinue | Stop-ScheduledTask -ErrorAction SilentlyContinue

    Write-Host "==> Stopping Service task ($ServiceTaskName)..." -ForegroundColor Cyan
    Get-ScheduledTask -TaskName $ServiceTaskName -ErrorAction SilentlyContinue | Stop-ScheduledTask -ErrorAction SilentlyContinue

    # Ensure any lighting.exe process executing from $RuntimeDir is stopped
    $procs = Get-CimInstance Win32_Process -Filter "Name = 'lighting.exe'" -ErrorAction SilentlyContinue
    foreach ($p in $procs) {
        if ($p.ExecutablePath -and $p.ExecutablePath.StartsWith($RuntimeDir, [System.StringComparison]::OrdinalIgnoreCase)) {
            Write-Host "Stopping runtime lighting process (PID: $($p.ProcessId))..."
            Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }

    # Also stop any running processes started from target-bridge-test if applicable
    foreach ($p in $procs) {
        if ($p.CommandLine -match "target-bridge-test") {
            Write-Host "Stopping legacy test lighting process (PID: $($p.ProcessId))..."
            Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Seconds 1
}

function Get-Runtime-Status {
    $serviceReachable = $false
    $serviceVersion = $null
    $serviceReason = $null
    $readyUrl = "$($ServiceUrl.TrimEnd('/'))/health/ready"
    $versionUrl = "$($ServiceUrl.TrimEnd('/'))/api/v1/version"

    try {
        $readyResp = Invoke-RestMethod -Uri $readyUrl -Method Get -TimeoutSec 2 -ErrorAction Stop
        if ($readyResp.ready -eq $true) {
            $serviceReachable = $true
            $serviceReason = $readyResp.reason
        }
    } catch {
        $serviceReason = $_.Exception.Message
    }

    $datastoreModeObserved = $null
    $surrealdbExpectedVersion = $null
    $surrealdbObservedVersion = $null
    if ($serviceReachable) {
        try {
            $verResp = Invoke-RestMethod -Uri $versionUrl -Method Get -TimeoutSec 2 -ErrorAction Stop
            $serviceVersion = $verResp.version
            $datastoreModeObserved = $verResp.datastore_mode
            $surrealdbExpectedVersion = $verResp.surrealdb_expected_version
            $surrealdbObservedVersion = $verResp.surrealdb_observed_version
        } catch { }
    }

    # Authority probe
    $authorityReachable = $false
    try {
        $body = @{
            check = @{
                request = @{
                    action_id = "runtime-status-probe"
                    principal_id = "agent:status-probe"
                    capability_id = "lantern.memory.create"
                    capability_version = "1"
                    scope = @{}
                    constraints = @{}
                }
                at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
            }
        } | ConvertTo-Json -Depth 5
        $authResp = Invoke-RestMethod -Uri "$($ServiceUrl.TrimEnd('/'))/api/v1/authority/check" -Method Post -ContentType "application/json" -Body $body -TimeoutSec 2 -ErrorAction Stop
        if ($authResp.decision) {
            $authorityReachable = $true
        }
    } catch { }

    # Tethers engine check
    $tethersEnginePath = $null
    $tethersCandidate = "D:\Projects\Tethers\tethers-lang\tethers-0.1\engine-ocaml\_build\install\default\bin\tethers_engine.exe"
    if ($env:TETHERS_ENGINE_PATH -and (Test-Path $env:TETHERS_ENGINE_PATH)) {
        $tethersEnginePath = $env:TETHERS_ENGINE_PATH
    } elseif (Test-Path $tethersCandidate) {
        $tethersEnginePath = $tethersCandidate
    }
    $tethersEnginePresent = ($null -ne $tethersEnginePath)

    # Scheduled Tasks
    $svcTask = Get-ScheduledTask -TaskName $ServiceTaskName -ErrorAction SilentlyContinue
    $svcTaskInfo = if ($svcTask) { Get-ScheduledTaskInfo -TaskName $ServiceTaskName } else { $null }

    $brTask = Get-ScheduledTask -TaskName $BridgeTaskName -ErrorAction SilentlyContinue
    $brTaskInfo = if ($brTask) { Get-ScheduledTaskInfo -TaskName $BridgeTaskName } else { $null }
    $bridgeTaskInstalled = ($null -ne $brTask)
    $bridgeTaskState = if ($brTask) { $brTask.State.ToString() } else { "NotInstalled" }
    $bridgeTaskHealthy = ($bridgeTaskInstalled -and ($bridgeTaskState -ne "Disabled"))

    # Bridge state
    $bridgeStateFile = Join-Path $RuntimeDir "bridge-state.json"
    if (-not (Test-Path $bridgeStateFile)) {
        $bridgeStateFile = Join-Path $ProjectRoot ".lighting-runtime\bridge-state.json"
    }
    $bridgeState = if (Test-Path $bridgeStateFile) {
        Get-Content $bridgeStateFile -Raw | ConvertFrom-Json
    } else { $null }

    # Git queue info from lantern-post
    $postRepoValid = $false
    $inboxHead = $null
    $checkpoint = if ($bridgeState) { $bridgeState.last_processed_inbox_commit } else { $null }
    $pendingCount = 0
    if (Test-Path (Join-Path $PostRepo ".git")) {
        try {
            $originUrl = (git -C $PostRepo remote get-url origin 2>$null)
            if ($originUrl) { $originUrl = $originUrl.Trim() }
            $branches = (git -C $PostRepo branch -a 2>$null)
            $hasInbox = ($branches -match "inbox")
            $hasReceipts = ($branches -match "receipts")
            if ($originUrl -like "*matthewjameswatkins1978-cyber/lantern-post*" -and $hasInbox -and $hasReceipts) {
                $postRepoValid = $true
            }
            $inboxHead = (git -C $PostRepo rev-parse refs/heads/inbox 2>$null)
            if ($inboxHead) {
                $inboxHead = $inboxHead.Trim()
                if ($checkpoint) {
                    $commits = git -C $PostRepo rev-list "$checkpoint..$inboxHead" 2>$null
                    $pendingCount = if ($commits) { ($commits -split "`n").Count } else { 0 }
                } else {
                    $commits = git -C $PostRepo rev-list $inboxHead 2>$null
                    $pendingCount = if ($commits) { ($commits -split "`n").Count } else { 0 }
                }
            }
        } catch { }
    }

    # Mirror status
    $mirrorStatusPath = Join-Path $GitRepo "mirror\status.json"
    $mirrorStatus = if (Test-Path $mirrorStatusPath) {
        Get-Content $mirrorStatusPath -Raw | ConvertFrom-Json
    } else { $null }

    # Running processes
    $runningProcs = Get-CimInstance Win32_Process -Filter "Name = 'lighting.exe'" -ErrorAction SilentlyContinue |
        Select-Object ProcessId, ExecutablePath, CommandLine

    $mutationReady = ($serviceReachable -and `
                      $authorityReachable -and `
                      $bridgeTaskHealthy -and `
                      $postRepoValid -and `
                      ($null -eq $bridgeState.last_terminal_error) -and `
                      ($null -ne $inboxHead))

    $statusObj = [PSCustomObject]@{
        timestamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        service = [PSCustomObject]@{
            task_name = $ServiceTaskName
            task_state = if ($svcTask) { $svcTask.State.ToString() } else { "NotInstalled" }
            last_run = if ($svcTaskInfo) { $svcTaskInfo.LastRunTime } else { $null }
            last_result = if ($svcTaskInfo) { $svcTaskInfo.LastTaskResult } else { $null }
            reachable = $serviceReachable
            version = $serviceVersion
            url = $ServiceUrl
            datastore_mode_observed = $datastoreModeObserved
            datastore_mode_configured = "embedded-surrealkv"
            datastore_path_configured = $DataDir
            surrealdb_expected_version = if ($surrealdbExpectedVersion) { $surrealdbExpectedVersion } else { "3.3.0" }
            surrealdb_observed_version = $surrealdbObservedVersion
        }
        authority = [PSCustomObject]@{
            authority_path_reachable = $authorityReachable
            tethers_engine_present = $tethersEnginePresent
            tethers_engine_path = $tethersEnginePath
            allowed_agents = $AllowAgents
        }
        bridge = [PSCustomObject]@{
            task_name = $BridgeTaskName
            task_installed = $bridgeTaskInstalled
            task_state = $bridgeTaskState
            task_healthy = $bridgeTaskHealthy
            last_run = if ($brTaskInfo) { $brTaskInfo.LastRunTime } else { $null }
            last_result = if ($brTaskInfo) { $brTaskInfo.LastTaskResult } else { $null }
            next_run = if ($brTaskInfo) { $brTaskInfo.NextRunTime } else { $null }
            post_repo = $PostRepo
            post_repo_valid = $postRepoValid
            git_repo = $GitRepo
            inbox_head = $inboxHead
            checkpoint = $checkpoint
            pending_count = $pendingCount
            last_cycle_at = if ($bridgeState) { $bridgeState.last_cycle_at } else { $null }
            last_success_at = if ($bridgeState) { $bridgeState.last_success_at } else { $null }
            last_terminal_error = if ($bridgeState) { $bridgeState.last_terminal_error } else { $null }
        }
        mirror = [PSCustomObject]@{
            generated_at = if ($mirrorStatus) { $mirrorStatus.generated_at } else { $null }
            records_count = if ($mirrorStatus) { if ($mirrorStatus.record_count -ne $null) { $mirrorStatus.record_count } else { $mirrorStatus.counts.records } } else { $null }
        }
        runtime_binary = [PSCustomObject]@{
            installed_path = $InstalledBinary
            exists = (Test-Path $InstalledBinary)
            running_processes = $runningProcs
        }
        mutation_ready = $mutationReady
    }

    if ($Json) {
        $statusObj | ConvertTo-Json -Depth 6
        return
    }

    Write-Host "==========================================================" -ForegroundColor Cyan
    Write-Host "            LANTERN KEEPER RUNTIME STATUS                 " -ForegroundColor Cyan
    Write-Host "==========================================================" -ForegroundColor Cyan
    Write-Host "Timestamp:                   $($statusObj.timestamp)"

    Write-Host "`n--- Lighting Service ---" -ForegroundColor Yellow
    Write-Host "Task Status:                 $($statusObj.service.task_name) [$($statusObj.service.task_state)]"
    Write-Host "Service Reachable:           $(if ($statusObj.service.reachable) { 'YES' } else { 'NO (' + $serviceReason + ')' })"
    Write-Host "Service URL:                 $($statusObj.service.url)"
    Write-Host "Service Version:             $($statusObj.service.version)"
    Write-Host "Datastore Mode (observed):   $(if ($statusObj.service.datastore_mode_observed) { $statusObj.service.datastore_mode_observed } else { '<unreachable or unknown>' })"
    Write-Host "Datastore Mode (configured): $($statusObj.service.datastore_mode_configured)"
    Write-Host "Datastore Path (configured): $($statusObj.service.datastore_path_configured)"
    Write-Host "SurrealDB Expected Version:  $($statusObj.service.surrealdb_expected_version)"
    Write-Host "SurrealDB Observed Version:  $(if ($statusObj.service.surrealdb_observed_version) { $statusObj.service.surrealdb_observed_version } else { '<unreachable or unknown>' })"

    Write-Host "`n--- Authority Subsystem ---" -ForegroundColor Yellow
    Write-Host "Authority Reachable:         $(if ($statusObj.authority.authority_path_reachable) { 'YES (reachable & responding)' } else { 'NO (unreachable)' })"
    Write-Host "Tethers Engine:              $(if ($statusObj.authority.tethers_engine_present) { 'YES (binary present)' } else { 'NO' })"
    Write-Host "Tethers Path:                $($statusObj.authority.tethers_engine_path)"
    Write-Host "Allowed Agents:              $($statusObj.authority.allowed_agents)"

    Write-Host "`n--- GitHub Bridge Transport ---" -ForegroundColor Yellow
    Write-Host "Bridge Task Installed:       $(if ($statusObj.bridge.task_installed) { 'YES' } else { 'NO' })"
    Write-Host "Bridge Task State:           $($statusObj.bridge.task_state)"
    Write-Host "Bridge Last Result:          $($statusObj.bridge.last_result)"
    Write-Host "Bridge Last Successful Cycle:$($statusObj.bridge.last_success_at)"
    Write-Host "Post Repository Valid:       $(if ($statusObj.bridge.post_repo_valid) { 'YES' } else { 'NO' })"
    Write-Host "Post Repository Path:        $($statusObj.bridge.post_repo)"
    Write-Host "Inbox Head:                  $($statusObj.bridge.inbox_head)"
    Write-Host "Durable Checkpoint:          $($statusObj.bridge.checkpoint)"
    Write-Host "Pending Count:               $($statusObj.bridge.pending_count)"
    Write-Host "Last Run:                    $($statusObj.bridge.last_run)"
    Write-Host "Next Run:                    $($statusObj.bridge.next_run)"
    Write-Host "Last Cycle:                  $($statusObj.bridge.last_cycle_at)"
    if ($statusObj.bridge.last_terminal_error) {
        Write-Host "Last Terminal Error:         $($statusObj.bridge.last_terminal_error)" -ForegroundColor Red
    }

    Write-Host "`n--- Mirror (lantern-git) ---" -ForegroundColor Yellow
    Write-Host "Mirror Last Gen:             $($statusObj.mirror.generated_at)"
    Write-Host "Mirror Records:              $($statusObj.mirror.records_count)"

    Write-Host "`n--- Overall Health ---" -ForegroundColor Yellow
    Write-Host "Mutation Ready:              $(if ($statusObj.mutation_ready) { 'YES (healthy)' } else { 'NO (blocked or unready)' })" -ForegroundColor $(if ($statusObj.mutation_ready) { "Green" } else { "Red" })
    Write-Host "Installed Binary:            $($statusObj.runtime_binary.installed_path)"
    Write-Host "==========================================================" -ForegroundColor Cyan
}

function Run-Doctor {
    if (-not (Test-Path $InstalledBinary)) {
        Write-Warning "Installed binary not found at $InstalledBinary. Checking target/release..."
        $fallback = Join-Path $ProjectRoot "target\release\lighting.exe"
        if (-not (Test-Path $fallback)) {
            $fallback = Join-Path $ProjectRoot "target\debug\lighting.exe"
        }
        if (Test-Path $fallback) {
            $binToRun = $fallback
        } else {
            throw "No lighting binary found. Please run 'lantern-runtime.ps1 install' first."
        }
    } else {
        $binToRun = $InstalledBinary
    }

    $doctorArgs = @("--service-url", $ServiceUrl, "bridge", "github", "doctor", "--post-repo", $PostRepo, "--git-repo", $GitRepo)
    if ($Json) {
        $doctorArgs += "--json"
    }
    & $binToRun @doctorArgs
}

function Update-Runtime {
    Write-Host "==> Updating Lantern Keeper Runtime..." -ForegroundColor Cyan
    
    # 1. Build new release binary first (while old runtime is still running!)
    Build-Release-Binary

    $newBinary = Join-Path $ProjectRoot "target\release\lighting.exe"
    if (-not (Test-Path $newBinary)) {
        throw "Build succeeded but $newBinary not found!"
    }

    # 2. Stop running runtime cleanly
    Stop-Runtime

    # 3. Controlled swap
    $backupBinary = "$InstalledBinary.old"
    if (Test-Path $InstalledBinary) {
        Move-Item -Path $InstalledBinary -Destination $backupBinary -Force
    }
    try {
        Copy-Item -Path $newBinary -Destination $InstalledBinary -Force
        Install-Runtime-Files # Regenerate scripts with current parameters
        Start-Runtime
        Write-Host "==> Update successful and runtime restarted." -ForegroundColor Green
    } catch {
        Write-Error "Update failed: $_. Rolling back..."
        if (Test-Path $backupBinary) {
            Move-Item -Path $backupBinary -Destination $InstalledBinary -Force
            Start-Runtime
        }
        throw
    }
}

function Uninstall-Runtime {
    Write-Host "==> Uninstalling Lantern Keeper Runtime..." -ForegroundColor Cyan
    Stop-Runtime

    # Unregister scheduled tasks
    Get-ScheduledTask -TaskName $BridgeTaskName -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false
    Get-ScheduledTask -TaskName $ServiceTaskName -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false
    Get-ScheduledTask -TaskName "LanternGitHubBridge" -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false

    # Remove installed bin files
    if (Test-Path $BinDir) {
        Remove-Item -Path $BinDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "==> Runtime tasks and binaries removed." -ForegroundColor Green
    Write-Host "NOTE: Datastore ($DataDir) and logs have been preserved." -ForegroundColor Yellow
}

# Main Dispatch
switch ($Action) {
    'install' {
        Install-Runtime-Files
        Register-Service-Task
        Register-Bridge-Task
        if (-not $NoStart) {
            Start-Runtime
            Get-Runtime-Status
        }
    }
    'start' {
        Start-Runtime
        Get-Runtime-Status
    }
    'stop' {
        Stop-Runtime
    }
    'restart' {
        Stop-Runtime
        Start-Sleep -Seconds 2
        Start-Runtime
        Get-Runtime-Status
    }
    'status' {
        Get-Runtime-Status
    }
    'doctor' {
        Run-Doctor
    }
    'update' {
        Update-Runtime
        Get-Runtime-Status
    }
    'uninstall' {
        Uninstall-Runtime
    }
}
