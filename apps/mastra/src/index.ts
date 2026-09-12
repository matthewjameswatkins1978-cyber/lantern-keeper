import { Mastra } from "@mastra/core/mastra";
import { Memory } from "@mastra/memory";
import { SurrealDBStore } from "@surrealdb/mastra-ai";

const lanternUrl = process.env.LANTERN_URL ?? "http://127.0.0.1:4317";

/**
 * Mastra owns working/conversational memory only. The returned runtime is
 * deliberately separate from Lantern's canonical Memory tables.
 */
export function createMastraRuntime() {
  const store = new SurrealDBStore({
    id: "lantern-mastra-working-memory",
    url: process.env.SURREALDB_URL ?? "ws://127.0.0.1:8000",
    username: process.env.SURREALDB_USERNAME ?? "root",
    password: process.env.SURREALDB_PASSWORD ?? "root",
    namespace: process.env.MASTRA_SURREAL_NAMESPACE ?? "lantern_mastra",
    database: process.env.MASTRA_SURREAL_DATABASE ?? "working_memory",
  });
  const memory = new Memory({ storage: store });
  const mastra = new Mastra({ storage: store });
  return { mastra, memory, store };
}

export type LanternCandidate = {
  content: string;
  scope?: string;
  kind?: string;
  identity_key?: string;
  confidence?: number;
  importance?: number;
  source_ids?: string[];
  derived_from?: string[];
};

/**
 * Promotion is explicit: an observer may propose a candidate, but only the
 * Rust Lantern service can reconcile it into canonical memory.
 */
export async function proposeToLantern(candidate: LanternCandidate) {
  const response = await fetch(`${lanternUrl}/api/v1/memory/remember`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(candidate),
  });
  if (!response.ok) {
    throw new Error(`Lantern rejected candidate (${response.status})`);
  }
  return response.json();
}

export async function initialiseMastraRuntime() {
  const runtime = createMastraRuntime();
  await runtime.store.init();
  return runtime;
}
