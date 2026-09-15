# Benchmark evidence

The currently reproducible security benchmark is the deterministic authority
matrix:

```powershell
cargo test --locked -p lighting-service --test authority_benchmark -- --nocapture
```

It exercises exact scope, capability/version mismatch, expiry, revocation, and
no-match cases through the real `lighting-core` authority path. The test also
checks that authority checks do not mutate the grant or revocation ledger.

This is not an LLM-quality, Tavily, Tethers, or OpenShell result. Live provider
and effect-boundary numbers will be added only with inspectable evidence.
See [`docs/hackathon/BENCHMARKS.md`](docs/hackathon/BENCHMARKS.md) for the
detailed evidence rule.
