# GIST — Security & Quality Review of M6 (2026-10-04)

**Role:** M6 R7 (independent review). **Reviewed tip:** `integration/m6-2026-10-03` @ `6cb848d`. **Review branch:** `worktree-agent-aa9c754bbe67f3665` (reviewer commits on top of `6cb848d`).
**Method:** as in `docs/security-quality-review-2026-09-29.md` / `docs/security-review-v2.md`: claims were re-derived from source and by running commands, not taken from `CLAUDE.md` or role reports. Anything not run is listed in §9.

## 1. Executive summary

M6 (PDF import on pinned pdfium, `Block::Table` + IR v2, PDF UI + scanned-PDF OCR fallback) is in good shape. No High/Medium defect was found. The supply-chain pin is genuinely enforced (the pinned SHA-256 matched a fresh download), the dev-only library-path fallbacks are genuinely compiled out of the shipped Rust core, every limit check I traced runs before the allocation it guards, and ADR-019's forward-compat claim is backed by real tests.

Two real (low-severity) defects were found, proven with a test or script, and fixed in this review: a heading-level `u8` wrap in `gist-parse-pdf` (F31) and a NaN pixel size in `PdfPageRenderer.pixelSize` (F32). Four further low/informational findings are reported, not fixed (F33–F36). The CLAUDE.md claim that `gist-model` compiles to `wasm32-unknown-unknown` was false and is now fixed (F37), and F29's `BSD-2-Clause` allow-list entry was pruned.

## 2. Findings table (proposed register IDs)

| ID | Severity | Status | Summary |
|----|----------|--------|---------|
| F31 | Low | Fixed (fdc73c7 + style 4th commit) | `layout.rs::level_of` did `(rank + 1) as u8` before clamping: a PDF with >=256 distinct heading font sizes produced `Block::Heading { level: 0 }` / `Section.heading` level 0. Regression test `many_distinct_heading_sizes_never_yield_level_zero` fails without the fix (`level 0`), passes with it. |
| F32 | Low | Fixed (Swift) | `PdfPageRenderer.pixelSize` with a non-finite page dimension returned NaN (`inf * (cap/inf)`), and `Int(px.width)` in `renderJPEG` would trap. Proven with a standalone swift script (`nan 1.0`). Reachability from real PDFKit output not proven (PDFKit may clamp). Guard + test `testPixelSizeIsAlwaysFiniteForHostileGeometry`. |
| F33 | Low | Open | pdfium (C++, in-process) parses untrusted files with no process isolation. A native crash is not caught by `ffi_catch!` and kills the app. Lost state is limited to unsaved in-memory UI edits (library/progress persist in SQLite); a PDF import in flight persists nothing. Recommend an XPC-service/out-of-process parse for a later milestone. |
| F34 | Low | Open | `fetch-pdfium.sh`: (a) the idempotence stamp short-circuits without re-hashing the on-disk dylib, so a tampered `artifacts/pdfium` is embedded unchecked; (b) `curl --proto '=https'` does not restrict redirect hops (`--proto-redir`), saved only by the SHA-256 check; (c) the archive is extracted with plain `tar -xzf` with no pre-listing. Extracted tree was clean (45 files, no symlinks, nothing outside the dir). Recommend re-hash on stamp hit and `--proto-redir '=https'`. |
| F35 | Low | Open (code reading, not run) | `OcrImportModel.beginPdf` runs the render in `Task.detached` and passes `isCancelled: { Task.isCancelled }`, which evaluates the *detached* task, so cancelling the outer `scanTask` does not stop rendering; it runs to completion (up to 2000 pages / 2 GiB temp) before the outer task notices and cleans up. No sweep of stale `gist-pdf-ocr-*` temp dirs after a crash. Recommend `withTaskCancellationHandler` forwarding to the inner task. |
| F36 | Informational | Open | Resource shape of PDF import: default `max_expanded_bytes` (512 MiB) is the only total-text budget, and extracted line text for all pages is retained until `build_document`, so a hostile 256 MiB PDF can drive multi-GB peak memory; `find_gutters`/`page_to_lines` are O(fragments x gutters) (<= ~500 gutters) per page; `index as PdfPageIndex` (u16) would truncate if `max_pages` were ever raised above 65 535. Local-only DoS; consider a smaller PDF-specific text budget. |
| F37 | Low | Fixed | `gist-model` did not compile for `wasm32-unknown-unknown` (uuid v7 needs an RNG source). Target-specific `uuid` `js` feature added; native builds and `Cargo.lock` unchanged. |
| F38 | Informational | Open (cannot run here) | Windows `core-test` leg and its VC++-runtime-import check on `gist_ffi.dll` are unrun for M6; see §6. |
| F27 | Informational | Still open | Windows security/quality review outstanding; macOS cannot do it. |
| F29 | Hygiene | Closed (partly) | `BSD-2-Clause` allow entry pruned (checks pass with it removed); the 5 duplicate-crate warnings remain (`getrandom`, `hashbrown`, `miniz_oxide`, `syn`, `windows-sys`). |
| F30 | Process | Recommend adopt | See §8. |

## 3. pdfium as a native dependency (checklist 1)

(to be extended)

## 4. gist-parse-pdf limits and layout (checklist 2)

(to be extended)

## 5. FFI, tables, Swift renderer (checklist 3-5)

(to be extended)

## 6. CI / workflows (checklist 6)

(to be extended)

## 7. Fuzzing (checklist 7)

(to be extended)

## 8. Hygiene, AGG licence, F27/F30 (checklist 8-9)

(to be extended)

## 9. Could not verify

(to be extended)
