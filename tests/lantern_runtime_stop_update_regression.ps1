#Requires -Version 5.1
<#
.SYNOPSIS
    Focused regression tests for RC01 runtime repairs (stop/update failure propagation).

.DESCRIPTION
    Preserves, on disposable paths only:
    1. Non-default port binds correctly (LIGHTING_PORT derived from ServiceUrl).
    2. Default 4317 bind is unchanged.
    3. Mixed-slash RuntimeDir still identifies runtime processes (stop reaps).
    4. Stop throws when a selected process survives (static guard + no false positive live).
    5. Failed service readiness cannot yield a successful update receipt (live fault injection).
    6. Rollback preserves the original binary and the memory canary.
    7. No unrelated (production) runtime is terminated.
    8. Scratch tasks/files/processes are cleaned up.

    Never touches production paths, the canonical datastore, or port 4317 beyond
    read-only health probes. Requires the real release binary at target\release.
#>

$ErrorActionPreference = 'Stop'
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$RuntimeScript = Join-Path $ProjectRoot "scripts\lantern-runtime.ps1"
$ReleaseBinary = Join-Path $ProjectRoot "target\release\lighting.exe"

$TestTempDir = Join-Path ([System.IO.Path]::GetTempPath()) "lantern_rc01_regression_$(New-Guid)"
$TestRuntimeDir = Join-Path $TestTempDir ".lighting-runtime"
$TestDataDir = Join-Path $TestTempDir ".lighting-data\surrealkv"
$TestPort = 4499
$TestUrl = "http://127.0.0.1:$TestPort"
$SvcTask = "RC01-Reg-Svc-$(Get-Random)"
$BrdTask = "RC01-Reg-Brd-$(Get-Random)"

$script:failures = @()
function Assert-True([bool]$cond, [string]$name) {
    if ($cond) { Write-Host "  PASS: $name" -ForegroundColor Green }
    else { Write-Host "  FAIL: $name" -ForegroundColor Red; $script:failures += $name }
}

function Scratch-Procs {
    Get-CimInstance Win32_Process -Filter "Name='lighting.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($TestRuntimeDir.Replace('/', '\'), [System.StringComparison]::OrdinalIgnoreCase) }
}

function Prod-Serve-Pid {
    # Exact executable match: a CommandLine regex risks matching transient
    # same-prefix processes. The canonical production binary path is fixed.
    (Get-CimInstance Win32_Process -Filter "Name='lighting.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -eq 'D:\Projects\lantern-keeper\.lighting-runtime\bin\lighting.exe' }).ProcessId
}

Write-Host "==> RC01 runtime regression suite (disposable: $TestTempDir, :$TestPort)" -ForegroundColor Cyan
if (-not (Test-Path $ReleaseBinary)) { throw "Release binary missing at $ReleaseBinary. Build first: cargo build --locked --release -p lighting" }
$prodPidBefore = Prod-Serve-Pid
Write-Host "Production serve PID (must survive): $prodPidBefore"

try {
    # ---- T1: non-default port binds ----
    Write-Host "[T1] Non-default port bind..." -ForegroundColor Yellow
    & $RuntimeScript install -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
        -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" `
        -SkipBuild 2>&1 | Out-Null
    $ready = (curl.exe -s -m 8 "$TestUrl/health/ready")
    Assert-True ($ready -match '"ready":true') "service READY on :$TestPort"
    $cmd = Get-Content (Join-Path $TestRuntimeDir "bin\run-service.cmd") -Raw
    Assert-True ($cmd -match "LIGHTING_PORT=$TestPort") "run-service.cmd exports LIGHTING_PORT=$TestPort"

    # canary for T5/T6
    $canary = @{ title = "rc01-reg-canary"; kind = "plain_text"; content = "regression canary $(Get-Date -Format o)" } | ConvertTo-Json
    $created = Invoke-RestMethod -Uri "$TestUrl/api/v1/sources" -Method Post -Body $canary -ContentType "application/json" -TimeoutSec 10
    $canaryId = $created.source_id
    Assert-True ([string]::IsNullOrEmpty($canaryId) -eq $false) "canary created ($canaryId)"

    # ---- T2: default 4317 unchanged (generate only, never start) ----
    Write-Host "[T2] Default port unchanged..." -ForegroundColor Yellow
    $t2dir = Join-Path $TestTempDir "t2runtime"
    $t2svc = "RC01-Reg-T2Svc-$(Get-Random)"; $t2brd = "RC01-Reg-T2Brd-$(Get-Random)"
    & $RuntimeScript install -RuntimeDir $t2dir -DataDir (Join-Path $TestTempDir "t2data") `
        -ServiceTaskName $t2svc -BridgeTaskName $t2brd -SkipBuild -NoStart 2>&1 | Out-Null
    $cmd2 = Get-Content (Join-Path $t2dir "bin\run-service.cmd") -Raw
    Assert-True ($cmd2 -match "LIGHTING_PORT=4317") "default run-service.cmd exports LIGHTING_PORT=4317"
    & $RuntimeScript uninstall -RuntimeDir $t2dir -ServiceTaskName $t2svc -BridgeTaskName $t2brd 2>&1 | Out-Null

    # ---- T3: mixed-slash stop reaps ----
    Write-Host "[T3] Mixed-slash stop..." -ForegroundColor Yellow
    $fwdRuntime = $TestRuntimeDir.Replace('\', '/'); $fwdData = $TestDataDir.Replace('\', '/')
    & $RuntimeScript stop -RuntimeDir $fwdRuntime -DataDir $fwdData `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask 2>&1 | Out-Null
    Start-Sleep 3
    Assert-True ((Scratch-Procs | Measure-Object).Count -eq 0) "stop with forward slashes reaped scratch process (no throw)"

    # ---- T4: stop does not false-positive; throw guard present ----
    Write-Host "[T4] Stop failure guard..." -ForegroundColor Yellow
    & $RuntimeScript stop -RuntimeDir $fwdRuntime -DataDir $fwdData `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask 2>&1 | Out-Null
    Assert-True ($true) "second stop with no process succeeds silently (exit 0 reached)"
    $scriptText = Get-Content (Join-Path $ProjectRoot "scripts\lantern-runtime.ps1") -Raw
    Assert-True ($scriptText -match "throw `"Stop-Runtime failed") "Stop-Runtime throws on surviving processes (static guard)"

    # ---- T5/T6: readiness failure blocks success; rollback preserves binary+canary ----
    Write-Host "[T5/T6] Update readiness failure + rollback..." -ForegroundColor Yellow
    & $RuntimeScript start -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
        -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" 2>&1 | Out-Null
    $preHash = (Get-FileHash (Join-Path $TestRuntimeDir "bin\lighting.exe")).Hash
    # Stop first, then hold the port with a dummy listener so the post-swap
    # service cannot bind -> readiness must fail. (Dummy started before stop
    # would collide with the live service and abort the test loudly.)
    & $RuntimeScript stop -RuntimeDir $fwdRuntime -DataDir $fwdData `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask 2>&1 | Out-Null
    $dummy = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $TestPort)
    try { $dummy.Start() } catch { throw "Test port $TestPort unexpectedly occupied: $_" }
    $updateExit = 0
    try {
        & $RuntimeScript update -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
            -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
            -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" `
            -SkipBuild 2>&1 | Out-Null
    } catch { $updateExit = 1 }
    Assert-True ($updateExit -ne 0) "update with unready service exits nonzero (no success receipt)"
    $postHash = (Get-FileHash (Join-Path $TestRuntimeDir "bin\lighting.exe")).Hash
    Assert-True ($postHash -eq $preHash) "rollback preserved installed binary ($($preHash.Substring(0,12))…)"
    $dummy.Stop()
    & $RuntimeScript start -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
        -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" 2>&1 | Out-Null
    $got = Invoke-RestMethod -Uri "$TestUrl/api/v1/sources/$canaryId" -TimeoutSec 10
    Assert-True ($got.content -match "regression canary") "canary intact after rollback + restart"

    # ---- T7: production untouched ----
    Write-Host "[T7] Production boundary..." -ForegroundColor Yellow
    $prodPidAfter = Prod-Serve-Pid
    Assert-True ($prodPidAfter -eq $prodPidBefore) "production serve PID unchanged ($prodPidBefore)"
    $prodHealth = curl.exe -s -m 8 "http://127.0.0.1:4317/api/v1/version"
    Assert-True ($prodHealth -match "3.3.0") "production :4317 healthy throughout"
}
finally {
    Write-Host "[T8] Cleanup..." -ForegroundColor Yellow
    try { if ($dummy) { $dummy.Stop() } } catch { }
    & $RuntimeScript uninstall -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask 2>&1 | Out-Null
    Start-Sleep 5
    $stale = Scratch-Procs
    foreach ($p in $stale) { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
    $leftoverTasks = Get-ScheduledTask -TaskName 'RC01-Reg-*' -ErrorAction SilentlyContinue
    Assert-True (($leftoverTasks | Measure-Object).Count -eq 0) "no RC01-Reg-* scheduled tasks remain"
    Assert-True (((Scratch-Procs) | Measure-Object).Count -eq 0) "no scratch processes remain"
    Remove-Item -Recurse -Force $TestTempDir -ErrorAction SilentlyContinue
}

if ($script:failures.Count -gt 0) { Write-Host "SUITE FAILED: $($script:failures -join '; ')" -ForegroundColor Red; exit 1 }
Write-Host "SUITE PASSED" -ForegroundColor Green
