# LanternBench

LanternBench is the deterministic behavioural harness for memory safety,
reconciliation, and retrieval. Synthetic fixtures may be committed; private
Matthew memory may not. The current executable suite is
`cargo test -p lighting-service --test lanternbench` and uses the real core and
service APIs with isolated embedded stores.

The suite covers these acceptance families:

- attribution isolation, silence, quotation, and assistant echo suppression;
- Matthew, Lucy, shared, and legacy perspective partitions;
- current versus historical retrieval, scope normalization, soft-memory recall,
  stale suppression, and item/token budgets;
- correction targeting through a Context Pack, provenance explanation,
  immutable history, and transitive stale/zombie severance.

The older `bench/lanternbench-v1.json` PowerShell runner remains a separate
HTTP smoke fixture for the pre-epistemic recall/context endpoints. It is not
the acceptance proof for the current memory model.

The existing `bench/lanternbench-v1.json` and runner remain the starting point.
The full packet's acceptance report must keep exactness, integration, and
performance evidence distinct.
