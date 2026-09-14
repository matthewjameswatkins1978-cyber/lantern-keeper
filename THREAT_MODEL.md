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
the append-only AuthorityGrant/AuthorityRevocation ledger, the deterministic
Tethers decision path, the OpenShell enforcement boundary, local
session/signing material, and Lantern's immutable provenance records.

The LLM, retrieved content, Claims, Beliefs, Context Packs, Dreamer, and
summaries are untrusted or epistemic inputs. They may be retained as evidence
and may affect reasoning, but there is deliberately no automatic edge from a
Belief to an AuthorityGrant.

## Required invariants

1. `ActorId` and authenticated `PrincipalId` are different types and meanings.
2. Dreamer and external evidence have no authority-write capability.
3. Grants are issued only by an authenticated control-plane action.
4. Revocations are authenticated append-only events, not model interpretations.
5. Exact capability, version, scope, constraints, time, principal, and
   revocation state are checked deterministically before an effect.
6. Denial, approval, and execution evidence are inspectable and replayable.
7. Uncertain effects are never retried blindly without an idempotency proof.

## Out of scope

This project does not claim to detect every malicious source, certify the truth
of a Claim, secure a compromised host kernel, or authenticate a person merely
because an epistemic Actor record uses that person's name.
