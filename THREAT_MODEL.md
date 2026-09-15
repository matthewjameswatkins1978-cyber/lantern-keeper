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

## Current authority provenance limitation

Authority mutation currently runs against a process-local ledger. The service
generates an `authority-control-*` Episode ID for each grant or revocation so
the provenance field cannot be supplied by a client, but the ID is not yet a
persisted Source/Episode event. Persisted authority, the real Lantern
Source/Episode bridge, Tethers, and OpenShell remain separate hardening work.
