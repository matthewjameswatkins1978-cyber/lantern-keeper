# C3-RELATIONS — hosted seed mapping (relations, history, bytes)

Scope: how `lighting-cli hosted-seed-sql` maps the remaining export tables
into the hosted D1 schema. Branch `cutover/c3-relations`; extends the
C3-IMPORT builder (`hosted-seed-sql-v1`). **No schema change**: both
mappings fit `0001_hosted_schema.sql` exactly as deployed.

## `memory_relation` → `epistemic_claim`

A Basic Memory relation *is* a subject/predicate/value triple, so it seeds
as a claim row on the existing table:

| relation record | claim column |
| --- | --- |
| `id` (`memory_relation:` prefix, backticks stripped) | `id` |
| `in_id` (full record reference) | `subject_key` |
| first segment of `dedupe_key` (the recorded relation type, e.g. `related_to`) | `predicate_key` |
| `out_id` | `value` |
| `origin` | `speaker_actor_id` (who asserted the relation) |
| `dedupe_key` namespaced as `relation\|<native key>` | `dedupe_key` (UNIQUE preserved, cannot collide with claim dedupe keys) |
| `created_at` (canonicalised) | `known_at`, `created_at` |
| `payload` (native relation payload string) | `payload` (byte-exact) |

Fixed mapping choices:

* `predicate_status = 'resolved'` — the relation type is fixed by the source
  system's own model (the dedupe key), not a registry lookup or a guess.
* `polarity = 1` — a recorded relation asserts the connection.
* `holder_actor_id = NULL`, `source_id = NULL` — relations are not document
  evidence and carry no holder.
* `scope_hash = scope_hash({})` — relations carry no scope natively; the
  empty-scope hash is the honest representation. Scoped reads therefore never
  observe a relation through a narrower scope.
* `episode_id` — set when an endpoint is an `episode:` reference (`in_id`
  preferred, else `out_id`), so the relation attaches to its evidence. A
  reference to an episode absent from the export fails the apply on the
  declared foreign key (fail closed); `--strict` moves that failure to build
  time.

`episode_project_relation` and `episode_marker_relation` stay **skipped and
counted**: native carries project membership on `episode.project_id` (seeded
with the episode row) and marker relations follow markers, which are outside
the hosted memory surface.

## `belief_revision` → `belief_revision`

Direct column fit (`id`, `belief_id`, `payload`, `created_at`), plus the one
derived column:

* `payload` — the native payload string, byte-exact (it already contains the
  full native revision: previous/new value, claim, transition type, trace).
* `from_updated_at` — the hosted CAS token this transition consumed. Native
  does not record tokens, so the builder derives the chain per belief:
  * revisions are grouped by `belief_id` and ordered by
    (`created_at`, `id`);
  * `from[0]` = the **zero token** `0001-01-01T00:00:00.000000000Z` when the
    first revision is the belief's own creation
    (`created_at == belief.created_at`) — a born belief consumed no prior
    token; otherwise `from[0] = belief.created_at`;
  * `from[k] = revision[k-1].created_at` (each correction advances the
    belief's `updated_at` to that correction's `now`).

Safety invariants (the genesis-row lesson applied to history):

* No derived `from_updated_at` may equal the seeded belief's current
  `updated_at` — that row would collide with the first post-seed
  correction's CAS insert. `--strict` aborts; the default path warns and
  omits the row so the produced SQL stays applicable.
* Duplicate `(belief_id, from_updated_at)` pairs inside one export are a
  build-time defect: `--strict` aborts, default warns and omits.
* A revision whose `belief_id` has no belief row in the export would abort
  the apply on the foreign key: `--strict` aborts at build time, default
  warns and still emits (the apply fails closed).
* Seeded revisions are historical records only. The live CAS guard is still
  inserted by `build_correction_batch` (`lighting-store-d1`
  `tests/atomic_semantics.rs`), so a seeded chain never substitutes for —
  and must never collide with — the first correction's guard row.

Emission order adds a fifth bucket: `source → episode → claim → belief →
belief_revision`, regardless of shard file order. Revision statements use
`ON CONFLICT(id) DO NOTHING` so re-seeding stays idempotent.

## `source` → `source` + scoped `memory_item` note projection

With a trusted operator scope (`--scope KEY=VALUE`, repeatable; all pairs
form one normalised scope), every classified `source` additionally emits an
ordinary-note projection so imported Markdown is reachable through the
ordinary hosted `lantern_list_notes` → `lantern_get_source` path
(LK-HOME-FINAL import visibility):

| projection field | value |
| --- | --- |
| `id` | `sha256("importnote\|<scope_hash>\|<source_id>")[:32]` — deterministic, so re-seeding collides on the primary key instead of duplicating |
| `scope_hash` | hash of the operator-supplied scope (the note is scoped; `source` itself carries no scope) |
| `content`, `payload` | the source's exact bytes, copied once from the same extraction as the `source` row |
| `kind` | `'note'` — never `epistemic_claim`/`belief`; an import cannot manufacture verified knowledge |
| `created_at` | the source's canonical timestamp (ordering reflects the original timeline) |
| `source_id` | the source row's id — the handle `lantern_get_source` resolves scope-confined |
| `actor` | `'import:hosted-seed-sql-v1'` (machine origin, not a principal) |

`ON CONFLICT(id) DO NOTHING` keeps the projection replay-safe, and
relations/history are untouched: only `memory_item` rows are added.

Scope requirement (fail closed): an export containing sources but no
`--scope` cannot be projected. `--strict` aborts before any artifact is
written; the default path records a warning (counted as unexplained loss,
non-zero exit) so the report never claims a fully mapped import. Requires
hosted schema `0002`+`0003` (`memory_item.source_id`, `actor`) applied
before the seed.

Lookup side: `lantern_list_notes` notes carry `source_id`, and its optional
`query` argument filters note content inside the same 128-row bound
(`search_memory_notes`), so notes older than the newest page stay findable.

Skipped record types keep their honest treatment: `marker`,
`ledger_event`, `project`, `memory`, `trace` (and relation kinds outside
the hosted surface) are counted per table in `HostedSeedSqlReport.skipped`
and named in the seed header — "not part of the hosted memory surface" —
never silently dropped and never partially mapped.

## Bytes

* Markdown `source.content` is copied verbatim (quote-doubling only; NUL
  bytes cannot survive SQL TEXT, so a NUL in content is a warning at minimum
  and an abort under `--strict`, never a silent strip).
* Nested `payload` strings (claims, beliefs, revisions) are byte-exact.
* Relation `payload` strings are byte-exact, like claim and belief payloads.

The end-to-end test proves byte equality against the seeded SQLite rows,
then runs a real keyed correction over the seeded history to prove the
derived tokens do not block it, and reconciles emitted+skipped counts
against the export manifest.

## `--strict`

* **Default** (synthetic path): malformed or missing rows become warnings;
  the SQL and report are still written for inspection, and the command then
  exits non-zero if any record among the mapped tables could not be mapped
  honestly (unchanged C3-IMPORT behaviour).
* **`--strict`** (production path): the first malformed/missing row, missing
  reference, NUL byte, or token hazard aborts **before any artifact is
  written**.
