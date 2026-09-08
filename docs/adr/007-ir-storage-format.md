# ADR-007: Document IR storage format

**Status:** Accepted
**Date:** 2026-09

## Decision

Store each parsed document as two JSON files on disk alongside a lightweight
`library_items` SQLite row. This resolves open question Q7 from the development
plan.

| File | Contents |
|---|---|
| `<storage_dir>/docs/<id>.json` | Full `Document` IR (block tree, metadata) |
| `<storage_dir>/docs/<id>.tokens.json` | Flat array of `Token` objects (text + kind + byte offset) |

The `library_items` SQLite row holds only lightweight metadata (id, title,
author, import date, file paths, progress offset). The FTS5 index is an
external-content table pointing at the `tokens` shadow table (see ADR-008),
not at the JSON blobs.

Binary blob formats (`postcard`, `bincode`) and fully-in-SQLite storage were
evaluated and rejected (see Reasoning).

## Reasoning

**Why not binary blob (postcard / bincode)?**
- Not human-readable; debugging a corrupt or mis-parsed document requires a
  separate decode tool.
- Schema evolution is harder: adding a field requires a migration that
  re-serialises every stored document.
- `serde_json` is already in the dependency tree; adding a second serialisation
  crate increases binary size for marginal performance gain.

**Why not SQLite rows for blocks?**
- A document block tree does not map cleanly to a flat table without a
  self-referential adjacency list or a path-encoded scheme; both complicate
  queries and serialisation.
- SQLite rows make lazy loading harder: fetching a subtree requires a recursive
  CTE, whereas `serde_json` streaming can deserialise only requested fields.
- The approach couples document structure to the SQLite schema, making schema
  migration much heavier.

**Why JSON + separate token file?**
- JSON is human-readable and inspectable with any editor — important for an
  open-source project where contributors debug parsing issues.
- `serde_json` supports streaming deserialisation; large documents can be
  processed without loading the entire file into memory.
- Separating the token stream into `<id>.tokens.json` means the RSVP engine
  and FTS5 indexer never load the full block tree. Lazy loading is achieved by
  file granularity.
- Schema evolution: adding a field to `Document` or `Token` requires only a
  `#[serde(default)]` annotation; existing stored files deserialise without a
  migration.

## Consequences

- `gist-store` must persist both `<id>.json` and `<id>.tokens.json` in the
  same transaction as the `library_items` insert.
- `get_item` loads `<id>.json` (full document).
- The RSVP session and FTS5 indexer load only `<id>.tokens.json`.
- Schema migration from a future token format requires a backfill pass that
  re-reads `<id>.json` (or re-parses from the original) to regenerate
  `<id>.tokens.json` for existing items.
- Storage overhead is modest: the token file is a subset of the document file's
  data in a slightly different shape; typical ePub token files are < 1 MB.
- Debugging: contributors can open any `<id>.json` or `<id>.tokens.json`
  directly to inspect a parse result.
