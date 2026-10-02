# Lantern Keeper ChatGPT plugin source

This directory is the source-controlled candidate for private Lantern Keeper v0.2.0. The installed v0.1.4 release and the separate `lantern-keeper-v2` plugin were inspected but not changed. Build or publish a private release only through the authorized Plugin Creator workflow.

## Source contents

- `plugin.json` and `.codex-plugin/plugin.json` hold synchronized plugin identity and interface metadata.
- `skills/lantern-memory/SKILL.md` defines historical-dependency routing, bounded retrieval, provenance, and mutation boundaries.
- `server/lantern-readonly.mjs` is the transparent native MCP process launcher used by the current plugin.
- `mcp.json.template` and `.mcp.json.template` are valid JSON templates. Replace `__PLUGIN_DIR__` and `__LANTERN_KEEPER_ROOT__` for the destination host. They intentionally contain no machine-specific path or credential.
- `tests/routing-cases.json` and `tests/test_source.py` hold reviewable routing examples and structural source checks.

The current native MCP catalogue has eight tools. Its launcher does not filter tools, so the skill explicitly limits routine reads and controls when the two canonical memory mutation tools may be used. GitHub fallback follows the existing read-only `lantern-git` manifest/index/record/sidecar contract from bridge baseline `8640948455252ac93cdb777655d635e10a30f913`; no GitHub write adapter, typed intent schema, or terminal receipt contract exists in that bridge. The current native service was unavailable during source preparation, so live calls and the installed plugin's ordinary-chat trigger behavior remain unverified. The GitHub read path was checked against the private mirror: its manifest/index and a known record plus integrity sidecar were fetchable and their returned blob SHA matched.

## Check

From this directory, run `python -m unittest discover -s tests -v`. These checks verify fixture coverage and synchronized metadata/policy text. They do not emulate ChatGPT's model-level plugin selection; that needs a separate ordinary-chat validation after Lucy installs the reviewed private release.
