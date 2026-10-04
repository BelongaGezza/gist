# M7 Execution Plan — Agent Roles & Team Structure

**Status:** ADOPTED 2026-10-04 (D1–D7 resolved in "Decisions resolved" below; D1 is a gate that R5 feeds). `CLAUDE.md`'s "proceed with development" pointer and `.claude/commands/proceed-with-development.md` now target this file. Scope authority remains `docs/development-plan-v2.md` and `docs/product-spec-reader-app-v3.md` §11 (phasing); this file refines them into assignable work, modelled on `docs/m6-agent-roles.md`.

**Platform scope:** macOS session (`apple(macOS/iOS)=yes`). Windows work (W4–W6, the `F27` review) cannot be built or tested from here; it is not in this plan except where it constrains macOS work (§1, item 7).

## Why this plan looks different from M3–M6

M6 closed every agent-executable item in the v1.0 backlog that the earlier plans knew about. What is left of *v1.0* is human/credential-gated (`docs/v1.0-release-checklist.md`: signed build, `N8`, notarization, manual QA, beta). So M7 is not "the next feature" — it is the work found by checking the **spec's v1.0 promises and the decided policies against the current code**, plus the open findings and the deferred v1.1/iOS items. Each gap below was checked against source on 2026-10-04, not copied from a status document:

1. **Three v1.0 library sort keys are specified but not implemented.** Spec §4/§11 lists v1.0 sort by *name / type / date added / date last read / progress*. `LibrarySortOrder` (`apps/apple/Shared/LibraryView.swift:56`) has only `dateAddedNewest/Oldest`, `titleAZ/ZA`, `authorAZ`. Missing: **source type, date last read, progress**. The data for the last two is not in one place: RSVP position lives in `gist-store`'s `reading_progress` (a *token index*), flow-view position lives in `UserDefaults` (`FlowScrollPositionStore`, a block *fraction*) — `CLAUDE.md` M2 item 5 deliberately kept them apart because they mean different things, and flagged a unified field as a schema question. Sorting by "last read"/"progress" therefore needs a schema decision (D2), not just a Swift picker.
2. **The deployment target contradicts a decided policy.** `docs/development-plan-v2.md` Q9 decided **minimum macOS 14**. `apps/apple/project.yml` sets `MACOSX_DEPLOYMENT_TARGET: "26.5"` (introduced by commit `cc5d539`, "fix: clean build on macOS 26 / Xcode 26.6", 2026-09-10; the original M0 scaffold used 14). As built, GIST would refuse to launch on any Mac older than 26.5. Whether that was intended is unknown — it needs an evidence-based decision (D1).
3. **PDF quality is unproven on real documents.** `gist-parse-pdf`'s layout heuristics (columns, headings, header/footer stripping) were tuned only on synthetic fixtures (M6 R1/R7 both say so). Development-plan risk **R1** ("PDF reading-order quality", L=H, I=H) is *mitigated on paper only*. Known gaps: rotated text, RTL scripts, footnotes, tables inside PDFs.
4. **PDF/import findings still open from the M6 review** (`docs/security-quality-review-2026-10-04.md`): `F33` (pdfium parses untrusted files in-process; a native crash kills the app), `F36` (a single 512 MiB text budget; hostile-PDF memory shape), and an R2 gap — `ResourceLimitExceeded` crosses FFI as an untyped `Core(String)`, so the UI cannot show a specific "too large / too many pages" message for PDFs.
5. **Localisation drift.** M6 R2/R3 added user-visible strings, but interpolated ones ("Table, N rows…", page counts) and several fixed strings have no `Localizable.xcstrings` entry and fall back to English.
6. **Deferred by decision, not forgotten:** paginated view (Q3, spec: v1.1), merged-cell tables (R3's documented gap: `gridSpan`/`vMerge`), and the whole **iOS** port (spec: follows macOS v1.0, ~35–45% of the macOS shell effort; M6 skipped it because no simulator runtime is installed).
7. **Windows constraint on macOS work.** The Windows flow reader (W5) must decode `Block::Table` and keep the **tab / newline separators** pinned by the Rust/Swift golden test, or annotation anchoring will misalign there too. Any M7 change to the IR or `section_text` rules must add a `PENDING_WINDOWS_CHANGES.md` entry.

## Decisions (the questions as asked; outcomes are in "Decisions resolved" below)

| # | Decision | Recommendation |
|---|---|---|
| D1 | **Minimum macOS version:** ratify 26.5, or restore the decided 14 (or 15)? | Run R5 (a compile-only availability audit) first, then decide with the list of APIs that actually need newer than 14. A 26.5 floor excludes essentially every user who has not already upgraded; if only a few call sites need it, lower the target and use `#available`. If 26.5 *was* intended, record it as a deliberate change to Q9 in the dev plan and README. |
| D2 | **Reading-state model** (needed for sort by date last read / progress): add `last_opened_at` + `progress_fraction` columns written by *both* readers (schema v6, migration, ADR), or keep RSVP/flow positions separate and sort by RSVP only? | Unified columns, with each reader still keeping its own *position* store — only the two summary fields (when, how far) are shared. This is what the spec's sort keys mean to a user and avoids "progress" depending on which mode you last used. |
| D3 | **`F33` pdfium isolation:** implement out-of-process parsing (XPC service) in M7, or document-and-accept? | R4 writes the ADR and a measured spike first (cost: signing/entitlements for a second bundle, IPC copy of up-to-256 MiB inputs, `N8`-style signing of the service). Decide implement-vs-accept from that data; do not build the service blind. |
| D4 | **Scope beyond Track A:** include v1.1 items (R7 merged cells, R8 paginated view) and/or the iOS bring-up (R9) in this milestone? | Track A + review is the v1.0-relevant core. R7/R8 are low-urgency polish; R9 only if D5 is resolved. Do not start the iOS *shell* (R10) in the same milestone as its build-system bring-up. |
| D5 | **iOS prerequisites:** install a simulator runtime (Xcode → Settings → Components); and decide Q5 explicitly — `URLSession` (plan text) **bypasses** `gist-web`'s SSRF-safe resolver (`F14`), so either re-implement the address filter in Swift or route iOS URL import through the Rust fetcher. | Route through `gist-web` (one audited implementation). Without a simulator, R9/R10 stay out. |
| D6 | **Real-PDF test corpus:** which documents may be added as fixtures, and who vouches for provenance? | Public-domain / US-federal-government works and Project Gutenberg only, each with source URL, licence and retrieval date in `fixtures/README.md`; no copyrighted PDFs, no user documents. Keep files small (trim pages) so the repo does not bloat; large ones stay out of git and are fetched by hash like pdfium. |
| D7 | **Does the v1.0 tag wait for M7?** | Only for the two items the spec promises for v1.0: **R1 (sort keys)** and **R2 (real-PDF validation)**. Everything else is post-v1.0. Record this explicitly so the release checklist and this plan agree. |

### Decisions resolved 2026-10-04 (user)

| # | Outcome |
|---|---|
| D1 | **Wait for R5.** R5 (compile-only availability audit) runs first and reports; the minimum-macOS decision (ratify 26.5 vs restore 14/15) is made *after* seeing its list of APIs. Q9 and `project.yml` stay as they are until then. R5's apply step is gated on that decision — the Team Leader asks the user again with R5's evidence. |
| D2 | **Add a `last_opened_at` column; take progress from the RSVP position only.** Schema v6 adds `last_opened_at` to `library_items`, written by *both* readers. **No `progress_fraction` column.** "Progress" for sorting and the row indicator is derived from the existing RSVP `reading_progress` (token index ÷ total tokens). **Known, accepted limitation (must be documented in the ADR, UI copy and `CHANGELOG.md`):** an item read only in the flow view has no RSVP position, so it shows 0% / sorts as unstarted for *progress* even though *last read* is correct. If that proves unacceptable, a shared progress column is a follow-up schema change (v7). |
| D3 | **Spike with measurements.** R4 writes the ADR and a measured prototype only; it does **not** implement out-of-process parsing in M7. Implementation, if wanted, is decided afterwards from R4's numbers. Measurements are on synthetic/hostile fixtures only (see D6). |
| D4 | **Include the v1.1 items** — R7 (merged-cell tables) and R8 (paginated view) are in scope for M7. |
| D5 | **iOS paused.** R9 (and any iOS work) is out of M7. No simulator runtime install and no Q5 decision are needed now; both stay open in the dev plan. |
| D6 | **Skip real PDFs.** R2 is **dropped**. No third-party PDFs are added as fixtures, and PDF layout quality stays *unmeasured on real documents* — development-plan risk R1 remains mitigated on paper only. This must stay visible: `CHANGELOG.md`'s known limitations and the v1.0 release checklist keep saying PDF layout is tuned on synthetic files only. Revisit if early users report bad extraction. R3's `F36` budget and R4's measurements therefore use the existing hostile/synthetic fixtures, not real-world documents. |
| D7 | **v1.0 waits for M7.** Because D6 dropped R2, the M7 gate for the v1.0 tag is **R1 (the missing sort keys) plus R-final** — not PDF quality. Record this in `docs/v1.0-release-checklist.md`. |

Batches (smaller than M6's to keep per-session agent load and usage low): **Batch 1:** R5, R1, R3. **Gate:** R5 reports → user decides D1 → R5 apply step. **Batch 2:** R4, R6, R7. **Batch 3:** R8. **Batch 4:** R-final.

---

## 0. Team Leader role

Follow `.claude/commands/proceed-with-development.md` (once repointed to this file), which carries the permanent protocol including **step 7, the independent security/quality review by a fresh agent** (`F30`, adopted 2026-10-04). In brief:

1. **Preflight.** `PENDING_APPLE_CHANGES.md` / `PENDING_WINDOWS_CHANGES.md`; `git status` clean; `git fetch`; confirm `main` is current. Create `integration/m7-<date>` off `main` and commit the plan adoption there **before spawning**, so every worktree contains the plan.
2. **Worktree base check.** Worktrees have been created from a stale commit before (M6: R1 started on a pre-adoption commit). Brief every agent to run `git log --oneline -1` first and `git reset --hard <integration tip>` if behind (safe: fresh worktree), and to read the plan by absolute path if it is missing.
3. **Compute the ready batch** from §2; spawn in parallel with `isolation: "worktree"`; self-contained prompts.
4. **Integrate one at a time** onto the integration branch; re-run the full gate between merges when two roles touch the same crate.
5. **Full verification on the integrated tree:** `cargo test --workspace` (with `./tools/fetch-pdfium.sh` and `GIST_REQUIRE_PDFIUM=1`), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources`, `cargo check -p gist-model --target wasm32-unknown-unknown`, `xcodegen generate` + `xcodebuild test` (scheme `GISTmacOS`, `CODE_SIGNING_ALLOWED=NO`, skip `KeychainKeyProviderIntegrationTests` headless).
6. **Independent review (R-final)** by a fresh agent that built none of the code; merge and re-verify its output like any other role.
7. **Update the record** (`CLAUDE.md` milestone + security registers, dev plan, CHANGELOG, this file's §2 status). Correct stale claims explicitly with dated notes.
8. **Report, then stop before publishing.** Branch → PR; wait for CI; merge manually (auto-merge is disabled).

Operational lessons from M6 to apply: keep agent tool calls small and run long commands in the background (an agent stalled writing a large file via a shell heredoc and was killed by the 600 s watchdog); `grep` is aliased to `ugrep` in this shell — scripts use `command grep`; macOS `head`/`sed` are BSD (no `head -n -1`).

---

## 1. Already done — do not re-assign

- Everything in M0–M6, including PDF import (`gist-parse-pdf`, embedded pinned pdfium, scanned-PDF → OCR routing), tables (`Block::Table`, conditional `ir_version` 2, flow-view grid), OCR, annotations + re-anchoring, TTS, 6-tab Settings, localisation catalog, integrity/encryption, IR versioning, unsigned release pipeline, `F30` review step, `F31`/`F32`/`F34`/`F35`/`F37`/`F38`.
- `ReadingLayout` protocol (the paginated view's seam) and `build-core-xcframework.sh`'s iOS slices.
- Full table *rendering* in the flow view — spec lists it as v1.1; it shipped in M6.

---

## 2. Backlog → roles

| # | Track | Role | Scope | Depends on | Status |
|---|---|---|---|---|---|
| R1 | A: v1.0 completeness | Rust+Swift — reading-state model and the missing sort keys (type / last read / progress) | ADR, `gist-store` schema v6 + migration, `gist-core`/`gist-ffi`, `CoreClient`, `LibraryView`/`CollectionDetailView`, row progress UI, tests | D2, D7 | Not started (D2: `last_opened_at` only; progress from RSVP) |
| R2 | A: v1.0 quality | Rust — real-PDF corpus and layout tuning | `fixtures/pdf/real/`, `fixtures/README.md`, `crates/gist-parse-pdf/src/layout.rs`, `corpus.rs`, quality notes | D6, D7 | **Dropped (D6: no real PDFs)** |
| R3 | A: PDF hardening | Rust+Swift — typed resource-limit error, PDF text budget (`F36`) | `gist-core`/`gist-ffi` error variants, `gist-parse-pdf` budget, `CoreClient` alert, tests | — | Not started |
| R4 | A: PDF hardening | Design+spike — `F33` out-of-process pdfium (ADR, measured prototype; implementation only if D3 says so) | `docs/adr/`, `apps/apple/` spike branch | D3 | Not started (D3: spike + measurements only, no implementation) |
| R5 | A: platform floor | Swift — deployment-target availability audit (compile at 14 / 15), then apply D1 | `apps/apple/project.yml`, `Shared/*`, `macOS/*`, README/dev plan | D1 | Not started (audit first; apply step gated on the user's D1 decision) |
| R6 | A: polish | Swift — localisation completion + string audit | `Localizable.xcstrings`, call sites | — | Not started |
| R7 | B: v1.1 | Rust+Swift — merged-cell tables (`gridSpan`/`vMerge`) | `gist-model`, DOCX/epub/web parsers, ADR-019 addendum (IR version decision), `FlowTableView.swift` | D4 | Included (D4) |
| R8 | B: v1.1 | Swift — paginated reading view (Q3) as a second `ReadingLayout` | `apps/apple/macOS/` | D4 | Included (D4) |
| R9 | C: iOS | Build system — iOS target, shared-code compile, simulator tests, CI | `project.yml`, `apps/apple/iOS/`, `.github/workflows/apple-build.yml` | D4, D5 | **Paused (D5)** |
| R-final | review | Independent security/quality review (fresh agent) + hygiene (`F29` duplicates, `THIRD-PARTY.md` drift) | `docs/security-quality-review-<date>.md` | all landed | Not started |

Batches: see "Decisions resolved 2026-10-04" above (R2 dropped, R9 paused). R1 and R3 both touch `gist-core`/`gist-ffi`, and R5/R6/R1 all touch Swift `Shared/` — integrate one at a time and re-run the gate.

### R1 — Reading-state model and the missing sort keys

- **Verify first** what exists: `reading_progress` columns, whether RSVP and flow both record "last opened", what `Metadata` exposes for source type, and how `list_items` orders. Report before designing.
- Write an ADR (next free number) deciding the reading-state model **per the resolved D2**: add **`last_opened_at`** (nullable timestamp) to `library_items`, set by *both* readers whenever an item is opened (RSVP and flow); **no `progress_fraction` column** — "progress" is *derived* from the existing RSVP `reading_progress` position (token index ÷ the item's total tokens; verify where the total is cheaply available — `tokens` table count, stored metadata, or the token blob — and avoid loading a whole token blob per row). State the accepted limitation in the ADR, the UI copy and `CHANGELOG.md`: an item read only in the flow view has no RSVP position, so its *progress* reads 0% / sorts as unstarted while its *last read* is correct (a shared progress column would be a later schema v7). Schema **v6** migration in the existing transactional style (version ceiling `SchemaTooNew`; never-opened = NULL), tests incl. the old-DB-compat harness in `crates/gist-store/tests/old_db_compat.rs`.
- Expose through `gist-core`/`gist-ffi` (`ffi_catch!`-wrapped); regenerate bindings; wrap in `CoreClient`; keep RSVP's `saveProgress` and flow's `FlowScrollPositionStore` semantics intact (they must additionally update the shared fields — do not merge the two position meanings).
- Add the missing `LibrarySortOrder` cases (source type, last read newest/oldest, and progress high/low derived from the RSVP position per D2) via `LibraryFiltering.sorted` (pure, unit-tested), the Sort menus in `LibraryView` and `CollectionDetailView`, and a small progress indicator on library rows. Items never opened sort last for "last read". Strings go through the catalog (see R6's conventions).
- Tests: store migration + round trip, both readers update the shared fields, each new sort order incl. ties and never-opened items. Update `docs/qa-manual-clickthrough-m2.md` with the new sort checks (not claimed as run).

### R2 — Real-PDF corpus and layout tuning — DROPPED (D6, 2026-10-04)

**Not part of M7.** The user decided not to add real third-party PDFs as fixtures. PDF layout quality therefore remains measured only on synthetic fixtures; `CHANGELOG.md` and the release checklist must keep saying so. The text below is retained as the design to revisit if early users report bad extraction.

- Per D6 add **8–12** real documents under `fixtures/real/pdf/` (or a hash-fetched set if large), covering: single column book, two-column paper, footnotes, headers/footers/page numbers, a table-heavy page, a scanned (image-only) page, a long multi-chapter document, a PDF with ligatures/hyphenation, and at least one with rotated or RTL content to *characterise* the known gaps. Each with source URL, licence, retrieval date, SHA-256 in `fixtures/README.md`.
- Build a **repeatable quality harness** (a `cargo test`/example that prints, per document: page count, section/heading count, characters extracted, header/footer lines wrongly kept, obvious column interleaving) with *recorded baselines* — numbers a regression would move. Do not invent a subjective score; assert on specific, checkable properties (e.g. "page numbers do not appear in body text", "two-column text is in reading order for fixture X").
- Tune `layout.rs` only where a recorded property fails; each fix lands with a regression test on a minimal synthetic case. Preserve every hostile-input guarantee (`ParseLimits` order, no `unwrap` on external data, `forbid(unsafe_code)`); re-run `fuzz_parse_pdf` ≥300 s after any layout change.
- Report honestly what still fails (rotated, RTL, footnote placement, tables-in-PDF) and put it in `CHANGELOG.md`'s known limitations — do not claim general PDF quality from 10 documents.

### R3 — Typed resource-limit error and PDF text budget

- Add a typed `GistError` variant for `ResourceLimitExceeded` (with a coarse reason, no paths), map it from `ImportError`, regenerate bindings, and give `CoreClient.importFile`/`importUrl` a specific, honest alert ("too many pages / too large for GIST's limits") instead of the generic error. All matching on the typed case, never strings. Check every other importer benefits (epub/docx/txt/OCR).
- `F36`: introduce a PDF-specific total-text budget smaller than the global 512 MiB `max_expanded_bytes` (justify the number from a hostile-fixture memory measurement (R2 was dropped, so there is no real-document data)), release per-page buffers as soon as a page is laid out where the algorithm allows, and test with the page-count-bomb and huge-object fixtures that peak memory stays bounded (measure, e.g. via `/usr/bin/time -l` on a test binary, and record the numbers).

### R4 — `F33`: out-of-process pdfium (design + measured spike; no implementation, D3)

- ADR first: threat model (hostile PDF → native crash/RCE-class bug in C++ pdfium inside the app process vs a sandboxed helper), options (XPC service with its own minimal sandbox; separate `posix_spawn`ed helper; accept risk with mitigations), and costs (second signed bundle and `N8` interplay, IPC of up to 256 MiB inputs, latency, packaging in the DMG pipeline, tests).
- Spike on a throwaway branch: a minimal XPC service that runs `parse_pdf` and returns the `Document`; measure round-trip time and peak memory on the R2 corpus vs in-process; verify a deliberately crashing parse (inject a fault in the spike) does **not** take the app down. Report numbers.
- Per D3 this role **does not implement** out-of-process parsing: it delivers the ADR, the measured prototype (numbers on the existing synthetic/hostile fixtures only — D6 means no real-world corpus; say so next to every number) and a recommendation. Whether to build it is a separate user decision made from those numbers. Never claim signed-build behaviour (no credentials here).

### R5 — Deployment-target availability audit

- Build the app with `MACOSX_DEPLOYMENT_TARGET` set to 14 (and 15) on a throwaway branch and collect every compile error / availability warning; for each, state the API, the minimum OS it needs, and whether a `#available` fallback is reasonable. Also check Swift/linker settings the xcframework build assumes (the Rust staticlib's `-mmacosx-version-min`, `MACOSX_DEPLOYMENT_TARGET` in `tools/build-core-xcframework.sh`, pdfium's own minimum OS from its dylib's `LC_BUILD_VERSION`/`minos`).
- You can compile-check lower targets here but **cannot run** on an old OS: report "compiles" separately from "verified on macOS N". **Stop after the audit report.** Applying D1 (lower the target with fallbacks, or ratify 26.5 and correct Q9, the README and `docs/BUILDING-macos.md`) is a second step the Team Leader starts only after the user has seen the evidence and decided. Update `entitlements`/`Info.plist` `LSMinimumSystemVersion` accordingly and check `apple-build.yml`/`release-macos.yml` runner assumptions.

### R6 — Localisation completion

- Audit every user-visible string added since R5b (M6 PDF/table/OCR-from-PDF UI, library sort labels from R1 once landed): find those not in `Localizable.xcstrings`, and interpolated strings that bypass the catalog. Add entries (en-GB source; no other languages exist), convert string-building call sites to `String(localized:)`/`LocalizedStringResource` so the extractor sees them, and add a small test that fails when a user-facing `Text(...)`/`String(localized:)` key is missing from the catalog if that can be done reliably; otherwise document the manual audit method. Do not machine-translate.

### R7 — Merged-cell tables (v1.1, included by D4)

- R3 (M6) documented that `vMerge` continuation cells become empty cells and `gridSpan` yields fewer cells. Decide the model (`colspan`/`rowspan` per cell vs repeating text), whether it needs an `ir_version` bump per the ADR-019 addendum policy (show the old-binary evidence again), update parsers, the Rust/Swift flattening (the tab/newline golden must still hold), the flow-view grid and VoiceOver labels, and add `PENDING_WINDOWS_CHANGES.md` notes.

### R8 — Paginated view (v1.1, Q3, included by D4)

- Short design note first (pagination strategy; how selection/search/annotations/TTS/progress map onto pages; the `FlowScrollPositionStore` position-meaning rule — a page index is a third meaning, do not conflate). Second `ReadingLayout` conformer sharing typography/theme/progress/search objects; Q8 (SwiftUI-native) already weighed text-selection limits, do not relitigate.

### R9 — iOS build-system bring-up — PAUSED (D5, 2026-10-04)

**Not part of M7.** iOS work is paused; no simulator runtime install and no Q5 (`URLSession` vs `gist-web` resolver) decision is needed until it resumes. The text below is retained for when it does.

- iOS app target in `project.yml` reusing `apps/apple/Shared/`; link `GistCore.xcframework`'s iOS slices (device + simulator), preserving the staticlib-by-explicit-path rule (`N8`); platform shims for AppKit-only code; iOS test bundle running the `CoreClient` tests on a simulator; `apple-build.yml` iOS job (SHA-pinned, `permissions: contents: read`). **Pdfium has no iOS binary in this plan** — decide (and document) that PDF import is macOS-only until an iOS pdfium story exists, rather than silently shipping a target that cannot import PDFs. **Gate:** if no simulator runtime can be installed, stop and report; claim no iOS verification. The iOS *shell* (navigation, share extension, camera, BackgroundTasks, App Store compliance) is a separate milestone.

### R-final — Independent review and hygiene

- Per the permanent protocol step: a fresh agent reviews the integrated diff against source and re-checks the `CLAUDE.md` claims for what M7 touched, in the style of `docs/security-quality-review-2026-10-04.md`. Specific attention this milestone: the schema v6 migration (data-loss/forward-compat), any XPC/IPC trust boundary if R4 implements, the typed-error mapping (no path/payload leaks), PDF fuzzing after layout changes, and deployment-target fallbacks. Hygiene: refresh `docs/THIRD-PARTY.md` from `Cargo.lock`, record the 5 known duplicate crates (`F29`), and note that `F27` (Windows review) is still outstanding unless a Windows session has run it.

---

## 3. Explicitly out of scope for M7 agents

- Signing/notarization, `N8`'s signed-Release launch check (now including the embedded `libpdfium.dylib`), tagging v1.0, the public beta — human/credential-gated, unchanged (`docs/v1.0-release-checklist.md`).
- Manual QA passes (`qa-manual-clickthrough-m2/-m3/-pdf.md`), VoiceOver/speech/Dynamic Type on real hardware, Vision OCR on real scans.
- Windows W4–W6 and the `F27` Windows review (needs Windows to build/test) — except the `PENDING_WINDOWS_CHANGES.md` notes in §1 item 7.
- The iOS shell (R10 in a later plan), cross-device sync, Android/Web, social features (spec non-goals / "Future, not committed").

## 4. Adopting this plan

When approved: (a) resolve D1–D7; (b) repoint `CLAUDE.md`'s "proceed with development" convention and `.claude/commands/proceed-with-development.md` from `docs/m6-agent-roles.md` to this file, dated, noting that M6's backlog is exhausted; (c) add an M7 row to the milestone register; (d) update `development-plan-v2.md` §5 with M7 and **correct Q9** according to D1; (e) create `integration/m7-<date>` off `main` and commit the adoption there before spawning. Until then nothing here is binding.
