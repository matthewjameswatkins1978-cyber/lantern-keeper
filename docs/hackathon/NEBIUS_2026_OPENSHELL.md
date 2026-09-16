# Nebius Milestone 4 — OpenShell effect boundary

This note records the local M4 acceptance boundary and the completed local
cross-repository proof. It deliberately separates verified local evidence from
hosted or compromised-host claims that remain unverified.

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

## Cross-repository closeout

The live M4 test
`host_execution::tests::m4_live_lantern_authority_openshell_chain` passed
against the local Lantern authority fixture and the real OpenShell gateway.
The fixture issued one exact synthetic grant through its trusted control seam;
the test then exercised the missing-grant denial, authenticated authority
check, Tethers policy, OpenShell effect, Trail outcome, and Lantern decision
and outcome receipt path. The final M5 artifact records the exact grant,
receipt, outcome, sandbox, and provider provenance identifiers:
[`m5-full-trust-chain.json`](evidence/m5-full-trust-chain.json).

## Not yet claimed

This checkpoint does not claim a live Nebius deployment, a controlled hostile
website, or a production gateway posture. The local gateway's TLS verification
bypass and unauthenticated-user setting are development-only choices. The
negative matrix beyond the exercised positive chain remains focused
regression/contract evidence rather than a single live end-to-end scenario.

For OpenShell's policy model and Docker/WSL support, see the
[official OpenShell policy documentation](https://docs.nvidia.com/openshell/sandboxes/policies),
[gateway configuration reference](https://docs.nvidia.com/openshell/reference/gateway-config),
and [official support matrix](https://github.com/NVIDIA/OpenShell#supported-platforms).
