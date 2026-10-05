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

- `parse_span_attr` accepts only ASCII digits, saturates overflow to `u32::MAX`, 0 and garbage -> 1. `layout_table` checks `nrows <= max_table_rows` first, rejects a cell whose `colspan` would pass `max_table_cols` using `want > max_cols - c` (no overflow), and clamps `rowspan` to the rows that exist, so allocation is bounded by rows x cols (default 2000 x 64 = 128k slots) whatever the declared spans say. Spans never overlap (a colspan stops at the first covered slot; slots covered from above also cover the current row, so lower rows are free).
- **Adversarial test** (temporary integration test, deleted, not committed): 200 000 random raw tables (up to 13 rows x 7 cells, tight limits 12x10, colspan/rowspan drawn from 0, `u32::MAX`, 1-100, 2-4, random `v_merge_continue`). ~28 k and ~65 k tables (two distribution runs) were accepted and every one satisfied: dimensions within limits, spans inside the grid, no two spans overlap, covered non-origin slots empty, `plain_text()` does not panic. The rest were correctly rejected. No panic, overflow or invariant break.
- Cost: O(cells + covered area), covered area <= grid; no quadratic path from many small spans.
- Serde: `spans` is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`; unmerged tables serialise byte-identically (test asserts no "spans" key). Forward compat (ADR-019 Addendum 2, no `ir_version` bump): the test `an_m6_binary_reads_a_spans_blob_with_the_grid_intact...` deserialises into a locally defined `pre_spans::Table{rows,header_row}` (no `spans`, no `deny_unknown_fields`) at max version 2 and gets the grid intact; the reverse (M6 blob without `spans`) loads with empty spans. The old-shape types are genuinely span-unaware, so the claim holds: an old binary shows merged cells as flat grid with empty covered slots, which is not misbehaviour.
- DOCX `gridSpan`/`vMerge` and epub/web `colspan`/`rowspan` all go through `layout_table`/`parse_span_attr` (fixtures and hostile-value tests in each crate pass). Swift side: `TableSpanMap` drops out-of-grid/overlapping spans and clips to the grid; the two cross-language golden strings match (`merged_table_golden_plain_text_matches_swift` and the Swift mirror, both pass in the 366/271 runs).
- Findings: F73 (per-cell `TableSpanMap` rebuild in `cellLabel`), F77 (continuation-cell text dropped). Also `TableSpanMap.init` has no early exit when a span overlaps, so a tampered blob with many full-grid spans costs O(spans x area); parsers never emit that.

## 7. Item 5 — Deployment target 14.0

Rebuilt with `./tools/build-core-xcframework.sh` (exit 0), then ran `vtool -show-build` on every object extracted (`ar x`) from both release slices:
- `aarch64-apple-darwin/release/libgist_ffi.a`: 895 objects; 504 at `minos 14.0` (C deps via `cc`), 391 at `minos 11.0` (Rust objects). Highest 14.0.
- `x86_64-apple-darwin/release/libgist_ffi.a`: 813 objects; 517 at `minos 14.0`, 296 Rust objects carry `LC_VERSION_MIN_MACOSX` 10.12. Highest 14.0.
- Debug `GIST.app` built by the test run: `Contents/MacOS/GIST` and `GIST.debug.dylib` arm64 `minos 14.0` (sdk 27.0); embedded `Frameworks/libpdfium.dylib` arm64 and x86_64 `minos 13.0`; `Info.plist` `LSMinimumSystemVersion` 14.0. The claim "highest minos 14.0" holds.
- The xcframework is still built from `libgist_ffi_universal.a` by explicit full path (`-library "$UNIVERSAL_LIB"`), so N8's static-link selection is intact; `export MACOSX_DEPLOYMENT_TARGET=14.0` is in the script.
- `xcodebuild test` log: 0 occurrences of "newer macOS"/"newer version"; the only warnings are two pre-existing Swift concurrency warnings in `OcrImportModel.swift`. The project builds at 14.0, so no unavailable API is used without `#available` (the compiler enforces that).
- `SearchFieldLocator` (`SearchFocusCompat.swift`): optional-chained `guard let`, no force unwrap, finite recursive walk of the window's view tree, keeps no references. Harmless no-op if no field is found. It is compile-verified only, never run on macOS 14.
- Not verified: a Release (`x86_64`+`arm64`) app link, and any run on a real macOS 14 machine.

## 8. Item 6 — fetch-pdfium.sh

Read in full (`tools/fetch-pdfium.sh`). Fail-closed paths: archive SHA-256 mismatch exits 1 before any extraction; `validate_archive` runs on the verified archive before `rm -rf`/`tar -xzf`; the extracted `lib/libpdfium.dylib` is re-hashed against `PDFIUM_DYLIB_SHA256` and `artifacts/pdfium` is removed on mismatch; the idempotent short-circuit re-hashes the on-disk dylib every run (the stamp is only written, never trusted). `curl` has `--proto '=https' --proto-redir '=https' --tlsv1.2`. Validation covers absolute, `..`, backslash/drive-letter, control-character names, entry count (cap 500), and non-`-`/`d` types plus GNU `link to` / `->` spellings. Running `./tools/fetch-pdfium.sh` printed "archive validated: 48 entries" and "verified and extracted".
TOCTOU: validation and extraction both read the same file in a private `mktemp -d` directory after the hash check; swapping it needs same-user write access (outside the threat model). Residual: only the dylib is hash-pinned individually; headers/licences extracted beside it are covered only by the archive hash.

**F71 (defect, fixed in b518d7f).** `bash tools/test-fetch-pdfium-guard.sh` on macOS: 12 ok, 1 `FAIL - fixture writer produced a tar that does not extract correctly`, with `cat: .../x/lib/libpdfium.dylib: Permission denied` and `rm: ... Directory not empty` leaving a temp tree. Cause: `write_entry` wrote mode 0644 for every entry including directories (typeflag 5), so bsdtar created `lib/` without a search bit. (It passes on Git Bash for Windows, where modes are not enforced.) Fix: 0755 for directories and `chmod -R u+rwx` before cleanup; rerun gives 13/13 ok. The self-test is still not wired into CI (F52).

## 9. Item 7 — Localisation guard

- Ran `bash tools/check-localisation.sh`: `358 literals checked, 0 missing`, exit 0. File mode is 100755 in git.
- Workflow (`apple-build.yml`): the new step is `run: ./tools/check-localisation.sh` with no `${{ }}` expressions or event data, so no shell-injection surface. Action pins unchanged (checkout, setup-xcode, rust-toolchain, cache are all 40-hex SHAs with version comments); `permissions: contents: read` intact.
- Negative test: I added a temporary Swift file with `Text("...")`, `Button("... \(1)")`, `.help("...")`, `Label("...")` strings absent from the catalog: all were reported (exit 1, 5 missing, 363 checked); the file was deleted.
- False negatives (F75): a triple-quoted `Text("""...""")` and a literal held in a `String` variable are not reported (the second is documented in the script header, the first is not). `Text(verbatim: "ok")` is reported as missing (false positive, fails closed). The catalog is parsed with a regex tied to Xcode's 4-space layout and dies if no keys parse, so format drift fails loudly.

## 10. Item 8 — Paginated view

- **Position-meaning rule holds.** `grep` for `FlowScrollPositionStore`, `saveProgress`, `persistsFlowScrollFraction`: the paged layout declares `persistsFlowScrollFraction = false`; `FlowReaderContainer` (line 89) writes the flow store only when that is true; `goToPage` writes only `PagedPositionStore`; the only reads of the flow store are in `seedProgress` (first entry, read-only). `core.saveProgress` is called only from `RsvpView`. `markItemOpened` is the only core call from either flow layout (via `openFlowDocument`).
- **Paginator robustness** (`Paginator.swift`): page height and spacing pass through `finite()` and a 40 pt floor; line/atomic heights are finite-clamped; NaN/inf/zero cannot loop, because every page places at least one element and a `cursor <= position` guard forces progress (a degenerate duplicate `lineStarts` list could make that guard skip the rest of a block, losing display of text, never hanging; the measurer emits strictly increasing starts). `pageIndex`/`pieces`/`clampedPage` clamp. `PagedPositionStore.load` clamps to >= 0 and `PagePosition.clamped` clamps to the document; non-`Int` values read as nil.
- **Annotation anchors** stay ADR-003 `(section id, UTF-8 byte offset)`; conversion goes through `PagedDocumentIndex` and does not involve page size. Mid-scalar byte offsets map to the end of the block (acceptable; anchors fall on scalar boundaries).
- No force unwraps or `try!` in the three new Swift files (`grep`). Repagination runs `.task(id:)` with a 120 ms debounce, a detached measuring task, and `guard !Task.isCancelled` before publishing results, so a stale result is discarded; the stale detached task still runs to completion (F74).
- **Findings:** F72 (O(N^2) `position(forAnchorSection:)`), F73 (per-cell `TableSpanMap`), F74 (per-cell `NSLayoutManager` in the table estimate; `NSFontManager.shared` off main), F78 (linear `firstLine` scan). Also `pieceView` recomputes `blockByteOffset` per rendered piece (O(index)), the same cost the scroll view already has. A single multi-megabyte paragraph builds an `AttributedString` for the whole block on every page render before slicing; unmeasured, no display here.
- No display access: layout fidelity, clipping and slack are untested by me (see `docs/qa-manual-clickthrough-m3.md`).

## 11. Item 9 — PDF isolation spike

- **Not part of any shipping target:** root `Cargo.toml` has `exclude = ["spikes"]`; the spike has its own `[workspace]` and lockfile; `cargo test --workspace` (366 tests), `cargo deny check` and `cargo clippy --workspace` never saw it; `grep` finds no reference in `project.yml` or any workflow. The only root `Cargo.lock` change is a `serde_json` dev-dependency edge on `gist-parse-pdf` (for its `parse_file` example).
- **Re-ran the crash-containment script** after `cargo build --release --offline` in `spikes/pdf-isolation` (12.8 s): `crash.sh` gave `HelperCrashed{signal=6}` (abort), `{signal=11}` (SEGV), `{timeout}` after 20 055 ms (hang), host exit 0 each time, and a normal parse afterwards returned `Ok` (26 571 B JSON), matching the results document. I did not re-run the benchmark table.
- **ADR-022 reasoning**: consistent with D3 (spike only, no product change) and with the numbers; the recommendation (accept for v1.0, portable helper later) follows from Low severity, the unmeasured signing/N8 risk and the weak privilege separation of an inherited sandbox. Two gaps (F76): it does not mention that a sandboxed app's spawned helper must be signed with `com.apple.security.inherit` to launch at all, and it carries an empty duplicate `## Recommendation` heading. The measurements are synthetic fixtures on one unsigned macOS 27 machine, which the document says itself.

## 12. Items 10, 11, 13 — Windows handoffs, claim check, hygiene

(to be filled)

## 13. Item 12 — Fuzzing

(to be filled)

## 14. Could not verify

(to be filled)

## 15. Gates run on the tree

(to be filled)
