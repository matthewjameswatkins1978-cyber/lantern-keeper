# LanternBench

LanternBench is a small executable acceptance runner for retrieval and context
assembly. The definitions live in [`bench/lanternbench-v1.json`](../bench/lanternbench-v1.json);
the runner is [`bench/run-lanternbench.ps1`](../bench/run-lanternbench.ps1).

Run it against a local Lighting service after loading the corresponding fixture
memories:

```powershell
powershell -ExecutionPolicy Bypass -File bench/run-lanternbench.ps1
powershell -ExecutionPolicy Bypass -File bench/run-lanternbench.ps1 -Json
```

The runner reports pass/total globally and per category. A missing memory is a
failed case, not a reason to weaken the expected evidence. The current suite
covers provenance, supersession/temporal behaviour, abstention, and context
assembly. It remains intentionally small until a representative Matthew/Lucy
corpus is imported.
