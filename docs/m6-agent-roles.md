# M6 Execution Plan — Agent Roles & Team Structure

**Status:** ADOPTED 2026-10-03 (D1–D5 resolved below). `CLAUDE.md`'s "proceed with development" pointer and `.claude/commands/proceed-with-development.md` now target this file. Scope authority remains `docs/development-plan-v2.md` and `docs/product-spec-reader-app-v3.md` §11 (phasing); this file refines them into assignable work, modelled on `docs/m5-agent-roles.md`.

**Platform scope:** macOS session (`apple(macOS/iOS)=yes`). Windows work (W4–W6) is governed by `docs/windows-development-plan.md` and cannot be built from here; it is not part of this plan except where noted.

## Why this plan looks different from M3–M5

M5's agent-executable backlog is exhausted; what's left of v1.0 is human/credential-gated (`docs/v1.0-release-checklist.md`). The "next" work therefore isn't a continuation — it comes from gaps found while scoping this plan, checked against current source on 2026-10-03:

1. **PDF import is in the v1.0 scope and doesn't exist.** `docs/product-spec-reader-app-v3.md` §11 lists PDF as a v1.0 (macOS) format, ADR-002 is Accepted, but `crates/gist-parse-pdf` has an empty `[dependencies]` table and a 1-line `lib.rs`. Nothing in M1–M5 assigned it; `CLAUDE.md` says only "pdfium build tooling deferred." Either it ships before v1.0 or v1.0's scope is knowingly cut. **That is a product decision (D1 below), not something this plan pre-decides.**
2. **Tables are documented as done but aren't in the IR.** The plan's Q2 line says "Tables parse+persist in model," but `gist_model::Block` has exactly four variants — `Heading`, `Paragraph`, `Image`, `List` — and no table. Whatever the DOCX parser does with tables, it isn't preserving them structurally. The v1.1 "full table rendering" item therefore starts with a model change (and an ADR-019 IR-version consideration), not just a Swift view. Verify the DOCX behaviour first (R3 step 1) before assuming the extent of the gap.
3. **There is no iOS target.** `apps/apple/project.yml` has no iOS target (`apps/apple/iOS/` is an empty directory), although `build-core-xcframework.sh` already emits iOS slices and the dev plan calls iOS "M6" (~8–10 weeks, spec: ~35–45% of the macOS shell effort). The iOS SDK is installed here; `xcrun simctl list runtimes` returned **no simulator runtimes**, so an iOS simulator may need installing (Xcode → Settings → Components) before any iOS role can run tests.
4. **Review debt (`F27`/`F29`/`F30`).** No Windows security review since the pre-W1 gate, `deny.toml` hygiene, and no standing review cadence after M3–M5 landed.

## Decisions needed from the user before spawning (not pre-decided here)

| # | Decision | Recommendation |
|---|---|---|
| D1 | Is PDF a blocker for v1.0, or is v1.0 cut to txt/epub/docx/OCR/URL and PDF moves to v1.1? | Ship v1.0 first if credentials/beta are ready — the app is useful without PDF — and run Track A in parallel; but note the spec's v1.0 phasing lists PDF, so the cut should be recorded in the spec/plan, not made silently. |
| D2 | Which tracks run, in what order? | A (PDF) → B (tables, paginated) in parallel with C (iOS bring-up); E (reviews) last. |
| D3 | Q3 (paginated view) — still v1.1? | Yes; it is the lowest-value item and the `ReadingLayout` protocol keeps it cheap later. |
| D4 | pdfium distribution: which prebuilt binaries, who vouches for them? | Pin a specific `bblanchon/pdfium-binaries` release by SHA-256 in a build script; record in an ADR-002 addendum and `docs/THIRD-PARTY.md`. Don't compile pdfium from source. |
| D5 | iOS: install a simulator runtime and build in this environment, or wait for a device/CI setup? | Install the runtime; R6 can't verify anything without it. |

### Decisions resolved 2026-10-03 (user)

| # | Outcome |
|---|---|
| D1 | **PDF stays in v1.0.** Track A gates v1.0 scope as the spec's §11 phasing is written. |
| D2 | **Tracks A (R1, R2) and B-tables (R3) run. Track C (iOS: R5, R6) and R4 (paginated view) do NOT run this milestone.** R7 runs last over what landed. |
| D3 | Q3 paginated view remains v1.1 (R4 not started). |
| D4 | **Pinned prebuilt pdfium, SHA-256 verified, fail closed** (no source build). Licence check done 2026-10-03: pdfium is BSD-3-Clause, the `pdfium-render` crate is MIT OR Apache-2.0, and the third-party code bundled in the binary (Apache-2.0, MIT, zlib, BSD, IJG, Unicode/ICU, FreeType Project License) is all permissive and MIT-compatible; the obligation is notice reproduction (`docs/THIRD-PARTY.md` + in-app licence screen, including FreeType's acknowledgement line). **Linking: the macOS prebuilt archive ships only `libpdfium.dylib` (install name `./libpdfium.dylib`, no static `.a`), so it is embedded in the app bundle (`Frameworks/`) and signed with the same Developer ID in the same pass as the app (`N8`).** The signed half cannot be verified without credentials — flag it open, do not claim it. |
| D5 | **Skip iOS** this milestone; no iOS verification is claimed. |

Batches for this run: **Batch 1:** R1, R3. **Batch 2:** R2 (needs R1). **Batch 3:** R7.

---

## 0. Team Leader role

Same protocol as `docs/m5-agent-roles.md` §0:

1. **Preflight.** Check `PENDING_APPLE_CHANGES.md`/`PENDING_WINDOWS_CHANGES.md`; confirm `git status` clean; confirm `main` is current (`git fetch`).
2. **Compute the ready batch** from §2.
3. **Spawn the batch in parallel**, one `Agent` call per ready role, `isolation: "worktree"`, self-contained prompts.
4. **Integrate one at a time** onto a fresh `integration/m6-<date>` branch off **`main`** (unlike M4/M5, `main` is now the only long-lived branch and already holds all prior work — see `CLAUDE.md`'s M5 row, 2026-10-01 update).
5. **Advance** to the next batch when dependencies are satisfied.
6. **Full verification on the integrated tree:** `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources`, `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`, `CODE_SIGNING_ALLOWED=NO`, skip `KeychainKeyProviderIntegrationTests` headless). For iOS roles also build/test the iOS scheme on a simulator.
7. **Update the record:** `CLAUDE.md` (milestone register, security register, open questions), `docs/development-plan-v2.md`, `CHANGELOG.md`, this file's §2 status column.
8. **Run a security/quality review of the integrated change set** (**permanent standing step — `F30` adopted by the user 2026-10-04**; the full text lives in `.claude/commands/proceed-with-development.md` step 7; see R7 for the M6 instance) before reporting.
9. **Report, then stop before publishing.** Branch → PR, never push `main` directly; wait for CI (auto-merge is disabled on this repo, merge manually after checks pass).

---

## 1. Already done — do not re-assign

- Everything in M0–M5's agent-executable backlogs, including OCR (`import_image_with_ocr`, Vision engine, review screen), annotations + re-anchoring, TTS, 6-tab Settings, localisation catalog, integrity/encryption (ADR-011/013/014), IR versioning (ADR-019), unsigned release pipeline.
- `gist-imageprep` and the OCR callback interface (ADR-009) — **reuse them** for image-only PDFs; don't build a second OCR path.
- `ReadingLayout` protocol (Swift, flow-view abstraction) — exists specifically so paginated can conform later.
- `build-core-xcframework.sh` already produces macOS + iOS slices.
- All six Dependabot PRs from 2026-10-03 merged; `cargo test`/`clippy`/`fmt` and 183 Xcode tests green on `main`.

---

## 2. Backlog → roles

| # | Track | Role | Scope | Depends on | Status |
|---|---|---|---|---|---|
| R1 | A: PDF | Rust — `gist-parse-pdf` on pdfium behind a `PdfParser` trait | `crates/gist-parse-pdf/`, `gist-core` import wiring, `deny.toml`, `docs/THIRD-PARTY.md`, ADR-002 addendum, fuzz target | D1, D4 | Done 2026-10-04 |
| R2 | A: PDF | Swift — PDF import UI + image-only-PDF → OCR routing | `apps/apple/Shared/*`, `project.yml` UTType/Info.plist | R1 | Done 2026-10-04 |
| R3 | B: v1.1 | Rust+Swift — table model + flow-view rendering | `gist-model`, DOCX/epub/web parsers, `gist-store` IR version, `FlowDocumentModel.swift`, `FlowViewSwiftUINative.swift` | — | Done 2026-10-03 (merged 2026-10-04) |
| R4 | B: v1.1 | Swift — paginated reading view (Q3) as a second `ReadingLayout` | `apps/apple/macOS/` | D3 | Not run this milestone (D3: stays v1.1) |
| R5 | C: iOS | Build system — iOS target, shared-code compile, simulator test target | `project.yml`, `apps/apple/iOS/`, CI | D5 | Not run this milestone (D5: iOS skipped) |
| R6 | C: iOS | iOS shell — navigation, library, RSVP/flow on touch, share extension | `apps/apple/iOS/`, shared views | R5 | Not run this milestone (D5: iOS skipped) |
| R7 | E: hygiene | Review — security/quality pass over M6 changes + `F29` prune | `docs/security-quality-review-<date>.md`, `deny.toml` | R1–R6 as landed | Done 2026-10-04 (see docs/security-quality-review-2026-10-04.md) |

Batches: **Batch 1:** R1, R3, R5 (independent). **Batch 2:** R2 (needs R1), R6 (needs R5). **Batch 3:** R4 (optional), R7. R3 touches `gist-model`, which R1 does not, but both may touch `gist-core` — integrate one at a time and re-run the full gate between them.

### R1 — Rust: PDF parser (Track A)

- Read ADR-002, spec §3.1, `ParseLimits` and an existing parser (`gist-parse-epub`) for conventions. Confirm `pdfium-render`'s current release, licence (BSD-3 for pdfium; check the crate's own), and that `cargo deny check bans licenses sources` stays green — add allow-list entries only for licences actually required, noting each in `deny.toml`.
- **Binary distribution (D4):** do not vendor a binary in git. A build script (extending `tools/build-core-xcframework.sh` or a sibling) downloads a pinned pdfium release per arch (arm64, x86_64), verifies a pinned SHA-256, and fails closed on mismatch. Document in ADR-002 addendum + `docs/THIRD-PARTY.md`. Decide static vs dynamic linking with `N8` in mind: **a separately-loaded `libpdfium.dylib` re-opens the signed-Release library-validation risk** (different Team ID); prefer static linking or embed-and-sign it in the same pass as the app, and say which in the ADR.
- `PdfParser` trait + pdfium implementation; `parse_pdf(bytes, &ParseLimits) -> Result<Document, ParseError>`: reading-order text extraction, heading inference from font size, paragraph breaks, header/footer/page-number stripping (margin-band repetition heuristic from ADR-002), encrypted/password PDFs rejected with a typed error (never attempt to bypass — same posture as ADR-004's DRM rule).
- Enforce `max_bytes`, `max_pages` **before** loading pages, `max_expanded_bytes` on extracted text, `max_nesting_depth` where relevant; check-before-allocate ordering (`F15`/`F16`/`N9`). Hostile-input tests: truncated PDF, page-count bomb, huge-object PDF, encrypted PDF, zero-text (image-only) PDF → typed `NoTextLayer`-style signal that R2 routes to OCR.
- Wire `Core::import_file` for `.pdf` (copy-on-import per ADR-006; checksum per ADR-013). No `unwrap()` on external data; `ffi_catch!` is already applied at the export layer — check any new export.
- Add `fuzz/fuzz_targets/fuzz_parse_pdf.rs` and run it ≥300s (same method as `N1`'s 2026-09-29 pass). A crash here is a real finding — fix, don't skip.
- Fixtures: add 3–5 small, synthetic or public-domain PDFs (text, two-column, image-only, encrypted, malformed) with provenance in `fixtures/README.md`; extend `crates/gist-core/tests/corpus.rs`.
- **Does not need Apple tooling**, but verify the macOS build links end-to-end (xcframework) — a pdfium link failure only shows there (this is exactly how `N6`'s bzip2/lzma issue hid).

### R2 — Swift: PDF import UI + OCR fallback

- Add PDF to the import file-picker's allowed types and the app's document-type declarations; DRM/encrypted/limit errors surface via the existing import error alerts (mirror the DRM alert's structure, don't string-match).
- Image-only PDF: render pages with `PDFKit` to images and feed the existing `import_image_with_ocr` + OCR review screen; confirm the page cap (`N9`) and the 256 MiB per-input cap still apply to the rendered images, and that rendering is bounded in memory (render+release page by page).
- Tests against a real temp-dir `GistCore` (repo convention): text-PDF import appears in the library and opens in RSVP/Flow; image-only PDF routes to OCR review; encrypted PDF shows the typed error. Extend `docs/qa-manual-clickthrough-m3.md` (or add a short PDF section) for the manual part.

### R3 — Rust+Swift: tables (v1.1)

- **Step 1, before any design:** run the DOCX, epub and web parsers on fixtures containing tables and record what happens today (flattened? dropped? cell text concatenated?). Correct the stale "tables parse+persist" claim in `docs/development-plan-v2.md` §7 if it's wrong.
- Add `Block::Table { rows: Vec<Vec<…>> }` (decide cell content: plain text vs runs), keeping `gist-model` wasm32-clean. Per ADR-019, an **additive** variant needs no version bump, but an *older binary reading a newer blob with an unknown enum variant will fail to deserialize* — test exactly that and decide (in an ADR-019 addendum) whether this variant warrants an `ir_version` bump. This is the first real exercise of the forward-compat policy; don't assume it.
- Update every `match` over `Block` (RSVP token generation, `plain_text`, FTS indexing, annotation anchoring's `section_text`, Swift `FlowBlockVM` decoding). RSVP and TTS should linearise tables sensibly (row by row) — decide and document. Keep the R1-era fix in mind: Rust `section_text` and Swift `concatenatedPlainText` must use the same separators (ADR-003 addendum) or annotations in/after tables will misanchor.
- Swift: render tables in the flow view (grid, theme-aware, accessible — VoiceOver should read rows/columns, not a flat blob). Add tests including a table inside a section that also has annotations.

### R4 — Swift: paginated view (Q3, optional)

- Second `ReadingLayout` conformer; page-turn navigation, shares typography/theme/progress/search objects with the flow view. Decide pagination strategy (measure text into fixed-size pages vs `NSTextView` column layout) in a short design note first; Q8 (SwiftUI-native) already weighed text-selection limits — don't relitigate. Progress persistence must not conflate meanings (see the `FlowScrollPositionStore` rationale). Only start if D3 confirms it's wanted for the first v1.1 cut.

### R5 — iOS build system

- Add an iOS app target to `project.yml` reusing `apps/apple/Shared/`; link `GistCore.xcframework`'s iOS slices (device + simulator); confirm `build-core-xcframework.sh` really produces both and the staticlib-by-explicit-path rule (`N8`) is preserved.
- Fix whatever in `Shared/` is AppKit-only (e.g. `NSApp`, `NSColor`, `NSOpenPanel`) behind `#if os(macOS)` / platform shims — keep the macOS build and its 183 tests green after every step. Add an iOS test bundle that runs the same `CoreClient` tests against a real temp-dir core on the simulator.
- Entitlements/Info.plist for iOS (no network beyond URL import; file access via document picker); Keychain provider works on iOS (`KeychainKeyProvider`). Extend `apple-build.yml` with an iOS-simulator build job; keep actions SHA-pinned and `permissions: contents: read`.
- **Gate:** if no simulator runtime can be installed (D5), stop and report; do not claim iOS verification.

### R6 — iOS shell

- Navigation (`NavigationSplitView`/stack adapted to compact width), library list with search/sort/filter/collections/tags, import via document picker and URL, RSVP view with touch controls (rotary dial as drag gesture + accessible stepper per spec §5.1.1), flow view, annotations (long-press selection), settings.
- iOS-specific per spec §8.5/§11: **share extension** (import from Share sheet — must pass untrusted input through the same `ParseLimits`/copy-on-import path; extension memory limits are tight, so hand off to the main app rather than parsing in the extension), camera capture → OCR review, `BackgroundTasks` for long imports, ATS-compliant URL fetch (Q5: Swift `URLSession` per plan; if chosen, SSRF protection `F14` must be re-implemented or the fetch routed through `gist-web`'s safe resolver — **decide explicitly, since `URLSession` bypasses `safe_resolve`**).
- App Store compliance notes (privacy manifest, `docs/PRIVACY.md` consistency). Dynamic Type must be real on iOS (the macOS pass could only approximate it); VoiceOver labels reuse the M3 audit.
- Tests on simulator; a manual device checklist `docs/qa-manual-clickthrough-ios.md` is written but not claimed as run.

### R7 — Review and hygiene

- Independent security/quality review of everything M6 landed, in the style of `docs/security-quality-review-2026-09-29.md`: pdfium as a new native dependency (supply chain, memory-safety posture, linking/signing interplay with `N8`), the table variant's forward-compat behaviour, the iOS share-extension/URL-fetch trust boundary, new `ffi_catch!`/`unwrap` coverage, fuzz results. Add register rows `F31+`.
- `F29`: prune `deny.toml`'s unused `BSD-2-Clause` entry (confirm it's still unused after pdfium) and note remaining duplicate crates.
- `F27`: if a Windows session is available, schedule the Windows review there; on macOS only record that it's still outstanding.
- Record `F30` outcome: **decided 2026-10-04 — adopted permanently** (user decision, after this review found two real defects and a false claim that every role's own tests had passed). It is now step 7 of `.claude/commands/proceed-with-development.md`, run by a fresh agent that built none of the code.

---

## 3. Explicitly out of scope for M6 agents

- Signing/notarization, `N8`'s signed-Release launch check, the public beta, tagging v1.0 — human/credential-gated, unchanged (`docs/v1.0-release-checklist.md`).
- Manual QA passes (`qa-manual-clickthrough-m2.md`/`-m3.md`), VoiceOver/speech/Dynamic Type verification on real hardware.
- Windows W4–W6 (needs Windows to build/test).
- Cross-device sync, Android/Web, social features (spec non-goals / "Future, not committed").

## 4. Adopting this plan

When approved: (a) resolve D1–D5; (b) repoint `CLAUDE.md`'s "proceed with development" convention and the `proceed-with-development` skill from `docs/m5-agent-roles.md` to this file, noting the date and that M5's remaining items are human-gated; (c) add an M6 row to the milestone register; (d) update `development-plan-v2.md` §5 with M6 and fix its Q2 table claim (R3 step 1 will confirm it). Until then nothing here is binding.
