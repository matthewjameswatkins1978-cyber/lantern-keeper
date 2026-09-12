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
    Invoke-Step -Name "cargo fmt --check" -Action { cargo fmt --all -- --check }
    Invoke-Step -Name "cargo check" -Action { cargo check --locked --workspace --all-targets --all-features }
    Invoke-Step -Name "cargo clippy" -Action { cargo clippy --locked --workspace --all-targets --all-features -- -D warnings }
    # The remote SurrealDB lane has a known transient write-conflict during
    # parallel cold-start tests. Serialize the canonical validation lane; the
    # parallel behaviour remains a separate qualification concern.
    Invoke-Step -Name "cargo test" -Action { cargo test --locked --workspace -- --test-threads=1 }
    Invoke-Step -Name "lighting version" -Action { cargo run --locked -p lighting -- version }
}
finally {
    Pop-Location
}

Write-Host "==> All validation steps passed" -ForegroundColor Green
