param(
    [string]$BaseUrl = "http://127.0.0.1:4317",
    [string]$Fixture = "$PSScriptRoot/lanternbench-v1.json",
    [switch]$Json
)

$ErrorActionPreference = "Stop"
$suite = Get-Content -Raw -LiteralPath $Fixture | ConvertFrom-Json
$results = @()

foreach ($case in $suite.cases) {
    $body = @{ phrase = $case.query; include_inactive = $true } | ConvertTo-Json -Depth 8
    $response = $null
    $errorText = $null
    try {
        if ($case.category -eq "context_assembly") {
            $response = Invoke-RestMethod -Method Post -Uri "$BaseUrl/api/v1/memories/context" -ContentType "application/json" -Body (@{ query = $case.query } | ConvertTo-Json)
            $actual = $response.context
            $passed = $true
            foreach ($section in $case.expected.required_sections) {
                if ($actual -notmatch [regex]::Escape($section)) { $passed = $false }
            }
            if ($case.expected.raw_history_dump -eq $false -and $response.memories.Count -gt 50) { $passed = $false }
        } else {
            $response = Invoke-RestMethod -Method Post -Uri "$BaseUrl/api/v1/memories/recall" -ContentType "application/json" -Body $body
            $memories = @($response.memories)
            $passed = ($response.abstained -eq [bool]$case.expected.abstain)
            if ($case.expected.required_source) {
                $passed = $passed -and (($memories | ForEach-Object { $_.derived_from }) -contains $case.expected.required_source)
            }
            if ($case.expected.required_status) {
                $passed = $passed -and (($memories.status -contains $case.expected.required_status) -or ($memories | Where-Object status -eq $case.expected.required_status).Count -gt 0)
            }
        }
        $results += [pscustomobject]@{ id = $case.id; category = $case.category; passed = [bool]$passed; error = $null }
    } catch {
        $errorText = $_.Exception.Message
        $results += [pscustomobject]@{ id = $case.id; category = $case.category; passed = $false; error = $errorText }
    }
}

$byCategory = $results | Group-Object category | ForEach-Object {
    [pscustomobject]@{
        category = $_.Name
        passed = @($_.Group | Where-Object passed).Count
        total = @($_.Group).Count
    }
}
$report = [pscustomobject]@{
    format = "lanternbench-result-v1"
    fixture = (Resolve-Path -LiteralPath $Fixture).Path
    passed = @($results | Where-Object passed).Count
    total = @($results).Count
    by_category = @($byCategory)
    cases = @($results)
}
if ($Json) { $report | ConvertTo-Json -Depth 8 } else {
    "LanternBench: $($report.passed)/$($report.total)"
    $byCategory | Format-Table -AutoSize
    $results | Format-Table id, category, passed, error -AutoSize
}
if ($report.passed -ne $report.total) { exit 1 }
