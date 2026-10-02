param(
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [switch]$Push
)

$ErrorActionPreference = 'Stop'
$sourceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$mirrorRepo = (Resolve-Path $RepoPath).Path
$expectedRepo = 'matthewjameswatkins1978-cyber/lantern-git'
$expectedHttps = 'https://github.com/matthewjameswatkins1978-cyber/lantern-git.git'

function Invoke-Git([string[]]$Arguments) {
    & git -C $mirrorRepo @Arguments
    if ($LASTEXITCODE -ne 0) { throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE" }
}

$remote = (& git -C $mirrorRepo remote get-url origin).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Could not read the mirror repository origin.' }
if ($remote -ne $expectedHttps -and $remote -ne 'git@github.com:matthewjameswatkins1978-cyber/lantern-git.git') {
    throw "Refusing unexpected mirror remote: $remote"
}

if ($Push) {
    $privateCheck = & gh repo view $expectedRepo --json isPrivate --jq '.isPrivate'
    if ($LASTEXITCODE -ne 0 -or $privateCheck.Trim() -ne 'true') {
        throw 'Publishing requires gh to confirm that lantern-git is private.'
    }
    $branch = (& git -C $mirrorRepo branch --show-current).Trim()
    if ($LASTEXITCODE -ne 0 -or $branch -ne 'main') {
        throw 'Publishing requires the mirror checkout to be on main.'
    }
}

& python (Join-Path $sourceRoot 'scripts/lantern_git_export.py') --output $mirrorRepo
if ($LASTEXITCODE -ne 0) { throw "Lantern export failed with exit code $LASTEXITCODE" }

if (-not $Push) {
    Write-Output 'Export is ready. Review the mirror diff; pass -Push to publish after confirming the repository is private.'
    Invoke-Git @('status', '--short', '--', 'mirror')
    exit 0
}

Invoke-Git @('add', '--', 'mirror')
& git -C $mirrorRepo diff --cached --quiet -- mirror
if ($LASTEXITCODE -eq 0) {
    Write-Output 'Mirror is already current; no GitHub write was needed.'
    exit 0
}
if ($LASTEXITCODE -ne 1) { throw "Could not inspect staged mirror changes (exit code $LASTEXITCODE)." }

Invoke-Git @('commit', '-m', 'Refresh Lantern read-only mirror', '--', 'mirror')
Invoke-Git @('push', 'origin', 'main')
Write-Output 'Private lantern-git mirror updated.'
