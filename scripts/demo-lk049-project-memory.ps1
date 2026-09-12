# Lantern Keeper -- LK-049 Repeatable End-to-End Project-Memory Demo
# -----------------------------------------------------------------------------
# Exercises the full Project-memory loop via the public CLI with --json on every
# command, proving the machine-readable contract end to end:
#
#   create Project -> add Markdown file -> revise it -> inspect history ->
#   produce handoff -> record result -> produce updated handoff
#
# Prerequisites: SurrealDB running, Lighting running (lighting serve).
# Uses a unique temporary workspace so repeated runs do not collide.
#
# Usage:
#   .\scripts\demo-lk049-project-memory.ps1
#   .\scripts\demo-lk049-project-memory.ps1 -ServiceUrl "http://127.0.0.1:9999"

param(
    [string]$ServiceUrl = "http://127.0.0.1:4317"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path "$ScriptDir\.."

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$runNonce = [guid]::NewGuid().ToString()
$ProjectName = "LK-049 Demo $timestamp"

$TempDir = Join-Path $env:TEMP "lantern-keeper-lk049-demo-${timestamp}"
$TestFile = Join-Path $TempDir "notes.md"
$ResultFile = Join-Path $TempDir "result.md"

$ResultEpisodeId = $null
$FirstSourceId = $null
$FirstEpisodeId = $null

# ---------------------------------------------------------------------------
# Helper: run a Lighting CLI command with --json, return parsed object.
# ---------------------------------------------------------------------------
function Invoke-LightingCliJson {
    param(
        [Parameter(Mandatory)]
        [string[]]$Arguments
    )

    $cliArgs = @(
        "run", "-p", "lighting", "--",
        "--service-url", $ServiceUrl
    ) + $Arguments + @("--json")

    # Temporarily relax error-action so cargo's build stderr does not cause
    # a terminating NativeCommandError under $ErrorActionPreference = "Stop".
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & cargo @cliArgs 2>$null
    } finally {
        $ErrorActionPreference = $prevEAP
    }
    $exitCode = $LASTEXITCODE

    if ($exitCode -ne 0) {
        $joined = $Arguments -join " "
        throw "CLI command failed (exit $exitCode): lighting $joined"
    }

    if (-not $output) {
        $joined = $Arguments -join " "
        throw "CLI produced no output: lighting $joined"
    }

    try {
        return ($output | Out-String) | ConvertFrom-Json
    } catch {
        $joined = $Arguments -join " "
        $raw = $output | Out-String
        throw "CLI --json output was not valid JSON: lighting $joined`nRaw output: $raw"
    }
}

# ---------------------------------------------------------------------------
# Wrap the entire demo body in try/finally so the temporary directory is
# always cleaned up, even when an assertion fails.
# ---------------------------------------------------------------------------
$success = $false
try {
    # -----------------------------------------------------------------------
    # 0. Precondition: Lighting must be reachable.
    # -----------------------------------------------------------------------
    Write-Host "=== LK-049 Project-Memory Demo ==="
    Write-Host "Service: $ServiceUrl"
    Write-Host "Project: $ProjectName"
    Write-Host ""

    Write-Host "[precondition] Checking Lighting readiness ..."
    # The `health --json` command exits with code 1 when Lighting is not
    # ready, but still prints valid JSON to stdout.
    $prevEAP = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $cliArgs = @(
            "run", "-p", "lighting", "--",
            "--service-url", $ServiceUrl,
            "health", "--json"
        )
        $healthOutput = & cargo @cliArgs 2>$null
        $healthExit = $LASTEXITCODE
        $healthJson = ($healthOutput | Out-String) | ConvertFrom-Json
    } catch {
        $healthExit = 1
        $healthJson = $null
    } finally {
        $ErrorActionPreference = $prevEAP
    }

    if ($healthExit -ne 0 -or -not $healthJson -or -not $healthJson.ready) {
        throw "Lighting is not available at $ServiceUrl. Start SurrealDB, then run: cargo run -p lighting -- serve"
    }
    Write-Host "  Lighting is ready."

    # -----------------------------------------------------------------------
    # 1. Create temporary workspace.
    # -----------------------------------------------------------------------
    New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

    # -----------------------------------------------------------------------
    # 2. Checkpoint 1: Create Project.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[1/7] Creating Project ..."
    $project = Invoke-LightingCliJson @("project-create", $ProjectName)

    if (-not $project.project_id -or $project.project_id -eq "") {
        throw "project-create returned empty project_id"
    }
    $ProjectId = $project.project_id
    Write-Host "  Project created: $ProjectId ($($project.name))"

    # -----------------------------------------------------------------------
    # 3. Checkpoint 2: Initial Markdown file capture.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[2/7] Capturing initial Markdown file ..."

    $initialContent = @"
# LK-049 Demo Notes

Run nonce: $runNonce

This is the initial version of the demo notes.
It contains an outline of the Project-memory loop.

## Steps
1. Create Project
2. Capture file
3. Revise file
4. Inspect history
5. Produce handoff
6. Record result
7. Verify updated handoff
"@
    [System.IO.File]::WriteAllText($TestFile, $initialContent, [System.Text.UTF8Encoding]::new($false))

    $add1 = Invoke-LightingCliJson @("project-add-file", $ProjectId, $TestFile)

    if ($add1.outcome -ne "stored") {
        $got = $add1.outcome
        throw "Expected initial capture outcome 'stored', got '$got'"
    }
    if ($add1.previous_source_id) {
        $got = $add1.previous_source_id
        throw "Expected no previous_source_id on first capture, got '$got'"
    }
    if (-not $add1.source_id -or $add1.source_id -eq "") {
        throw "Missing source_id on first capture"
    }
    if (-not $add1.episode_id -or $add1.episode_id -eq "") {
        throw "Missing episode_id on first capture"
    }
    $FirstSourceId = $add1.source_id
    $FirstEpisodeId = $add1.episode_id
    Write-Host "  Source  : $FirstSourceId"
    Write-Host "  Episode : $FirstEpisodeId"
    Write-Host "  Outcome : stored (first capture)"

    # -----------------------------------------------------------------------
    # 4. Checkpoint 3: Revise and re-capture.
    #    Accepts outcome "stored" or "revision_captured_existing_project_link"
    #    but insists on a new Source revision (previous_source_id) and
    #    unchanged Episode ID.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[3/7] Revising and re-capturing same file ..."

    $revisedContent = @"
# LK-049 Demo Notes

Run nonce: $runNonce

This is the **revised** version of the demo notes.
It contains an outline of the Project-memory loop plus verification steps.

## Steps
1. Create Project
2. Capture file
3. Revise file
4. Inspect history
5. Produce handoff
6. Record result
7. Verify updated handoff

## Verification
Each step is a machine-readable --json checkpoint.
"@
    [System.IO.File]::WriteAllText($TestFile, $revisedContent, [System.Text.UTF8Encoding]::new($false))

    $add2 = Invoke-LightingCliJson @("project-add-file", $ProjectId, $TestFile)

    if ($add2.outcome -ne "revision_captured_existing_project_link") {
        $got = $add2.outcome
        throw "Expected revision outcome 'revision_captured_existing_project_link', got '$got'"
    }
    if ($add2.previous_source_id -ne $FirstSourceId) {
        $got = $add2.previous_source_id
        throw "Expected previous_source_id '$FirstSourceId', got '$got'"
    }
    if ($add2.episode_id -ne $FirstEpisodeId) {
        $got = $add2.episode_id
        throw "Expected same episode_id after revision, got '$got' instead of '$FirstEpisodeId'"
    }
    Write-Host "  Source   : $($add2.source_id) (new revision)"
    Write-Host "  Previous : $($add2.previous_source_id)"
    Write-Host "  Episode  : $($add2.episode_id) (reused, no duplication)"

    # -----------------------------------------------------------------------
    # 5. Checkpoint 4: source-history shows the revision chain.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[4/7] Inspecting source-history ..."

    $history = Invoke-LightingCliJson @("source-history", $TestFile)

    if ($history.revisions.Count -lt 2) {
        $cnt = $history.revisions.Count
        throw "Expected at least 2 revisions, got $cnt"
    }

    $rev0 = $history.revisions[0]
    $rev1 = $history.revisions[1]

    if ($rev0.source_id -ne $FirstSourceId) {
        $got = $rev0.source_id
        throw "First revision source_id mismatch: expected '$FirstSourceId', got '$got'"
    }
    if ($rev1.previous_source_id -ne $FirstSourceId) {
        $got = $rev1.previous_source_id
        throw "Second revision should chain to first: expected previous_source_id '$FirstSourceId', got '$got'"
    }
    if (-not $rev1.current) {
        throw "Latest revision should be marked current"
    }

    Write-Host "  Revisions: $($history.revisions.Count)"
    foreach ($r in $history.revisions) {
        $marker = if ($r.current) { " (current)" } else { "" }
        Write-Host "    $($r.source_id)$marker"
        if ($r.previous_source_id) {
            Write-Host "      previous: $($r.previous_source_id)"
        }
    }

    # -----------------------------------------------------------------------
    # 6. Checkpoint 5: project-handoff produces a deterministic context
    #    package.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[5/7] Producing Project handoff ..."

    $handoff1 = Invoke-LightingCliJson @("project-handoff", $ProjectId)

    if (-not $handoff1.project) {
        throw "Handoff missing project object"
    }
    if ($handoff1.project.name -ne $ProjectName) {
        $got = $handoff1.project.name
        throw "Handoff project name mismatch: expected '$ProjectName', got '$got'"
    }
    if (-not $handoff1.context_package) {
        throw "Handoff missing context_package"
    }
    if (-not $handoff1.context_package.content -or $handoff1.context_package.content.Trim() -eq "") {
        throw "Handoff context_package.content is empty"
    }
    if ($handoff1.context_package.content -notmatch $FirstEpisodeId) {
        throw "Handoff content does not reference Episode $FirstEpisodeId"
    }

    Write-Host "  Format  : $($handoff1.context_package.format)"
    Write-Host "  Audience: $($handoff1.context_package.audience)"
    Write-Host "  Episodes: $($handoff1.episodes.Count)"
    Write-Host "  Context package produced and contains the Episode."

    # -----------------------------------------------------------------------
    # 7. Checkpoint 6: project-record-result writes a result back.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[6/7] Recording result ..."

    $resultLines = @(
        "# LK-049 Proof Result",
        "",
        "The demo successfully completed the first six checkpoints.",
        "",
        "- Project created: $ProjectId",
        "- File captured as Source $FirstSourceId",
        "- Revised and re-captured with revision chain",
        "- History shows $($history.revisions.Count) revisions",
        "- Project handoff produced",
        "",
        "This result file exercises the write-back path through project-record-result."
    )
    $resultContent = $resultLines -join [System.Environment]::NewLine
    [System.IO.File]::WriteAllText($ResultFile, $resultContent, [System.Text.UTF8Encoding]::new($false))

    $record = Invoke-LightingCliJson @(
        "project-record-result", $ProjectId, $ResultFile,
        "--title", "LK-049 Proof Result"
    )

    if ($record.outcome -ne "recorded") {
        $got = $record.outcome
        throw "Expected outcome 'recorded', got '$got'"
    }
    if (-not $record.episode_id -or $record.episode_id -eq "") {
        throw "record-result returned empty episode_id"
    }
    $ResultEpisodeId = $record.episode_id

    Write-Host "  Outcome  : recorded"
    Write-Host "  Source   : $($record.source_id)"
    Write-Host "  Episode  : $ResultEpisodeId"
    Write-Host "  Bytes    : $($record.start_byte)..$($record.end_byte)"

    # -----------------------------------------------------------------------
    # 8. Checkpoint 7: Final handoff visibly includes the recorded result.
    # -----------------------------------------------------------------------
    Write-Host ""
    Write-Host "[7/7] Verifying updated handoff includes recorded result ..."

    $handoff2 = Invoke-LightingCliJson @("project-handoff", $ProjectId)

    if (-not $handoff2.context_package -or -not $handoff2.context_package.content) {
        throw "Updated handoff has no content"
    }

    $content = $handoff2.context_package.content

    if ($content -notmatch $ResultEpisodeId) {
        throw "Updated handoff does not contain result Episode $ResultEpisodeId"
    }

    Write-Host "  Result Episode $ResultEpisodeId found in updated handoff."
    Write-Host "  Total Episodes in handoff: $($handoff2.episodes.Count)"

    # -----------------------------------------------------------------------
    # All checkpoints passed.
    # -----------------------------------------------------------------------
    $success = $true

    Write-Host ""
    Write-Host "=== Demo PASSED ==="
    Write-Host "Project  : $ProjectId"
    Write-Host "Source 1 : $FirstSourceId"
    Write-Host "Episode  : $FirstEpisodeId"
    Write-Host "Revisions: $($history.revisions.Count)"
    Write-Host "Result   : $ResultEpisodeId"
    Write-Host "Handoff  : $($handoff2.episodes.Count) episodes in final context package"
    Write-Host ""
    Write-Host "All 7 checkpoints verified. The full Project-memory loop works."

} catch {
    $failureMessage = $_.Exception.Message
    if (-not $failureMessage) { $failureMessage = "$_" }
} finally {
    # Always clean up temporary files, regardless of success or failure.
    Write-Host ""
    Write-Host "Cleaning up temporary files ..."
    if (Test-Path -LiteralPath $TempDir) {
        Remove-Item -Recurse -Force -LiteralPath $TempDir -ErrorAction SilentlyContinue
        Write-Host "  Removed: $TempDir"
    } else {
        Write-Host "  (nothing to remove)"
    }

    if (-not $success) {
        Write-Host ""
        Write-Host "=== Demo FAILED ==="
        if ($failureMessage) {
            Write-Host "Error: $failureMessage"
        }
        exit 1
    }
}

exit 0
