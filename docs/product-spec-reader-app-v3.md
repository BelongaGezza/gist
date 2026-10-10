# Product Specification: GIST — Cross-Platform Text Reader

**Version:** 1.6
**Status:** Draft — reconciled with the implementation on 2026-10-09
**Licence:** MIT (open source)
**Platforms in scope:** macOS (v1.0 lead platform), Windows (in development), iOS (paused). Linux and Android have development plans and early spikes but are post-v1.0 and not committed (§1.2, §11).
**Supersedes:** v1.5
**Changes from v1.5:** a documentation-reconciliation pass (2026-10-09) against the code, the ADRs and the milestone history. No product intent was removed. (1) Each section now carries **implementation-status notes** where the build differs from, or has not yet reached, what the text promises. (2) Decisions made during development are written into the text: the RSVP rotary dial became a slider with a 200–700 WPM UI band; RSVP pacing is wall-clock driven rather than `CVDisplayLink`; tables and the paginated view shipped in v1.0 work rather than v1.1; `panic = "unwind"` replaced `panic = "abort"`; minimum macOS is 14. (3) §10 open questions are resolved or re-pointed to their ADRs. (4) Owner decisions of 2026-10-09 applied: the RSVP UI band is 200–700 WPM; `ocrConfidence[]` is dropped from the model; OCR, annotations and export on platforms where they are not built are unplanned future capability (§11). (5) New **§12** lists every spec requirement that is not yet implemented, each with an ID (`SC-nn`) and a scheduled home in `docs/development-plan-v2.md` §5 (M8), so none is left orphaned in a completed milestone.

**How to read status notes.** `Status: Implemented` means built and covered by automated tests on the stated platform. `Status: Partial` and `Status: Not implemented` name the gap and its `SC-nn` ID in §12. "Implemented" does not mean a person has clicked through it on real hardware; manual verification status lives in `PLATFORM_VERIFICATION.md` and `docs/v1.0-release-checklist.md`.

---

## 1. Overview

GIST is a general-purpose reading application that lets a user import text from a wide range of sources (documents, scanned images, and web articles) into a unified library and read it in a clean, distraction-free, customisable interface. The application is free and open source under the MIT licence. All data is stored locally on-device; there is no account system, no backend, and no cross-device sync in v1.

### 1.1 Objectives
- Provide frictionless import from the most common text sources people actually encounter (files, scans, web pages).
- Normalise everything into a consistent internal reading format so the reading experience is uniform regardless of source.
- Deliver a comfortable, accessible, distraction-free reading UI with strong typographic control, including both standard flow reading and RSVP speed-reading.
- Store all library data, reading progress, annotations, and preferences locally on-device with no dependency on network connectivity or external accounts.
- Work fully offline at all times — no features require an internet connection except URL import fetch.

### 1.2 Non-goals (v1)
- Cross-device sync of any kind (library, progress, annotations, preferences).
- User accounts or any form of backend/cloud infrastructure.
- Full e-commerce/bookstore integration.
- Social features (sharing, reviews, book clubs).
- Collaborative annotation/multi-user editing.
- Web client. (Android and Linux desktop clients are no longer "possible future phases only": `docs/android-development-plan.md` and `docs/linux-development-plan.md` exist and a Linux GTK4 spike runs, but both are post-v1.0, not committed, and must not delay the macOS v1.0 release.)

---

## 2. Target Users & Core Use Cases

| Use case | Example |
|---|---|
| Import a downloaded document | User has a PDF report or DOCX and wants to read it comfortably instead of in the source app |
| Import an eBook | User has an ePub from a non-DRM source |
| Digitise a paper document | User photographs/scans a printed article or handout |
| Save a web article for later | User pastes a URL and wants a clean, ad-free reading view |
| Manage a growing library | User organises, searches, and filters dozens/hundreds of imported items on a single device |

---

## 3. Import & Ingestion

This is the core differentiator and needs the most precision. Five source types are in scope:

### 3.1 PDF
- Extract text layer where present (preserve reading order, not just raw stream order — many PDFs have non-linear text streams from multi-column layouts).
- Where no text layer exists (image-only PDF), route through the OCR pipeline (§3.5).
- Preserve: headings (where inferable from font-size heuristics), paragraph breaks, footnotes (optional, collapsible), embedded images (optional inclusion, off by default in RSVP/flow reading modes to avoid layout breaks).
- Strip: headers/footers, page numbers, watermarks (heuristic-based, user-correctable).
- Enforce resource limits before processing: maximum page count and maximum total extracted text size. Files exceeding these limits are rejected with a clear error rather than partially processed.

> **Status (macOS): Partial.** Text extraction, multi-column reading order, heading inference from font size, header/footer/page-number stripping, encrypted-PDF rejection (never bypassed) and image-only-PDF routing to OCR are implemented (`gist-parse-pdf`, `pdfium-render` on a pinned, hash-verified `libpdfium.dylib`, ADR-002; M6). Resource limits include a 64 MiB extracted-text budget and a typed "Can't Import This File" error. **Not implemented:** collapsible footnotes, optional embedded images, watermark stripping and user-correctable stripping (`SC-11`); rotated text and right-to-left scripts. Layout heuristics have been tuned on synthetic PDFs only, so real-document quality is unmeasured. pdfium parses in-process with no isolation (`F33`, ADR-022: accepted for v1.0 by the owner, 2026-10-09; helper process scheduled post-v1.0). Packaged and exercised on macOS only: the Linux spike has no pdfium configured, and Windows packaging of the pdfium library is unverified.

### 3.2 ePub
- Parse OPF/spine and NCX/nav for chapter structure and reading order.
- Preserve embedded CSS semantics (headings, emphasis, lists) but re-flow into the app's own typography engine rather than rendering the book's own stylesheet.

**DRM policy — mandatory, non-negotiable:**
- Before any content is read, the parser checks for `META-INF/encryption.xml`. If present, it inspects the `<enc:EncryptionMethod>` elements.
- If any spine item is encrypted with a commercial DRM algorithm (i.e. not IDPF font obfuscation), the import is rejected immediately with `ParseError::DrmProtected`. No encrypted content is read.
- The user sees a specific, actionable error: "This ePub is DRM-protected and cannot be imported."
- GIST does not circumvent, work around, or partially process DRM. This is a legal requirement (DMCA and equivalent legislation) and is not configurable.
- Zip-bomb protection: decompressed content size is checked during streaming; files exceeding the limit are rejected before memory is exhausted.

> **Status: Partial.** DRM detection and refusal, OPF/spine reading order, XHTML-to-block mapping (headings, paragraphs, lists, emphasis, tables) and all resource limits (including a separate 4 MiB cap on container/OPF/`encryption.xml` metadata reads) are implemented. **Not implemented:** reading the NCX or EPUB 3 `nav` document for chapter structure, so the table of contents is derived only from headings found in the XHTML (`SC-05`); embedded images are not extracted (`SC-04`).

### 3.3 TXT
- Encoding detection (UTF-8/16, legacy code pages) with fallback prompt if ambiguous.
- Paragraph detection from blank-line/line-break heuristics, with a manual "re-flow" option for poorly formatted plain text.
- File size enforced against the resource limit before reading.

> **Status: Partial.** Paragraph detection (blank-line heuristics, CRLF/CR normalisation) and the size limit are implemented. **Not implemented:** encoding detection and the ambiguity prompt. `gist-parse-txt` declares `encoding_rs` and `chardetng` but does not use them; input must be valid UTF-8 and anything else fails with a UTF-8 error (`SC-01`). The manual "re-flow" option does not exist (`SC-02`).

### 3.4 DOCX
- Extract text via the document's own structure (headings, paragraphs, lists, tables) rather than a blind text dump, so heading levels map onto the app's internal document model.
- Heading detection uses style resolution (`w:pStyle` → `styles.xml` → `w:basedOn` chain), not tag-matching.
- Images: extracted and optionally retained inline or stripped, per user preference.
- Track changes / comments: ignored by default (reading, not editing, tool); flag if the doc contains unresolved tracked changes so the user is aware content may be incomplete.
- Tables: parsed and persisted in the document model as `Block::Table` (header row, merged cells via optional `spans`) and rendered as an accessible, theme-aware grid in the macOS flow view; linearised row by row for RSVP and read-aloud. *(v1.5 text said "flattened in v1.0, full rendering in v1.1". That was superseded: tables shipped in M6 and merged cells in M7. The original claim that tables were already parsed and persisted was false until M6.)*
- Zip-bomb protection: same streaming decompression limit as ePub.

> **Status: Partial.** Style-resolved headings, lists, tables, tracked-change handling and all limits are implemented. **Not implemented:** (a) the "unresolved tracked changes" notice. The parser sets `source_type = "docx:tracked-changes"` as a stop-gap (its own comment says "proper metadata field in M2"), but no UI reads it, so the user is never told (`SC-03`). (b) Image extraction and the inline-or-strip preference: no parser emits `Block::Image` (`SC-04`).

### 3.5 Scanned documents (OCR)
- Input: camera capture (mobile) or imported image files (all platforms).
- Pipeline: image pre-processing (deskew, contrast normalisation) → platform OCR recognition → text reconstruction → same normalisation path as other imports.
- Platform OCR engines:
  - iOS/macOS: native Vision framework text recognition.
  - Windows: Windows.Media.Ocr, or a bundled OCR engine (e.g. Tesseract) if consistent cross-platform accuracy is prioritised over native integration.
- Multi-page capture flow for mobile (batch photograph a multi-page document, then process as one item).
- Confidence scoring: low-confidence OCR regions flagged/highlighted for user review rather than silently presented as ground truth.
- OCR recognition runs entirely on-device on all platforms. No image data leaves the device.

> **Status (macOS): Partial.** Image and scanned-PDF import, per-page size/dimension caps, Vision-backed on-device recognition, a multi-page review screen with editable text and low-confidence highlighting, and a single Rust commit call (ADR-009) are implemented. **Not implemented:** deskew and contrast normalisation — `gist-imageprep` only converts to greyscale, downsizes to 2048 px and re-encodes (`SC-08`; this was listed as "remaining M3 work" and was not done); camera capture and the mobile multi-page capture flow (`SC-10`, blocked on the iOS shell); persisting per-region confidence in the document (dropped from the spec 2026-10-09, `SC-09`; see §7). Windows OCR is decided (`Windows.Media.Ocr`, ADR-020, closes the old Q8) but not built; Android is proposed (ADR-028).

### 3.6 Web articles / URL import
- User pastes a URL, or uses a share-sheet/browser extension "Send to GIST" action.
- On-device fetch + readability-style content extraction (strip navigation, ads, comments; retain article body, images optionally, byline, publish date).
- Store the canonical source URL and retrieved date with the item for attribution/reference.
- Fetch policy (governed by ADR 005):
  - TLS only via `rustls`; no plain HTTP.
  - Maximum 5 redirects; abort and surface an error if exceeded.
  - Response size limit enforced; oversized responses rejected before buffering.
  - No persistent cookie storage.
  - Respect `robots.txt` where applicable.
- Hard-stop on paywall/authentication boundaries — no bypass of paywalls or authentication.

> **Status: Partial.** HTTPS-only fetch, SSRF-safe address resolution on every hop (`F14`), 5-redirect cap, streaming size limit, no cookie jar, `robots.txt` and `<article>`/`<main>`/`<body>` extraction are implemented, with the source URL stored in `source_ref`. **Not implemented:** byline, publish date and retrieved date (the parser leaves `author` and `import_date` unset), optional images, and the share-sheet/browser-extension "Send to GIST" entry point (`SC-06`, `SC-07`). Paste-a-URL is the only entry point.

### 3.7 Common post-import normalisation
All sources are converted into one internal semantic document model (§7) before reaching the reading UI, so the reading engine itself is source-agnostic. All parsers share a common resource limit policy enforced before allocation.

---

## 4. Library Management

- **Views:** grid (covers/thumbnails) and list (metadata-dense). Both views support the same sort, filter, and multi-select operations.

  > **Status: Partial.** Only the list view exists on macOS and Windows. There is no grid view and no cover/thumbnail generation: `library_items.cover_path` exists in the schema but nothing populates it (`SC-12`).

- **Sort:** items in both views can be sorted by the following keys, each in ascending or descending order:
  - **Name** — alphabetical A–Z or Z–A on the item title.
  - **Source type** — groups items by format (PDF, ePub, plain text, DOCX, web article, scanned document); secondary sort within each group is by date added descending.
  - **Date added** — newest or oldest first (default sort for a new library).
  - **Date last read** — most recently or least recently opened.
  - **Reading progress** — furthest through or least started (as a percentage of the token stream consumed).

  The active sort key and direction are persisted independently for the grid view and the list view, and survive app restarts. A sort control (key selector + direction toggle) is always visible in the library toolbar.

  > **Status: Partial.** All five spec sort keys are implemented on macOS (name, source type, date added, last read, progress; ADR-021; author sort is an extra), as a single menu of combined key-and-direction choices rather than a key selector plus a direction toggle. The choice is held in view state only and is **not persisted** across restarts, and there is no per-view persistence because there is no grid (`SC-14`). **Progress** is derived from the RSVP position only, so an item read only in the flow view shows a correct last-read date but 0% progress (accepted v1.0 limitation, ADR-021; a shared progress column would be schema v8).  Windows sort keys are tracked in `docs/windows-development-plan.md`.

- **Organisation:** folders/collections (user-defined) and tags; smart collections (e.g. "Unread," "In progress," "Finished," "Web articles").

  > **Status: Partial.** User-defined collections and tags (create, assign, remove, filter by tag, per-item tag editor) are implemented. **Smart collections are not implemented**; the sidebar offers "Library" and user collections only (`SC-13`). Now that `last_opened_at` and reading progress exist (schema v7), the data needed for Unread / In progress / Finished is available.

- **Search:** metadata search (title/author/source) always available; **full-text search across library content is a v1.0 feature** (FTS5 index built at import time — see §8).

- **Metadata:** title, author/source, source type, date added, date last opened, reading progress %, word count/estimated reading time, cover/thumbnail (generated for text-only sources).

  > **Status: Partial.** Title, author, source type, date added, date last opened and progress % are stored and shown. `word_count` is computed and stored in the document but is not exposed through the FFI item record or shown anywhere, so there is no word count or estimated reading time in the UI (`SC-15`). Covers: see Views above (`SC-12`). Note that `Metadata.import_date` and `Metadata.language` are never populated by any parser; "date added" comes from the database `created_at` column.

- **Item removal:** items may be removed from the library individually or in bulk.
  - **Single-item removal:** available from the item's context menu (right-click / long-press) and from a toolbar button when exactly one item is selected.
  - **Bulk removal:** available via multi-select (⌘-click or checkbox mode in list view) followed by a "Remove" toolbar action. The multi-select affordance must be keyboard-accessible on macOS/Windows.
  - **Confirmation:** every removal — single or bulk — presents a confirmation dialogue before any data is deleted. The dialogue states: (a) the number of items to be removed and their titles (up to a reasonable display limit); (b) whether the original source file(s) will also be deleted from local storage. The default is to delete the source file alongside the library entry; this default is configurable in preferences.
    - *Clarification (ADR-006, implemented 2026-09-12):* "source file" always means GIST's own sandboxed, content-addressed copy under `originals/`. Removal never touches the user's real file at its real location.
    - *Platform status:* macOS shows one destructive "Remove" action whose copy-deletion default comes from Settings → Storage (`deleteSourceFilesOnRemoval`, default on). Windows shows a single unconditional "complete delete" with no toggle yet. A copy shared by two items (identical bytes) is kept until the last referencing item is removed.
  - **What is deleted:** removal atomically deletes all of the following for each affected item in a single SQLite transaction: library metadata record, annotations, highlights, bookmarks, reading progress, and the item's FTS5 index entries. If the source file deletion option is selected, the source file is deleted from the app's sandboxed storage directory after the database transaction commits successfully. If any part of the database transaction fails, nothing is deleted and the user receives a specific error; the source file is never deleted if the database transaction did not commit.
  - **Post-removal state:** collections, tags, and smart views that referenced removed items are updated immediately. If a removed item was the currently open document in the reader, the reader closes and returns to the library view. Removal cannot be undone from within the app; the confirmation dialogue makes this explicit.

- **Storage management:** per-item file size shown; option to keep only extracted text and discard original source file to save space (configurable, off by default for PDFs/ePubs where re-reference to original formatting may matter).

  > **Status: Partial.** Settings → Storage shows a library-wide disk-usage breakdown (`originals/` versus document blobs) and a "Verify Library Integrity" action (ADR-013). **Not implemented:** per-item file size and the keep-only-extracted-text option (`SC-16`).

---

## 5. Reading Interface

### 5.1 Reading modes
- **Standard flow view** (default): continuous, re-flowable text.
- **Paginated view**: optional, page-turn style for users who prefer it. The flow view is built on a layout abstraction (`ReadingLayout`) from day one so paginated is a second implementation, not a rewrite. *(v1.5 deferred this to v1.1. It was built early, in M7 — ADR-023 — as an optional second layout, Scroll remaining the default, chosen from a Scroll/Pages control in the reader toolbar or Settings → Reading. macOS only; logic-tested, not yet verified on a real display.)*
- **RSVP (speed-reading) view**: key feature, available for any imported item regardless of source, since all sources normalise into the common document model (§7). Words (or short word-groups) are presented one at a time in a fixed position, removing eye movement and letting the reader increase pace comfortably.

#### 5.1.1 RSVP controls
- **Play/pause:** primary control, large and easily reachable (tap/click, plus spacebar on macOS/Windows); pausing holds on the current word rather than blanking the display, so the reader's place is never lost.
- **Speed dial:** a dial (rotary-style control) for adjusting words-per-minute (WPM) in real time, during playback as well as before starting.
  - Range: 100–1000 WPM, with sensible default (~250 WPM) for first-time use. Limits enforced in the Rust core — no UI value outside this range is accepted.
  - Dial gesture: rotate to adjust; platform-appropriate equivalents — touch drag/rotation on iOS, click-and-drag or scroll on macOS/Windows, with an accessible numeric-entry/stepper alternative for users who cannot use a rotary gesture.
  - Current WPM value displayed numerically alongside the dial at all times.
  - Adjustments take effect immediately without needing to pause first.

  > **Status: Implemented, with two recorded deviations.** (1) The macOS rotary dial was **replaced by a slider** (5-WPM steps) after the dial's drag lost direction when the pointer crossed its centre; the numeric readout and stepper remain, so the accessibility requirement is still met. Windows uses a slider plus number box. (2) The UI offers a practical **200–700 WPM band** (`RsvpSpeedRange`), confirmed by the owner 2026-10-09; the Rust core still accepts and clamps 100–1000, and the Settings default is 250. The "Range: 100–1000" line above is therefore the core's contract, not the UI's. Windows should adopt the same band.
- **Scrub/seek:** ability to jump backward/forward within the RSVP sequence (e.g. back 5 words, or drag a position scrubber) to re-read a missed word or skip ahead.
- **Punctuation-aware pacing:** slightly longer hold on words followed by sentence-ending punctuation, and on paragraph/section breaks, to preserve natural comprehension pauses (configurable on/off).
- **Exit to flow view:** one-tap/click return to standard flow view at the same reading position, so RSVP and flow reading stay interchangeable rather than being separate silos.
- **Session stats:** words read, elapsed time, and effective WPM shown at the end of an RSVP session.

> **Status: Partial.** On macOS: ORP highlighting, play/pause, scrub/seek, back-5-words, punctuation-pause toggle (`Config::pause_on_punctuation`), exit-to-flow navigation and session stats are built. Windows (W4) has play/pause, WPM slider and number box, word stepping, scrubber and progress save/restore. Since 2026-10-09 both shells drive playback from the shared Rust engine through `FfiRsvpSession` (no per-shell port of the pacing maths). Position is persisted in the RSVP `reading_progress` store; the flow and paginated views keep their own positions (ADR-021, ADR-023), so **"exit to flow view at the same reading position" is not met**: `exitToFlowView()` only pushes the flow view for the item, which opens at its own last-saved position, not the RSVP position (`SC-19`).

### 5.2 Typography & display controls
- Font family (a curated set optimised for on-screen reading, plus system font option).
- Font size, line height, paragraph spacing, margins/text width — all independently adjustable.
- Justification and hyphenation toggle.

> **Status: Partial.** macOS and Windows implement font size, a small font-design choice (macOS: system / serif / rounded; Windows: default / serif) and three line-spacing steps, with defaults in Settings → Typography (macOS). **Not implemented:** a curated reading-font set, paragraph spacing, margin/text-width control (the flow view uses a fixed 700 pt column), justification and hyphenation (`SC-17`). The flow-view source states this scoping explicitly ("letter-spacing, hyphenation, or a reading-width control; those can follow").

### 5.2.1 Theme / dark mode
- **Follow OS theme** (default): the app automatically switches between light and dark presentation in step with the platform's system-wide appearance setting.
- **Manual override:** user can pin the app to light, dark, sepia, or a true black (OLED) theme regardless of the OS setting.
- Theme choice applies consistently across flow view, paginated view, and RSVP view.
- Theme preference is persisted locally on-device.

> **Status: Implemented (macOS, Windows).** System-follow, light, dark, sepia and OLED on both shells; theme colours are checked against WCAG AA contrast by a dedicated helper on macOS.

### 5.3 Navigation & interaction
- Chapter/section jump list (table of contents) where structure exists.
- Progress bar / percentage / page-of-total, configurable.
- Tap/swipe and keyboard/trackpad navigation (platform-appropriate).
- Search within document.

> **Status: Partial.** Heading-nested table of contents, in-document search, keyboard/trackpad navigation, a progress bar with percentage (flow view) and "Page x of y" (paginated view) are implemented. The progress display is **not configurable** (`SC-18`).

### 5.4 Annotation
- Highlights (multiple colours) and margin notes.
- Bookmarks.
- Export of highlights/notes (Markdown or plain text).

> **Status: Implemented (macOS).** Highlights in six colours, notes, bookmarks, a sidebar with jump-to, Markdown export via `.fileExporter`, and content-based re-anchoring with an explicit "orphaned" badge (ADR-003). **Not on Windows:** the Windows plan lists annotations and export as out of scope (`SC-22`). Export is Markdown only; a plain-text variant is not offered separately.

### 5.5 Accessibility
- Full support for platform screen readers (VoiceOver on iOS/macOS, Narrator on Windows).
- Dynamic Type / OS-level text scaling respected.
- Text-to-speech read-aloud as a v1 feature (not RSVP, but a straightforward accessibility/convenience win using native TTS engines on each platform).
- Sufficient colour contrast in all themes; colour is never the sole means of conveying state (e.g. read/unread).
- RSVP speed dial must have an accessible numeric-entry/stepper alternative — a rotary-only control is an accessibility failure.

> **Status: Partial.** macOS has a VoiceOver label/trait pass, Dynamic Type previews, on-device read-aloud (`AVSpeechSynthesizer`) and WCAG contrast tests, all verified by code inspection and automated tests only: **no live VoiceOver run, no listening check of read-aloud, and no real-display Dynamic Type check has happened** (`docs/qa-manual-clickthrough-m3.md`). On Windows, the Narrator / Accessibility Insights / text-scale / high-contrast pass is the unstarted W6 phase, and read-aloud is outside the Windows plan altogether (`SC-21`, `SC-22`). The "colour is never the sole means of conveying state" rule has not been audited (`SC-21`).

---

## 6. Local Persistence

All application state is stored on-device only. There is no sync, no account system, and no data leaves the device except during URL import fetch (§3.6).

- **Library database:** SQLite (via the Rust core) with WAL mode and foreign-key constraints enabled. Stores all document metadata, reading progress, annotations, bookmarks, user preferences, and the FTS5 full-text index.
- **Schema migrations:** every migration runs in a transaction. If the on-disk schema version is newer than the current app's known version, the app surfaces a clear error (`SchemaTooNew`) rather than silently operating on an unknown schema — protecting users who run an older app version after a schema upgrade.
- **Source file storage:** original imported files retained in the app's sandboxed storage directory (PDF, ePub, DOCX, etc.), alongside the normalised internal document representation.
- **Progress & annotations:** stored to the local database immediately on change; no deferred write or sync queue.
- **Preferences:** persisted locally per-device; each device maintains independent settings.
- **Backup:** users may back up their device through standard OS mechanisms (iCloud device backup on iOS, Time Machine on macOS, Windows Backup) — this is outside the app's remit and requires no app-level implementation.
- **Export:** users can export their library data (annotations, highlights, progress) as Markdown or plain text (§5.4) to any location they choose via standard OS file-save dialogs.

> **Status: Partial.** Persistence is SQLite (WAL, foreign keys, FTS5), currently **schema v7**, with transactional migrations and a `SchemaTooNew` refusal. Stored document blobs are wrapped in an `ir_version` envelope with a typed `IrVersionTooNew` refusal (ADR-019). Original copies are content-addressed under `originals/` (ADR-006). Two things beyond the v1.5 text: BLAKE3 checksum sidecars verified on read (ADR-013) and opt-in **per-item** AES-256-GCM encryption at rest (ADR-011/014; new imports remain plaintext by default). Annotations export as Markdown; **exporting progress is not implemented** (`SC-19`). Preferences are stored per device (`UserDefaults` on macOS).

---

## 7. Internal Document Model (source-agnostic)

A minimal semantic schema all imports normalise into, so the reading engine, search, TTS, and annotation systems only need to work against one representation:

```
Document
├── id (UUIDv7 — time-ordered, globally unique)
├── metadata (title, author, source type, source reference, import date, language)
├── sections[] (ordered)
│   ├── heading (level, text) [optional]
│   └── blocks[] (ordered)
│       ├── paragraph (text runs with inline emphasis)
│       ├── image (reference, alt text, caption)
│       ├── list (ordered/unordered, items[])
│       └── table (parsed and stored in v1.0; rendered as flattened text until v1.1)
```

> **Status: Partial; the model has moved on from this sketch.** As built (`gist-model`): `Block` is `Paragraph | Image | List | Table`, where `Table { rows, header_row, spans? }` carries merged-cell spans (additive, no version bump). Documents are stored in an `ir_version` envelope (version 2 only when a table is present). `Metadata` also carries `source_copy_ref` (ADR-006) and `word_count`. **`ocrConfidence[]` has been removed from this schema (owner decision 2026-10-09):** OCR confidence is used only by the review screen and is deliberately not persisted. Persisting it would be a potential future capability, not planned (`SC-09`, closed). `Metadata.language` and `Metadata.import_date` exist but no parser sets them. No parser emits `Block::Image` (`SC-04`).

**Token stream:** computed once at import and persisted alongside the document. Serves as the shared substrate for RSVP, TTS, FTS5 indexing, and reading-time estimation. All systems consume this stream, not the block tree directly.

**Annotation anchoring:** stored as `(block_id, start, len, prefix_hash, quote_hash)`. Re-anchored by content search when hashes mismatch (e.g. after re-import or a parser update), with an explicit "orphaned annotations" state surfaced to the user rather than silent data loss.

**Document IDs** use UUIDv7 — time-ordered for database locality, globally unique, not correlated to the import timestamp in a way that leaks timing information.

---

## 8. Architecture: Rust Core + Native UI Shells

### 8.1 Approach
A single shared core, written in Rust, owns all platform-independent logic. Each platform ships a thin native UI shell that calls into this core via FFI.

**Rust crates (implemented):**

| Crate | Responsibility |
|---|---|
| `gist-model` | Document model, IR types, token stream, annotation anchors, serde, errors |
| `gist-parse-txt` | Plain text encoding detection and import |
| `gist-parse-epub` | ePub/OPF parsing, DRM detection, XHTML→block mapping |
| `gist-parse-docx` | DOCX style-resolution, list/table parsing |
| `gist-parse-pdf` | PDF text extraction via pdfium-render, reading-order reconstruction |
| `gist-imageprep` | OCR image pre-processing and post-processing; recognition is native |
| `gist-web` | URL fetch (ureq + rustls), readability extraction, robots.txt |
| `gist-rsvp` | Pure RSVP pacing engine — no I/O, no timers |
| `gist-store` | SQLite schema, migrations, queries, FTS5 index |
| `gist-core` | Import pipeline, library operations, error taxonomy, import observer callbacks |
| `gist-ffi` | uniffi scaffolding (proc-macro mode), staticlib/cdylib, `.xcframework` packaging |

**Key architectural decisions (recorded as ADRs):**
- **FFI:** uniffi proc-macro mode. No hand-rolled C ABI for the Swift surface.
- **PDF backend:** `pdfium-render` (Google's PDFium, BSD-3 licence). Kept behind a swappable trait.
- **HTTP:** `ureq` + `rustls`. Avoids tokio and system OpenSSL.
- **MVP platform:** macOS. iOS follows using the same bindings and `.xcframework` (iOS slices build; the iOS shell is not started and is paused).
- **DRM:** detect via `META-INF/encryption.xml`; never circumvent. (ADR 004)
- **Web fetch policy:** TLS-only, max 5 redirects, response size limit, no cookie jar, robots.txt respect. (ADR 005)

**Native per platform (UI shells only):**
- SwiftUI (iOS/macOS) and WinUI 3 (Windows) for all UI.
- Platform OCR engines (Vision framework; Windows.Media.Ocr or Tesseract), TTS engines (AVSpeechSynthesizer; Windows Speech/SAPI), file/share integrations, and background execution.

### 8.2 FFI Safety
The FFI boundary between Rust and Swift is a safety-critical seam. Policy:
- Every function exported via `#[uniffi::export]` wraps its body in `std::panic::catch_unwind`. Panics map to a typed `GistError::InternalPanic` — they never propagate into Swift as undefined behaviour.
- The release profile must keep `panic = "unwind"`. *(v1.5 required `panic = "abort"` as belt-and-braces. That was wrong: abort defeats `catch_unwind`, so a panic would kill the host app instead of returning `InternalPanic`. Corrected 2026-09-20 and proved in release builds by `cargo run --release -p gist-ffi --features test-panic --example panic_containment`.)*
- The OCR recognition step is modelled as a `uniffi` callback interface (`OcrEngine` trait) implemented in Swift, so the Rust core orchestrates multi-page processing while recognition stays native — and the same interface serves the Windows (ADR-020) and Android (ADR-028) implementations. The shipped contract is `recognize_page(page_index, image_bytes) -> Option<OcrPageResult>` (`None` = cancel); ADR-009's and ADR-020's older text describes a richer shape that does not exist (`F68`).

### 8.3 Parser Safety
All parsers share a common resource limit policy (`ParseLimits`):
- **Max bytes:** limits total input file size before reading.
- **Max pages:** limits page/spine-item count before iteration.
- **Max nesting depth:** limits XML element depth in DOCX/ePub to prevent stack overflow.
- **Max expanded bytes:** limits decompressed output during streaming decompression for zip-based formats (ePub, DOCX), providing zip-bomb protection without reading the full decompressed content first.

Parsers return `ParseError::ResourceLimitExceeded` before allocation. No format-specific UI is needed — the error surface in §3 covers it.

### 8.4 Rationale
- CPU-bound, deterministic logic (parsing, normalisation, RSVP timing, persistence) plays to Rust's strengths.
- Avoids three independent reimplementations of the document model and parsers drifting out of sync.
- Fully native UI per platform preserves RSVP dial responsiveness.
- WASM build of the core remains a realistic option for a future web client.
- SQLite in the Rust core means the local database layer is consistent across platforms.

### 8.5 Platform Integration Summary

| Concern | iOS | macOS | Windows |
|---|---|---|---|
| Shared core | Rust core via uniffi Swift bindings | Rust core via uniffi Swift bindings | Rust core via C ABI (C#/WinRT) |
| UI framework | SwiftUI | SwiftUI/AppKit | WinUI 3 |
| Local storage | SQLite via gist-store (sandboxed container) | SQLite via gist-store (Application Support) | SQLite via gist-store (LocalAppData) |
| OCR recognition | Vision framework | Vision framework | Windows.Media.Ocr or bundled Tesseract |
| TTS | AVSpeechSynthesizer | AVSpeechSynthesizer | Windows Speech/SAPI |
| File import | Files app, share sheet, "Open in" | Finder, drag-and-drop, Services menu | File Explorer, drag-and-drop, "Open with" |
| Distribution | App Store / TestFlight | Direct DMG, notarised | Direct installer / Microsoft Store (optional) |

> **Corrections to the table above (2026-10-09).** Windows does not use a "C ABI (C#/WinRT)" shim: it consumes the same uniffi surface through a pinned, security-reviewed `uniffi-bindgen-cs` (ADR-015), with DPAPI key custody (ADR-016) and a WinUI 3 / unpackaged-then-MSIX shell (ADR-017/018). Windows OCR is `Windows.Media.Ocr` (ADR-020, not yet built). The Windows plan places OCR, annotations, TTS, paginated view, PDF and export out of scope for its v1.0. Linux (Rust GTK4, in-process `gist-core`, `docs/linux-development-plan.md`) and Android (Kotlin + Compose over UniFFI Kotlin bindings, ADR-024–029, `docs/android-development-plan.md`) are planned, post-v1.0 and uncommitted.

### 8.6 Trade-offs to flag
- Three UI codebases still need building — the Rust core reduces duplicated *logic*, not duplicated *UI* work.
- FFI boundary adds engineering overhead (binding maintenance, cross-language debugging, build tooling).
- Team skill requirements include Rust competency in addition to Swift/C#.
- Open source under MIT: all three platform shells and the Rust core are in the same repository.

---

## 9. Non-Functional Requirements

### 9.1 Performance
- Import + normalisation of a typical 20-page PDF/DOCX in under 5 seconds on-device.
- Library with 1,000+ items must remain responsive (virtualised lists, paged library API).
- RSVP frame timing stable across the UI's WPM band (200–700; the core accepts up to 1000) — driven by `CVDisplayLink`, not `Timer`.

> **Status: Deviation, unmeasured.** Playback is driven by a wall-clock-anchored engine (`token_at_elapsed`) that recomputes the token from real elapsed time on every redraw, so a late tick self-corrects instead of drifting; it does not use `CVDisplayLink` (reasoning recorded in `RsvpWallClockEngine`'s doc comment, retained in the shared Rust session). A 10-minute soak at 600 WPM on Windows showed no cumulative drift. **Stability at the top of the band (700 WPM) has been shown only by the Windows soak at 600 WPM; no macOS measurement exists** (`SC-20`, now a measurement task only). Library responsiveness at 1,000+ items is supported by a paged library API and `criterion` benchmarks (M4 R2).

### 9.2 Privacy
- No content is transmitted off-device except during URL import fetch (§3.6).
- OCR runs on-device on all platforms.
- No telemetry, analytics, crash reporting, or data collection of any kind in v1.
- `source_ref` (the imported file's full filesystem path) is stored in the local database for provenance. It is never transmitted. Any future diagnostic feature must strip or hash file paths before use. This constraint is documented in `docs/PRIVACY.md` and must be reviewed before adding any logging or crash-reporting integration.
- Log output at default log levels must not include full filesystem paths. Debug-level logging is permitted.

### 9.3 Security
- **Local storage sandboxed** per platform norms; no network listener or server component.
- **FFI panic safety:** every exported Rust function catches panics and converts them to typed errors. No undefined behaviour at the FFI boundary.
- **Parser resource limits:** all parsers enforce `ParseLimits` before allocation. Zip-bomb protection is enforced during streaming decompression for ePub and DOCX.
- **DRM non-circumvention:** ePub DRM is detected and rejected; no encrypted content is ever read (§3.2).
- **Supply chain:** all third-party CI actions pinned to immutable commit SHAs. `cargo-deny` enforces licence allowlist, advisory blocking (RUSTSEC), banned crates, and allowed source registries on every pull request.
- **Schema safety:** migrations run in transactions; a `SchemaTooNew` error is returned if the on-disk schema version exceeds the app's known version.
- **Document IDs** are UUIDv7 — not predictable timestamps that could leak import timing.
- **Minimum entitlements:** the macOS app runs with the App Sandbox enabled. Entitlements are explicitly declared and reviewed against least-privilege before each release.

### 9.4 Reliability
- Import failures must produce a clear, specific error (unsupported format, DRM detected, network failure, resource limit exceeded, low-confidence OCR) rather than a silent drop or a generic failure message.
- Schema migrations are transactional — a partial migration is rolled back, never committed.
- The app degrades gracefully if the FFI core returns an error; it does not crash.
- Item removal runs in a single SQLite transaction; a partial removal is never committed. Source file deletion occurs only after the database transaction commits successfully (§4).

### 9.5 Open Source Hygiene
- `CONTRIBUTING.md`, per-platform build instructions, and CI for the Rust core required before first public release.
- Licence compliance (`cargo-deny`) is a blocking CI gate.
- Third-party font and fixture provenance recorded in `docs/THIRD-PARTY.md` and `fixtures/README.md`.

### 9.6 Localisation
- UI text externalised for translation from v1 (string catalogs), even if only English (UK) ships initially.

> **Status: Implemented (macOS).** `Localizable.xcstrings` with `tools/check-localisation.sh` as a CI guard (358 literals audited, none missing). The Windows app has no `.resw` resource files and its plan does not schedule localisation (`SC-23`).

---

## 10. Open Questions / Decisions Needed

This section previously listed eight "still open" questions (Q1–Q8) as pre-implementation gates. All eight have since been decided. **Numbering warning:** this spec, `docs/development-plan-v2.md` §7 and `CLAUDE.md` each number their questions independently (for example "Q3" is the flow-view question here, the paginated-view question in `CLAUDE.md`, and the Windows OCR question in the plan). Always cite the ADR, not the Q number.

**Resolved:**
1. ~~Cross-platform strategy~~ — Rust core with native UI shells per platform (§8). FFI = uniffi proc-macro mode (ADR-001).
2. ~~Sync backend~~ — No sync in v1. All state is local (§6).
3. ~~Monetisation~~ — Free and open source, MIT.
4. ~~MVP platform~~ — macOS first.
5. ~~Full-text search~~ — v1.0. FTS5 index built at import time (ADR-008).
6. ~~Table support~~ — Decided parse-and-store in v1.0, render in v1.1. **Superseded:** parsed, stored (`Block::Table`) and rendered in v1.0 work (M6), merged cells in M7.
7. ~~Paginated view~~ — Decided v1.1. **Superseded:** built in M7 as an optional second `ReadingLayout` (ADR-023).
8. ~~DRM policy~~ — Detect via `META-INF/encryption.xml`, reject, never circumvent (ADR-004).

**Former Q1–Q8, now closed:**

| Former Q | Question | Outcome |
|---|---|---|
| Q1 | Copy-on-import vs reference-in-place | **Copy** (ADR-006; implemented 2026-09-12 after a gap was found where removal could delete the user's real file) |
| Q2 | IR on-disk format | JSON blobs `<id>.json` + `<id>.tokens.json`, metadata in SQLite (ADR-007) |
| Q3 | SwiftUI `Text` vs TextKit 2 for the flow view | **SwiftUI-native**, both prototyped (decided 2026-09-12; revisit only if real text selection becomes non-negotiable) |
| Q4 | Minimum macOS | **macOS 14.0** (confirmed 2026-10-04; the project had drifted to 26.5 for a linker-warning workaround). Compile-verified only; never run on a real macOS 14/15 machine |
| Q5 | Schema/IR versioning policy | `ir_version` envelope + `IrVersionTooNew`, alongside `SchemaTooNew` (ADR-019) |
| Q6 | At-rest integrity and confidentiality | BLAKE3 sidecars (ADR-013); opt-in per-item AES-256-GCM (ADR-011, ADR-014) |
| Q7 | iOS URL fetching: `ureq` vs `URLSession` | **Not decided in an ADR** and not exercised: no iOS shell exists. Carried into the iOS plan (`SC-24`) |
| Q8 | Windows OCR | `Windows.Media.Ocr` behind the `OcrEngine` callback (ADR-020, design only until implemented) |

**Open decisions (owner input needed):**

| Decision | State |
|---|---|
| `F33` — in-process pdfium vs isolation (ADR-022) | **Decided 2026-10-09:** accept for v1.0; portable helper process scheduled post-v1.0 |
| `SC-20` — RSVP UI band | **Decided 2026-10-09:** 200–700 WPM; spec amended |
| `SC-09` — `ocrConfidence[]` in the document model | **Decided 2026-10-09:** dropped from §7; potential future capability, unplanned |
| `SC-22` — OCR, annotations and export on platforms without them | **Decided 2026-10-09:** unplanned future capability (§11). TTS, paginated view and PDF on Windows remain undecided |

---

## 11. Phasing

**v1.0 (macOS):** PDF, ePub (DRM-detected and rejected, non-DRM imported), TXT, DOCX import; OCR capture; URL import; library management with full-text search, sort by name/type/date added/date last read/progress, and item removal (single and bulk); standard flow reading view with typography controls; RSVP speed-reading view with speed control and play/pause; light/dark/sepia/OLED themes with OS-follow; annotations; TTS; all state stored locally on-device. **Built early and now part of v1.0 work:** table rendering with merged cells, and the optional paginated view (both previously v1.1). **Specified for v1.0 but not yet built:** the items in §12 marked "v1.0" — notably TXT encoding detection, the DOCX tracked-changes notice, grid view and smart collections, persisted sort, and the full typography control set. Release also waits on human/credential-gated work in `docs/v1.0-release-checklist.md` (signing and notarisation, manual QA, public beta).

**v1.1:** Remaining §12 items not needed for v1.0 (for example footnotes in PDF, library-data export, configurable progress display).

**iOS (follows macOS v1.0):** Same Rust core, same `.xcframework` (iOS slices build). Additional iOS-specific work: the shell itself (not started), share extension, camera capture, BackgroundTasks, App Store compliance (~35–45% of the macOS shell effort). Paused by decision D5 (2026-10-04).

**Windows:** in development in parallel (`docs/windows-development-plan.md`): library, collections, tags, themes, RSVP reader (W4) and flow reader (W5) are built; accessibility, hardening and packaging (W6) are not. OCR, annotations and export are unplanned future capability (below); TTS, the paginated view and PDF are outside that plan's v1.0 and not yet decided (`SC-22`).

**Potential future capability, not planned (decided 2026-10-09):** OCR, annotations and export on any platform that does not have them today (Windows, Linux, Android; iOS inherits whatever the shared Rust core and its shell provide). They are built on macOS only. Platform plans must not list them as scheduled work, and Windows/Linux/Android builds need no placeholder UI for them.

**Post-v1.0, not committed:** Linux desktop (GTK4 spike runs; `docs/linux-development-plan.md`), Android (`docs/android-development-plan.md`, ADR-024–029; host toolchain partly verified, no app yet). **Future, not committed:** cross-device sync (mechanism TBD); Web client; social/sharing features.

---

## 12. Spec items not yet implemented

This register exists because the build plans (M0–M7) were derived from status notes rather than by re-reading this specification, so several requirements were silently left behind in milestones that closed. Each row was checked against source on 2026-10-09. IDs are stable; scheduling is in `docs/development-plan-v2.md` §5 (M8). "Target" is a proposal for the owner to confirm, not a decision.

| ID | Spec § | Requirement not met | Evidence | Target |
|---|---|---|---|---|
| SC-01 | 3.3 | TXT encoding detection (UTF-16, legacy code pages) with ambiguity prompt | `gist-parse-txt` calls `std::str::from_utf8`; `encoding_rs`/`chardetng` declared, unused | v1.0 |
| SC-02 | 3.3 | Manual "re-flow" for poorly formatted text | No code | v1.1 |
| SC-03 | 3.4 | Tell the user a DOCX has unresolved tracked changes | Flag stored as `source_type = "docx:tracked-changes"`; nothing reads it | v1.0 |
| SC-04 | 3.1, 3.2, 3.4, 3.6 | Extract embedded images; optional inline retention; per-user preference | `Block::Image` exists; no parser emits it | v1.1 |
| SC-05 | 3.2 | Chapter structure from NCX / EPUB 3 `nav` | Only XHTML headings are used | v1.1 |
| SC-06 | 3.6, 7 | Byline, publish date, retrieved date, canonical URL for web items; `import_date`, `language` | `gist-web` sets `author: None`, `import_date: None`; no parser sets `language` | v1.0 |
| SC-07 | 3.6, 8.5 | Share-sheet / browser-extension "Send to GIST"; drag-and-drop and Services-menu import | No `onDrop`/Services code; paste-URL and file picker only | v1.1 |
| SC-08 | 3.5 | OCR deskew and contrast normalisation | `prepare_image` is greyscale + resize + PNG only; `imageproc`/`rayon` work listed as "remaining M3" was not done | v1.1 |
| SC-09 | 7, 3.5 | ~~`ocrConfidence[]` in the document model~~ | **Closed 2026-10-09:** dropped from the spec; future capability, unplanned | Closed |
| SC-10 | 3.5 | Camera capture; mobile multi-page capture | Needs the iOS/Android shells | With iOS |
| SC-11 | 3.1 | PDF footnotes (collapsible), optional images, watermark stripping, user-correctable stripping; rotated and RTL text | Not implemented; real-document layout quality unmeasured | v1.1 |
| SC-12 | 4 | Grid view; cover/thumbnail generation | List only; `cover_path` never populated | v1.0 |
| SC-13 | 4 | Smart collections (Unread, In progress, Finished, Web articles) | Sidebar has Library and user collections only | v1.0 |
| SC-14 | 4 | Persist sort key and direction across restarts; key selector + direction toggle | Sort is `@State` only; combined-choice menu | v1.0 |
| SC-15 | 4 | Word count and estimated reading time displayed | `word_count` stored, not on the FFI item record | v1.0 |
| SC-16 | 4 | Per-item file size; keep-only-extracted-text option | Only a library-wide usage breakdown exists | v1.1 |
| SC-17 | 5.2 | Curated font set, paragraph spacing, margins/text width, justification, hyphenation | Size, 3 font designs, 3 line spacings only; fixed 700 pt column | v1.0 |
| SC-18 | 5.3 | Configurable progress display (bar / percentage / page-of-total) | Percentage only (flow); page x of y (paginated) | v1.1 |
| SC-19 | 5.1.1, 6 | Same-position hand-off between RSVP / flow / paginated; export of reading progress | Three separate position stores (ADR-021, ADR-023); `exitToFlowView()` carries no position; no progress export | v1.0 (hand-off), v1.1 (shared progress, schema v8) |
| SC-20 | 9.1 | Measured frame-timing stability across 200–700 WPM on macOS (band itself decided 2026-10-09) | Only a Windows 600 WPM soak exists | v1.0 (measure) |
| SC-21 | 5.5 | Live VoiceOver / read-aloud / Dynamic Type verification (macOS); Narrator, text scale, high contrast (Windows); colour-never-sole-state audit | Code inspection and unit tests only; Windows W6 not started | v1.0 gate |
| SC-22 | 8.5, 11 | Windows: TTS, paginated view, PDF (OCR, annotations and export are now unplanned future capability) | Windows plan §6 lists all as "out of scope" | Owner decision on the remaining three |
| SC-23 | 9.6 | Localisation on Windows | No `.resw` files; not in the Windows plan | With Windows |
| SC-24 | 10 (former Q7) | iOS URL-fetch decision (`ureq` vs `URLSession`) | Never decided; no iOS shell | With iOS |

---

*End of specification.*
