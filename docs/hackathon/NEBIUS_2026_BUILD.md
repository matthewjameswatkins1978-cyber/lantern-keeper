# Nebius × NVIDIA 2026 Build Log

This living checklist records only work actually implemented and verified.

## Phases

- [x] A — preflight, isolated branch, and baseline record.
- [x] B — independent Principal and authority foundation (local ledger and
  control-plane API).
- [ ] C — Tethers decision bridge and receipts (receipt core exists; bridge is
  still pending).
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
deterministic decisions. A trusted bootstrap binds each control session to a
principal, and the public mutation API accepts only grant/revocation intent.
The service owns issuer, session ID, timestamps, generated IDs, and the
`authority-control-*` provenance placeholder; unknown security fields are
rejected and cross-principal revocation is denied. There is no public endpoint
that creates a control session.

The placeholder is intentionally honest: it prevents arbitrary client
provenance but is not yet a persisted Lantern Source/Episode event. Persisted
authority and the real Source/Episode bridge remain out of this closeout.

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
