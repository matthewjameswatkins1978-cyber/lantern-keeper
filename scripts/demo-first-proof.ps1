# Lantern Keeper — First Proof Demonstration
# -----------------------------------------------------------------------------
# Seeds a harmless local development demonstration and runs the first complete
# retrieval proof: Source → Episode → Project + Marker association → retrieve.
#
# Prerequisites: SurrealDB running, Lighting running (lighting serve).
# Safe to rerun — uses idempotent APIs and .local/first-proof-demo.json state.
#
# Usage:
#   .\scripts\demo-first-proof.ps1
#   .\scripts\demo-first-proof.ps1 -ServiceUrl "http://127.0.0.1:9999"

param(
    [string]$ServiceUrl = "http://127.0.0.1:4317"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path "$ScriptDir\.."
$FixturePath = "$RepoRoot\fixtures\first-proof.md"
$LocalDir = "$RepoRoot\.local"
$StateFile = "$LocalDir\first-proof-demo.json"
$LocalData = @{}

# ---------------------------------------------------------------------------
# 1. Preconditions
# ---------------------------------------------------------------------------

$hostParts = $ServiceUrl -replace '^https?://' -split ':'
if ($hostParts[0] -ne "127.0.0.1" -and $hostParts[0] -ne "localhost") {
    Write-Error "ServiceUrl must be localhost or 127.0.0.1, got: $($hostParts[0])"
    exit 1
}

function Invoke-Lighting {
    param([string]$Method, [string]$Path, [object]$Body)
    $uri = "$ServiceUrl$Path"
    $params = @{ Uri = $uri; Method = $Method; ContentType = "application/json"; ErrorAction = "Stop" }
    if ($Body) { $params.Body = ($Body | ConvertTo-Json -Compress) }
    try {
        $response = Invoke-RestMethod @params
        return $response
    } catch {
        $statusCode = $_.Exception.Response.StatusCode.value__
        $body = if ($_.ErrorDetails.Message) { $_.ErrorDetails.Message } else { $_ | Out-String }
        Write-Warning "HTTP $Method $Path → $statusCode"
        if ($statusCode -eq 404) { return $null }
        throw "Lighting request failed: $statusCode — $body"
    }
}

Write-Host "=== Lantern Keeper First Proof Demonstration ==="
Write-Host "Service: $ServiceUrl"

Write-Host "[step 1] Checking Lighting readiness ..."
$health = Invoke-Lighting -Method Get -Path "/health/ready"
if (-not $health.ready) {
    Write-Error "Lighting is not ready at $ServiceUrl. Start SurrealDB, then run: cargo run -p lighting -- serve"
    exit 1
}
Write-Host "  Lighting is ready."

# ---------------------------------------------------------------------------
# 2. Read fixture and calculate UTF-8 byte offsets
# ---------------------------------------------------------------------------

Write-Host "[step 2] Reading fixture ..."
$fixtureText = Get-Content -Path $FixturePath -Raw -Encoding UTF8
Write-Host "  Fixture: $($fixtureText.Length) chars read."

# Convert to UTF-8 bytes and find "## The Human Relay Problem" section
$utf8Bytes = [System.Text.Encoding]::UTF8.GetBytes($fixtureText)
$fixtureString = $fixtureText

# Find the heading and the next heading
$heading1 = $fixtureString.IndexOf("## The Human Relay Problem")
if ($heading1 -lt 0) {
    Write-Error "Fixture missing '## The Human Relay Problem' heading"
    exit 1
}
$heading2 = $fixtureString.IndexOf("## Unrelated Section")

# Calculate UTF-8 byte offsets for the heading section
# Convert char positions to byte positions
$startCharPos = $heading1
$endCharPos = if ($heading2 -gt 0) { $heading2 } else { $fixtureString.Length }

# Byte offsets: count UTF-8 bytes up to these char boundaries
$startBytePos = [System.Text.Encoding]::UTF8.GetBytes($fixtureString.Substring(0, $startCharPos)).Length
$endBytePos = [System.Text.Encoding]::UTF8.GetBytes($fixtureString.Substring(0, $endCharPos)).Length

Write-Host "  'The Human Relay Problem' section: UTF-8 bytes $startBytePos..$endBytePos"

# ---------------------------------------------------------------------------
# 3. Add Source (idempotent)
# ---------------------------------------------------------------------------

Write-Host "[step 3] Adding fixture as Source ..."
$sourceResult = Invoke-Lighting -Method Post -Path "/api/v1/sources" -Body @{
    title = "Lantern Keeper First Proof"
    kind  = "markdown"
    content = $fixtureText
}
$sourceId = $sourceResult.source_id
Write-Host "  Source: $sourceId ($($sourceResult.outcome))"

# ---------------------------------------------------------------------------
# 4. Load or create objects
# ---------------------------------------------------------------------------

if (Test-Path $StateFile) {
    $LocalData = Get-Content $StateFile -Raw | ConvertFrom-Json -AsHashtable
    Write-Host "[step 4] Loaded local state from $StateFile"
} else {
    New-Item -ItemType Directory -Force -Path $LocalDir | Out-Null
    $LocalData = @{}
}

function Get-Or-Create {
    param([string]$Key, [scriptblock]$IdGetter, [scriptblock]$Creator, [string]$Description)
    if ($LocalData.ContainsKey($Key)) {
        $existingId = $LocalData[$Key]
        $exists = & $IdGetter $existingId
        if ($exists) {
            Write-Host "  $Description (reusing): $existingId"
            return $existingId
        }
        Write-Host "  $Description (stale, recreating)"
    }
    $result = & $Creator
    $LocalData[$Key] = $result
    Write-Host "  $Description (created): $result"
    return $result
}

# --- Project ---
$projectId = Get-Or-Create -Key "project_id" `
    -IdGetter { param($id) (Invoke-Lighting -Method Get -Path "/api/v1/projects/$id") -ne $null } `
    -Creator { (Invoke-Lighting -Method Post -Path "/api/v1/projects" -Body @{ name = "Lantern Keeper First Proof"; status = "active" }).project_id } `
    -Description "Project"

# --- Episode ---
$episodeId = Get-Or-Create -Key "episode_id" `
    -IdGetter { param($id) (Invoke-Lighting -Method Get -Path "/api/v1/episodes/$id") -ne $null } `
    -Creator { (Invoke-Lighting -Method Post -Path "/api/v1/episodes" -Body @{
        title = "The Human Relay Problem"
        source_id = $sourceId
        start_byte = $startBytePos
        end_byte = $endBytePos
    }).episode_id } `
    -Description "Episode"

# --- Marker (idempotent API, no state needed) ---
Write-Host "  Creating Marker (idempotent) ..."
$markerResult = Invoke-Lighting -Method Post -Path "/api/v1/markers" -Body @{
    text = "human network cable"
}
$markerId = $markerResult.marker_id
Write-Host "  Marker: $markerId"

# ---------------------------------------------------------------------------
# 5. Associations (idempotent)
# ---------------------------------------------------------------------------

Write-Host "[step 5] Linking Episode → Project ..."
Invoke-Lighting -Method Post -Path "/api/v1/episodes/$episodeId/projects" -Body @{
    project_id = $projectId
    kind = "primary"
} | Out-Null
Write-Host "  Linked."

Write-Host "[step 5] Linking Episode → Marker ..."
Invoke-Lighting -Method Post -Path "/api/v1/episodes/$episodeId/markers" -Body @{
    marker_id = $markerId
} | Out-Null
Write-Host "  Linked."

# ---------------------------------------------------------------------------
# 6. Save local state
# ---------------------------------------------------------------------------

$LocalData | ConvertTo-Json | Set-Content -Path $StateFile -Encoding UTF8

# ---------------------------------------------------------------------------
# 7. Retrieve
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "=== Running: lighting retrieve 'human network cable' ==="
Write-Host ""

$retrieveBody = @{ text = "human network cable" } | ConvertTo-Json -Compress
$retrieveResult = Invoke-Lighting -Method Post -Path "/api/v1/retrieval/markers" -Body @{ text = "human network cable" }

Write-Host "Marker: $($retrieveResult.marker.display_text)"
Write-Host ""

foreach ($ep in $retrieveResult.episodes) {
    $idx = [array]::IndexOf($retrieveResult.episodes, $ep) + 1
    Write-Host "$idx. $($ep.title)"
    Write-Host "   Why: $($ep.why_matched)"
    Write-Host "   Source: $($ep.source_id), bytes $($ep.start_byte)..$($ep.end_byte)"
    Write-Host "   Excerpt:"
    $excerpt = $ep.excerpt
    if ($ep.excerpt.Length -gt 1300) {
        $excerpt = $ep.excerpt.Substring(0, 1200) + "`n[... truncated at 1200 characters ...]"
    }
    foreach ($line in ($excerpt -split "`n")) {
        Write-Host "      $line"
    }
    Write-Host ""
}

# ---------------------------------------------------------------------------
# 8. Summary
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "=== Demonstration Complete ==="
Write-Host "Source ID   : $sourceId"
Write-Host "Project ID  : $projectId"
Write-Host "Episode ID  : $episodeId"
Write-Host "Marker ID   : $markerId"
Write-Host "Byte range  : $startBytePos..$endBytePos"
Write-Host "Retrieval   : human network cable"
Write-Host ""
Write-Host "State saved to: $StateFile"
Write-Host "Rerun at any time — objects are reused via stored IDs."
