$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$RuntimeDir = Join-Path $ProjectRoot ".lighting-runtime"
$PidFile = Join-Path $RuntimeDir "surrealdb.json"
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

    return $null
}

$PortOwner = Get-NetTCPConnection -LocalAddress 127.0.0.1 -LocalPort 8000 -ErrorAction SilentlyContinue |
    Where-Object { $_.State -eq "Listen" } |
    Select-Object -First 1

$Record = $null
if (Test-Path -LiteralPath $PidFile) {
    $Record = Get-Content -LiteralPath $PidFile -Raw | ConvertFrom-Json
}

if (-not $Record) {
    if ($PortOwner) {
        $OwnerProcess = Get-Process -Id $PortOwner.OwningProcess -ErrorAction SilentlyContinue
        $OwnerName = if ($OwnerProcess) { $OwnerProcess.ProcessName } else { "unknown" }
        Write-Host "Port 8000 is occupied by PID $($PortOwner.OwningProcess) ($OwnerName), but no Lighting PID file exists."
        exit 2
    }

    Write-Host "Lighting SurrealDB is not running."
    exit 1
}

$Process = Get-Process -Id $Record.pid -ErrorAction SilentlyContinue
if (-not $Process) {
    Write-Host "Lighting PID file is stale; PID $($Record.pid) is not running."
    exit 1
}

if (-not $PortOwner -or $PortOwner.OwningProcess -ne $Record.pid) {
    Write-Host "Lighting SurrealDB process PID $($Record.pid) exists, but it is not listening on 127.0.0.1:8000."
    exit 3
}

$Surreal = Find-Surreal
if (-not $Surreal) {
    Write-Host "Lighting SurrealDB PID $($Record.pid) is listening, but surreal.exe was not found for health check."
    exit 3
}

& $Surreal is-ready --endpoint $Endpoint --log none *> $null
if ($LASTEXITCODE -ne 0) {
    Write-Host "Lighting SurrealDB PID $($Record.pid) is listening, but is-ready failed."
    exit 3
}

Write-Host "Lighting SurrealDB is running on 127.0.0.1:8000 with PID $($Record.pid)."
Write-Host "Data directory: $($Record.data_dir)"
exit 0
