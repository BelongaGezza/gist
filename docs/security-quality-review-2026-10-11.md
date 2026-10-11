# GIST — Security & Quality Review of the M8 Rust change set (2026-10-11)

**Role:** M8 independent review (protocol step F30). **Reviewed tip:** `integration/m8-2026-10-11` @ `3b30676` (base `main`). Reviewer commits sit on top.
**Scope:** `git diff main..integration/m8-2026-10-11 -- crates fixtures docs/adr`: SC-01 TXT encoding detection (`gist-parse-txt`/`gist-core`/`gist-ffi`), SC-03/SC-06 `Metadata` fields, web byline/dates/canonical URL, ePub language, `import_date` stamping, SC-15 schema v8 `word_count`.
**Method:** as in `docs/security-quality-review-2026-10-05.md`: claims re-derived from source and by running code, not from CLAUDE.md or role reports. Swift/C# and the Apple/Windows apps are out of scope. **ID note:** register continues after F78, so findings start at F79.

## 1. Executive summary

No High or Medium defect. Every limit on untrusted input in the new code is checked before the allocation it guards, the schema v8 migration is transactional and index-backed, ADR-019's "additive, no `ir_version` bump" claim holds when tested against a reader built from `main`'s `gist-model`, and the SSRF protections (`F14`) are untouched. Two real Low defects were found and fixed with regression tests (F79 bidi controls surviving into the byline; F80 two checked-in DOCX fixtures that could not be imported because they lacked `word/styles.xml`). The rest are Info items: a fuzz-coverage gap for the new web metadata code, a 3x worst-case expansion budget that rejects very large legacy-encoded files, a design caution about stored canonical URLs, and spec/CLAUDE.md drift.

## 2. Findings table (proposed register IDs)

| ID | Severity | Status | Summary |
|----|----------|--------|---------|
| F79 | Low | Fixed (c08e70b) | `sanitize_meta_text` (`gist-model/src/metadata_util.rs`) dropped only whitespace and `char::is_control` (category Cc). Bidi embedding/override/isolate characters (U+202A-202E, U+2066-2069) are category Cf and survived into `Metadata.author`, so a hostile page's `<meta name=author content="…&#x202E;…">` could reorder surrounding UI text where a byline is displayed. Fix: those characters are dropped; ZWNJ/ZWJ/LRM/RLM are kept because legitimate names (e.g. Persian) use them. Test `sanitize_strips_bidi_overrides_but_keeps_marks_and_joiners`. Verified with a throwaway probe: `Evil&#x202E;Name` -> `EvilName`. |
| F80 | Low | Fixed (0dc183c) | `fixtures/docx/with_tracked_changes.docx` and `minimal_valid.docx` had no `word/styles.xml` (the parser requires it, `gist-parse-docx` `parse` reads it unconditionally), so neither fixture could be imported. The M8 role noticed this, worked around it with a hand-built document in a `gist-core` test, and left the fixture broken. Regenerated both with a minimal `styles.xml`; new `crates/gist-parse-docx/tests/fixtures.rs` imports both (confirmed to fail on the old files, pass on the new). The `docx:tracked-changes` -> `tracked_changes` flag is now exercised from a real file. |
| F81 | Info | Open | The `fuzz_web_extract` target calls only `extract_content`; the new SC-06 code (`extract_page_meta`, `build_document_at`, `next_day_bound`) is `pub(crate)` and is not reached by any fuzz target. Hand-run hostile probes (section 7) found nothing, but the code has no fuzz coverage. Recommend a `pub` (or `#[doc(hidden)]`) wrapper and a fuzz call. |
| F82 | Info | Open | Expansion budget is conservative: a legacy single-byte file is budgeted at 3 bytes of UTF-8 per input byte (`max_utf8_buffer_length`), and a file with any invalid UTF-8 at 3x `body.len()`, both against `max_expanded_bytes` (512 MiB). Measured: a 180 MiB windows-1252 file is rejected (`ExpandedTooLarge`, 82 ms, before any decode) although its real output would be about 2x. Safe (fail closed), only an unusually large plain-text file is affected. Also `detect_text_encoding` performs a full decode just to report the encoding (up to 3x the file in memory); same limits apply, so it is bounded. |
| F83 | Info | Open (design caution) | `canonical_url` accepts any absolute `http(s)` URL without credentials, including `http://127.0.0.1/…`, `https://[::1]/` and private hostnames (verified). It is stored and never fetched (nothing in `crates/` reads it back; `fetch_url` is called only with the user's URL). If a UI ever turns it into a clickable link, "refresh from canonical", or a prefetch, it must go through `fetch_url`/`safe_resolve` (F14) and be shown as untrusted text. Record this in the SC-06 UI role. |
| F84 | Info | Open (docs) | Drift: `docs/product-spec-reader-app-v3.md` status notes at §3.4 (line 88, still says the parser sets `source_type = "docx:tracked-changes"`), §3.6 (line 114, "byline, publish date and retrieved date … not implemented"; "leaves `author` and `import_date` unset") and §4 (line 146, `word_count` "not exposed", `import_date`/`language` "never populated") contradict the updated §12 rows and the code. CLAUDE.md still says schema v7 (repo layout/ADR table, "schema is now v7") and does not mention ADR-030/v8. `FfiDocumentMetadata` normalises legacy `docx:tracked-changes` rows, so the old marker still exists in stored data; any other consumer of `source_type` should treat it as `docx`. Update the notes in the same commit that closes the SC rows. |
| F85 | Info | Open | Minor behaviour notes, no action needed: (a) `word_count` is 0 for any item without `tokens` rows even if its IR metadata says otherwise (items normally always have them; `insert_item` writes both in one transaction); (b) a UTF-8 BOM'd file now imports without U+FEFF in its first paragraph, so re-importing an old BOM'd file yields different text offsets from the first import (existing annotations are anchored to the stored IR, which is unchanged); (c) `ParseError::Utf8` is now unreachable from `parse` (kept for API stability). |

## 3. Item 1 — Check-before-allocate in the TXT decoder

Source: `crates/gist-parse-txt/src/lib.rs` `decode`, `pick_encoding`, `sniff_utf16_without_bom`.

- `max_bytes` is the first statement of `decode`, before any detection, sniffing or decoding; `Core::import_txt_with_encoding`/`import_file` also check `fs::metadata` before `fs::read` (the pre-existing TOCTOU window of F13 is unchanged).
- UTF-8 path: valid input is borrowed (`Cow::Borrowed`, no copy). Invalid input is budgeted as `body.len().saturating_mul(3)` against `max_expanded_bytes` before `decode_without_bom_handling` allocates. Non-UTF-8 path: `Decoder::max_utf8_buffer_length(len)` (returns `None` on overflow, mapped to `usize::MAX`, so rejected) is compared with `max_expanded_bytes` before `decode_with_bom_removal` allocates. No arithmetic on external values other than `saturating_mul`. A run with `max_bytes = max_expanded_bytes = usize::MAX` and a 1000-byte input did not overflow.
- Detector sample: `DETECT_SAMPLE_BYTES` (1 MiB) bounds `chardetng`; `from_utf8` and the NUL sniff are linear over the whole input (cheap). Encoding label from the caller: capped at 64 bytes before `for_label`; `replacement` (which covers UTF-7 and other unsafe labels) is rejected as `UnknownEncoding`.
- UTF-16: odd-length input (`[FF FE 41]`, trailing byte) decodes lossily without panic; `as_chunks::<2>()` ignores a trailing odd byte, and the sniff's `pairs == 0` guard handles 0/1-byte input. BOM-only files (`EF BB BF`, `FF FE`, `FE FF`) give an empty document; a single `FF` is windows-1252, flagged uncertain.
- Measured (release, throwaway probe, deleted): 64 MiB of high bytes parsed in 1.75 s, 100 MiB random bytes in 3.5 s; 180 MiB legacy rejected in 82 ms (F82).
- No `unwrap`/`expect`/`panic!`/index-by-external-value in the new non-test code. The one `as` cast on external-derived data (`metadata().len() as usize`) is on a 64-bit target and was already the pattern in `import_txt`.
- Fuzz: see section 9. The existing target only called `parse`; I extended it (af7bbac) to call `parse_with_encoding` with fuzzer-chosen labels (a table of WHATWG labels incl. `replacement`, `x-user-defined`, `iso-2022-jp`, plus a label taken from the input bytes) and `detect_encoding`.

## 4. Item 2 — FFI surface and error mapping

- New exports: `detect_text_encoding`, `import_file_with_encoding`, `get_item_metadata` (methods on `GistCore`) and `estimate_reading_seconds` (free function). All four bodies are inside `ffi_catch!`.
- New variant `GistError::UnsupportedTextEncoding` is mapped from both `CoreError::Parse(UnknownEncoding)` and `ImportError::UnknownEncoding`, with fixed text (no label echoed back), and the unit test `text_encoding_detect_and_override_over_ffi` covers both paths. `limit_kind_to_error` is unchanged and exhaustive. `FfiDocumentMetadata`/`FfiTextEncodingInfo`/`FfiEncodingSource` are plain records/enums.
- `estimate_reading_seconds`: widens to `u64` before `* 60`, `div_ceil`, then `as u32`; the maximum is `u32::MAX * 60 / 100`, which fits. `wpm` is clamped to 100..=1000 (no division by zero).
- `FfiLibraryItem.word_count` is read through `row.get::<i64>().clamp(0, u32::MAX) as u32`, so a corrupt negative or huge column cannot wrap.

## 5. Item 3 — Schema v7 -> v8 migration

- `open_internal`: after the `SchemaTooNew` ceiling check, `if version < 8 { BEGIN; ALTER TABLE library_items ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0; UPDATE … SET word_count = (SELECT COUNT(*) FROM tokens …); PRAGMA user_version = 8; COMMIT; }` in one `execute_batch`. A failure drops the connection and the open transaction is rolled back (same mechanism as v7). `SCHEMA_VERSION` is 8, so a v9 file is `SchemaTooNew` (ceiling check precedes the migration; existing `newer_schema_is_rejected_with_schema_too_new` test, not re-read for its exact version).
- Backfill cost: `EXPLAIN QUERY PLAN` on a scratch SQLite database with the real `tokens` schema and `idx_tokens_item_idx` shows `SCAN library_items` plus `SEARCH t USING COVERING INDEX idx_tokens_item_idx (item_id=?)`; one index range count per item, no blob read, no decryption. It is a one-off pass at first open; I did not time it on a 100k-item library.
- Encrypted items: the backfill reads only the plaintext `tokens` table, so it needs no key and works for encrypted rows. Items with no token rows get 0 (F85a). `insert_item` computes the count from the same `token_stream` it writes to `tokens` in one transaction (both filter `TokenKind::Word`; `Document::new` uses the same definition), so the three cannot disagree for newly imported items. The unit test `migration_v7_to_v8_backfills_word_count_from_tokens_table` rewinds a real database to v7 and checks 5/120/0 words, and `old_db_compat` migrates real v3 and v5 fixtures to 8 and compares `word_count` to `COUNT(*)` of the `tokens` table.

## 6. Item 4 — ADR-019 additive compatibility (tested)

- **Old blob -> new struct:** `old_metadata_without_sc03_sc06_fields_decodes_with_defaults` plus the real old-version fixtures in `old_db_compat` (blobs written by older binaries) all load.
- **New blob -> old reader, independently:** I built `main`'s `gist-model` (extracted with `git archive main`) into a scratch binary, serialised a `Document` with all four new fields set using the reviewed tree, and had the main-built reader deserialise it: `old reader ok: title=rev words=4 tokens=4`. The old reader's re-serialised output (no new fields) then loaded in the new code with defaults (`tracked_changes = false`, other fields `None`) and an identical token count. So unknown keys are ignored (no `deny_unknown_fields` on `Metadata`) and no `ir_version` bump is needed; `CURRENT_IR_VERSION` is untouched.
- `cargo check -p gist-model --target wasm32-unknown-unknown` passes. `metadata_util` is pure (no I/O, no clock); the clock calls live in `gist-core`/`gist-web`.
- Caveat already recorded in ADR-019 addendum 3: an old binary that re-saves a document drops the new fields; documents are immutable after import.

## 7. Item 5 — `gist-web` metadata extraction on hostile HTML

Throwaway release-mode probe (deleted, not committed), `build_document_at` on:

| Input | Result |
|-------|--------|
| 1,000,000 `<meta>` elements | 0.77 s total; only the first 500 are inspected (`MAX_META_ELEMENTS`) |
| 200,000 empty `itemprop=author` / `rel=author` elements | 0.26 s; `take(5)`, text read capped at 1,000 chars |
| 30 MB `content` attribute on the author meta | 12 ms; byline capped at 200 chars |
| 100,000 malformed canonical links | 71 ms (`take(5)`) |
| canonical: `a b`, CRLF header-injection style query, `127.0.0.1`, `[::1]`, `‮` in path, `host\@evil`, `data:`, `javascript:`, `ftp:`, `file:`, `user:pw@` | non-http(s) and credentialed forms -> `None`; others normalised by `Url` (CR/LF stripped or percent-encoded, bidi percent-encoded, backslash treated as path). Private/loopback hosts are accepted and stored (F83) |
| author with CR/LF and U+0000 | CR/LF collapse to a space; NUL becomes U+FFFD in the HTML parser; byline is a single line (headers are never built from it) |
| malformed dates (`2024-13-45`, `2023-02-29`, `0000-00-00`, `<script>`, empty) | rejected; years limited to 1000-9999; future dates beyond retrieval + 1 day dropped |

The canonical URL is never fetched (no read path), and the diff does not touch `build_agent`/`safe_resolve`/`fetch_url`'s request path (only the retrieved-timestamp stamp), so F14 is unchanged. Language tags are ASCII-only, at most 35 bytes, hyphenated alphanumeric subtags. ePub `dc:language` goes through the same `normalize_language_tag`; a 100,000-char value yields `None` (tested).

## 8. Item 6 — Behavioural regression

- Previously invalid UTF-8 failed with `ParseError::Utf8`; it now imports (lossy or guessed). Intended (SC-01); `Core` tests cover it. Valid UTF-8 without BOM is byte-identical to before (borrowed, same `build_document`), so `section_text` offsets, annotation anchors and the Rust/Swift golden tests for ordinary files are unaffected. Only BOM'd UTF-8 changes (leading U+FEFF no longer in the first paragraph; F85b), which changes offsets only for new imports of such files.
- DOCX `source_type` is now `docx` rather than `docx:tracked-changes`: anything keyed on the old string must use the new flag. In-tree consumers: none found in `crates/` (the FFI normalises legacy rows). Apple/Windows sources were not part of this review.
- `import_date` is now stamped on every import path (txt, file, url, OCR) via `format_rfc3339_utc` (clamped, no panic before epoch).

## 9. Commands and results

Run on this worktree (branch `worktree-agent-a6a447b152eed9722` from `integration/m8-2026-10-11`):

- `cargo test --workspace`: 432 passed, 0 failed on the reviewed tip; **435 passed, 0 failed** after my commits (the new docx fixture tests and the bidi test).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean (rc 0). `cargo deny check bans licenses sources`: `bans ok, licenses ok, sources ok`. (`chardetng` 1.0.0 and `encoding_rs` 0.8.x are Apache-2.0/MIT, with `encoding_rs` also BSD-3-Clause; both already listed in `docs/THIRD-PARTY.md`.)
- `cargo check -p gist-model --target wasm32-unknown-unknown`: passes.
- Fuzz (`cd fuzz; cargo +nightly fuzz run … -- -max_total_time=130`): **`fuzz_parse_txt` 248,865 runs in 131 s** (with the extended label-driven target; cov 3,877, 3,663 corpus entries), **`fuzz_web_extract` 1,307,506 runs in 131 s**; zero crashes, no artifacts. Generated corpus entries were discarded (not committed). Same caveats as N1: one machine, short run.

## 10. Reviewer commits

| Commit | Change |
|--------|--------|
| af7bbac | `fuzz/fuzz_targets/fuzz_parse_txt.rs` also calls `parse_with_encoding` (fuzzer-chosen/hostile labels) and `detect_encoding` |
| 0dc183c | Regenerated `fixtures/docx/with_tracked_changes.docx` and `minimal_valid.docx` with `word/styles.xml`; `crates/gist-parse-docx/tests/fixtures.rs` (F80) |
| c08e70b | Strip bidi override/isolate controls in `sanitize_meta_text` + test (F79) |

## 11. Not done

No Swift/C# review; no Apple or Windows build; no timing of the v8 backfill on a 100k-item library; no real-world (non-synthetic) legacy-encoding corpus, so `chardetng` accuracy and the `MIN_NON_ASCII_FOR_CONFIDENCE = 16` uncertainty heuristic are untested beyond the unit tests and fixtures.
