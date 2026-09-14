# Nebius × NVIDIA 2026 Baseline

Recorded: 2026-09-14

This is the before/after boundary for the hackathon work. The baseline is
copied from the clean local Lantern feature checkout; it is not a rewritten
history or a claim that pre-existing work was created for this event.

## Revisions

| Repository | Baseline branch | Baseline SHA | Evidence |
| --- | --- | --- | --- |
| Lantern Keeper | `feature/lantern-full-move` | `d3b48d0b75d88b2a66a1728e5af495b6ca2e77ea` | clean local authoritative checkout |
| Tethers | `main` | `ddc5d0fccfc0a38ff0af3013c5e9ff6ba357e0b2` | remote `origin/main` tip verified 2026-09-14 |

The hackathon branch starts at the Lantern SHA above. The local Tethers
working checkout was on an unrelated dirty implementation branch and was not
used as a base or modified by this build.

## Already present

- Source, Episode, Project, Claim, Belief, Memory Item, Relation, Trace, and
  Proposal domain records.
- Explicit perspective and provenance, current/history separation,
  correction, supersession, stale invalidation, and bounded Context Packs.
- Bounded Lucy-owned Foreman review and candidate-only Dreamer foundations.
- Logical export/restore, embedded SurrealKV, a remote SurrealDB qualification
  lane, and the existing LanternBench suite.
- A bounded, read-only Tethers preview bridge.
- Rust 1.98.1 / Edition 2024 and SurrealDB 3.3.0-beta.4.

## Baseline validation

- `cargo fmt --all -- --check`: passed.
- `cargo check --locked --workspace --all-targets --all-features`: passed.
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Offline serialized workspace tests: passed; 158 unit tests, 32 integration
  tests, and the pre-existing ignored live-Tethers test were observed green in
  the baseline run. The full test report also included the existing embedded
  export/restore and LanternBench suites.
- A second full workspace test compilation was interrupted during the
  repository's disposable remote lane before a complete report was emitted;
  that interrupted invocation is not counted as a pass.

## Existing integrations

- Embedded and remote SurrealDB/SurrealKV storage.
- Local HTTP and stdio MCP surfaces.
- Optional Tethers 0.1 preview through an external engine process.
- No verified Nebius Token Factory, Tavily, OpenShell, or public demo
  integration exists at this baseline.

## Known incomplete areas

The current documentation identifies fresh Basic Memory delta intake, richer
typed/graph retrieval, full Dreamer operations, and hosted product paths as
partial or pending. The baseline has no independent authenticated Principal
domain, AuthorityGrant ledger, authority control plane, effect receipt path,
Nemotron adapter, Tavily hostile-evidence lane, OpenShell demo worker, or
adversarial authority benchmark.
