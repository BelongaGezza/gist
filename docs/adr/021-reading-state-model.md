# ADR 021 — Reading-state model (last read, derived progress, source type)

**Date:** 2026-10-04
**Status:** Accepted, implemented and tested (Rust + macOS; Windows follow-up logged in `PENDING_WINDOWS_CHANGES.md`)
**Resolves:** M7 R1 / decision D2 (`docs/m7-agent-roles.md`)

## Context

The v1.0 spec (`docs/product-spec-reader-app-v3.md` §4) promises library sort by *name / type / date added / date last read / progress*. `LibrarySortOrder` only had date added, title and author. Two of the missing keys need data that no one stored in one place, and the third (type) needed a field the library DTO did not expose.

### What was verified to exist (source, 2026-10-04, base `1668bca`)

- **Schema:** already **v6** (v6 = ADR-003 `annotations`), so this change is **schema v7**, not v6 as the M7 plan text says. Plan wording is stale; the decision (D2) is unchanged.
- **`reading_progress`** (v1): `item_id PK → library_items ON DELETE CASCADE`, `token_index INTEGER`, `updated_at INTEGER`. Written only by `Store::save_progress` (upsert) from RSVP (`CoreClient.saveProgress` / the session close path). `token_index` is a position in the **full** token stream. `updated_at` is "last RSVP save", not "last opened": it is not written at open, and it is never written by the flow view.
- **Nothing recorded "last opened".** `library_items` had `created_at`/`updated_at` (both set at insert; `updated_at` is never touched afterwards). The flow view's position lives only in `UserDefaults` (`FlowScrollPositionStore`, a block fraction), which Rust cannot see.
- **Source type:** `Metadata.source_type` exists and is serialised into `library_items.metadata_json`, but was not exposed by `LibraryItem`/`FfiLibraryItem`. Values written by importers: `txt`, `epub`, `docx` (or `docx:tracked-changes` when tracked changes were found), `web`, `pdf`, `ocr`; `""` for `Metadata::minimal`. It is set by the parser/importer itself (not inferred from a file extension), so it is reliable for every import path including URL imports (`web`), OCR (`ocr`) and PDF (`pdf`) — and it does not depend on `source_ref` or the ADR-006 `source_copy_ref`. Only caveat: pre-existing rows whose `metadata_json` lacks the field read as `""`.
- **Listing queries** (`list_items`, `list_items_in_collection`, `list_items_by_tag`, `get_item_by_id` — search resolves ids through the latter) all ordered `created_at DESC` and returned id/title/authors/paths/`content_encrypted` only; `LibraryItem.token_count` was a permanent `None` TODO.
- **Total token count:** not stored anywhere in SQL. Options considered: (a) load the `<id>.tokens.json` blob per row — rejected (decrypt + checksum + parse a whole book per library row); (b) `COUNT(*)` on `tokens` — that table indexes **Word tokens only** (`insert_item` skips punctuation/breaks) and had no index on `item_id`, so a per-row aggregate is a scan; (c) a new `token_total` column — rejected as extra schema + a backfill problem, and D2 asks for no new progress state. **Chosen:** `MAX(token_idx)` over the item's rows in `tokens`, made an index seek by a new covering index `idx_tokens_item_idx ON tokens(item_id, token_idx)`.

## Decision

### Model

1. **`library_items.last_opened_at INTEGER NULL`** (Unix ms). `NULL` = never opened. Set by **both** readers (RSVP and flow) once per open, through `Store::mark_item_opened` → `Core::mark_item_opened` → `GistCore.markItemOpened`. Importing does not set it. Nothing is backfilled.
2. **No `progress_fraction` column.** Progress is *derived* in the listing SQL from the existing RSVP `reading_progress.token_index`:

   `progress = 0 if token_index = 0 or the item has no indexed words; else min(1.0, token_index / max(last_word_token_idx, 1))`

   The denominator is the stream index of the item's last Word token, i.e. effectively "how far through the words". A session finished at the very end of the stream (cursor on trailing punctuation/break) therefore clamps to exactly 1.0. Computed only for rows that actually have a saved position (a `CASE` guard), so unread items cost nothing.
3. **`source_type`** is returned in the DTO, normalised in `gist-store` (`docx:tracked-changes` → `docx`), via `json_extract(metadata_json, '$.source_type')` guarded by `json_valid` so a corrupt row can never fail a whole listing.
4. The two reader *positions* stay separate and unmerged: RSVP keeps its token index in `reading_progress`; the flow view keeps its block fraction in `FlowScrollPositionStore`. This ADR adds only *when* (shared) and *RSVP-derived how far*.

### Accepted limitation (D2) — must stay visible

**An item read only in the flow view has no RSVP position, so its progress shows 0% / sorts as unstarted, while its last-read date is correct.** This is documented in: this ADR, the library row/Sort-menu tooltip (`LibraryRowReadingState.limitationHelp`, user-visible), `CHANGELOG.md` (Known limitations), and covered by tests (`flow_only_item_has_last_opened_but_zero_progress`, `testFlowReaderOpenStampsLastOpenedButLeavesProgressAtZero`). If it proves unacceptable, the follow-up is a shared progress column in a **schema v8** (v7 is this change) that the flow view also writes.

### Sort semantics (Swift, `LibraryFiltering.sorted`, pure)

- Type: grouped alphabetically by type string; unknown (`""`) last.
- Last read (newest / oldest): never-opened items are **always last**, in both directions.
- Progress (highest / lowest): by the derived fraction; unstarted (0) are last for highest, first for lowest.
- All ties keep the incoming order (date added, newest first), enforced with an explicit index tie-break rather than relying on sort stability. Sorting remains client-side over the loaded pages (as title/author sorts already are).

### Migration (v6 → v7)

Transactional `execute_batch("BEGIN; ALTER TABLE library_items ADD COLUMN last_opened_at INTEGER; CREATE INDEX IF NOT EXISTS idx_tokens_item_idx ON tokens(item_id, token_idx); PRAGMA user_version = 7; COMMIT;")`, in the existing migration style, after the `version > SCHEMA_VERSION` ceiling check. The column is nullable with no default, so existing rows need no rewrite. Building the index is a one-off cost proportional to the number of indexed words in the library (an `O(n log n)` pass over `tokens`; not measured here on a large real library). Tested from the v3 and v5 `old_db_compat` fixtures and a fresh database.

### Forward compatibility

A binary built before this change reads `PRAGMA user_version = 7` as `> SCHEMA_VERSION (6)` and refuses to open the database with `StoreError::SchemaTooNew` (the existing ADR-019-era mechanism, now also covered by an explicit test `newer_schema_is_rejected_with_schema_too_new`). There is no silent partial read. Downgrade after a v7 write is therefore not possible; this is the same policy as v3–v6.

### Query cost

`LIBRARY_ITEM_SELECT` is one statement: `library_items LEFT JOIN reading_progress` (PK lookup) plus a correlated `MAX(token_idx)` subquery evaluated **only** for rows with `token_index > 0`. With `idx_tokens_item_idx` the subquery is a b-tree seek to the last entry for that `item_id`, i.e. `O(log N_tokens)` per started item, `O(1)` for the rest. A unit test asserts via `EXPLAIN QUERY PLAN` that the index is used. Pagination (`LIMIT/OFFSET`, M4 R2) is unchanged. For a library of thousands of items with hundreds of started: thousands of index-free row reads plus hundreds of seeks — not benchmarked on a real large library here.

Cost of the new index on write: one extra b-tree entry per indexed word at import; removal cascades unchanged (and benefits from the index).

### Windows impact

`FfiLibraryItem` gained three fields (`source_type`, `last_opened_at`, `progress_fraction`) and `GistCore` gained `mark_item_opened`. The Windows `CoreClient.Map(FfiLibraryItem[])` builds `LibraryItemVM` with an object initialiser, so the new record fields do not break it. To get the feature on Windows the Windows readers must call `MarkItemOpened` and the library must adopt the new fields and sort keys: logged in `PENDING_WINDOWS_CHANGES.md`. A Windows build of the new bindings is also required to confirm the regenerated C# record still compiles.

## Consequences

- Schema is now v7; older binaries cannot open a v7 database.
- Row-level progress is a derived, RSVP-only number; flow-only reading does not move it (see above).
- Tooltips and docs state this wherever progress is shown.
