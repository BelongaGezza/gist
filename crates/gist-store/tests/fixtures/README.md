# gist-store test fixtures — old-version SQLite databases

These two files are **real GIST library databases written by the previous
dependency stack**: `rusqlite` 0.31.0 / `libsqlite3-sys` 0.28.0 / bundled
**SQLite 3.45.0**. `tests/old_db_compat.rs` opens copies of them to prove that
a library already on a user's disk survives the upgrade to `rusqlite` 0.40.2 /
`libsqlite3-sys` 0.38.2 / bundled **SQLite 3.53.2** — it opens, migrates, reads
and (critically) still full-text-searches.

Nothing else in the test suite covers this: every other test builds its
database from scratch using whatever SQLite is currently linked, so a
format-level regression would pass unnoticed.

| File | Schema version when written | What it holds |
|---|---|---|
| `library_v5_rusqlite031.db` | 5 (current) | 3 items (Treasure Island / Moby Dick / Frankenstein) with authors, an FTS5 token index, 2 collections, 4 tag links and one `reading_progress` row |
| `library_v3_rusqlite031.db` | 3 (pre-ADR-006, pre-ADR-011) | 1 item + tokens/FTS, 1 collection, 1 tag — written *before* the `source_copy_path` (v4) and `content_encrypted` (v5) columns existed, so opening it forces the `ALTER TABLE` migrations to run on an old-SQLite-created file |

## Rules

* **Never regenerate these to make a test pass.** Their whole value is that
  they were produced by the *old* library. Regenerating them under the current
  dependency set turns the compatibility test into a tautology. Same reasoning
  as the `decrypts_blob_written_by_aes_gcm_0_10` known-answer vector in
  `src/lib.rs`.
* **Never open them in place.** `Store::open` migrates and writes. The test
  helper copies each file into a fresh temp directory first.
* They contain no absolute paths: `doc_path` values were rewritten to the
  portable form `storage/<id>.json` and the database `VACUUM`ed afterwards, so
  no generation-machine path survives on a freed page. Nothing reads those
  paths — the compatibility tests exercise the SQL/FTS layer only, and the
  `.json` blobs they would point at are deliberately not committed.

## How they were produced

At commit `aede513` (the last commit before the `rusqlite` 0.40 bump, i.e. with
`rusqlite = "0.31"` still in `crates/gist-store/Cargo.toml`), a throwaway
integration test was added at `crates/gist-store/tests/gen_fixture.rs` and run
with `cargo test -p gist-store --test gen_fixture -- --nocapture`. It:

1. **Fixture A** — opened a `Store` in a temp dir (which migrates a new file
   straight to v5), inserted the three documents via `Store::insert_item`,
   created the collections/tags/progress rows through the ordinary public API,
   asserted `search_items("Ishmael", 10)` returned exactly one hit *under the
   old version* (so the fixture is known-good at generation time), dropped the
   `Store` so the WAL was checkpointed away, then on a plain `Connection` ran
   `UPDATE library_items SET doc_path = 'storage/' || id || '.json'; VACUUM;`
   and copied the resulting single `.db` file here.
2. **Fixture B** — created a second database on a raw `rusqlite::Connection`
   and executed the historical v1, v2 and v3 migration batches **verbatim** as
   they appear in `Store::open_internal` (ending at `PRAGMA user_version = 3`),
   then inserted one `library_items` row using only the v3-era column list,
   six `tokens` rows (which the `tokens_ai` trigger indexes into `fts_index`),
   one collection, one membership row and one tag, and copied that file here.

The generator itself is not committed: it necessarily depends on the old
manifest, so a copy living in the tree would either fail to express that or
silently regenerate against the new stack. The procedure above is the
reproducible record. To recreate the fixtures, check out `aede513`, re-add a
generator following the steps above, and confirm it reports
`SELECT sqlite_version()` = `3.45.0` before trusting the output.
