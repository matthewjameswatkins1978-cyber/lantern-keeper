# Lantern GitHub Bridge — Operational Architecture & Protocol (v1)

The Lantern GitHub Bridge turns GitHub into a durable, two-way transport for cross-client AI agent memory interaction (e.g. ChatGPT/Lucy, Pi) without violating Lantern's central architectural invariant:

> **Lantern remains the canonical memory authority. GitHub transports evidence and typed intentions. GitHub never becomes a second Lantern database.**

---

## 1. System Architecture

```text
                               READ PATH (Read-Only Projection)

    ┌─────────────────┐       refresh        ┌──────────────────────┐   connector fetch   ┌────────────────────┐
    │  Lantern Keeper │ ───────────────────► │     lantern-git      │ ──────────────────► │ ChatGPT / Lucy, Pi │
    │   (Canonical)   │                      │ (read-only snapshot) │                     └────────────────────┘
    └─────────────────┘                      └──────────────────────┘


                               WRITE PATH (Typed Intent Transport)

    ┌────────────────────┐    create file     ┌──────────────────────┐      fetch       ┌────────────────────────┐
    │ ChatGPT / Lucy, Pi │ ─────────────────► │     lantern-post     │ ───────────────► │   Lantern Git Bridge   │
    └────────────────────┘    intents/{id}    │     (inbox branch)   │                  │ (local daemon / once)  │
                                              └──────────────────────┘                  └───────────┬────────────┘
                                                                                                    │
                                                                                                    ▼
                                                                                        ┌────────────────────────┐
                                                                                        │      Tethers Gate      │
                                                                                        │   ALLOW / ASK / DENY   │
                                                                                        └───────────┬────────────┘
                                                                                                    │ (ALLOW)
                                                                                                    ▼
    ┌──────────────────────┐  direct fetch    ┌──────────────────────┐   record receipt ┌────────────────────────┐
    │ ChatGPT / Lucy, Pi │ ◄───────────────── │     lantern-post     │ ◄─────────────── │    Lantern Mutation    │
    └────────────────────┘    receipts/{id}   │   (receipts branch)  │                  │  (/api/v1/memories)    │
                                              └──────────────────────┘                  └───────────┬────────────┘
                                                                                                    │ (APPLIED)
                                                                                                    ▼
                                                                                        ┌────────────────────────┐
                                                                                        │ Refresh lantern-git    │
                                                                                        │ (coalesced cycle)      │
                                                                                        └────────────────────────┘
```

---

## 2. Repositories & Roles

1. **`matthewjameswatkins1978-cyber/lantern-keeper`** (Local: `D:\Projects\lantern-keeper-bridge-v1`):
   - Canonical implementation repository containing the Lighting service, CLI, core domain, and the GitHub bridge engine (`lighting bridge github`).
2. **`matthewjameswatkins1978-cyber/lantern-git`** (Local: `D:\Projects\lantern-git`):
   - **Disposable, read-only** evidence projection.
   - Contains `mirror/manifest.json`, `mirror/status.json`, search index shards (`index/*.jsonl`), and deterministic markdown records (`records/{type}/{active|archived}/{h1}/{h2}/{uuid}.md`) with `.integrity.json` sidecars.
   - Strictly read-only; never accepts inbound intents or mutations.
3. **`matthewjameswatkins1978-cyber/lantern-post`** (Local: `D:\Projects\lantern-post`):
   - **Private write transport queue** for typed requests and durable receipts.
   - `main` branch: Stable protocol descriptor `protocol.json`.
   - `inbox` branch: Append-only queue of immutable intent files (`intents/{intent_id}.json`).
   - `receipts` branch: Immutable execution receipts (`receipts/{intent_id}.json`) and cheap health summary (`status/bridge.json`).

---

## 3. Protocol Specification

### Protocol Descriptor (`lantern-post/protocol.json`)

Stored on `main` branch of `lantern-post`:

```json
{
  "protocol": "lantern-git-bridge",
  "version": 1,
  "intent_schema": "lantern.intent.v1",
  "receipt_schema": "lantern.receipt.v1",
  "inbox_branch": "inbox",
  "receipts_branch": "receipts",
  "intent_path": "intents/{intent_id}.json",
  "receipt_path": "receipts/{intent_id}.json",
  "status_path": "status/bridge.json",
  "supported_actions": [
    "memory.create",
    "memory.archive",
    "mirror.refresh"
  ],
  "disabled_actions": {
    "memory.reinforce": "Lantern Keeper currently lacks an atomic, idempotent reinforcement storage seam"
  },
  "terminal_statuses": [
    "APPLIED",
    "DENIED",
    "REQUIRES_APPROVAL",
    "CONFLICT",
    "EXPIRED",
    "FAILED",
    "INDETERMINATE",
    "SUSPECT"
  ]
}
```

### Intent Schema (`lantern.intent.v1`)

Strict JSON format, `deny_unknown_fields`, max 64 KiB payload, no executable patterns:

```json
{
  "schema": "lantern.intent.v1",
  "intent_id": "01K9V1E2E00000000000000002",
  "action": "memory.create",
  "requested_by": {
    "actor": "chatgpt-lucy",
    "transport": "github"
  },
  "created_at": "2026-10-02T05:07:00Z",
  "target": {
    "record_id": "optional-uuid"
  },
  "observed": {
    "snapshot_generated_at": "2026-10-02T05:00:00Z",
    "record_sha256": "sha256:f604b13e5362168d143d135daeee52a86c53c81ecc2e5a30f2823da6478fbe87"
  },
  "payload": {
    "content": "Lucy verified: cross-client write transport established between ChatGPT and Lantern Keeper via GitHub",
    "kind": "fact"
  }
}
```

### Receipt Schema (`lantern.receipt.v1`)

Deterministic response written to `receipts/{intent_id}.json` on the `receipts` branch:

```json
{
  "schema": "lantern.receipt.v1",
  "intent_id": "01K9V1E2E00000000000000002",
  "intent_commit": "13e20eb3276566e52a3c86493752adde32f10c02",
  "status": "APPLIED",
  "authority": {
    "decision": "ALLOW"
  },
  "lantern": {
    "record_id": "d1b0a0da-a362-4bd1-9ce0-ae622d2f3cae",
    "result_sha256": "sha256:6ef88269e88b63e9b1eb8e5fc819777f35a4d46b76a084c7faea24ec7649d2cb"
  },
  "processed_at": "2026-10-02T05:08:15.610123Z",
  "bridge_version": "0.1.0",
  "mirror_refresh": "REQUESTED"
}
```

### Terminal Receipt Statuses

- `APPLIED`: Mutation definitely committed to canonical Lantern database.
- `DENIED`: Rejected by Tethers policy or bridge access control (fail-closed).
- `REQUIRES_APPROVAL`: Operator manual intervention needed (`ASK` decision).
- `CONFLICT`: Stale write detected; observed hash does not match current state or record is already archived. Nothing modified.
- `EXPIRED`: Request exceeded maximum validity duration before processing.
- `FAILED`: Unrecoverable execution error (malformed JSON, service error).
- `INDETERMINATE`: Mutation status in Lantern could not be verified; bridge refuses to retry blind to prevent duplicates.
- `SUSPECT`: Invariant violation detected (rewritten Git history, modified existing intent, or replay with different payload).

---

## 4. Supported & Disabled Actions

1. `memory.create`: Creates a new Living Memory in Lantern via `POST /api/v1/memories`.
2. `memory.archive`: Archives an existing Living Memory via `POST /api/v1/memories/archive`. Validates target record ID and `observed.record_sha256`.
3. `mirror.refresh`: Requests an immediate coalesced refresh of `lantern-git` without mutating memories.
4. `memory.reinforce` (**DISABLED**): Explicitly rejected with evidence. Lantern Keeper currently lacks an atomic, idempotent reinforcement storage seam; advertising and failing ambiguously is forbidden.

---

## 5. Security & Trust Boundaries

- **Tethers Gate**: Every intent is evaluated against the Tethers authority engine (`POST /api/v1/tethers/authority/check`) or local policy (`LANTERN_BRIDGE_ALLOW_AGENTS`). If Tethers is unavailable or returns an unrecognized response, the bridge **fails closed** (`DENIED`).
- **Private Repository Enforcement**: The bridge verifies `gh repo view ... --json isPrivate` before pushing. If the repository is ever made public or remote URL points elsewhere, execution aborts immediately.
- **Strict Fast-Forward History**: `git merge-base --is-ancestor` proves inbox history is linear. Force-pushes or rewritten histories are flagged as `SUSPECT` and stop processing.
- **Immutable Files**: Any commit modifying or deleting an existing intent is flagged as `SUSPECT`.
- **Client Portability**: Remote clients (ChatGPT, Pi) know only GitHub paths (`intents/{id}.json`, `receipts/{id}.json`, `status/bridge.json`). They have no awareness of local PC file paths, ports, or service architectures.

---

## 6. CLI Commands

```powershell
# Perform one complete cycle and exit
lighting bridge github once `
    --post-repo D:\Projects\lantern-post `
    --git-repo D:\Projects\lantern-git

# Run continuous background daemon with backoff
lighting bridge github run `
    --post-repo D:\Projects\lantern-post `
    --git-repo D:\Projects\lantern-git `
    --poll-interval-secs 30

# Inspect bridge health without mutating Lantern
lighting bridge github doctor `
    --post-repo D:\Projects\lantern-post
```

---

## 7. Generic Client Example (ChatGPT / Pi / External Agent)

```python
import time
import requests

GITHUB_TOKEN = "ghp_..."
REPO = "matthewjameswatkins1978-cyber/lantern-post"
HEADERS = {
    "Authorization": f"Bearer {GITHUB_TOKEN}",
    "Accept": "application/vnd.github.v3+json",
}

# 1. Fetch protocol descriptor
protocol = requests.get(
    f"https://api.github.com/repos/{REPO}/contents/protocol.json?ref=main",
    headers=HEADERS
).json()

# 2. Submit new intent (create file on inbox branch)
intent_id = "01K9V1E2E00000000000000008"
intent_content = {
    "schema": "lantern.intent.v1",
    "intent_id": intent_id,
    "action": "memory.create",
    "requested_by": {"actor": "chatgpt-lucy", "transport": "github"},
    "created_at": "2026-10-02T06:00:00Z",
    "payload": {
        "content": "Verified client submission",
        "kind": "fact"
    }
}
import base64, json
encoded_content = base64.b64encode(json.dumps(intent_content).encode("utf-8")).decode("ascii")

requests.put(
    f"https://api.github.com/repos/{REPO}/contents/intents/{intent_id}.json",
    headers=HEADERS,
    json={
        "message": f"Submit intent {intent_id}",
        "content": encoded_content,
        "branch": "inbox"
    }
)

# 3. Direct deterministic receipt fetch (no search required)
receipt_url = f"https://api.github.com/repos/{REPO}/contents/receipts/{intent_id}.json?ref=receipts"
for _ in range(12):
    time.sleep(5)
    resp = requests.get(receipt_url, headers=HEADERS)
    if resp.status_code == 200:
        receipt_data = json.loads(base64.b64decode(resp.json()["content"]).decode("utf-8"))
        print(f"Receipt status: {receipt_data['status']}")
        if receipt_data["status"] == "APPLIED":
            print(f"Canonical Record ID: {receipt_data['lantern']['record_id']}")
        break
```

---

## 8. Windows Task Scheduler Automation

Three helper scripts are provided under `scripts/`:

1. `scripts/install-bridge-task.ps1`: Registers a scheduled task `LanternGitHubBridge` running every 2 minutes.
2. `scripts/status-bridge-task.ps1`: Checks current task state, last run time, and last exit code.
3. `scripts/uninstall-bridge-task.ps1`: Safely stops and unregisters the scheduled task.
