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

(to be filled)

## 4. Item 2 — Typed resource-limit errors

(to be filled)

## 5. Item 3 — PDF text budget and find_gutters

(to be filled)

## 6. Item 4 — Merged-cell tables

(to be filled)

## 7. Item 5 — Deployment target 14.0

(to be filled)

## 8. Item 6 — fetch-pdfium.sh

(to be filled)

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
