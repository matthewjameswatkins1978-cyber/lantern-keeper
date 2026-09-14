# LanternBench

LanternBench is the deterministic behavioural harness for memory safety, reconciliation, provenance, and retrieval. Synthetic fixtures may be committed; private Matthew memory may not.

The canonical suite uses the real core and service APIs with isolated embedded stores:

```text
cargo test -p lighting-service --test lanternbench
```

The acceptance families cover:

- attribution isolation, silence, quotation, and assistant echo suppression;
- Matthew, Lucy, shared, and legacy perspective partitions;
- current versus historical retrieval, scope normalization, soft-memory recall, stale suppression, and item/token budgets;
- correction targeting through a Context Pack, provenance explanation, immutable history, and transitive stale/zombie severance.

The older `bench/lanternbench-v1.json` runner remains a separate HTTP smoke fixture for the pre-epistemic recall/context endpoints. It is not acceptance proof for the current memory model. Keep exactness, integration, and performance evidence distinct; a fast run is not evidence of epistemic correctness.
