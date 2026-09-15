# Lantern Keeper benchmark evidence

The deterministic authority matrix is the first hackathon benchmark slice. It
covers no-match, exact-scope mismatch, capability-version mismatch, expiry,
and revocation cases using the real `lighting-core` authority decision path.
It does not make a semantic claim about an LLM and does not require Nebius,
Tavily, OpenShell, or a live database.

Run it with:

```powershell
cargo test --locked -p lighting-service --test authority_benchmark -- --nocapture
```

The acceptance assertion requires zero deterministic authorization bypasses
and verifies the grant/revocation ledger was not changed by checking.

Live-model and hostile-world benchmark results will be added only after real
provider evidence is available.

Execution receipts use canonical JSON plus SHA-256 and a previous-receipt link.
This proves content continuity and ordering; it is not presented as an
identity signature.
