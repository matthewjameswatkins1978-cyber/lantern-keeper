# Lantern Warden M6 Trust Console

The Trust Console is a local, demo-gated judge surface for the accepted M4/M5
chain. It makes four distinct states visible:

1. **HEARD** — external evidence and Source → Episode provenance.
2. **THOUGHT** — candidate interpretation only.
3. **AUTHORISED** — Tethers policy plus the authenticated Warden decision.
4. **DONE** — OpenShell, Trail, and receipt outcome.

## Run locally

Use the normal embedded SurrealKV configuration and set
`LANTERN_TRUST_CONSOLE=1` before starting the Lighting service. The console is
then available at `http://127.0.0.1:4317/console`.

The browser receives only the sanitized console state. Provider credentials,
the Lantern service credential, audit token, and control-session values remain
server-side. `GRANT AUTHORITY` and `REVOKE` delegate to the existing
authenticated `AuthorityService` control path.

## Primary judge path

1. Select **RESET DEMO**.
2. Select **RUN ATTACK** and observe `DENY · NO_AUTHENTICATED_GRANT`.
3. Select **GRANT AUTHORITY** for `demo-agent`,
   `demo.export_summary@1`, and the narrow M6 demo destination.
4. Select **RETRY ACTION** and observe the accepted execution evidence,
   explicitly labelled `REPLAY`.
5. Select **TRY DIFFERENT DESTINATION** or **REVOKE**, then retry to observe
   the fail-closed denial.

The controlled hostile fixture is available at
`/console/controlled-hostile-page`. It is local synthetic evidence; public
hosting and Tavily indexing are not claimed. The **LIVE cognitive lookup**
toggle calls the existing Tavily → Source/Episode → Nemotron route when those
providers are configured. Live mode never turns candidate output into
authority, and the execution display remains accepted replay evidence.

Machine-readable evidence is in
`docs/hackathon/evidence/m6-trust-console.json`.
