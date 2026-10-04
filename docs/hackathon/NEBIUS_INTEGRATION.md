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
- Live Token Factory model listing: passed 2026-10-04 (25 models; configured Nemotron model available).
- Live Nemotron Dreamer completion: passed through the real adapter; candidate validation, provider metadata, and the absence of authority fields were confirmed. See [live evidence](evidence/m7-token-factory-live.json).
