# Nebius Milestone 3 — Tethers authority bridge

This milestone connects the Rust Tethers host to Lantern's persistent
authority ledger without moving policy ownership into Lantern.

The host performs the ordered gates: resolve the exact pinned capability,
validate its schema and existing Tethers scope, evaluate Tethers policy, then
ask Lantern only when the host configuration marks that capability as
authority-required. Tethers `DENY` and `UNAVAILABLE` stop before the Lantern
call; `ASK` remains an exact one-time Tethers approval and is not converted by
an active Lantern grant. A Lantern deny also stops before provider dispatch.

The wire contracts are `lantern.authority.check/1` and
`lantern.receipt.intent/1`. Tethers sends only host-established action,
principal, capability/version, canonical scope, and empty-by-default
constraints. Lantern supplies the decision clock, receipt ID, predecessor,
canonical hash, and timestamps. Tethers never submits a pre-sealed receipt.

Decision receipts are sent before dispatch. Successful, failed, and uncertain
provider outcomes are sent only after the existing durable Tethers Trail
outcome. A post-execution Lantern sync failure is surfaced as an audit failure
and is never retried, because the external effect may already have happened.

The bridge uses a small host-owned `AuthorityProvider` seam and a bounded
standard-library HTTP client for the local `http://` Lantern endpoint. The
`LANTERN_TETHERS_AUDIT_TOKEN` is read from process environment only; it is not
runtime configuration, action input, receipt content, export material, or an
OpenShell secret.

The optional authority configuration is host-owned:

```json
{
  "authority": {
    "endpoint": "http://127.0.0.1:4317",
    "principal_id": "agent:lucy",
    "required_capabilities": [
      { "name": "demo.export_summary", "version": 1 }
    ]
  }
}
```

No OpenShell capability or enforcement claim is made by this milestone.
