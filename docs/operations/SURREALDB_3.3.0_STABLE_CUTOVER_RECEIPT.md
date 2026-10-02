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
- **SurrealKV Version**: `0.7.2` (resolved in `Cargo.lock` from `surrealdb 3.3.0`)

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

## Bridge Transport & Verification Evidence

- **Authorized Mutation Cycle**:
  - **Intent**: `lucy-bridge-smoke-20261002-consolidated`
  - **Agent**: `chatgpt-lucy`
  - **Inbox Commit**: `9dae9545084931a7c49045b3310cb2ee092ea205`
  - **Receipt Commit**: `a3770e0ddaa0ae9666014ba73bca5860feea1b0d`
  - **Mirror Commit**: `19d19a08254482262815366ecd140c4b442d241a`
  - **Authority Decision**: `ALLOW`
  - **Receipt Status**: `APPLIED`
  - **Lantern Memory Record**: `1d1eae88-b2e5-485d-af59-33f93db0fb9a`
- **Unauthorized Fail-Closed Cycle**:
  - **Intent**: `unauthorized-smoke-20261002-failclosed`
  - **Agent**: `malicious-actor`
  - **Inbox Commit**: `631b889558dd48e66588b62798035a16904d409d`
  - **Receipt Commit**: `ff01d2a450e1d033fafe7f20108db283bf1e0d3d`
  - **Authority Decision**: `DENY` (`UnauthorizedAgent: agent 'malicious-actor' not in allowlist`)
  - **Receipt Status**: `Skipped`
  - **Result**: Fail-closed enforced; zero records written to canonical memory; zero mirror pollution.

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
