$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$RuntimeDir = Join-Path $ProjectRoot ".lighting-runtime"
$DataDir = Join-Path $ProjectRoot ".lighting-data\surreal"
$PidFile = Join-Path $RuntimeDir "surrealdb.json"
$StdoutLogFile = Join-Path $RuntimeDir "surrealdb.out.log"
$StderrLogFile = Join-Path $RuntimeDir "surrealdb.err.log"
$Endpoint = "ws://127.0.0.1:8000"

function Find-Surreal {
    $Command = Get-Command surreal -ErrorAction SilentlyContinue
    if ($Command) {
        return $Command.Source
    }

    $LocalPath = Join-Path $env:LOCALAPPDATA "SurrealDB\surreal.exe"
    if (Test-Path -LiteralPath $LocalPath) {
        return $LocalPath
    }

    throw "surreal executable was not found. Install SurrealDB, then reopen PowerShell or add it to PATH."
}

function Get-PortOwner {
    Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort 8000 -ErrorAction SilentlyContinue |
        Where-Object { $_.State -eq "Listen" } |
        Select-Object -First 1
}

New-Item -ItemType Directory -Force -Path $RuntimeDir, $DataDir | Out-Null

$ExistingRecord = $null
if (Test-Path -LiteralPath $PidFile) {
    $ExistingRecord = Get-Content -LiteralPath $PidFile -Raw | ConvertFrom-Json
    $ExistingProcess = Get-Process -Id $ExistingRecord.pid -ErrorAction SilentlyContinue
    if ($ExistingProcess) {
        Write-Host "Lighting SurrealDB already appears to be running with PID $($ExistingRecord.pid)."
        exit 0
    }

    Remove-Item -LiteralPath $PidFile -Force
}

$PortOwner = Get-PortOwner
if ($PortOwner) {
    $OwnerProcess = Get-Process -Id $PortOwner.OwningProcess -ErrorAction SilentlyContinue
    $OwnerName = if ($OwnerProcess) { $OwnerProcess.ProcessName } else { "unknown" }
    throw "Port 8000 is already occupied by PID $($PortOwner.OwningProcess) ($OwnerName). Refusing to start or stop an unrelated process."
}

$Surreal = Find-Surreal
$Arguments = @(
    "start",
    "--no-banner",
    "--bind", "127.0.0.1:8000",
    "--username", "root",
    "--password", "root",
    "--log", "info",
    "`"surrealkv:$DataDir`""
)

$Process = Start-Process -FilePath $Surreal -ArgumentList $Arguments -WorkingDirectory $ProjectRoot -WindowStyle Hidden -RedirectStandardOutput $StdoutLogFile -RedirectStandardError $StderrLogFile -PassThru

$Deadline = (Get-Date).AddSeconds(20)
do {
    Start-Sleep -Milliseconds 500
    if ($Process.HasExited) {
        $StdoutLog = if (Test-Path -LiteralPath $StdoutLogFile) { Get-Content -LiteralPath $StdoutLogFile -Raw } else { "" }
        $StderrLog = if (Test-Path -LiteralPath $StderrLogFile) { Get-Content -LiteralPath $StderrLogFile -Raw } else { "" }
        $Log = "$StdoutLog`n$StderrLog"
        throw "SurrealDB exited during startup with code $($Process.ExitCode). $Log"
    }

    & $Surreal is-ready --endpoint $Endpoint --log none *> $null
    if ($LASTEXITCODE -eq 0) {
        $Record = [ordered]@{
            pid = $Process.Id
            executable = $Surreal
            data_dir = $DataDir
            endpoint = $Endpoint
            started_at = (Get-Date).ToString("o")
        }
        $Record | ConvertTo-Json | Set-Content -LiteralPath $PidFile -Encoding UTF8
        Write-Host "Lighting SurrealDB started on 127.0.0.1:8000 with PID $($Process.Id)."
        Write-Host "Data directory: $DataDir"
        exit 0
    }
} while ((Get-Date) -lt $Deadline)

throw "SurrealDB did not become ready within 20 seconds. Check $StdoutLogFile and $StderrLogFile"
