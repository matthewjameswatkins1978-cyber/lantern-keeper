# Lantern Inquiry v0.1 Evaluation

Status: deterministic implementation evaluation for the experimental branch. This is evidence about the bounded mechanism, not a claim of general model-level reasoning improvement.

## Comparison

| Case | Existing Lantern path | Inquiry path |
| --- | --- | --- |
| Competing explanations | Claims and Beliefs can preserve source lineage, but there is no typed competing-hypothesis lifecycle. | An Inquiry requires at least two typed hypotheses and keeps their current status together. |
| New observation | A source, episode, or claim can be captured. | Exact observation text is first captured as Source/Episode evidence, then linked to one hypothesis in a new InquiryRevision. |
| Revision history | Existing claim/belief history is available through their own projections. | Revisions preserve the complete hypothesis and evidence snapshot, previous revision ID, lesson, and trace ID. |
| Next useful action | No inquiry-specific distinguishing question is emitted. | A deterministic bounded question identifies unknowns, assumptions, the competing hypotheses, the distinguishing observation, and the cheapest safe test. |
| Governance | Dreamer candidates and Foreman proposals remain candidate/review state. | Dreamer may create a pending `inquiry_hypothesis` proposal; Foreman remains the review gate and no Belief is changed automatically. |

## Frozen scenarios

### Technical freeze

Question: why does an application freeze?

Hypotheses:

1. CPU saturation.
2. Lock or contentions.

Observation: the freeze occurred with low CPU utilisation.

Result: the CPU hypothesis becomes `Challenged`; it is not deleted. The exact observation remains source evidence, the revision history contains both hypotheses, and the next question is to capture one bounded thread dump during a reproducible freeze.

### Cross-domain seedling

Question: will this seedling survive transplant?

Hypotheses:

1. Root shock is the limiting factor.
2. Moisture mismatch is the limiting factor.

The same inquiry/revision/evidence structure applies without debug-specific assumptions. This demonstrates schema generality, not domain expertise.

## Adversarial checks

- Derived evidence and model restatements do not count as independent original sources.
- Hostile or instruction-shaped evidence is stored as evidence data and does not become an instruction to Lantern.
- A contradictory observation changes hypothesis status but does not erase the competing explanation or its prior revision.
- A Dreamer candidate is validated for subject and evidence shape before entering the pending proposal queue.

## Verification evidence

- `cargo test -p lighting-core`: 72 passed.
- `cargo test -p lighting-service --test epistemic_reconciliation`: 5 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed after the final correction.
- Focused package checks for `lighting-core`, `lighting-service`, `lighting-store-surreal`, and `lighting`: passed after the final correction.
- The full workspace test reached linking but could not complete because the installed Visual Studio toolchain lacks `atlmfc\\lib\\x64\\secur32.lib`; the linker command also exposed stale cross-checkout target artifacts. This is an environment gate, not evidence of a code test pass.

## Decision

The bounded vertical slice is suitable for Lucy review as an experimental mechanism: it makes competing hypotheses, provenance, contradiction, revision, and next-question selection explicit and inspectable. It does not yet justify a claim that Lantern generally improves reasoning. A later gate should compare model-in-the-loop tasks across domains and measure whether the typed inquiry path improves evidence preservation and distinguishing-question quality without increasing unsupported certainty.
