# Lantern Warden × Tethers × NVIDIA OpenShell — Milestone 4 closeout

This note records the local M4 acceptance boundary. It deliberately separates
the contract/mock lane from the real OpenShell and live Lantern Warden
cross-repository lane. It is not a Nebius deployment claim.

## Verified workstation lane

- Windows WSL2 with Docker Desktop 4.91.0 and Docker Engine 29.8.0.
- OpenShell CLI/gateway 0.0.116.
- Local TLS/JWT-enabled gateway on loopback port 17670; health endpoint passed.
- Docker compute driver and a `Ready` named sandbox using hard Landlock.
- Tethers M4 closeout branch: `codex/openshell-m4-closeout`.

The portable fixture and reproduction notes live in the Tethers worktree at
`tethers-0.1/host-rust/tests/openshell/`. The local gateway copy and generated
TLS/JWT material stay outside Git.

## Observed effect probes

The synthetic line `Lantern synthetic summary` was written through
`openshell sandbox exec` to
`/sandbox/outbox/approved/summary.txt` and read back successfully. A write to
`/sandbox/outbox/forbidden/summary.txt` failed with `Permission denied`.
`curl https://example.com` from the sandbox failed with `CONNECT tunnel failed,
response 403`. A filtered environment probe returned no Lantern, token, key,
OpenAI, or Anthropic variables.

The Tethers OpenShell-only opt-in test
`openshell_executor::tests::valid_grant_executes_once_in_openshell` passed with
`LANTERN_OPENSHELL_E2E=1`. It uses the existing Tethers policy/resolution,
durable replay admission, shared boundary, supervised child, and result-anchor
path. OpenShell errors map to a fail-closed uncertain outcome; the adapter has
no unsandboxed fallback or retry.

## Live Lantern Warden authority-to-effect run

The closeout fixture is
`crates/lighting-service/examples/m4_live_authority_server.rs`. It uses the
existing trusted in-process control-session seam to seed one exact grant, then
exposes only the existing bearer-authenticated Tethers authority check and
receipt routes over loopback. It uses embedded SurrealKV persistence; no
public session or grant bootstrap route is added.

From the Lantern repository, use an isolated store and start the fixture:

```powershell
$env:LANTERN_TETHERS_AUDIT_TOKEN = '<local-test-token>'
$env:LANTERN_M4_AUTHORITY_STORE = Join-Path $env:TEMP 'lantern-m4-authority-evidence'
$env:LANTERN_M4_AUTHORITY_BIND = '127.0.0.1:4318'
cargo run -p lighting-service --example m4_live_authority_server
```

In a second terminal, from the Tethers repository, run the gated live test:

```powershell
$env:LANTERN_OPENSHELL_E2E = '1'
$env:LANTERN_M4_AUTHORITY_ENDPOINT = 'http://127.0.0.1:4318'
$env:LANTERN_TETHERS_AUDIT_TOKEN = '<local-test-token>'
cargo test host_execution::tests::m4_live_lantern_authority_openshell_chain -- --exact --nocapture
```

The test verifies the actual ordering: Tethers policy and binding, live
authenticated Lantern Warden authority check, decision receipt, durable
Tethers intent/replay admission, guard admission, one OpenShell sandbox
effect, Tethers Trail outcome, and the Lantern outcome receipt. The OpenShell
executor never receives the bearer token and has no unsandboxed fallback or
retry. The action supplies one real opaque scope value (`sandbox/...`); guarded
consequential execution must never use an empty or dummy scope.

## Negative matrix

The authority API tests cover missing authentication, no matching grant,
scope mismatch, revoked grants, expired grants, and malformed/unknown receipt
fields. Tethers tests cover policy DENY, missing/unavailable authority,
guard rejection and indeterminate admission with zero provider calls, and
result-anchor taxonomy preservation. The real OpenShell lane covers gateway
or sandbox unavailability as fail-closed provider uncertainty, hard-Landlock
forbidden-path denial, default-deny network egress, and credential isolation.

## Not yet claimed

This closeout does not claim a live Nebius deployment. The local gateway's TLS
verification bypass and unauthenticated-user setting are development-only
choices. A compromised host, gateway, Docker Desktop, kernel, or operator is
outside this proof.

For OpenShell's policy model and Docker/WSL support, see the
[official OpenShell policy documentation](https://docs.nvidia.com/openshell/sandboxes/policies),
[gateway configuration reference](https://docs.nvidia.com/openshell/reference/gateway-config),
and [official support matrix](https://github.com/NVIDIA/OpenShell#supported-platforms).
