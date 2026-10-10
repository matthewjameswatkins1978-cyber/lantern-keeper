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
    Assert-True ($scriptText -match "throw `"Stop-Runtime failed") "Stop-Runtime throws on surviving processes (static guard, see T4b for behavior)"

    # ---- T4b: stop throws on a genuinely surviving process (behavioral) ----
    # Dot-source a dispatch-stripped copy of the runtime script and shadow
    # Stop-Process with a no-op mock, so a REAL scratch service process
    # survives the reap and the bounded wait must throw. Temp scaffolding
    # only; the product script is untouched by this technique.
    Write-Host "[T4b] Stop-failure behavior..." -ForegroundColor Yellow
    & $RuntimeScript start -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
        -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" 2>&1 | Out-Null
    $victimPid = (Scratch-Procs | Select-Object -First 1).ProcessId
    Assert-True ($victimPid -gt 0) "scratch service running for survivor test (PID $victimPid)"
    $rawScript = Get-Content $RuntimeScript -Raw
    $dispatchIdx = $rawScript.IndexOf('switch ($Action)')
    if ($dispatchIdx -lt 0) { throw "Test anchor lost: action dispatch switch not found in runtime script" }
    $stripped = Join-Path $TestTempDir "runtime-nodispatch.ps1"
    $rawScript.Substring(0, $dispatchIdx) | Out-File $stripped -Encoding UTF8
    $script:mockKills = 0
    function Stop-Process { param($Id, [switch]$Force) $script:mockKills++; Write-Host "  (mock kill suppressed for PID $Id)" }
    # Dot-sourcing runs the stripped file's top-level statements in THIS scope,
    # clobbering $ScriptDir/$ProjectRoot: snapshot and restore them.
    $savedScriptDir = $ScriptDir; $savedProjectRoot = $ProjectRoot
    . $stripped -Action status -RuntimeDir $fwdRuntime -DataDir $fwdData `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask
    $ScriptDir = $savedScriptDir; $ProjectRoot = $savedProjectRoot
    $stopThrew = $false
    try { Stop-Runtime } catch { $stopThrew = ($_.Exception.Message -match "still alive") }
    Remove-Item function:Stop-Process
    Assert-True ($script:mockKills -ge 1) "kill was attempted before giving up ($($script:mockKills) attempt(s))"
    Assert-True ((Get-Process -Id $victimPid -ErrorAction SilentlyContinue) -ne $null) "process genuinely survived (mock suppressed the kill)"
    Assert-True $stopThrew "Stop-Runtime threw on the surviving process"
    & $RuntimeScript stop -RuntimeDir $fwdRuntime -DataDir $fwdData `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask 2>&1 | Out-Null
    Start-Sleep 3
    Assert-True ((Scratch-Procs | Measure-Object).Count -eq 0) "real stop reaps after mock removed"

    # ---- T5/T6: readiness failure blocks success; rollback preserves binary+canary ----
    Write-Host "[T5/T6] Update readiness failure + rollback..." -ForegroundColor Yellow
    & $RuntimeScript start -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
        -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
        -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" 2>&1 | Out-Null
    # Plant a distinguishable OLD binary: the rc01-worktree release build has
    # identical sources but a different hash (worktree paths embedded). This
    # proves the rollback restored rather than never swapping.
    $distinctOld = "D:/Projects/lantern-rc01/target/release/lighting.exe"
    if (-not (Test-Path $distinctOld)) { throw "Distinct old binary missing at $distinctOld" }
    $installedBin = Join-Path $TestRuntimeDir "bin\lighting.exe"
    $backupBin = "$installedBin.old"
    # Unregister scratch tasks first: the service task is registered with
    # RestartCount=3/1min, so Task Scheduler would otherwise restart the
    # killed service mid-proof and re-lock the binary.
    Get-ScheduledTask -TaskName $SvcTask -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false
    Get-ScheduledTask -TaskName $BrdTask -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false
    Start-Sleep 3
    foreach ($p in (Scratch-Procs)) { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep 3
    Assert-True ((Scratch-Procs | Measure-Object).Count -eq 0) "no service to auto-restart during plant"
    # The planted copy may hit a transient lock (AV/indexer) right after the
    # stop; retry briefly rather than failing the proof on it.
    $copied = $false
    for ($i = 0; $i -lt 6 -and -not $copied; $i++) {
        try { Copy-Item $distinctOld $installedBin -Force -ErrorAction Stop; $copied = $true } catch { Start-Sleep 5 }
    }
    Assert-True $copied "distinct old binary planted"
    $hashD = (Get-FileHash $installedBin).Hash
    $hashR = (Get-FileHash (Join-Path $ProjectRoot "target\release\lighting.exe")).Hash
    Assert-True ($hashD -ne $hashR) "old/new artifacts distinguishable ($($hashD.Substring(0,12))… vs $($hashR.Substring(0,12))…)"
    Remove-Item $backupBin -ErrorAction SilentlyContinue
    $dummy = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $TestPort)
    try { $dummy.Start() } catch { throw "Test port $TestPort unexpectedly occupied: $_" }
    $updateExit = 0
    $updateLog = Join-Path $TestTempDir "update-allstreams.log"
    try {
        # *> captures ALL streams (success/error/warning) to file: avoids
        # in-memory merge subtleties hiding the rollback diagnostic.
        & $RuntimeScript update -RuntimeDir $TestRuntimeDir -DataDir $TestDataDir `
            -ServiceUrl $TestUrl -ServiceTaskName $SvcTask -BridgeTaskName $BrdTask `
            -BridgeIntervalMinutes 720 -PostRepo "$TestTempDir/post" -GitRepo "$TestTempDir/git" `
            -SkipBuild *> $updateLog
    } catch { $updateExit = 1 }
    $updateOut = Get-Content $updateLog -Raw -ErrorAction SilentlyContinue
    if ($updateExit -ne 0) { $updateOut | Out-File C:\Users\Matmus\AppData\Local\Temp\update-failure-output.txt -Encoding UTF8 }
    Assert-True ($updateExit -ne 0) "update with unready service exits nonzero (no success receipt)"
    Assert-True ($updateOut -match "Rolling back") "rollback path executed (diagnostic observed)"
    $postHash = (Get-FileHash $installedBin).Hash
    Assert-True ($postHash -eq $hashD) "rollback restored the pre-update binary ($($hashD.Substring(0,12))…)"
    Assert-True (-not (Test-Path $backupBin)) "backup consumed by restore (no stale .old)"
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
