# MCP and Lucy Integration

The current supported machine-facing boundary is the local HTTP API used by
the CLI. JSON responses and the stable API error shape are the working
contract. The optional Tethers preview is separate and does not write Lantern
memory.

A local Lucy-native MCP surface is a future cutover gate. It should expose only
bounded capabilities such as context, remember, search, provenance,
correction, status, and Lucy-owned proposal review. It must not expose raw
database mutation or unauthenticated remote writes.

Do not claim Lucy integration is complete until a real supported client has
retrieved context, captured a soft memory without filing instructions, searched
it, explained provenance, corrected a harmless test belief, and retrieved the
corrected state after restart.
