# Tests

Most tests run with `cargo test --workspace`.

The real SurrealDB integration test connects to a disposable local database whose
name starts with `lighting_test_`. Start SurrealDB first:

```powershell
docker compose up -d surrealdb
```

To deliberately skip the integration test:

```powershell
$env:LIGHTING_SKIP_INTEGRATION_TESTS="1"
cargo test --workspace
```
