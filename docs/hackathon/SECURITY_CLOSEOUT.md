# PR #4 security closeout

PR #4's authority mutation boundary is closed for the current process-local
implementation.

## Enforced boundary

- `AuthenticatedControlSession` stores session ID, CSRF token, bound
  `PrincipalId`, issue time, and expiry.
- Session creation is a trusted service/bootstrap operation and is not an HTTP
  or Dreamer capability.
- Grant HTTP requests contain only delegate, capability, version, scope,
  expiry, and constraints. The service creates the grant ID, issuer, session
  ID, timestamps, and provenance.
- Revocation HTTP requests contain only `grant_id`. The service creates the
  revocation ID, issuer, session ID, timestamp, and provenance.
- Unknown security fields are rejected. The ledger also requires a revocation
  issuer to match the original grant issuer.
- Each mutation receives a generated `authority-control-*` Episode ID. This is
  a service-owned placeholder until a real persisted Lantern Source/Episode
  event is bridged; clients cannot select it.

## Regression evidence

The focused API integration suite covers missing and wrong CSRF, principal
spoofing, server-owned grant and revocation session IDs, arbitrary provenance,
cross-principal revocation, and expired sessions. Existing Dreamer and
external-evidence non-amplification tests remain in the suite.

This closeout does not claim persisted authority, a Tethers provider/Trail
bridge, genuine OpenShell enforcement, or live Nebius/Tavily proof.
