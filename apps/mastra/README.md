# Lantern Keeper Mastra runtime

This package is the optional TypeScript working-memory boundary. Mastra
conversation state and observational memory are stored in the separate
`lantern_mastra` namespace/database through `@surrealdb/mastra-ai`.

Mastra observations are candidates only. Durable promotion must call
`proposeToLantern`, which sends the candidate to the Rust reconciliation API.
Mastra never writes Lantern's canonical memory tables directly.

Requirements are Node 22+, SurrealDB 3.x, and a running Lantern service for
candidate promotion. Copy the environment values from `.env.example` as
needed, then run `npm install` and `npm run typecheck`.
