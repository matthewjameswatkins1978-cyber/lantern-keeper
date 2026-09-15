# Nebius × NVIDIA 2026 Build Log

This living checklist records only work actually implemented and verified.

## Phases

- [x] A — preflight, isolated branch, and baseline record.
- [x] B — independent Principal and authority foundation (persistent ledger,
  provenance, receipts, and control-plane API).
- [x] C — Tethers decision bridge and receipts (local HTTP bridge, persistent
  decision/outcome receipts, and fail-closed execution gates).
- [x] D — bounded Nemotron Dreamer and candidate validation (live provider
  proof is still pending).
- [ ] E — Tavily external-evidence lane and non-amplification tests.
- [ ] F — adversarial benchmark and baseline comparison.
- [ ] G — genuine OpenShell effect boundary.
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
are converted into explicitly external Dreamer evidence and have no authority
or canonical-mutation operation. Live Tavily credentials and a hostile-world
run remain unverified, so Phase E is still open.

## PR #4 security closeout

- Old PR head: `caf5ab0c4b9634c922be8e850b668c8cb1e9f7d7`.
- Verified security-closeout head: `194c7c1d1c014d9dbf574569101642746fe7d142`.
- PR #4 merged normally into `master` as
  `79da8f83c1f41b8ba7ad7e92cb89fdfc9f14c4bf` after green GitHub CI.
- The closeout proves that no client-supplied field can make an authenticated
  session issue or revoke authority as another principal. The focused API
  suite has seven passing regression tests, and the full GitHub workspace
  suite passed on the merged head's source commit.
