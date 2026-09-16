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

Authority grants, revocations, and execution receipts are persisted in the
SurrealKV-backed store and are restored into the authority decision path after
restart. Each mutation first records canonical authenticated evidence as a
Lantern PlainText Source and an Episode whose SourceRange covers that evidence;
the resulting Episode ID is then stored on the grant or revocation. The
Source/Episode is audit provenance only: reading or creating evidence never
creates, changes, or revokes authority.

Control-session bindings and CSRF tokens are deliberately ephemeral. They are
not exported or persisted as a session registry; a new trusted bootstrap must
create a fresh session after restart. Persisted session IDs on authority
records are audit bindings, not reusable authentication material. If durable
authority state cannot be read, checks return an unavailable error and do not
fall back to an empty ledger or allow an effect.

Logical export/restore includes authority grants, revocations, and receipts,
alongside their Lantern provenance records. It excludes control sessions,
CSRF tokens, and private authentication material.

The receipt core records canonical JSON, a SHA-256 hash, and the previous
receipt hash. This proves content continuity and ordering. It does not prove
signer identity or secure a compromised host.

## OpenShell effect boundary

The M4 effect lane preserves the same separation. Tethers policy and Lantern
authority remain before dispatch; OpenShell is only the final sandbox effect
enforcer. The adapter accepts a Tethers `DispatchReadyAction` for the one
synthetic `demo.export_summary@1` capability, uses a fixed approved output
path, and clears the child environment except for the Windows system-path
allowlist needed to start WSL. It does not read or forward Lantern control
credentials, audit tokens, or provider secrets, and it has no unsandboxed
fallback or retry.

The verified local sandbox uses hard Landlock filesystem rules, a writable
approved outbox only, and default-deny network egress. The forbidden outbox
write and HTTPS egress probes were run against the real OpenShell gateway and
returned denial. `gateway-insecure` and local unauthenticated-user mode are
development-only settings for the self-signed local gateway and are not a
production deployment posture.

The remaining gate is a single cross-repository run showing a live Lantern
authority decision receipt and outcome receipt surrounding the OpenShell
effect. The current local evidence must not be presented as Nebius deployment
or as proof that a compromised host cannot bypass its own operating system.

Known limitations and the remaining Tethers, Tavily, OpenShell, persistence,
and deployment work are tracked in [`CURRENT.md`](CURRENT.md) and
[`THREAT_MODEL.md`](THREAT_MODEL.md).
