# GIST — Security & Quality Review of M7 (2026-10-05)

**Role:** M7 R-final (independent review, protocol step F30). **Reviewed tip:** `integration/m7-2026-10-04` @ `c0c8ff6` (base `main` @ `1668bca`). Reviewer commits sit on top of `c0c8ff6`.
**Method:** as in `docs/security-quality-review-2026-10-04.md`: claims re-derived from source and by running commands, not from CLAUDE.md or role reports. Anything not run is listed in section 14.

**ID note:** the brief said to start at F39, but `CLAUDE.md`'s register already holds F39–F70 (Windows W4/W5 reviews). Findings below therefore use **F71 onward**.

## 1. Executive summary

No High or Medium defect found. Schema v7, the typed limit errors, the PDF text budget, merged-cell layout and the deployment-target change all hold up against source and adversarial tests. One real defect was found and fixed (F71, the fetch-pdfium guard self-test fails on macOS). Several Low performance/robustness items in the new paginated view and table code are reported (F72–F75).

## 2. Findings table (proposed register IDs)

| ID | Severity | Status | Summary |
|----|----------|--------|---------|
| F71 | Low | Fixed (b518d7f) | `tools/test-fetch-pdfium-guard.sh` fails on macOS (12 of 13 ok, 1 FAIL, temp tree not removable): its ustar writer gave directory entries mode 0644, so extraction produced a directory with no search bit ("Permission denied" reading `lib/libpdfium.dylib`). Fix: 0755 for typeflag 5 plus `chmod -R u+rwx` before cleanup. Now 13/13. Not run in CI (F52 still open). |
| F72 | Low | Open | `PagedDocumentIndex.position(forAnchorSection:byteStart:)` (`Paginator.swift`) calls `FlowSectionVM.blockByteOffset(at:)` (O(i), recomputes `plainText`) inside a loop over all blocks: O(N^2) per annotation jump in a section with N blocks. Fix: accumulate the offset incrementally. |
| F73 | Low | Open | `TableAccessibility.cellLabel` builds a fresh `TableSpanMap` (allocates rows x cols slots) for every data cell body evaluation when `headerRow` is true; `cell(...)` already has the map. Wasteful on big tables (up to 2000x64). Pass the map in. |
| F74 | Low | Open | `PageBlockMeasurer.estimatedTableHeight` creates an `NSTextStorage`+`NSLayoutManager` per non-empty cell (up to 128k at the table caps) on a detached task that is not cooperatively cancellable; `NSFontManager.shared` is also used off the main thread. Spinner, not a hang, but unmeasured. |
| F75 | Info | Open | `tools/check-localisation.sh` false negatives/positives: triple-quoted string literals and strings held in `String` variables are not checked (the latter is documented, the former is not); `Text(verbatim:)` literals are flagged as missing (false positive). Verified by a temp Swift file: plain `Text`/`Button`/`.help` misses are caught. |
| F76 | Info | Open | ADR-022 omits that a sandboxed app's `posix_spawn`ed helper needs `com.apple.security.inherit` (plus app-sandbox) to launch, which affects option (b)'s cost; it also has an empty duplicate `## Recommendation` heading. |
| F77 | Info | Open | DOCX `w:vMerge` continuation cells that contain text have that text dropped by `layout_table` (spec says they are empty; fidelity only). |
| F78 | Info | Open | `Paginator.firstLine(atOrAfter:)` is a linear scan per page start: O(lines x pages) inside one huge block. Binary search would do. |
| F27 | Info | Closed per CLAUDE.md | Windows review exists (`docs/security-review-windows.md`). Not re-reviewed here. |
| F29 | Hygiene | Open | `cargo deny` still lists the 5 benign duplicate crates; checks pass. |

## 3. Item 1 — Schema v7 and reading state

- **Migration** (`gist-store/src/lib.rs` ~960): `BEGIN; ALTER TABLE library_items ADD COLUMN last_opened_at INTEGER; CREATE INDEX IF NOT EXISTS idx_tokens_item_idx ON tokens(item_id, token_idx); PRAGMA user_version = 7; COMMIT;` in one `execute_batch`, guarded by `version < 7` and after the `SchemaTooNew` ceiling check. Nullable column, no rewrite, no backfill; no data loss. A failure mid-batch drops the connection (rollback). The index build is a one-off O(n log n) pass over `tokens` at first open of a v6 database; not timed on a large library. The index also helps the `ON DELETE CASCADE` on `tokens.item_id`. Tests: `newer_schema_is_rejected_with_schema_too_new`, `old_db_compat` from v3 and v5.
- **SQL** (`LIBRARY_ITEM_SELECT`): constant string; the only variable parts are bound parameters (`?1`/`?2`), so no injection surface. `json_extract` is guarded by `json_valid`. Progress: `COALESCE(CASE WHEN COALESCE(rp.token_index,0)>0 THEN MIN(1.0, ti*1.0/MAX((SELECT MAX(token_idx)...),1)) END, 0.0)` — NULL (no tokens) yields 0.0, divisor floored at 1, result also clamped in Rust. `tokens.token_idx` is the stream index of Word tokens (`insert_item`), matching `reading_progress.token_index`. The subquery runs only for rows with a saved position; `item_row_queries_use_the_token_index` asserts the plan uses the index.
- **`mark_item_opened`**: one bound `UPDATE`, poison-tolerant lock (`unwrap_or_else(|p| p.into_inner())`), unknown id is a no-op. No new bare `.lock().unwrap()` in the diff.
- **FFI**: exactly one new `#[uniffi::export]` (`mark_item_opened`), `ffi_catch!`-wrapped. I checked every `pub fn` before the first `#[cfg(test)]` in `gist-ffi/src/lib.rs` with a script: 57 of 57 contain `ffi_catch!` (plus the panic probe). `FfiLibraryItem` gained `source_type`, `last_opened_at`, `progress_fraction`; the four duplicated mappings were replaced by one `From`.
- Result: no defect. Plan text saying "v6" is stale (ADR-021 says so itself); source is v7.

## 4. Item 2 — Typed resource-limit errors

- `gist_model::LimitKind` (7 values) is carried by every parser's `ResourceLimitExceeded` (a required field, so the compiler enforces that no construction site omits it). I listed every non-test `LimitKind::` site (`grep`): txt, epub, docx, pdf, web, imageprep (`map_imageprep_error`), core OCR page/size caps; kinds match the limit (nesting -> `TooDeeplyNested`, entry count -> `TooManyEntries`, tables -> `TableTooLarge`, redirects -> `Other`).
- `gist-core` maps epub/docx/pdf/txt/web limit errors into `ImportError::ResourceLimitExceeded{kind}`; `CoreError::limit_kind` covers `import_txt`. `gist-ffi::limit_kind_to_error` is an exhaustive `match` (a new kind fails to compile). The 7 flat `GistError` variants have fixed `#[error]` text: no paths, numbers or payloads; test `every_limit_kind_maps_to_a_distinct_path_free_gist_error` feeds a limit string containing a path and a count and asserts neither appears.
- Swift: `ImportLimitMessage` switches on the case only; it shows the user's own file name (last path component) or URL host. `CoreClient.importFile/importUrl` have the new branch; DRM, `PdfEncrypted`, `PdfUnavailable` and `PdfNoTextLayer` branches are untouched in the diff. Xcode build is exhaustive over `GistError`, so no case is unhandled.
- Result: no defect found.

## 5. Item 3 — PDF text budget and find_gutters

- `text_budget(limits) = min(max_expanded_bytes, MAX_PDF_TEXT_BYTES = 64 MiB)`. Order in `pdfium_backend::extract`: `max_pages` before any page loads; per page, pdfium's char count `n` is checked against `MAX_GLYPHS_PER_PAGE` and `remaining_chars` before `glyphs.reserve(n)`, then `remaining_chars` is reduced; `lib.rs` re-checks UTF-8 text bytes per page and `build_document` checks the total. Chars vs UTF-8 bytes: the char check bounds allocation, the byte check catches multi-byte text at most one page (<= 8 MB) late.
- `checked_page_index` uses `PdfPageIndex::try_from` (u16); out-of-range is a `TooManyPages` error, not a wrap (unit test covers 65 536).
- Consuming `pages.into_iter().zip(mask)` compiles without use-after-move; output unchanged (existing layout tests pass).
- **`find_gutters` rewrite**: `left = x1s.partition_point(|v| v <= gx0+bw)` equals the old `count(x1 <= gx0+bw)` and `right = n - x0s.partition_point(|v| v < gx1-bw)` equals `count(x0 >= gx1-bw)` for finite values; `-0.0`/`0.0` compare equal under the predicates so `total_cmp` ordering cannot differ. Independent check: I temporarily added a differential test (reverted, not committed) running the old implementation against the new on 40 000 adversarial layouts (integer-quantised coordinates to force ties with bin edges, zero-width and negative-x fragments, 8-67 fragments, page widths 612/0/-5/1e9/1): all equal, 14 800 of them with non-empty gutters.
- No defect found. The 64 MiB figure rests on the role's own memory measurements on synthetic PDFs; I did not re-measure.

## 6. Item 4 — Merged-cell tables

(to be filled)

## 7. Item 5 — Deployment target 14.0

(to be filled)

## 8. Item 6 — fetch-pdfium.sh

Read in full (`tools/fetch-pdfium.sh`). Fail-closed paths: archive SHA-256 mismatch exits 1 before any extraction; `validate_archive` runs on the verified archive before `rm -rf`/`tar -xzf`; the extracted `lib/libpdfium.dylib` is re-hashed against `PDFIUM_DYLIB_SHA256` and `artifacts/pdfium` is removed on mismatch; the idempotent short-circuit re-hashes the on-disk dylib every run (the stamp is only written, never trusted). `curl` has `--proto '=https' --proto-redir '=https' --tlsv1.2`. Validation covers absolute, `..`, backslash/drive-letter, control-character names, entry count (cap 500), and non-`-`/`d` types plus GNU `link to` / `->` spellings. Running `./tools/fetch-pdfium.sh` printed "archive validated: 48 entries" and "verified and extracted".
TOCTOU: validation and extraction both read the same file in a private `mktemp -d` directory after the hash check; swapping it needs same-user write access (outside the threat model). Residual: only the dylib is hash-pinned individually; headers/licences extracted beside it are covered only by the archive hash.

**F71 (defect, fixed in b518d7f).** `bash tools/test-fetch-pdfium-guard.sh` on macOS: 12 ok, 1 `FAIL - fixture writer produced a tar that does not extract correctly`, with `cat: .../x/lib/libpdfium.dylib: Permission denied` and `rm: ... Directory not empty` leaving a temp tree. Cause: `write_entry` wrote mode 0644 for every entry including directories (typeflag 5), so bsdtar created `lib/` without a search bit. (It passes on Git Bash for Windows, where modes are not enforced.) Fix: 0755 for directories and `chmod -R u+rwx` before cleanup; rerun gives 13/13 ok. The self-test is still not wired into CI (F52).

## 9. Item 7 — Localisation guard

(to be filled)

## 10. Item 8 — Paginated view

(to be filled)

## 11. Item 9 — PDF isolation spike

(to be filled)

## 12. Items 10, 11, 13 — Windows handoffs, claim check, hygiene

(to be filled)

## 13. Item 12 — Fuzzing

(to be filled)

## 14. Could not verify

(to be filled)

## 15. Gates run on the tree

(to be filled)
