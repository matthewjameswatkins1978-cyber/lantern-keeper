param(
    [switch]$KeepData
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$lighting = Join-Path $repo "target\debug\lighting.exe"
if (-not (Test-Path -LiteralPath $lighting)) {
    throw "Build target/debug/lighting.exe first."
}

$proofId = [guid]::NewGuid().ToString("N")
$storePath = Join-Path $env:TEMP "lantern-lucy-mcp-proof-$proofId"
$serviceUrl = "http://127.0.0.1:4317"
$server = $null
$mcp = $null
$nextId = 1
$records = [ordered]@{}

function Start-Lantern {
    $process = [Diagnostics.Process]::new()
    $process.StartInfo.FileName = $lighting
    $process.StartInfo.Arguments = "serve"
    $process.StartInfo.WorkingDirectory = $repo
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.CreateNoWindow = $true
    $process.StartInfo.Environment["LIGHTING_STORAGE"] = "embedded-surrealkv"
    $process.StartInfo.Environment["LIGHTING_SURREAL_PATH"] = $storePath
    $process.StartInfo.Environment["LIGHTING_HOST"] = "127.0.0.1"
    $process.StartInfo.Environment["LIGHTING_PORT"] = "4317"
    if (-not $process.Start()) { throw "failed to start Lighting" }
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 300
        try {
            $ready = Invoke-RestMethod -Method Get -Uri "$serviceUrl/health/ready" -TimeoutSec 2
            if ($ready.ready -eq $true) { return $process }
        } catch { }
    } while ((Get-Date) -lt $deadline)
    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    throw "Lighting did not become ready"
}

function Stop-Lantern([Diagnostics.Process]$process) {
    if ($null -ne $process -and -not $process.HasExited) {
        $process.Kill()
        $process.WaitForExit()
    }
}

function Start-Mcp {
    $process = [Diagnostics.Process]::new()
    $process.StartInfo.FileName = $lighting
    $process.StartInfo.Arguments = "mcp"
    $process.StartInfo.WorkingDirectory = $repo
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.CreateNoWindow = $true
    $process.StartInfo.RedirectStandardInput = $true
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.Environment["LIGHTING_SERVICE_URL"] = $serviceUrl
    if (-not $process.Start()) { throw "failed to start MCP bridge" }
    return $process
}

function Stop-Mcp([Diagnostics.Process]$process) {
    if ($null -ne $process -and -not $process.HasExited) {
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(3000)) {
            $process.Kill()
            $process.WaitForExit()
        }
    }
}

function Send-Mcp([Diagnostics.Process]$process, [string]$method, $params) {
    $id = $script:nextId++
    $request = @{ jsonrpc = "2.0"; id = $id; method = $method; params = $params } |
        ConvertTo-Json -Depth 20 -Compress
    $process.StandardInput.WriteLine($request)
    $line = $process.StandardOutput.ReadLine()
    if ([string]::IsNullOrWhiteSpace($line)) { throw "MCP returned no response for $method" }
    $response = $line | ConvertFrom-Json
    if ($null -ne $response.error) { throw "MCP $method failed: $($response.error.message)" }
    return $response.result
}

function Call-Tool([Diagnostics.Process]$process, [string]$name, $arguments) {
    $result = Send-Mcp $process "tools/call" @{ name = $name; arguments = $arguments }
    if ($result.isError -eq $true) { throw "MCP tool $name failed: $($result.content[0].text)" }
    return $result.structuredContent
}

try {
    New-Item -ItemType Directory -Force -Path $storePath | Out-Null
    $server = Start-Lantern
    $null = Invoke-RestMethod -Method Post -Uri "$serviceUrl/api/v1/predicates" -ContentType "application/json" -Body (@{
        key = "test_lantern_colour"; aliases = @("lantern colour"); value_type = "text";
        allowed_dimensions = @(); description = "isolated MCP proof predicate"; status = "active"; actor_id = "lucy"
    } | ConvertTo-Json -Depth 10)
    $mcp = Start-Mcp
    $null = Send-Mcp $mcp "initialize" @{}

    $soft = Call-Tool $mcp "lantern_remember" @{
        kind = "idea"; content = "A harmless odd idea about a blind drummer game";
        originator_actor_id = "matthew"; holder_actor_id = "shared:matthew-lucy"; salience = 0.8
    }
    $softId = $soft.memory_item.id
    $search = Call-Tool $mcp "lantern_search" @{ phrase = "blind drummer"; include_archived = $false; limit = 10 }
    if (-not (@($search.memory_items) | Where-Object { $_.id -eq $softId })) { throw "soft memory was not searchable" }
    $records.soft_memory_id = $softId

    $created = Call-Tool $mcp "lantern_remember" @{
        kind = "claim"; content = "blue"; subject_key = "matthew";
        predicate_key = "test_lantern_colour"; holder_actor_id = "matthew";
        originator_actor_id = "matthew"; scope = @{}
    }
    $belief = $created.reconciliation.belief
    if ($belief.current_value -ne "blue") { throw "MCP claim did not create the blue belief" }
    $records.current_belief_id = $belief.id
    $records.initial_claim_id = $created.claim.id

    $context = Call-Tool $mcp "lantern_context" @{ query = "What is my test lantern colour?"; actor = "lucy"; item_budget = 5; token_budget = 512 }
    if (@($context.context.current_beliefs)[0].current_value -ne "blue") { throw "context did not retrieve blue" }
    $records.context_pack_id = $context.context.id
    $why = Call-Tool $mcp "lantern_why" @{ belief_id = $belief.id }
    if (-not (@($why.supporting_claims) | Where-Object { $_.id -eq $created.claim.id })) { throw "why did not return the claim lineage" }

    $correction = Call-Tool $mcp "lantern_correct" @{
        target_belief_id = $belief.id; correction_text = "No, my test lantern colour is green.";
        replacement_value = "green"; context_pack_id = $context.context.id; scope = @{}
    }
    $replacement = $correction.correction.reconciliation.belief
    if ($replacement.current_value -ne "green") { throw "MCP correction did not create green" }
    $records.correction_claim_id = $correction.correction.claim.id
    $records.correction_source_id = $correction.correction.source_id
    $records.correction_episode_id = $correction.correction.episode_id
    $records.replacement_belief_id = $replacement.id

    $current = Call-Tool $mcp "lantern_context" @{ query = "What is my test lantern colour now?"; actor = "lucy"; item_budget = 5; token_budget = 512 }
    if (@($current.context.current_beliefs)[0].current_value -ne "green") { throw "current context did not return green" }
    $history = Call-Tool $mcp "lantern_context" @{ query = "What was my test lantern colour before?"; actor = "lucy"; item_budget = 5; token_budget = 512 }
    if (-not (@($history.context.historical_beliefs) | Where-Object { $_.current_value -eq "blue" })) { throw "historical context did not retain blue" }

    Stop-Mcp $mcp; $mcp = $null
    Stop-Lantern $server; $server = $null
    $server = Start-Lantern
    $mcp = Start-Mcp
    $null = Send-Mcp $mcp "initialize" @{}
    $afterRestart = Call-Tool $mcp "lantern_context" @{ query = "What is my test lantern colour now?"; actor = "lucy"; item_budget = 5; token_budget = 512 }
    if (@($afterRestart.context.current_beliefs)[0].current_value -ne "green") { throw "restart lost green" }
    $afterRestartWhy = Call-Tool $mcp "lantern_why" @{ belief_id = $replacement.id }
    if (-not (@($afterRestartWhy.supporting_claims) | Where-Object { $_.id -eq $correction.correction.claim.id })) { throw "restart lost correction provenance" }
    $records.restart_current_value = @($afterRestart.context.current_beliefs)[0].current_value
    $records.restart_historical_value = @((Call-Tool $mcp "lantern_context" @{ query = "What was my test lantern colour before?"; actor = "lucy"; item_budget = 5; token_budget = 512 }).context.historical_beliefs)[0].current_value
    [pscustomobject]@{ status = "PASS"; proof = "supported local stdio MCP bridge"; records = [pscustomobject]$records } |
        ConvertTo-Json -Depth 20
}
finally {
    Stop-Mcp $mcp
    Stop-Lantern $server
    if (-not $KeepData -and (Test-Path -LiteralPath $storePath)) {
        Remove-Item -LiteralPath $storePath -Recurse -Force
    }
}
