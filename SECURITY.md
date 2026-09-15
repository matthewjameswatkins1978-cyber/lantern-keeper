# Security boundary

Lantern Keeper is a memory trust kernel, not a claim that an LLM is
trustworthy. A compromised or prompt-injected reasoning model may influence
epistemic content, but it cannot acquire operational authority from that
content.

The independent authorization path is:

```text
authenticated Principal -> AuthorityGrant -> deterministic check -> effect
```

`ActorId`, `PrincipalId`, `AuthorityGrant`, and `AuthorityRevocation` are
separate concepts. Dreamer candidates, retrieved sources, Claims, Beliefs,
and external evidence expose no operation that creates or revokes authority.
Authority writes in the current service require a server-created,
principal-bound control session and CSRF token. The HTTP mutation API accepts
only grant/revocation intent: issuer, session ID, timestamps, identifiers, and
provenance are constructed by the service. Unknown security fields are
rejected. A revocation is accepted only when the authenticated session
principal is the grant issuer. Session creation is a trusted UI/demo bootstrap
operation and is not exposed on an agent or public HTTP route.

The current process-local service uses a service-owned
`authority-control-*` Episode ID as an auditable provenance placeholder. It is
not a client-supplied claim and does not yet represent a persisted Lantern
Source/Episode record; that bridge remains a separate acceptance gate.

The receipt core records canonical JSON, a SHA-256 hash, and the previous
receipt hash. This proves content continuity and ordering. It does not prove
signer identity or secure a compromised host.

Known limitations and the remaining Tethers, Tavily, OpenShell, persistence,
and deployment work are tracked in [`CURRENT.md`](CURRENT.md) and
[`THREAT_MODEL.md`](THREAT_MODEL.md).
