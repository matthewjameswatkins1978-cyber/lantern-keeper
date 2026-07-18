$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$RuntimeDir = Join-Path $ProjectRoot ".lighting-runtime"
$PidFile = Join-Path $RuntimeDir "surrealdb.json"

if (-not (Test-Path -LiteralPath $PidFile)) {
    Write-Host "No Lighting SurrealDB PID file found. Nothing to stop."
    exit 0
}

$Record = Get-Content -LiteralPath $PidFile -Raw | ConvertFrom-Json
$Process = Get-Process -Id $Record.pid -ErrorAction SilentlyContinue
if (-not $Process) {
    Remove-Item -LiteralPath $PidFile -Force
    Write-Host "Removed stale Lighting SurrealDB PID file."
    exit 0
}

$CimProcess = Get-CimInstance Win32_Process -Filter "ProcessId = $($Record.pid)"
$CommandLine = if ($CimProcess) { $CimProcess.CommandLine } else { "" }
$ExpectedDataDir = [string]$Record.data_dir
$ExpectedExe = [string]$Record.executable

if ($Process.Path -ne $ExpectedExe -or $CommandLine -notlike "*$ExpectedDataDir*") {
    throw "PID $($Record.pid) does not match the Lighting SurrealDB process record. Refusing to stop it."
}

Stop-Process -Id $Record.pid
$Process.WaitForExit(10000) | Out-Null

if (Get-Process -Id $Record.pid -ErrorAction SilentlyContinue) {
    throw "Lighting SurrealDB PID $($Record.pid) did not stop within 10 seconds."
}

Remove-Item -LiteralPath $PidFile -Force
Write-Host "Lighting SurrealDB stopped."
