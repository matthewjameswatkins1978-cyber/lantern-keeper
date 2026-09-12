# Lantern Keeper — Local First Proof Runner
# -----------------------------------------------------------------------------
# Starts Lighting temporarily, runs the first-proof demo, then stops Lighting.
# Requires SurrealDB to already be running. Use scripts/start-surreal.ps1 first.

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path "$ScriptDir\.."
$RuntimeDir = "$RepoRoot\.lighting-runtime"
$TargetDir = ".lighting-runtime\target-demo"
$StdoutLogFile = "$RuntimeDir\lighting-demo.out.log"
$StderrLogFile = "$RuntimeDir\lighting-demo.err.log"

New-Item -ItemType Directory -Force -Path $RuntimeDir | Out-Null

function Find-FreeLightingPort {
    param([int]$StartPort = 4317)

    for ($Port = $StartPort; $Port -lt ($StartPort + 20); $Port++) {
        $Owner = Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort $Port -ErrorAction SilentlyContinue |
            Where-Object { $_.State -eq "Listen" } |
            Select-Object -First 1
        if (-not $Owner) {
            return $Port
        }
    }

    throw "No free localhost port found from $StartPort to $($StartPort + 19)."
}

$Port = Find-FreeLightingPort
$ServiceUrl = "http://127.0.0.1:$Port"
$OldLightingPort = $env:LIGHTING_PORT
$env:LIGHTING_PORT = [string]$Port

Write-Host "Building Lighting demo binary ..."
& cargo build --target-dir $TargetDir -p lighting --bin lighting
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build Lighting demo binary."
}

$LightingExe = Resolve-Path "$TargetDir\debug\lighting.exe"

Write-Host "Starting temporary Lighting service on $ServiceUrl ..."
$LightingProcess = Start-Process `
    -FilePath $LightingExe `
    -ArgumentList @("serve") `
    -WorkingDirectory $RepoRoot `
    -WindowStyle Hidden `
    -RedirectStandardOutput $StdoutLogFile `
    -RedirectStandardError $StderrLogFile `
    -PassThru

try {
    $Deadline = (Get-Date).AddSeconds(180)
    $Ready = $false

    do {
        Start-Sleep -Milliseconds 800

        if ($LightingProcess.HasExited) {
            $StdoutLog = if (Test-Path -LiteralPath $StdoutLogFile) { Get-Content -LiteralPath $StdoutLogFile -Raw } else { "" }
            $StderrLog = if (Test-Path -LiteralPath $StderrLogFile) { Get-Content -LiteralPath $StderrLogFile -Raw } else { "" }
            throw "Lighting exited during startup with code $($LightingProcess.ExitCode).`nSTDOUT:`n$StdoutLog`nSTDERR:`n$StderrLog"
        }

        try {
            $Health = Invoke-RestMethod "$ServiceUrl/health/ready" -TimeoutSec 2
            $Ready = [bool]$Health.ready
        } catch {
            $Ready = $false
        }
    } while (-not $Ready -and (Get-Date) -lt $Deadline)

    if (-not $Ready) {
        throw "Lighting did not become ready within 180 seconds. Check $StdoutLogFile and $StderrLogFile"
    }

    Write-Host "Lighting is ready."
    & "$ScriptDir\demo-first-proof.ps1" -ServiceUrl $ServiceUrl -LightingExe $LightingExe
} finally {
    if (-not $LightingProcess.HasExited) {
        Stop-Process -Id $LightingProcess.Id -ErrorAction SilentlyContinue
        $LightingProcess.WaitForExit(10000) | Out-Null
        Write-Host "Temporary Lighting service stopped."
    }
    $env:LIGHTING_PORT = $OldLightingPort
}
