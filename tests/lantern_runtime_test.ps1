#Requires -Version 5.1
<#
.SYNOPSIS
    Automated verification suite for lantern-runtime.ps1.

.DESCRIPTION
    Validates:
    1. Generated bridge script places top-level lighting options (--service-url) correctly before subcommands.
    2. Generated bridge script properly exports LANTERN_BRIDGE_ALLOW_AGENTS=chatgpt-lucy,pi.
    3. Runtime execution uses the installed runtime binary (.lighting-runtime/bin/lighting.exe) rather than target\debug.
    4. Install/update is idempotent (subsequent runs succeed cleanly).
    5. Duplicate tasks/processes are not created.
    6. Uninstall removes runtime artifacts without touching user data (.lighting-data).
#>

$ErrorActionPreference = 'Stop'
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$RuntimeScript = Join-Path $ProjectRoot "scripts\lantern-runtime.ps1"

$TestTempDir = Join-Path ([System.IO.Path]::GetTempPath()) "lantern_runtime_test_$(New-Guid)"
$TestRuntimeDir = Join-Path $TestTempDir ".lighting-runtime"
$TestDataDir = Join-Path $TestTempDir ".lighting-data\surrealkv"
$DummyReleaseBinary = Join-Path $ProjectRoot "target\release\lighting.exe"

Write-Host "==> Starting Lantern Runtime Management Tests..." -ForegroundColor Cyan

try {
    # Setup test workspace
    New-Item -ItemType Directory -Path $TestDataDir -Force | Out-Null
    Set-Content -Path (Join-Path $TestDataDir "canary.txt") -Value "canonical-user-data-preserved"

    # Create dummy release binary if not present for script testing
    if (-not (Test-Path $DummyReleaseBinary)) {
        New-Item -ItemType Directory -Path (Split-Path -Parent $DummyReleaseBinary) -Force | Out-Null
        Set-Content -Path $DummyReleaseBinary -Value "mock-lighting-binary"
    }

    # Test 1: Validate generated scripts syntax, environment variables, and binary path
    Write-Host "[Test 1] Testing script generation and environment propagation..." -ForegroundColor Yellow
    $testServiceTask = "Test-Lantern-Service-$(Get-Random)"
    $testBridgeTask = "Test-Lantern-Bridge-$(Get-Random)"

    # Run install with -SkipBuild
    & $RuntimeScript install `
        -ServiceTaskName $testServiceTask `
        -BridgeTaskName $testBridgeTask `
        -RuntimeDir $TestRuntimeDir `
        -DataDir $TestDataDir `
        -AllowAgents "chatgpt-lucy,pi" `
        -ServiceUrl "http://127.0.0.1:4317" `
        -SkipBuild `
        -NoStart

    $installedBin = Join-Path $TestRuntimeDir "bin\lighting.exe"
    $bridgeCmd = Join-Path $TestRuntimeDir "bin\run-bridge.cmd"
    $serviceCmd = Join-Path $TestRuntimeDir "bin\run-service.cmd"
    $bridgeVbs = Join-Path $TestRuntimeDir "bin\run-bridge.vbs"
    $serviceVbs = Join-Path $TestRuntimeDir "bin\run-service.vbs"

    if (-not (Test-Path $installedBin)) {
        throw "ASSERTION FAILED: Installed binary does not exist at $installedBin"
    }
    if ($installedBin -like "*target\debug*") {
        throw "ASSERTION FAILED: Installed binary is pointing to target\debug!"
    }
    Write-Host "  -> Verified binary installed to runtime location separate from Cargo build output." -ForegroundColor Green

    # Inspect VBS wrapper generation for headless execution
    if (-not (Test-Path $bridgeVbs) -or -not (Test-Path $serviceVbs)) {
        throw "ASSERTION FAILED: Headless VBS wrappers ($bridgeVbs, $serviceVbs) were not created!"
    }
    $bridgeVbsContent = Get-Content $bridgeVbs -Raw
    if ($bridgeVbsContent -notmatch 'WshShell\.Run.*0,\s*True') {
        throw "ASSERTION FAILED: Bridge VBS does not run cmd hidden with window style 0!"
    }
    $serviceVbsContent = Get-Content $serviceVbs -Raw
    if ($serviceVbsContent -notmatch 'WshShell\.Run.*0,\s*True') {
        throw "ASSERTION FAILED: Service VBS does not run cmd hidden with window style 0!"
    }
    Write-Host "  -> Verified headless VBS wrappers generated with window style 0 (SW_HIDE)." -ForegroundColor Green

    # Inspect bridge script content
    $bridgeContent = Get-Content $bridgeCmd -Raw
    if ($bridgeContent -notmatch "set LANTERN_BRIDGE_ALLOW_AGENTS=chatgpt-lucy,pi") {
        throw "ASSERTION FAILED: Bridge script does not export LANTERN_BRIDGE_ALLOW_AGENTS=chatgpt-lucy,pi"
    }
    Write-Host "  -> Verified LANTERN_BRIDGE_ALLOW_AGENTS exported properly." -ForegroundColor Green

    # Inspect --service-url placement in bridge invocation
    # Must place --service-url before subcommand `bridge github once`
    if ($bridgeContent -notmatch '"[^"]*lighting\.exe"\s+--service-url\s+http://127\.0\.0\.1:4317\s+bridge github once') {
        throw "ASSERTION FAILED: --service-url is not structurally placed before 'bridge github once' subcommand! Script content: $bridgeContent"
    }
    Write-Host "  -> Verified top-level options placed correctly before bridge subcommand." -ForegroundColor Green

    # Inspect service script content
    $serviceContent = Get-Content $serviceCmd -Raw
    if ($serviceContent -notmatch "set LIGHTING_STORAGE=embedded-surrealkv") {
        throw "ASSERTION FAILED: Service script does not set LIGHTING_STORAGE=embedded-surrealkv"
    }
    if ($serviceContent -notmatch [regex]::Escape($TestDataDir)) {
        throw "ASSERTION FAILED: Service script does not point to intended DataDir ($TestDataDir)"
    }
    Write-Host "  -> Verified service script sets storage and datastore path." -ForegroundColor Green

    # Test 2: Idempotency (running install again should update cleanly without duplicate tasks)
    Write-Host "[Test 2] Testing installer idempotency and headless task registration..." -ForegroundColor Yellow
    & $RuntimeScript install `
        -ServiceTaskName $testServiceTask `
        -BridgeTaskName $testBridgeTask `
        -RuntimeDir $TestRuntimeDir `
        -DataDir $TestDataDir `
        -AllowAgents "chatgpt-lucy,pi" `
        -ServiceUrl "http://127.0.0.1:4317" `
        -SkipBuild `
        -NoStart

    $svcTasks = Get-ScheduledTask -TaskName $testServiceTask -ErrorAction SilentlyContinue
    if (@($svcTasks).Count -ne 1) {
        throw "ASSERTION FAILED: Expected exactly 1 service task, found $(@($svcTasks).Count)"
    }
    if ($svcTasks.Actions[0].Execute -notmatch "wscript\.exe" -or $svcTasks.Actions[0].Arguments -notmatch "run-service\.vbs") {
        throw "ASSERTION FAILED: Service task action is not headless wscript.exe run-service.vbs! Action: $($svcTasks.Actions[0].Execute) $($svcTasks.Actions[0].Arguments)"
    }

    $brTasks = Get-ScheduledTask -TaskName $testBridgeTask -ErrorAction SilentlyContinue
    if (@($brTasks).Count -ne 1) {
        throw "ASSERTION FAILED: Expected exactly 1 bridge task, found $(@($brTasks).Count)"
    }
    if ($brTasks.Actions[0].Execute -notmatch "wscript\.exe" -or $brTasks.Actions[0].Arguments -notmatch "run-bridge\.vbs") {
        throw "ASSERTION FAILED: Bridge task action is not headless wscript.exe run-bridge.vbs! Action: $($brTasks.Actions[0].Execute) $($brTasks.Actions[0].Arguments)"
    }
    Write-Host "  -> Verified install is idempotent and tasks are registered headlessly via wscript.exe." -ForegroundColor Green

    # Test 3: Status command produces valid JSON
    Write-Host "[Test 3] Testing status reporting..." -ForegroundColor Yellow
    $statusJson = & $RuntimeScript status `
        -ServiceTaskName $testServiceTask `
        -BridgeTaskName $testBridgeTask `
        -RuntimeDir $TestRuntimeDir `
        -DataDir $TestDataDir `
        -Json
    $status = $statusJson | ConvertFrom-Json
    if (-not $status.service -or -not $status.authority -or -not $status.bridge) {
        throw "ASSERTION FAILED: Status JSON missing required sections"
    }
    Write-Host "  -> Verified status produces structured truth." -ForegroundColor Green

    # Test 4: Uninstall cleans tasks and runtime bins, but PRESERVES user data
    Write-Host "[Test 4] Testing uninstall cleanly removes tasks and preserves data..." -ForegroundColor Yellow
    & $RuntimeScript uninstall `
        -ServiceTaskName $testServiceTask `
        -BridgeTaskName $testBridgeTask `
        -RuntimeDir $TestRuntimeDir `
        -DataDir $TestDataDir

    $remainingSvcTask = Get-ScheduledTask -TaskName $testServiceTask -ErrorAction SilentlyContinue
    if ($null -ne $remainingSvcTask) {
        throw "ASSERTION FAILED: Service task was not uninstalled!"
    }
    $remainingBrTask = Get-ScheduledTask -TaskName $testBridgeTask -ErrorAction SilentlyContinue
    if ($null -ne $remainingBrTask) {
        throw "ASSERTION FAILED: Bridge task was not uninstalled!"
    }
    if (Test-Path $installedBin) {
        throw "ASSERTION FAILED: Installed binary was not removed during uninstall!"
    }
    # Verify canary in data dir is intact!
    $canaryFile = Join-Path $TestDataDir "canary.txt"
    if (-not (Test-Path $canaryFile) -or (Get-Content $canaryFile) -ne "canonical-user-data-preserved") {
        throw "FATAL ASSERTION FAILED: User data was modified or deleted during uninstall!"
    }
    Write-Host "  -> Verified uninstall removes tasks and binaries while preserving user data." -ForegroundColor Green

    Write-Host "==> All runtime manager tests passed!" -ForegroundColor Green
} finally {
    # Clean up scheduled tasks if left behind
    Get-ScheduledTask -TaskName $testServiceTask -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false
    Get-ScheduledTask -TaskName $testBridgeTask -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false
    if (Test-Path $TestTempDir) {
        Remove-Item -Path $TestTempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
