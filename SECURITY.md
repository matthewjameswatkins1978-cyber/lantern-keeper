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
Authority writes in the current service require a server-created control
session and CSRF token; public routes only list, explain, or check authority.

The receipt core records canonical JSON, a SHA-256 hash, and the previous
receipt hash. This proves content continuity and ordering. It does not prove
signer identity or secure a compromised host.

Known limitations and the remaining Tethers, Tavily, OpenShell, persistence,
and deployment work are tracked in [`CURRENT.md`](CURRENT.md) and
[`THREAT_MODEL.md`](THREAT_MODEL.md).
