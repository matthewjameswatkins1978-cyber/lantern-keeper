# Parity harness (M0 feasibility proof)

Fixed-fixture check that portable `lighting-core` reconciliation logic
produces byte-identical decisions natively and under WebAssembly.

- Fixture: one Claim + one Belief with fixed timestamps (no clock reads in
  the harness itself), `reconcile_claim(..., independent_evidence = true)`.
- Expected: `{"action":"contradict","reason":"different value lacks direct holder authority"}`
  (see `expected.json`).

## Native

```bash
cargo test -p parity-harness
```

Runs in CI as part of the workspace. Guards the fixture and the exact
serialised decision shape.

## WASM (wasip1, manual)

```bash
cargo build -p parity-harness --target wasm32-wasip1
node crates/parity-harness/driver-wasi.js \
  target/wasm32-wasip1/debug/parity_harness.wasm
```

Requires the `wasm32-wasip1` std (`rustup target add wasm32-wasip1`).
The driver supplies the five `wasi_snapshot_preview1` imports the module
needs (`random_get` via WebCrypto, stub env/fd). A `PARITY OK` line means
the wasm build reached the same decision byte-for-byte.

## WASM (unknown-unknown, Workers lane)

`lighting-core` checks clean for `wasm32-unknown-unknown` via the
target-cfg `uuid/js` + `chrono/wasmbind` features in
`crates/lighting-core/Cargo.toml`. Running it needs the wasm-bindgen
host glue (entropy + externref heap), which Cloudflare Workers provides
through `workers-rs` — hand-rolled node glue is deliberately not
supported here. Full runtime parity belongs to the wrangler-local lane.
