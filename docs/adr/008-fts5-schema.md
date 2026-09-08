# ADR-008: FTS5 schema design

**Status:** Accepted
**Date:** 2026-09

## Decision

Integrate FTS5 full-text search in `gist-store` using an **external-content**
virtual table backed by a shadow `tokens` table. This resolves [A1] from the
architecture register.

### Schema additions (migration v1 → v2)

```sql
-- Shadow content table: one row per Word token per document
CREATE TABLE IF NOT EXISTS tokens (
    rowid      INTEGER PRIMARY KEY,
    item_id    TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
    token_idx  INTEGER NOT NULL,
    token_text TEXT NOT NULL
);

-- FTS5 external-content table over token text
CREATE VIRTUAL TABLE IF NOT EXISTS fts_index USING fts5(
    token_text,
    content='tokens',
    content_rowid='rowid',
    tokenize='porter unicode61'
);

-- Trigger: keep FTS5 in sync on insert
CREATE TRIGGER tokens_ai AFTER INSERT ON tokens BEGIN
    INSERT INTO fts_index(rowid, token_text) VALUES (new.rowid, new.token_text);
END;

-- Trigger: keep FTS5 in sync on delete
CREATE TRIGGER tokens_ad AFTER DELETE ON tokens BEGIN
    INSERT INTO fts_index(fts_index, rowid, token_text)
        VALUES ('delete', old.rowid, old.token_text);
END;
```

### Import flow

After building the token stream, insert all `TokenKind::Word` tokens into the
`tokens` table within the same transaction as the `library_items` insert.

### Search query

```sql
SELECT DISTINCT item_id
FROM tokens
WHERE rowid IN (
    SELECT rowid FROM fts_index WHERE token_text MATCH ?1
)
```

Snippet extraction uses `fts5_snippet(fts_index, 0, '<b>', '</b>', '…', 10)`.

### Migration strategy (v1 → v2)

1. Create `tokens` table, `fts_index` virtual table, and both triggers.
2. For each existing `library_items` row: load `<id>.tokens.json`; if absent,
   re-parse from `<id>.json`; insert all `Word` tokens into `tokens` (triggers
   populate `fts_index` automatically).
3. Bump schema version to 2.

## Reasoning

**External-content vs content=**

A standard `content=` FTS5 table stores a copy of every indexed string inside
the FTS shadow tables, doubling storage for the token text. An external-content
table (`content='tokens'`) points FTS5 at the `tokens` table as its authoritative
source, avoiding duplication. The trade-off is that the `tokens` table and FTS5
index must be kept in sync manually via triggers; the two triggers above cover
all mutations (import and delete; updates do not occur).

**Porter tokenizer**

`porter unicode61` applies Porter stemming over Unicode-aware tokenisation.
Stemming lets a search for "reading" match documents containing "reads",
"reader", "read", etc., which is the expected behaviour for a library search
box. `unicode61` handles non-ASCII text correctly. The combination is available
natively in SQLite's FTS5 with no additional dependencies.

**Why token_idx?**

`token_idx` stores the ordinal position of the token within its document. This
enables future ranked search (proximity scoring) and jump-to-result navigation
without re-parsing the document IR.

## Consequences

- `gist-store` inserts tokens in the same transaction as `library_items`; a
  failed import rolls back both, keeping the index consistent.
- `ON DELETE CASCADE` on `tokens.item_id` removes tokens automatically when a
  `library_items` row is deleted; the `tokens_ad` trigger then removes the
  corresponding FTS5 entries.
- Non-Word tokens (punctuation, whitespace, structural markers) are excluded
  from the index, keeping index size proportional to prose content.
- The migration re-uses token files where available; re-parse only occurs for
  items imported before ADR-008 took effect.
- FTS5 `fts5_snippet` requires the external-content table to be intact;
  deleting rows from `tokens` without the trigger would corrupt snippet
  generation — this is prevented by always going through the ORM layer.
