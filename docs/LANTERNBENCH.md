# LanternBench

LanternBench is the deterministic behavioural harness for memory safety, reconciliation, provenance and retrieval. Synthetic fixtures may be committed; private user memory must not be committed.

The canonical suite uses the real core and service APIs with isolated embedded stores:

```bash
cargo test -p lighting-service --test lanternbench
```

The acceptance families cover:

- attribution isolation, silence, quotation and assistant echo suppression;
- human, assistant, shared and legacy perspective partitions;
- current-versus-historical retrieval, scope normalisation, soft-memory recall, stale suppression and item/token budgets;
- correction targeting through a Context Pack, provenance explanation, immutable history and transitive stale/zombie severance.

Historical fixtures may retain the actor labels used while Lantern was being developed. Those labels are test data, not architectural requirements.

The older `bench/lanternbench-v1.json` runner remains a separate HTTP smoke fixture for the pre-epistemic recall/context endpoints. It is not acceptance proof for the current memory model.

Keep correctness, integration and performance evidence distinct: a fast run is not evidence of epistemic correctness.
