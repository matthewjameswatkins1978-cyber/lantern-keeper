# Tests

The normal validation lane is:

    pwsh -NoProfile -File .\scripts\validate.ps1

Most tests use isolated embedded SurrealKV stores. Remote integration tests
are opt-in and use exact SurrealDB 3.3.0-beta.4:

    .\scripts\start-surreal.ps1
    $env:LIGHTING_STORAGE = "remote-surreal"
    $env:LIGHTING_SURREAL_USERNAME = "root"
    $env:LIGHTING_SURREAL_PASSWORD = "root"
    cargo test --locked --workspace -- --test-threads=1
    .\scripts\stop-surreal.ps1

The serial test setting keeps the canonical lane deterministic around the
known transient remote-store write conflict during parallel cold startup. To
deliberately skip remote integration tests, set
`LIGHTING_SKIP_INTEGRATION_TESTS=1` for the test process. Never commit local
environment files or database state.
