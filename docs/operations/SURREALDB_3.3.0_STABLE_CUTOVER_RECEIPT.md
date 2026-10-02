# SurrealDB 3.3.0 Stable Cutover Receipt

## Overview

- **Packet ID**: `LANTERN-RUNTIME-CONSOLIDATION-ACCEPTANCE-REPAIR-20261002`
- **Parent Packet**: `LANTERN-RUNTIME-CONSOLIDATION-STABLE-20261002`
- **Baseline Master Commit**: `3afaeaa09a108848f6fb6625077598c7380ba777`
- **Implementation PR**: [#12](https://github.com/matthewjameswatkins1978-cyber/lantern-keeper/pull/12)
- **Branch**: `fix/lantern-runtime-consolidation-stable-3.3.0`
- **Cutover Date**: 2026-10-02

## Datastore Identity and Mode

- **Canonical Datastore Mode**: `embedded-surrealkv`
- **Canonical Datastore Absolute Path**: `D:\Projects\lantern-keeper\.lighting-data\surrealkv`
- **How Canonical Identity Was Established**:
  - Live process table inspection identified the running production Lighting service instance bound to port 4317 and configured with `LIGHTING_STORAGE=embedded-surrealkv` and `LIGHTING_SURREAL_PATH=D:\Projects\lantern-keeper\.lighting-data\surrealkv`.
  - The store content was verified to contain existing canonical memory records, including ChatGPT Lucy's end-to-end smoke test record `6cc6947b-5d8a-4c14-a01c-327afd1ebdbf` from intent `lucy-bridge-smoke-20261002-b`.
  - Invariant enforced: No destructive in-place migrations or resets were executed. The production store was retained and upgraded safely with a verified cold backup.

## SurrealDB & SurrealKV Versions

- **Pre-migration SurrealDB Version**: `3.3.0-beta.4` (workspace dependency `=3.3.0-beta.4`, engine `surrealdb 3.3.0-beta.4`)
- **Post-migration Expected SurrealDB Version**: `3.3.0`
- **Post-migration Observed SurrealDB Version**: `3.3.0` (authoritatively queried via database diagnostic and exposed via `GET /api/v1/version`)
- **SurrealKV Version**: `0.21.4` (resolved in `Cargo.lock` from `surrealdb 3.3.0`)

## Pre-Cutover Backup & Verification

- **Backup Path**: `D:\Projects\lantern-keeper\.lighting-backup\surrealkv-pre-3.3.0-stable-20261002-0850`
- **Backup Manifest Digest**: `D43D3ACFED8289E433BD55686BD5841D76AC97AB8BA3D34B8F023514C567EDCF` (`manifest.sha256`)
  - `LOCK`: `5E2F9954770A9EBBC6E1F7A1C530ECB62E07546A36FC659AF96FA9213EA93FEC`
  - `manifest\00000000000000000000.manifest`: `7BF25CF2639AF27D2B82DC9A3F5BFD6D2684F436BD9BF47ACD82024E8AA3C829`
  - `wal\00000000000000000000.wal`: `52C9713D37C05886C89E852A625247089F6D64B6160B82EE281F0E276BE0482A`
- **Copied-Store Validation Result**:
  - The cold backup directory was mounted in an isolated temporary location and verified using SurrealDB 3.3.0 stable binaries.
  - All existing records were readable and schema checks passed with zero errors or schema drift.
- **Live-Store Validation Result**:
  - Production service successfully mounted `D:\Projects\lantern-keeper\.lighting-data\surrealkv` using stable SurrealDB 3.3.0.
  - Status and doctor endpoints confirmed `database_connection: connected`, `health: ok`, `server_version: 3.3.0`.
  - All 5 pre-existing canonical records were intact and accessible.

## Record Counts & Smoke Test Identifiers

- **Pre-cutover Useful Record Count**: 5 records
- **Post-cutover Useful Record Count**: 5 records (pre-existing) + 1 record created during authorized post-consolidation smoke test
- **Known Smoke-Test Record IDs**:
  - Prior Lucy Smoke Test Record: `6cc6947b-5d8a-4c14-a01c-327afd1ebdbf` (from intent `lucy-bridge-smoke-20261002-b`)
  - Post-Consolidation Smoke Test Record: `1d1eae88-b2e5-485d-af59-33f93db0fb9a` (from intent `lucy-bridge-smoke-20261002-consolidated`)
  - Replay Duplicate Record (Superseded): `663e02d3-d650-419d-a5ca-4417598ed91d` (superseded at `2026-10-02T13:18:26.189432Z`; active recall confirmed 4 useful memories + 1 canonical consolidated smoke memory)

## Bridge Transport & Verification Evidence

- **Authorized Mutation Cycle**:
  - **Intent**: `lucy-bridge-smoke-20261002-consolidated`
  - **Agent**: `chatgpt-lucy`
  - **Inbox Commit**: `b2675b1928a8ad42c3f6a3889d0f1285ab49df9a`
  - **First Durable Receipt Commit**: `08dc4a0af1dd21d1166b158b9717f8057e9c65bf`
  - **Restored Canonical Receipt Commit**: `9d505bd86498ff0c313264c91a03975ba69a5316`
  - **Private Mirror Commit**: `4f6e977c0c16922a96a4b13a774ea088718a7b1d`
  - **Authority Decision**: `ALLOW`
  - **Receipt Status**: `APPLIED`
  - **Canonical Lantern Memory Record**: `1d1eae88-b2e5-485d-af59-33f93db0fb9a`
- **Unauthorized Fail-Closed Cycle**:
  - **Intent**: `unauthorized-smoke-20261002-failclosed`
  - **Agent**: `malicious-actor`
  - **Inbox Commit**: `631b889558dd48e66588b62798035a16904d409d`
  - **Receipt Commit**: `ff01d2a450e1d033fafe7f20108db283bf1e0d3d`
  - **Authority Decision**: `DENY` (`UnauthorizedAgent: agent 'malicious-actor' not in allowlist`)
  - **Receipt Status**: `Skipped`
  - **Result**: Fail-closed enforced; zero records written to canonical memory; zero mirror pollution.

## Discovered Replay Defect & Durable Idempotency Repair

- **Discovered Defect**:
  - Intent `lucy-bridge-smoke-20261002-consolidated` was executed once at `08dc4a0`, creating memory record `1d1eae88-b2e5-485d-af59-33f93db0fb9a`.
  - When local `bridge-state.json` was absent/lost during subsequent test runs, execution relied only on local state and re-executed `POST /api/v1/memories`, creating duplicate active memory `663e02d3-d650-419d-a5ca-4417598ed91d` and overwriting the git receipt in `c9294ed`.
- **Architectural Seam Repair (4-Layer Defense)**:
  1. **Layer 1 (Git Receipts Immutability)**: `GitQueue::get_receipt` inspects `refs/heads/receipts:receipts/{intent_id}.json` directly from git object storage before any network or mutation call. `GitQueue::write_receipts_and_status` rejects conflicting modifications with `ForbiddenModification`.
  2. **Layer 1.5 (Local State Cache)**: `BridgeStateStore` provides high-speed cache and detects tampering (canonical digest mismatch -> `SUSPECT`).
  3. **Layer 2 (Canonical Datastore Idempotency Claim)**: Moved ultimate replay authority into SurrealDB (`bridge_intent` table, schema migration v11) with deterministic record ID `bridge_intent:<intent_id>`, unique constraints, and atomic check-and-claim API (`/api/v1/bridge/intents/claim`, `/complete`). An intent can be claimed at most once; replays return `Existing` with original record ID and zero mutation.
  4. **Layer 3 (Startup State Reconciliation)**: `BridgeRunner::reconcile_state` reconciles local `bridge-state.json` from SurrealDB and git receipts, advancing the checkpoint commit to the highest ancestor among known processed intents.
- **Historical Remediation**:
  - Memory `663e02d3-d650-419d-a5ca-4417598ed91d` was superseded via `POST /api/v1/memories/663e02d3/supersede`.
  - Canonical receipt in `lantern-post` was restored to point to `1d1eae88-b2e5-485d-af59-33f93db0fb9a` (commit `9d505bd`).
  - Read-only mirror in `lantern-git` was refreshed and pushed (commit `4f6e977`).

## Headless Windows Runtime Packaging

- **Windows Scheduled Task Execution**:
  - Scheduled tasks `LanternKeeper-Service` and `LanternKeeper-Bridge` now execute via Windows Script Host (`wscript.exe //B //Nologo run-*.vbs`).
  - VBS launchers use `WshShell.Run ..., 0, True` (`SW_HIDE` window style 0).
  - Eliminates all flashing console, cmd.exe, PowerShell, and terminal windows during background 2-minute bridge polling cycles.
  - Environment variables (`LANTERN_BRIDGE_ALLOW_AGENTS=chatgpt-lucy,pi`, datastore paths) and log redirection (`service.log`, `bridge.log`) are strictly preserved.

## Controlled Transport Mutation-Readiness Tests

- **Negative Test (Disabled Bridge Task)**:
  - Bridge task `LanternKeeper-Bridge` was placed into `Disabled` state.
  - `lantern-runtime.ps1 status` was evaluated.
  - Result: `Bridge Task State: Disabled`, `Mutation Ready: NO (blocked or unready)`.
- **Recovery Test (Re-enabled Bridge Task)**:
  - Bridge task `LanternKeeper-Bridge` was re-enabled.
  - `lantern-runtime.ps1 status` was evaluated.
  - Result: `Bridge Task State: Ready`, `Mutation Ready: YES (healthy)`.

## Rollback Readiness

- The pre-migration backup at `D:\Projects\lantern-keeper\.lighting-backup\surrealkv-pre-3.3.0-stable-20261002-0850` is retained in cold storage.
- Rollback procedure if needed:
  1. Stop Windows scheduled tasks: `LanternKeeper-Bridge` and `LanternKeeper-Service`.
  2. Restore files from backup directory to `D:\Projects\lantern-keeper\.lighting-data\surrealkv`.
  3. Revert binary in `.lighting-runtime\bin\lighting.exe` if needed.
  4. Restart tasks using `scripts/lantern-runtime.ps1 start`.
- Current operational status: Active and healthy on SurrealDB 3.3.0 stable. No rollback needed.
