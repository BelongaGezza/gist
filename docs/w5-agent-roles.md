# W5 Execution Plan — Agent Roles & Team Structure

**Status:** ADOPTED 2026-10-04 (Team Leader session, Windows 11 / ProArt13), on `integration/w4-2026-10-04`
(W5 builds on W4's merged work; the branch accumulates per this repo's pre-2026-10-01 convention until the
user decides to publish).
**Scope authority:** `docs/windows-development-plan.md` §5 W5; `docs/windows-ui-spec.md` §7.2, §8, §9, §10.
Modelled on `docs/w4-agent-roles.md`; same Team Leader protocol (§0 there), same platform rules.

**Platform scope:** Windows session. **No edits under `apps/apple/`, `ios/`, `macos/`, Xcode projects,
entitlements or `Info.plist`.** Any shared-Rust change must be flagged for Apple CI (`apple-build`).
`docs/windows-development-plan.md` W5 says `import_image_with_ocr` is `todo!()`; that is stale (implemented
in M3) — the Q4 note below must say so.

## 1. Already done — do not re-assign

- `GistCore::get_document_json` exists over FFI and `CoreClient.GetDocumentJsonAsync` wraps it already.
- The Rust `Document` IR: sections → blocks (`Heading`, `Paragraph`, `Image`, `List`, and since M6 `Table`
  with `rows`/`header_row`); text runs carry bold/italic/code flags. Apple's `FlowDocumentModel.swift` is the
  reference decoder; `section_text` separators are pinned by a shared golden test (Rust ↔ Swift): tab within
  table rows, newline between rows, `"\n\n"` between blocks — any C# plain-text/find logic that must agree
  with annotations must match that convention.
- W0–W4 in full (see W4 closeout in the Windows plan).

## 2. Backlog → roles

| # | Role | Scope | Depends on | Status |
|---|---|---|---|---|
| R1 | C# logic — document model + decoder, typography, find, TOC, progress/position store (UI-free, in `GIST.Core`) | `apps/windows/GIST.Core/Flow/`, `GIST.Core.Tests/Flow/` | — | Ready |
| R2 | WinUI — `FlowPage`: virtualised block list, per-block templates, typography menu, TOC flyout, find box, progress bar, keyboard, entry wiring | `apps/windows/GIST.App`, `GIST.App.UITests` | R1 | Blocked on R1 |
| R3 | Docs — Q4 design note (Windows OCR), "Rounded" font decision, W5 QA checklist section | `docs/`, `docs/qa-manual-clickthrough-windows.md` | — | Ready |
| R4 | Measurement — ≥100k-word fixture: scroll smoothness / UI-thread stall / memory, recorded | `GIST.Core.Tests`, harness, `docs/windows-development-plan.md` | R2 | Blocked on R2 |
| R5 | Review — F30 independent security/quality pass over the whole W5 diff | `docs/security-quality-review-<date>-w5.md` | R1–R4 | Blocked |

Batches: **1:** R1, R3. **2:** R2. **3:** R4. **4:** R5 (a fresh agent that wrote none of W5).

### R1 — C# logic
Custom System.Text.Json converter(s) for the serde-enum shapes, tested against **real** output from the
`fixtures/` corpus (txt/epub/docx/web, plus a table document) through a real `GistCore`, not hand-written
JSON only. Unknown block/run kinds must degrade (skip/placeholder), never throw on content. Decoder limits:
cap depth/size sanely (the JSON comes from the user's own store but a corrupt blob must fail typed, not OOM).
`TypographySettings` (size 13–28 default 17; font Default/Serif/Rounded as persisted enum; line spacing
+2/+6/+12), `SearchState` (case-insensitive, `StringInfo` text-element offsets per plan Q8, empty/no-match,
multi-byte/emoji/combining fixtures), `TocEntry.IndentLevel = max(level-1,0)` with headless sections
excluded, `ReadingProgress` (0…1, clamps, ignores non-finite), a file-backed per-item position store
(`FlowScrollPosition.<itemId>`, mirroring `FileRsvpSettingsStore`'s best-effort style), and a generic
`IReadingLayout`-style seam so a paginated view could conform later. Item ids used as file names must be
validated (no path traversal).

### R2 — WinUI FlowPage
Spec §7.2 in full: `ItemsRepeater`/`ListView` virtualisation, `RichTextBlock` runs (code spans Cascadia
Mono always), images with alt text as the Narrator name, list prefixes, table grid (accessible, theme-aware),
reading column ~70 chars, selection/Ctrl+C, "Aa" menu, Contents flyout, find with `TextHighlighter` + F3 /
Shift+F3 / Ctrl+G, bottom progress bar, restore-before-first-render, Home/End and native PgUp/PgDn.
Replace the "coming later" notice on "Open in Flow View" in Library and Collection. Theme through the
existing `GistBackground`/`GistForeground`/`GistAccent`/`GistSecondaryText` mechanism like `RsvpPage`.
Heed W4's review lessons: guard every FFI call (F50), coalesce property-change-driven redraws (F49), dispose
native handles in `finally`, never show raw exception text.

### R3 — Docs
Q4 as a design note only (no implementation): `Windows.Media.Ocr` vs Tesseract against ADR-009's callback
interface, ParseLimits caps, licensing, packaging, language packs. Decide the "Rounded" font (drop or
Trebuchet MS) keeping the persisted enum value. Add a Flow reader section to the Windows QA checklist
(`[automated: …]` / `[manual only]` tagging per the existing convention). Fix stale statements in the Windows
plan's W5 bullet about OCR.

### R4 — Measurement
Generate (deterministically, not committed if large) a ≥100k-word document, import it, open the flow page,
and record: scroll smoothness, longest UI-thread stall (target ≤ 50 ms), peak working set; report honestly
what an unattended session could and couldn't measure.

### R5 — Review
Same method as `docs/security-quality-review-2026-10-04-w4.md`. Priorities: decoder robustness against hostile
JSON, position-store path handling, UI virtualisation actually virtualising, FFI error paths, test quality.

## 3. Out of scope
W6 (hardening, accessibility audits, MSIX, ARM64 gate), anything Apple-side, annotations/TTS/OCR
implementation on Windows, human visual/Narrator passes, push/PR.
