# Lantern Keeper Threat Model

## Security claim

Lantern Keeper does not prevent prompt injection and does not make an LLM
trustworthy. Its narrower claim is:

> A compromised reasoning model does not automatically acquire operational
> authority.

Memory can influence thought. Only explicit authority can permit action.

## Attacker-controlled inputs

The model and the authority boundary assume an attacker may control web pages,
documents, retrieved text, tool responses, imported memory, prompts, model
output, Dreamer proposals, summaries, paraphrases, and repeated derived
observations.

The attacker may convince the reasoning model that an unauthorised action is
legitimate. That conviction is not an authority signal.

## Trust boundaries

The narrow trusted computing base is the authenticated authority control plane,
its trusted session bootstrap, the append-only
AuthorityGrant/AuthorityRevocation ledger, the deterministic Tethers decision
path, the OpenShell enforcement boundary, local session/signing material, and
Lantern's immutable provenance records.

The LLM, retrieved content, Claims, Beliefs, Context Packs, Dreamer, and
summaries are untrusted or epistemic inputs. They may be retained as evidence
and may affect reasoning, but there is deliberately no automatic edge from a
Belief to an AuthorityGrant.

## Required invariants

1. `ActorId` and authenticated `PrincipalId` are different types and meanings.
2. Dreamer and external evidence have no authority-write capability.
3. A control session binds a server-established principal, CSRF token, issue
   time, and expiry; agent input cannot create or change that binding.
4. The public grant/revocation API accepts intent only. IDs, issuer, session,
   timestamps, and provenance are server-owned.
5. Grants are issued only by an authenticated control-plane action.
6. A revocation is accepted only from the grant's issuer principal and is an
   authenticated append-only event, not a model interpretation.
7. Exact capability, version, scope, constraints, time, principal, and
   revocation state are checked deterministically before an effect.
8. Denial, approval, and execution evidence are inspectable and replayable.
9. Uncertain effects are never retried blindly without an idempotency proof.

## Out of scope

This project does not claim to detect every malicious source, certify the truth
of a Claim, secure a compromised host kernel, or authenticate a person merely
because an epistemic Actor record uses that person's name.

## Authority persistence and provenance

Authority grants, revocations, and execution receipts are durable SurrealKV
records. On restart, the service reconstructs the append-only authority ledger
from persisted grants and revocations; failed persistent reads are explicit
unavailability and fail closed rather than using an empty in-memory ledger.
Logical export/restore preserves active grants, revoked grants, receipts, and
their Source/Episode provenance.

For each authority mutation, the service writes canonical authenticated
operation evidence to a PlainText Source and creates an Episode with a full
SourceRange over that evidence. The operation stores that Episode ID as
provenance. The semantic memory path has no operation that can interpret this
Source as a grant: evidence can explain an authority decision, but it cannot
confer authority.

Control sessions and CSRF tokens are intentionally runtime-only. They are not
persisted or exported, so restart requires a fresh trusted session bootstrap;
the session ID retained on a grant or revocation is an audit association, not
authentication material.

Tethers, genuine OpenShell enforcement, trust-console work, and live provider
proof remain separate hardening gates.

# Milestone 3 authority bridge boundary

The Tethers bridge is a separate authenticated service boundary. Lantern
checks only the host-supplied principal, resolved capability/version, exact
canonical Tethers scope, and constraints; it does not parse planner arguments
or decide Tethers policy. Tethers DENY, UNAVAILABLE, ASK, schema failure,
scope failure, replay failure, and Trail failure remain decisive before any
provider effect.

The bridge token is process environment material (`LANTERN_TETHERS_AUDIT_TOKEN`)
and is not accepted in request bodies, runtime configuration, receipts,
exports, or OpenShell inputs. Lantern owns receipt IDs, timestamps, predecessor
links, and hashes. Outcome receipt failure after provider invocation is an
audit failure and is not retried.
