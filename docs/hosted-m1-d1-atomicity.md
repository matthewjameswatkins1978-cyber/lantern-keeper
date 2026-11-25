# D1 atomicity requirement (M1 → future D1 slice)

**Mission:** LK-HOST-001 v1.1 · **Status:** requirement note only. No D1 code, no
provisioning, no wrangler lane. This records what the future D1 adapter must
satisfy, and why the native path cannot satisfy it as it stands.

## What the native path does today

**Updated by Q18 (correction-consistency):** the native correction write set is
now ONE transaction. `record_correction` and `NativeStagedEnvelopeWriter` both
resolve the dedupe lookups first and then commit Source, Episode, Claim, the
CAS-guarded projection transition (trace + revisions) and the receipt together
through `EpistemicRepository::apply_correction_batch` — a single
`BEGIN … COMMIT` whose every statement is inspected (`refuse_statements`).

The two previously measured consequences of the staged design are retired with
proof in `crates/lighting-service/tests/write_envelope_m1.rs`:

1. **Partial commit — retired.** `native_envelope_transaction_rolls_back_every_stage`
   injects a failing projection statement and asserts the Source, Claim and
   revisions from the same attempt are all rolled back, with no receipt.
2. **False receipt — retired.** A rolled-back batch returns `Err` (statement
   inspection, `c57907f`), and the receipt lives inside the same transaction,
   so no receipt can describe work that did not commit.

`NATIVE_PARTIAL_COMMIT_RISK` now reads `RETIRED by Q18 …` so existing
references notice the change instead of implying the old risk. The direct
transition path (`apply_belief_transition_guarded`) additionally binds the read
revision token at the write with a THROW guard (consumed-token CAS, Q18 gap 3).

The D1 requirements below still bind the future hosted adapter.

## What the D1 adapter must satisfy

1. **One boundary.** Source exact bytes/hash, Episode, Claim, Belief revision,
   affected stale/dependency projections, idempotency record and mutation receipt
   commit in one verifiable all-or-nothing boundary, or none of them do.
2. **Database-enforced precondition.** The expected revision must abort the whole
   logical transition. A conditional `UPDATE` that affects zero rows is **not**
   rollback and does not satisfy this; if bounded SQL cannot express it, choose a
   single-writer variant rather than inventing distributed transactions.
3. **Statement failures surface.** Every statement's result must be inspected. A
   batch that did not commit must never be reported as accepted, and no receipt
   may be written for work that is not durable.
4. **Idempotency inside the boundary.** The key is
   `principal|scope_hash|caller_key`; payload hash covers the canonical envelope
   including `lantern-keeper-write-envelope-v1`. Exact retries recover the original
   receipt; a changed payload under the same binding conflicts; a receipt from
   another envelope version is refused.
5. **Traversal capped before commit.** Dependency/stale traversal is bounded
   before the write; overflow returns a typed nonacceptance with no partial commit.
6. **Reads write nothing.** `status`, `search`, `context` and `why` perform zero D1
   writes — no persisted Context Pack or Trace per invocation.

## Acceptance evidence for that slice

Reuse the existing contract tests as the D1 checklist, since they already assert
the required behaviour against the bounded surface:

- `storage_contract_m1.rs`: `bounded_pages_are_keyset_ordered_without_omission`,
  `bounded_page_rejects_invalid_cursor_and_limit`,
  `idempotent_transition_replays_without_duplicate_records`,
  `idempotency_key_binds_principal_scope_and_payload_hash`,
  `stale_revision_is_refused_without_partial_state`,
  `concurrent_idempotent_transitions_commit_exactly_one`,
  `rejected_statement_surfaces_as_failure_not_success`,
  `dependency_traversal_is_bounded_and_fails_closed`.
- `write_envelope_m1.rs`: the envelope contract, plus the two native consequences
  above as the behaviour the D1 adapter must **not** inherit.

A D1 adapter is not accepted until the envelope tests run against it with the same
assertions, an injected mid-write fault leaves no orphan revision or receipt, and
the two native consequences above are demonstrated to be impossible there.

## Explicitly out of scope here

No D1 schema, no migrations, no wrangler configuration, no Cloudflare
provisioning, no paid resources, no live memory. Those need separate approval
(LK-HOST-001 v1.1 §5 and the M4 gate).
