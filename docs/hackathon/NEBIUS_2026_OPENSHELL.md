# Nebius Milestone 4 — OpenShell effect boundary

This note records the local M4 acceptance boundary. It deliberately separates
verified local evidence from the remaining cross-repository acceptance gate.

## Verified workstation lane

- Windows WSL2 with Docker Desktop 4.91.0 and Docker Engine 29.8.0.
- OpenShell CLI/gateway 0.0.116.
- Local TLS/JWT-enabled gateway on loopback port 17670; health endpoint passed.
- Docker compute driver and a `Ready` named sandbox using hard Landlock.
- Tethers M4 branch: `codex/openshell-m4`.

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

The Tethers opt-in test
`openshell_executor::tests::valid_grant_executes_once_in_openshell` passed with
`LANTERN_OPENSHELL_E2E=1`. It uses the existing Tethers policy/resolution,
durable replay admission, shared boundary, supervised child, and result-anchor
path. OpenShell errors map to a fail-closed uncertain outcome; the adapter has
no unsandboxed fallback or retry.

## Not yet claimed

This checkpoint does not claim a live Nebius deployment or a complete
cross-repository run in which a live Lantern authority decision receipt and
outcome receipt surround the OpenShell effect. That run remains the next hard
acceptance gate. The local gateway's TLS verification bypass and
unauthenticated-user setting are development-only choices.

For OpenShell's policy model and Docker/WSL support, see the
[official OpenShell policy documentation](https://docs.nvidia.com/openshell/sandboxes/policies),
[gateway configuration reference](https://docs.nvidia.com/openshell/reference/gateway-config),
and [official support matrix](https://github.com/NVIDIA/OpenShell#supported-platforms).
