#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot

function Invoke-Step {
    param(
        [Parameter(Mandatory)]
        [string]$Name,

        [Parameter(Mandatory)]
        [scriptblock]$Action
    )

    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Action
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

Push-Location $ProjectRoot

try {
    Invoke-Step -Name "cargo fmt --check" -Action { cargo fmt --check }
    Invoke-Step -Name "cargo clippy" -Action { cargo clippy --workspace --all-targets -- -D warnings }
    Invoke-Step -Name "cargo test" -Action { cargo test --workspace }
    Invoke-Step -Name "lighting version" -Action { cargo run -p lighting -- version }
}
finally {
    Pop-Location
}

Write-Host "==> All validation steps passed" -ForegroundColor Green
