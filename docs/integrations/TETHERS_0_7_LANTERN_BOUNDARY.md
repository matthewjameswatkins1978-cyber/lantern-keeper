# Tethers 0.7 boundary for Lantern Keeper

Status: contract and compatibility checkpoint, 12 September 2026

The current Tethers repository is `matthewjameswatkins1978-cyber/tethers-lang`,
whose main branch documents the 0.7.0 release-candidate host. Tethers remains
the authority layer; Lantern remains the canonical memory layer.

## Boundary

Lantern exposes semantic operations. Tethers may authorise consequential use of
those operations. Neither side embeds the other's storage or language runtime.

| Lantern capability | Default authority | Escalation |
| --- | --- | --- |
| search, read, context, recent | allow in authorised scope | deny cross-scope |
| propose, reinforce | allow | review protected scopes |
| supersede, correct | allow with evidence | ask for protected rules/conflicts |
| import promotion, bulk mutation | ask | explicit operator authority |
| forget, purge, ownership/security change | ask or deny | explicit destructive authority |

## Current code state

Lantern currently retains a preview-only adapter for the historical 0.1
newline-delimited protocol. It does not execute actions and is not the 0.7
integration. The current 0.7 host must be inspected and exercised from the
Windows Tethers checkout before replacing this adapter.

The intended 0.7 integration is a reviewed Lantern capability manifest and a
small host-side policy binding. The manifest should expose only the semantic
Lantern surface, with scopes and effects; it must not expose SurrealQL,
credentials, or raw database access.

## Verification gate

Before enabling the integration:

1. run `tethers describe --json` and `tethers doctor --json` from the current
   0.7 checkout;
2. inspect the capability contract and host policy for Lantern operations;
3. exercise read/propose/supersede/forget against an isolated Lantern database;
4. verify Trail entries correlate with Lantern memory events;
5. confirm `ASK`/`DENY` behavior for destructive, bulk and protected changes.

Until that gate is run, the preview adapter remains explicitly non-authoritative
and no Tethers binary is required for normal Lantern startup.
