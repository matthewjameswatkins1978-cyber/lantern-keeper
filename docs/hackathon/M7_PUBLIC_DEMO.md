# Lantern Warden M7 Public Demo

This is the release runbook for the Nebius-facing Lantern Warden demo. The
public image is deliberately replay-first: it exposes a synthetic judge flow
over accepted M5 evidence, not the production authority or execution plane.

## Public safety boundary

Set `WARDEN_PUBLIC_DEMO=1`. In this mode the process mounts only:

- `/` (redirects to `/console`)
- `/console` and `/console/controlled-hostile-page`
- `/health`, `/health/live`, `/health/ready`
- `/api/v1/version`
- the read-only console state endpoint and replay action endpoints

The public router does not register grant administration, revocation
administration, Tethers authority checks or receipt ingestion, memory/source
mutation, cognitive search, provider configuration, or execution routes. The
replay actions are in-memory presentation fixtures and do not call Nebius,
Tavily, Tethers, OpenShell, or a real `AuthorityService`.

The console must visibly disclose `REPLAY`, `public-demo`, synthetic Alex
inputs, offline providers, and the fact that accepted M5 execution evidence is
being replayed. Do not describe a public browser interaction as a fresh
production grant or execution.

## Build and run

From a clean checkout:

```powershell
docker build --platform linux/amd64 -t lantern-warden/demo:m7 .
docker run --rm --name lantern-warden-m7 -p 4317:4317 lantern-warden/demo:m7
```

The image is multi-stage, uses the pinned Rust toolchain for the build, runs a
minimal Debian runtime as the non-root `lantern` user, and stores only the
container-local embedded SurrealKV data under `/data/surrealkv`. Do not add
`.env`, API keys, local databases, or private migration material to the build
context.

Check:

```powershell
curl.exe --fail http://127.0.0.1:4317/health
curl.exe --fail http://127.0.0.1:4317/console
```

The health response is intentionally sanitized and reports `mode` as
`public-demo` and `live_cognition` as `false` for the release image.

## Judge path

1. Open `/console` and confirm the public-demo/replay disclosure.
2. Reset the fixture.
3. Run the hostile attempt and observe `DENY` with
   `NO_AUTHENTICATED_GRANT`.
4. Show the accepted replay grant, then retry the action.
5. Observe `ALLOW` only as replayed accepted M5 evidence, with the linked
   OpenShell, Trail, and receipt references.
6. Show wrong-scope denial and accepted-revoke denial.

The fixed replay references come from the committed M5 evidence artifact. They
are not newly issued public credentials or authority.

## Release evidence

The machine-readable release record is
[`docs/hackathon/evidence/m7-public-demo.json`](evidence/m7-public-demo.json).
It records the exact image identity, clean-room build, endpoint identity (when
deployed), health result, route probes, restart result, CI state, cost-control
state, and limitations. Unknown or unavailable deployment fields must remain
explicitly unverified; never fill them with placeholders that look like live
facts.

## Claims and limitations

This milestone demonstrates a safe public presentation of the accepted trust
chain. It does not claim that a public visitor can administer authority,
exercise Tethers, invoke OpenShell, spend provider credits, search arbitrary
Tavily content, or run live Nemotron cognition. Live cognition remains a
local-only optional M6 capability and is disabled in the public release.

