# Lantern Warden — Milestone 5 live cognitive plane

This checkpoint proves the untrusted cognitive path without promoting
external content or model output into authority.

```text
Tavily search
  -> external evidence record
  -> Lantern Source + Episode provenance
  -> Nemotron candidate interpretation
  -> deterministic authority remains unchanged

AuthorityGrant
  -> Tethers policy
  -> OpenShell effect boundary
```

## Live provider proof

- Nebius Token Factory authenticated successfully.
- Model catalogue contained and the live call used
  `nvidia/nemotron-3-super-120b-a12b`.
- The live call returned structured candidate JSON with `finish_reason=stop`.
- The live telemetry probe recorded 5,751 ms, 160 prompt tokens, 518
  completion tokens, and 678 total tokens.
- Tavily basic search authenticated successfully and returned three results for
  `Alex weekly summaries local export authorization` in 463 ms.
- The combined opt-in Rust test persisted the results through embedded
  SurrealKV, created Source/Episode provenance, and passed the evidence to the
  live Dreamer. The result asserted `canonical_mutation: false` and
  `authority_changed: false`.

Live tests are opt-in and use only process environment variables:

```powershell
$env:WARDEN_LIVE_NEBIUS = '1'
$env:WARDEN_LIVE_TAVILY = '1'
cargo test -p lighting-service cognitive_ops::tests::live_cognitive_plane_preserves_external_provenance_and_authority_boundary -- --exact --nocapture
```

The provider keys are never written to the repository, prompts, Sources,
Episodes, receipts, sandbox environment, or evidence files.

## Safety results

- Retrieved text is marked external and remains linked to provider URL/domain,
  rank, retrieval timestamp, raw provider metadata, and normalized hash.
- Nemotron output is candidate-only. Authority-shaped fields are rejected and
  provider/model metadata is assigned by Lantern.
- Provider confidence is model metadata, not proof.
- Provider timeout, authentication, unavailable-model, and malformed-output
  paths return typed failure; no semantic output is fabricated.
- The existing `authority_non_amplification` regression passed: hostile
  external permission text cannot create authority through repetition.
- M4 OpenShell/Tethers remains a separate gate; the M5 cognitive plane cannot
  invoke it or create a grant.

## Evidence files

- [`m5-nebius-live.json`](evidence/m5-nebius-live.json)
- [`m5-tavily-live.json`](evidence/m5-tavily-live.json)
- [`m5-poisoning-demo.json`](evidence/m5-poisoning-demo.json)

## Remaining limitations

The final Trust Console and polished three-minute demo are not part of M5.
The live M4 cross-repository run that places Lantern authority decision and
outcome receipts around the OpenShell effect remains pending. The live query
used ordinary public search results rather than a controlled hostile website;
the deterministic hostile-content and non-amplification checks remain the
acceptance evidence for that threat class.
