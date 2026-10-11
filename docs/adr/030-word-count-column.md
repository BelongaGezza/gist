# ADR 030 — Per-item word count: stored column (schema v8)

**Date:** 2026-10-11
**Status:** Accepted, implemented and tested in Rust; Swift/C# follow-ups logged
**Resolves:** spec `SC-15` (word count and estimated reading time)

## Context

`Metadata.word_count` is computed by `Document::new` and sits inside `library_items.metadata_json`, but `LibraryItem`/`FfiLibraryItem` did not expose it. The library list needs it for every row, and it is a natural sort key.

## Definition

`word_count` = the number of `TokenKind::Word` tokens in the document's token stream. This is exactly (a) what `Document::new` writes to `Metadata.word_count`, (b) the number of rows `Store::insert_item` writes to the `tokens` table for the item, and (c) the units the RSVP reader plays as words (punctuation and paragraph/section-break tokens are not counted). Words are whitespace-separated runs, so "state-of-the-art" is one word; the count therefore matches what the reader will play.

## Options

1. **Compute on read** from `tokens` (`COUNT(*)`): an index range scan over every word of every listed item on each list/search/collection query. O(total words in the library) per page of the library. Rejected.
2. **`json_extract(metadata_json, '$.word_count')`**: cheap, no schema change, but ties a hot DTO field and any future sort to a JSON blob that can be absent or `0` for hand-built metadata, and cannot be indexed.
3. **Stored column** `library_items.word_count INTEGER NOT NULL DEFAULT 0` — **chosen.** Written in `insert_item` from the same token stream used to fill `tokens` (so it can never disagree with them), read by the shared `LIBRARY_ITEM_SELECT`, so `list_items`, `get_item_by_id` (used by search), `list_items_in_collection` and `list_items_by_tag` all carry it.

## Migration (v7 -> v8)

One transaction: `ALTER TABLE ... ADD COLUMN word_count ... DEFAULT 0`, then `UPDATE library_items SET word_count = (SELECT COUNT(*) FROM tokens WHERE item_id = ...)`, then `user_version = 8`. The backfill is answered from the existing covering index `idx_tokens_item_idx` (ADR-021), so **no document or token blob is read, decrypted or held in memory**; cost is one pass over the index at first open. `SCHEMA_VERSION` is 8, so a v9 file is rejected with `SchemaTooNew`. Items with no word tokens get 0. A row inserted by an older binary against a v8 file is not possible (older binaries reject v8 as too new). No IR change, so no `ir_version` bump (ADR-019 unaffected).

## Reading time

`gist_store::estimate_reading_seconds(word_count, wpm)` (also exported over FFI as `estimateReadingSeconds`): `ceil(words * 60 / wpm)`, `wpm` clamped to 100..=1000 like `gist-rsvp`. Callers pass their configured WPM; the UI chooses the display rounding (e.g. minutes).
