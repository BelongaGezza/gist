# M8 Execution Plan — Agent Roles & Team Structure

**Status:** DRAFT 2026-10-11 — not yet adopted. Nothing below is repointed: `CLAUDE.md`'s "proceed with development" pointer and `.claude/commands/proceed-with-development.md` still target `docs/m7-agent-roles.md` until the user adopts this file and resolves D1–D6. Scope authority is `docs/product-spec-reader-app-v3.md` §12 (`SC-01`–`SC-24`) and `docs/development-plan-v2.md` §5 M8 (owner-confirmed 2026-10-09); this file refines them into assignable work, modelled on `docs/m7-agent-roles.md`.

**Platform scope:** macOS session (`apple(macOS/iOS)=yes`). Windows shell work cannot be built here; where a role changes something Windows decodes, it adds a `PENDING_WINDOWS_CHANGES.md` entry rather than editing `apps/windows/**` blind.

## Why M8 looks like this

M7 closed the agent-executable v1.0 backlog the earlier plans knew about. M8 is the spec-conformance sweep: requirements the spec states but no milestone built. Only the items marked **v1.0** in spec §12 are in this plan's agent-executable core; the rest is deferred or human-gated:

| Group | Items | In this plan? |
|---|---|---|
| A. Import fidelity | `SC-01`, `SC-03`, `SC-06` | **Yes** — R1, R2 (Rust), R4 (Swift) |
| B. Library completeness | `SC-12`, `SC-13`, `SC-14`, `SC-15` | **Yes** — R3 (Rust), R4–R7 (Swift) |
| C. Reading controls | `SC-17`, `SC-19`, `SC-20` | **Yes** — R8, R9, R10 |
| D. Verification gates | `SC-21` | **No — human only** (`docs/qa-manual-clickthrough-*.md`, `docs/v1.0-release-checklist.md`) |
| E. Deferred to v1.1 | `SC-02`, `SC-04`, `SC-05`, `SC-07`, `SC-08`, `SC-11`, `SC-16`, `SC-18` | **No** unless D5 says otherwise |
| F. Platform follow-ons | `SC-10`, `SC-22`, `SC-23`, `SC-24` | **No** — blocked on iOS shell / Windows scope |

`SC-09` is closed (dropped from the spec 2026-10-09). **Exit criterion** (from the dev plan): every `SC-nn` is either implemented and tested, or the spec text is amended to say it is out of scope with the decision dated in spec §10. A role closes its spec §12 row only when the item is implemented *and tested*; otherwise the row stays and gets a dated status note.

## Decisions needed before adoption

| # | Decision | Recommendation |
|---|---|---|
| D1 | **Cover generation (`SC-12`):** (a) typographic placeholder covers rendered in Swift from title/author/theme, (b) extract the real cover from ePub/PDF first page, (c) both. | (a) for v1.0. Extracting embedded images is `SC-04` (v1.1) and drags in decoder limits; a PDF first-page render needs pdfium on the UI path. Populate `library_items.cover_path` from Swift-rendered PNGs in `<storage>/covers/`, deletable and regenerable (never authoritative data). |
| D2 | **Curated reading fonts (`SC-17`):** which fonts ship? | Prefer system fonts only (SF, New York, SF Rounded, plus a dyslexia-friendly option such as OpenDyslexic **only if** its OFL licence and file are vetted into `docs/THIRD-PARTY.md` and the app bundle). Bundling any font file needs the licence review `R4` in the risk table demands; do not add one without it. |
| D3 | **Position hand-off (`SC-19`):** carry a *fraction* RSVP → flow only (v1.0), or also flow → RSVP and paginated? | RSVP → flow and flow → RSVP via a fraction (token index ÷ total, block-fraction ↔ token-fraction), v1.0. A shared progress column (schema v8) stays v1.1, per the dev plan. Keep the three position stores' meanings separate (ADR-021/023): the hand-off is a one-shot translation at mode switch, not a merge. |
| D4 | **Smart collections (`SC-13`):** computed on read or persisted? | Computed on read from `last_opened_at` + RSVP progress + `source_type` (no new schema, per the dev plan). Unread / In progress / Finished / Web articles, with a documented progress threshold for "Finished" (suggest ≥ 98%). Flow-only items show 0% (ADR-021's accepted limitation) — state this in UI copy. |
| D5 | **Pull any Group E item into M8?** | No. Re-run this decision when Group A–C land. If one is pulled in, the first candidate is `SC-18` (configurable progress display): small, Swift-only, reuses the `ReadingProgress` object. |
| D6 | **Fold in open Low/Info review findings** (`F39`–`F78`)? | Only those that touch files a role already edits; otherwise leave them in the register. Do not widen scope to chase them. |

## Roles

| # | Track | Role | Scope | Depends on | Status |
|---|---|---|---|---|---|
| R1 | A | Rust — TXT encoding detection (`SC-01`) | `gist-parse-txt` (`encoding_rs`/`chardetng`), detected-encoding + uncertainty surfaced via `gist-core`/`gist-ffi`, explicit-override entry point, fixtures, tests | — | **Done 2026-10-11** (Rust; merged on `integration/m8-2026-10-11`) |
| R2 | A | Rust — import metadata (`SC-03`, `SC-06`) | Real `Metadata` field for DOCX tracked changes (replace the `source_type = "docx:tracked-changes"` hack); web byline / publish date / retrieved date / canonical URL; populate `import_date` and `language`. Additive `serde(default)` fields, ADR-019 compatibility tests, no schema columns | — | **Done 2026-10-11** (Rust; merged on `integration/m8-2026-10-11`) |
| R3 | B | Rust — word count and reading time (`SC-15`) | `word_count` on `FfiLibraryItem`, reading-time helper, schema v8 if a stored column is warranted (ADR, migration tests from v7), counting consistent with the RSVP tokenizer | — | **Done 2026-10-11** (Rust; merged on `integration/m8-2026-10-11`) |
| R4 | A+B | Swift — surface R1–R3 in the UI | Encoding-prompt alert + re-import with override (`SC-01`); "unresolved tracked changes" notice (`SC-03`); byline/dates shown for web items (`SC-06`); word count and "≈ N min" in rows (`SC-15`); string-catalog entries; `CoreClient` wrappers; tests | R1, R2, R3 merged | **Done 2026-10-11** (macOS; Windows pending) |
| R5 | B | Swift — persisted, per-view sort with key + direction (`SC-14`) | Replace the combined-choice menu with key selector + direction toggle in `LibraryView`/`CollectionDetailView`; persist per view in `UserDefaults`; pure `LibraryFiltering` logic, tests | R4 (shared `LibraryView`) | Not started |
| R6 | B | Swift — smart collections (`SC-13`) | Sidebar section: Unread / In progress / Finished / Web articles, per D4; computed from existing fields; tests for each bucket incl. never-opened and flow-only items | R5 | Not started |
| R7 | B | Swift — grid view and covers (`SC-12`) | List/grid toggle, `LazyVGrid`, cover generation per D1, `cover_path` populated and cleaned on removal, progress ring on grid cells, VoiceOver labels | D1, R6 | Not started |
| R8 | C | Swift — reading controls (`SC-17`) | Curated fonts (D2), paragraph spacing, text width, justification, hyphenation in `TypographySettings` / flow view / paginated view; Settings → Typography defaults; tests for the pure settings logic | D2 | Not started |
| R9 | C | Swift — position hand-off (`SC-19`) | `exitToFlowView()` carries the RSVP position; flow → RSVP; per D3; tests for fraction translation at edges (0, 1, empty doc, table-heavy doc) | D3 | Not started |
| R10 | C | Measurement — frame timing 200–700 WPM (`SC-20`) | Instrumented macOS run over the decided band, recorded numbers and method in `docs/` (honest about the wall-clock engine ≠ `CVDisplayLink`); no behaviour change unless a measured problem is found | — | Not started |
| R-final | review | Independent security/quality review (fresh agent, `F30`) | `docs/security-quality-review-<date>.md`; all new parser/decoder code fuzzed; re-check every `CLAUDE.md`/spec §12 claim touched | all landed | **Done 2026-10-11** for the R1–R4 slice (`docs/security-quality-review-2026-10-11.md`; F79–F85) |

R1–R3 were started ahead of formal adoption because they are Rust-only, verifiable here, and independent. Their briefs are the scope column above; if the user changes D-decisions affecting them, re-brief via `SendMessage`.

### Batches and conflict map

- **Batch 1 (in flight):** R1, R2, R3, in parallel worktrees. Overlap is `gist-core` and `gist-ffi` only (R3 alone owns `gist-store` schema; R2 alone owns `Metadata`). Integrate sequentially, re-running the full Rust gate between merges.
- **Batch 2:** R4 (needs generated bindings from the merged Rust), alongside R10 (independent) and R8 (if D2 is settled; touches typography files, not `LibraryView`).
- **Batch 3:** R5 → R6 → R7 serially: all three edit `LibraryView.swift`/`SidebarView.swift` (a file that has previously hit Swift type-checker timeouts — keep `body` split into small properties/helpers; see `N6`). R9 may run in parallel with these (reader files only).
- **Batch 4:** R-final.

---

## 0. Team Leader role

Follow `.claude/commands/proceed-with-development.md` (once repointed to this file); its permanent protocol, including **step 7 (independent review)**, applies unchanged. Specifics for M8:

1. **Preflight:** `PENDING_APPLE_CHANGES.md` / `PENDING_WINDOWS_CHANGES.md`; `git status` clean; `git fetch`; create `integration/m8-<date>` off `main` and commit the plan adoption there **before spawning** so every worktree contains it. (Batch 1 predates this: its worktrees branch from `main` without the plan; merge them onto the integration branch once it exists.)
2. **Worktree base check:** brief every agent to run `git log --oneline -1` first and `git reset --hard <integration tip>` if behind.
3. **Integrate one at a time**; resolve `gist-core`/`gist-ffi` conflicts by hand and re-run the gate.
4. **Full verification on the integrated tree:** `cargo test --workspace` (`./tools/fetch-pdfium.sh` and `GIST_REQUIRE_PDFIUM=1`), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources`, `cargo check -p gist-model --target wasm32-unknown-unknown`, `./tools/gen-bindings.sh`, `xcodegen generate` + `xcodebuild test` (scheme `GISTmacOS`, `CODE_SIGNING_ALLOWED=NO`, skip `KeychainKeyProviderIntegrationTests` headless), and `tools/check-localisation.sh`.
5. **Record update:** `CLAUDE.md` milestone + security registers, spec §12 rows (close only what is implemented and tested), dev plan §5 M8, `CHANGELOG.md`, `PLATFORM_VERIFICATION.md`, and this file's status column — dated in-place notes.
6. **Report, then stop before publishing** (branch → PR; CI; manual merge).

Operational lessons to apply: keep agent tool calls small and run long commands in the background; `grep` is aliased to `ugrep` here (scripts use `command grep`); macOS `head`/`sed` are BSD.

## Per-role notes

### R1 — TXT encoding detection (`SC-01`)
Honour BOM first, then valid-UTF-8 fast path (byte-identical behaviour for existing UTF-8 files), then `chardetng` guess with a confidence/uncertain flag, decoded with `encoding_rs`. `ParseLimits.max_bytes` is checked before decoding; decoded size is bounded too (UTF-16 → UTF-8 can grow). The caller can force an encoding (re-import path). FFI carries the detected encoding name and an `uncertain` bool; no panics, `ffi_catch!` throughout. UI prompt is R4.

### R2 — Import metadata (`SC-03`, `SC-06`)
Fields are additive `serde(default)` on `Metadata`, no `ir_version` bump (ADR-019), proven with old-blob-decodes and new-blob-in-old-shape tests; `gist-model` stays wasm32-clean. Web: byline, publish date, canonical URL from `<meta>`/JSON-LD-free simple extraction, `retrieved` stamped by the importer, all length-bounded; hostile/malformed dates tolerated. Windows must tolerate the new fields (`PENDING_WINDOWS_CHANGES.md`).

### R3 — Word count and reading time (`SC-15`)
Document the definition of "word" and keep it equal to what the RSVP tokenizer plays. If a column is stored (schema v8), the migration is transactional with a `SchemaTooNew` ceiling update, migration tests from v7, and a bounded backfill that does not load every blob at once. ADR records column-vs-compute.

### R4 — UI for Group A/B Rust work
Strings through the catalog; typed errors/flags only (no string matching); the encoding prompt must not lose the user's chosen file on cancel; tracked-changes notice is informational, not blocking.

### R5–R7 — Library
Pure logic (`LibraryFiltering`) stays unit-tested without SwiftUI state; sort persistence keys are per view and tolerate unknown stored values (fall back to default). Covers are derived artefacts: removal deletes them, a missing cover regenerates, no cover path is ever trusted from outside `<storage>/covers/`. Grid cells carry accessibility labels and the progress state is never colour-only (`SC-21` audit input).

### R8 — Reading controls
Bundled fonts are a supply-chain/licence change: licence text into `docs/THIRD-PARTY.md` and the in-app notices, file hash recorded, no network fetch at runtime. Text-width and spacing must apply identically in flow and paginated layouts (the `ReadingLayout` seam) and must not change annotation anchoring (anchors are content-based, not layout-based — add a test that asserts it).

### R9 — Position hand-off
One-shot translation at mode switch using fractions; never write one store's meaning into another (ADR-021/023). Clamp and handle empty/single-token documents.

### R10 — Frame timing
Report method, hardware and numbers; do not claim `CVDisplayLink` behaviour. Any fix is a separate, measured change.

### R-final
Fresh agent that built none of it. Must verify against source: `encoding_rs`/`chardetng` usage is check-before-allocate; no `unwrap` on external data in new code; every new `#[uniffi::export]` is `ffi_catch!`-wrapped; migration safety; bundled-font licence/supply chain if D2 added one; fuzz `fuzz_parse_txt` and `fuzz_web_extract` ≥ 120 s on the changed parsers; re-check spec §12 claims.
