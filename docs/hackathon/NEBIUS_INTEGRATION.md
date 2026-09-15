# Nebius Token Factory integration

The Dreamer adapter uses Nebius Token Factory's OpenAI-compatible
`/v1/chat/completions` interface. The base URL and model are configuration, not
committed credentials. The adapter sends a bounded context, requests JSON mode,
and validates the returned object locally against the strict candidate DTO.

The currently documented Nemotron example is
`nvidia/nemotron-3-super-120b-a12b` at the Token Factory API. Account/model
availability must be checked with `GET /v1/models` before a live run; this
repository does not claim that a live call has passed until that evidence is
recorded here.

Suggested live preflight:

```powershell
$headers = @{ Authorization = "Bearer $env:NEBIUS_API_KEY" }
Invoke-RestMethod -Uri "$env:NEBIUS_BASE_URL/models" -Headers $headers
```

Required variables are `NEBIUS_API_KEY`, `NEBIUS_BASE_URL`, and
`NEBIUS_MODEL`. No API key is logged, returned in diagnostics, or stored in
Dreamer candidates.

## Current evidence

- Adapter implementation: present on the hackathon branch.
- Offline strict DTO/failure-path tests: passed.
- Live Token Factory model listing: not yet run in this workspace.
- Live Nemotron Dreamer completion: not yet run in this workspace.
