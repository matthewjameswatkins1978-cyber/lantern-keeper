# Nebius × NVIDIA 2026 Build Log

This living checklist records only work actually implemented and verified.

## Phases

- [x] A — preflight, isolated branch, and baseline record.
- [x] B — independent Principal and authority foundation (persistent ledger,
  provenance, receipts, and control-plane API).
- [x] C — Tethers decision bridge and receipts (local HTTP bridge, persistent
  decision/outcome receipts, and fail-closed execution gates).
- [x] D — bounded Nemotron Dreamer and candidate validation, including live
  Nebius Token Factory proof.
- [x] E — Tavily external-evidence lane, provenance, and non-amplification
  regressions.
- [ ] F — adversarial benchmark and baseline comparison.
- [x] G — bounded OpenShell effect boundary (local WSL/Docker, authenticated
  Lantern Warden authority, Tethers Trail, and receipt ordering proven).
- [ ] H — POWER / STILL TRUE / WHY trust console.
- [ ] I — resettable public synthetic demo.
- [ ] J — submission hardening, clean clone, CI, and release evidence.

## Current checkpoint

Phase B is implemented as a pure `lighting-core` authority domain plus a
trusted-session service/API. It defines typed authenticated principals,
append-only grants and revocations, exact scope matching, expiry, and
deterministic decisions. Durable SurrealKV repositories reconstruct authority
and receipt state after restart. Each mutation creates a service-owned
canonical Source and full-range Episode, then stores the real Episode ID on
the durable grant or revocation. A trusted bootstrap binds each control
session to a principal, and the public mutation API accepts only
grant/revocation intent. The service owns issuer, session ID, timestamps,
generated IDs, and provenance; unknown security fields are rejected and
cross-principal revocation is denied. There is no public endpoint that creates
a control session.

Control sessions and CSRF tokens are deliberately ephemeral and excluded from
logical export/restore. Authority grants, revocations, receipts, and their
Lantern provenance are exportable and restorable. Source evidence explains an
authority operation but cannot semantically create or revoke authority.
Persistent read failures return unavailable and fail closed.

The Milestone 2 persistence/provenance acceptance suite contains all required
restart, export/restore, provenance, non-amplification, and ephemeral-secret
regressions. The local full workspace test command is additionally subject to
the host's intermittent Windows linker resource exhaustion; CI-equivalent
compile, clippy, and focused suites remain green locally.

## Phase C — Tethers bridge

The Rust Tethers host now checks host-configured authority-required
capabilities against Lantern using `lantern.authority.check/1`, records a
server-sealed decision receipt before dispatch, and records an outcome receipt
after the existing durable Trail outcome. Tethers policy remains the first
authority: Deny and Unavailable do not query Lantern, Ask remains an exact
one-time approval, and no Lantern grant can disable a Tethers gate.

The bridge is local `http://` transport only and uses the process environment
variable `LANTERN_TETHERS_AUDIT_TOKEN`; the token is not persisted or exported.
OpenShell remains outside this milestone. Cross-repository acceptance is
complete for this milestone through the clean committed branches and passing
Lantern Keeper and Tethers CI checks.

## Evidence rule

Live Nebius, Tavily, OpenShell, hosted-demo, and Devpost claims remain
unverified until their real commands or browser flows produce inspectable
evidence. Offline fixtures may prove failure behaviour and architecture but
must not be described as live provider proof.

Phase D has a candidate-only Nebius adapter with strict local response
validation. The adapter can call the OpenAI-compatible Token Factory endpoint
when configured, but live credentials/model availability have not been proved
in this workspace.

The Tavily adapter is now implemented as an untrusted evidence client. Results
are persisted as Source/Episode evidence with provider metadata and normalized
hashes, then passed to the candidate-only Dreamer. The live Tavily search,
live Nemotron interpretation, combined embedded-SurrealKV cognitive-plane
test, and existing hostile-content non-amplification regression passed. This
does not claim the public demo, a controlled hostile website, or a final UI.

## Phase D/E — live cognitive plane

Lantern Warden now exposes a bounded
`POST /api/v1/cognitive/search-interpret` route. It searches Tavily with the
basic, low-cost mode, stores each result as external Source/Episode evidence,
and invokes Nebius Token Factory using the configured NVIDIA Nemotron model.
The response includes only evidence records, a typed candidate, sanitized
provider telemetry, and explicit `canonical_mutation: false` and
`authority_changed: false` markers. It has no authority-write or execution
operation.

The exact live commands and machine-readable results are recorded in
[`NEBIUS_2026_M5.md`](NEBIUS_2026_M5.md) and `docs/hackathon/evidence/`.

## Phase G — bounded OpenShell effect boundary

The M4 working branches add one capability-specific effect lane for
`demo.export_summary@1`. Tethers remains responsible for action validation,
capability resolution, Tethers policy, Lantern authority when configured,
durable pre-dispatch intent, replay admission, and post-effect outcome
receipts. The OpenShell adapter receives only a `DispatchReadyAction` and
writes the fixed synthetic output path
`/sandbox/outbox/approved/summary.txt`; it cannot choose an arbitrary path or
fall back to an unsandboxed process.

On the verified Windows workstation, Docker Desktop 4.91.0 / Docker Engine
29.8.0 under WSL2 ran OpenShell 0.0.116 with a TLS/JWT-enabled local gateway.
The sandbox applied a hard Landlock policy, allowed the approved write/read,
rejected a forbidden-path write with `Permission denied`, rejected HTTPS
egress through the default-deny proxy with HTTP 403, and exposed no Lantern
control credential or audit token. The Tethers opt-in test also passed through
the real durable replay and shared result-anchor boundary.

This is local cross-repository evidence, not a claim of a hosted deployment.
The authenticated Lantern Warden authority check and server-sealed decision
and outcome receipts were exercised around one successful OpenShell effect on
the M4 closeout branches. The exact commands and environment requirements are
recorded in [`NEBIUS_2026_OPENSHELL.md`](NEBIUS_2026_OPENSHELL.md), with the
machine-readable linkage in
[`evidence/m5-full-trust-chain.json`](evidence/m5-full-trust-chain.json).

## PR #4 security closeout

- Old PR head: `caf5ab0c4b9634c922be8e850b668c8cb1e9f7d7`.
- Verified security-closeout head: `194c7c1d1c014d9dbf574569101642746fe7d142`.
- PR #4 merged normally into `master` as
  `79da8f83c1f41b8ba7ad7e92cb89fdfc9f14c4bf` after green GitHub CI.
- The closeout proves that no client-supplied field can make an authenticated
  session issue or revoke authority as another principal. The focused API
  suite has seven passing regression tests, and the full GitHub workspace
  suite passed on the merged head's source commit.
