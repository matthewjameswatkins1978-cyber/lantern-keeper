# MCP and Lucy Integration

The current supported boundary is the local HTTP API used by the CLI. The
existing Tethers endpoint remains preview-only and does not write Lantern
storage. A real Lucy-native MCP/connector path is a required pre-cutover gate:
it must read context, record Matthew and Lucy turns, capture soft memory,
search, record corrections, and explain provenance without making Matthew file
records manually.

Do not claim ChatGPT integration is complete until that real client path has
been exercised end to end.
