# Product Specification: GIST — Cross-Platform Text Reader

**Version:** 1.5
**Status:** Draft
**Licence:** MIT (open source)
**Platforms in scope:** Windows, iOS, macOS
**Supersedes:** v1.4
**Changes from v1.4:** §4 Library Management expanded — sort by source type added as an explicit sort key; item removal mechanism (single-item and bulk) specified with confirmation, transactional deletion, and source-file handling behaviour. Sort key and direction persistence added. §9.4 Reliability updated to reflect transactional removal guarantee.

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
- Android/Linux/Web clients (noted as a possible future phase only).

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

### 3.2 ePub
- Parse OPF/spine and NCX/nav for chapter structure and reading order.
- Preserve embedded CSS semantics (headings, emphasis, lists) but re-flow into the app's own typography engine rather than rendering the book's own stylesheet.

**DRM policy — mandatory, non-negotiable:**
- Before any content is read, the parser checks for `META-INF/encryption.xml`. If present, it inspects the `<enc:EncryptionMethod>` elements.
- If any spine item is encrypted with a commercial DRM algorithm (i.e. not IDPF font obfuscation), the import is rejected immediately with `ParseError::DrmProtected`. No encrypted content is read.
- The user sees a specific, actionable error: "This ePub is DRM-protected and cannot be imported."
- GIST does not circumvent, work around, or partially process DRM. This is a legal requirement (DMCA and equivalent legislation) and is not configurable.
- Zip-bomb protection: decompressed content size is checked during streaming; files exceeding the limit are rejected before memory is exhausted.

### 3.3 TXT
- Encoding detection (UTF-8/16, legacy code pages) with fallback prompt if ambiguous.
- Paragraph detection from blank-line/line-break heuristics, with a manual "re-flow" option for poorly formatted plain text.
- File size enforced against the resource limit before reading.

### 3.4 DOCX
- Extract text via the document's own structure (headings, paragraphs, lists, tables) rather than a blind text dump, so heading levels map onto the app's internal document model.
- Heading detection uses style resolution (`w:pStyle` → `styles.xml` → `w:basedOn` chain), not tag-matching.
- Images: extracted and optionally retained inline or stripped, per user preference.
- Track changes / comments: ignored by default (reading, not editing, tool); flag if the doc contains unresolved tracked changes so the user is aware content may be incomplete.
- Tables: parsed and persisted in the document model in v1.0; rendered as flattened text in v1.0 reading views. Full table rendering in v1.1.
- Zip-bomb protection: same streaming decompression limit as ePub.

### 3.5 Scanned documents (OCR)
- Input: camera capture (mobile) or imported image files (all platforms).
- Pipeline: image pre-processing (deskew, contrast normalisation) → platform OCR recognition → text reconstruction → same normalisation path as other imports.
- Platform OCR engines:
  - iOS/macOS: native Vision framework text recognition.
  - Windows: Windows.Media.Ocr, or a bundled OCR engine (e.g. Tesseract) if consistent cross-platform accuracy is prioritised over native integration.
- Multi-page capture flow for mobile (batch photograph a multi-page document, then process as one item).
- Confidence scoring: low-confidence OCR regions flagged/highlighted for user review rather than silently presented as ground truth.
- OCR recognition runs entirely on-device on all platforms. No image data leaves the device.

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

### 3.7 Common post-import normalisation
All sources are converted into one internal semantic document model (§7) before reaching the reading UI, so the reading engine itself is source-agnostic. All parsers share a common resource limit policy enforced before allocation.

---

## 4. Library Management

- **Views:** grid (covers/thumbnails) and list (metadata-dense). Both views support the same sort, filter, and multi-select operations.

- **Sort:** items in both views can be sorted by the following keys, each in ascending or descending order:
  - **Name** — alphabetical A–Z or Z–A on the item title.
  - **Source type** — groups items by format (PDF, ePub, plain text, DOCX, web article, scanned document); secondary sort within each group is by date added descending.
  - **Date added** — newest or oldest first (default sort for a new library).
  - **Date last read** — most recently or least recently opened.
  - **Reading progress** — furthest through or least started (as a percentage of the token stream consumed).

  The active sort key and direction are persisted independently for the grid view and the list view, and survive app restarts. A sort control (key selector + direction toggle) is always visible in the library toolbar.

- **Organisation:** folders/collections (user-defined) and tags; smart collections (e.g. "Unread," "In progress," "Finished," "Web articles").

- **Search:** metadata search (title/author/source) always available; **full-text search across library content is a v1.0 feature** (FTS5 index built at import time — see §8).

- **Metadata:** title, author/source, source type, date added, date last opened, reading progress %, word count/estimated reading time, cover/thumbnail (generated for text-only sources).

- **Item removal:** items may be removed from the library individually or in bulk.
  - **Single-item removal:** available from the item's context menu (right-click / long-press) and from a toolbar button when exactly one item is selected.
  - **Bulk removal:** available via multi-select (⌘-click or checkbox mode in list view) followed by a "Remove" toolbar action. The multi-select affordance must be keyboard-accessible on macOS/Windows.
  - **Confirmation:** every removal — single or bulk — presents a confirmation dialogue before any data is deleted. The dialogue states: (a) the number of items to be removed and their titles (up to a reasonable display limit); (b) whether the original source file(s) will also be deleted from local storage. The default is to delete the source file alongside the library entry; this default is configurable in preferences.
  - **What is deleted:** removal atomically deletes all of the following for each affected item in a single SQLite transaction: library metadata record, annotations, highlights, bookmarks, reading progress, and the item's FTS5 index entries. If the source file deletion option is selected, the source file is deleted from the app's sandboxed storage directory after the database transaction commits successfully. If any part of the database transaction fails, nothing is deleted and the user receives a specific error; the source file is never deleted if the database transaction did not commit.
  - **Post-removal state:** collections, tags, and smart views that referenced removed items are updated immediately. If a removed item was the currently open document in the reader, the reader closes and returns to the library view. Removal cannot be undone from within the app; the confirmation dialogue makes this explicit.

- **Storage management:** per-item file size shown; option to keep only extracted text and discard original source file to save space (configurable, off by default for PDFs/ePubs where re-reference to original formatting may matter).

---

## 5. Reading Interface

### 5.1 Reading modes
- **Standard flow view** (default): continuous, re-flowable text.
- **Paginated view**: optional, page-turn style for users who prefer it. Deferred to v1.1; the flow view is built on a layout abstraction from day one so paginated is a second implementation, not a rewrite.
- **RSVP (speed-reading) view**: key feature, available for any imported item regardless of source, since all sources normalise into the common document model (§7). Words (or short word-groups) are presented one at a time in a fixed position, removing eye movement and letting the reader increase pace comfortably.

#### 5.1.1 RSVP controls
- **Play/pause:** primary control, large and easily reachable (tap/click, plus spacebar on macOS/Windows); pausing holds on the current word rather than blanking the display, so the reader's place is never lost.
- **Speed dial:** a dial (rotary-style control) for adjusting words-per-minute (WPM) in real time, during playback as well as before starting.
  - Range: 100–1000 WPM, with sensible default (~250 WPM) for first-time use. Limits enforced in the Rust core — no UI value outside this range is accepted.
  - Dial gesture: rotate to adjust; platform-appropriate equivalents — touch drag/rotation on iOS, click-and-drag or scroll on macOS/Windows, with an accessible numeric-entry/stepper alternative for users who cannot use a rotary gesture.
  - Current WPM value displayed numerically alongside the dial at all times.
  - Adjustments take effect immediately without needing to pause first.
- **Scrub/seek:** ability to jump backward/forward within the RSVP sequence (e.g. back 5 words, or drag a position scrubber) to re-read a missed word or skip ahead.
- **Punctuation-aware pacing:** slightly longer hold on words followed by sentence-ending punctuation, and on paragraph/section breaks, to preserve natural comprehension pauses (configurable on/off).
- **Exit to flow view:** one-tap/click return to standard flow view at the same reading position, so RSVP and flow reading stay interchangeable rather than being separate silos.
- **Session stats:** words read, elapsed time, and effective WPM shown at the end of an RSVP session.

### 5.2 Typography & display controls
- Font family (a curated set optimised for on-screen reading, plus system font option).
- Font size, line height, paragraph spacing, margins/text width — all independently adjustable.
- Justification and hyphenation toggle.

### 5.2.1 Theme / dark mode
- **Follow OS theme** (default): the app automatically switches between light and dark presentation in step with the platform's system-wide appearance setting.
- **Manual override:** user can pin the app to light, dark, sepia, or a true black (OLED) theme regardless of the OS setting.
- Theme choice applies consistently across flow view, paginated view, and RSVP view.
- Theme preference is persisted locally on-device.

### 5.3 Navigation & interaction
- Chapter/section jump list (table of contents) where structure exists.
- Progress bar / percentage / page-of-total, configurable.
- Tap/swipe and keyboard/trackpad navigation (platform-appropriate).
- Search within document.

### 5.4 Annotation
- Highlights (multiple colours) and margin notes.
- Bookmarks.
- Export of highlights/notes (Markdown or plain text).

### 5.5 Accessibility
- Full support for platform screen readers (VoiceOver on iOS/macOS, Narrator on Windows).
- Dynamic Type / OS-level text scaling respected.
- Text-to-speech read-aloud as a v1 feature (not RSVP, but a straightforward accessibility/convenience win using native TTS engines on each platform).
- Sufficient colour contrast in all themes; colour is never the sole means of conveying state (e.g. read/unread).
- RSVP speed dial must have an accessible numeric-entry/stepper alternative — a rotary-only control is an accessibility failure.

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
└── ocrConfidence[] [only present for OCR-sourced documents]
```

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
- **MVP platform:** macOS. iOS follows using the same bindings and `.xcframework`.
- **DRM:** detect via `META-INF/encryption.xml`; never circumvent. (ADR 004)
- **Web fetch policy:** TLS-only, max 5 redirects, response size limit, no cookie jar, robots.txt respect. (ADR 005)

**Native per platform (UI shells only):**
- SwiftUI (iOS/macOS) and WinUI 3 (Windows) for all UI.
- Platform OCR engines (Vision framework; Windows.Media.Ocr or Tesseract), TTS engines (AVSpeechSynthesizer; Windows Speech/SAPI), file/share integrations, and background execution.

### 8.2 FFI Safety
The FFI boundary between Rust and Swift is a safety-critical seam. Policy:
- Every function exported via `#[uniffi::export]` wraps its body in `std::panic::catch_unwind`. Panics map to a typed `GistError::InternalPanic` — they never propagate into Swift as undefined behaviour.
- The FFI crate's release profile sets `panic = "abort"` as a belt-and-braces measure.
- The OCR recognition step is modelled as a `uniffi` callback interface (`OcrEngine` trait) implemented in Swift, so the Rust core orchestrates multi-page processing while recognition stays native — and the same interface serves a future Windows implementation.

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
- RSVP frame timing stable at 1000 WPM — driven by `CVDisplayLink`, not `Timer`.

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

---

## 10. Open Questions / Decisions Needed

**Resolved:**
1. ~~**Cross-platform strategy**~~ — Rust core with native UI shells per platform (§8). FFI = uniffi proc-macro mode.
2. ~~**Sync backend**~~ — No sync in v1. All state is local (§6).
3. ~~**Monetisation model**~~ — Free and open source, MIT licence.
4. ~~**FFI binding approach**~~ — uniffi proc-macro mode (not hand-rolled).
5. ~~**MVP platform**~~ — macOS first. iOS follows using the same bindings.
6. ~~**Full-text search**~~ — v1.0 feature. FTS5 index built at import time. Retrofitting requires schema migration + full re-index; deferring is worse than designing in from the start.
7. ~~**Table support**~~ — Tables are parsed and stored in the document model in v1.0; rendered as flattened text in v1.0 reading views; full table rendering in v1.1. Data preserved at import, display improved later.
8. ~~**Paginated view**~~ — v1.1. Flow view built on a `ReadingLayout` abstraction so paginated is a second implementation, not a rewrite.
9. ~~**DRM policy**~~ — Detect via `META-INF/encryption.xml`; reject with a specific error; never circumvent. (ADR 004)

**Still open — decide before implementation begins:**

| Q | Question | Must decide by |
|---|---|---|
| **Q1** | Copy-on-import vs reference-in-place for source files? *(Recommendation: copy — simplifies sandboxing and iOS enormously)* | M1 |
| **Q2** | Document IR on-disk format: JSON vs `postcard`/`bincode` vs blocks as SQLite rows? *(Interacts with FTS5 and lazy loading of large documents)* | M1 |
| **Q3** | SwiftUI `Text` vs TextKit 2 for the flow view? *(Prototype both in M2; choice drives selection precision and annotation implementation)* | M2 end |
| **Q4** | Minimum macOS version? *(Recommendation: macOS 14 — unlocks `@Observable`, modern `NavigationSplitView`, string catalogs)* | M0 |
| **Q5** | Schema/IR versioning and forward compatibility policy? *(Required before first public beta — no server-side migration safety net exists)* | M4 start |
| **Q6** | At-rest document integrity: do stored document blobs require BLAKE3 checksums given the threat model? | M4 start |
| **Q7** | URL fetching on iOS: Rust `ureq` or Swift `URLSession`? *(URLSession gets system proxy, ATS compliance, cellular-awareness)* | M1 |
| **Q8** | Windows OCR: `Windows.Media.Ocr` vs Tesseract? *(The `OcrEngine` trait supports both; decide at Windows kickoff)* | Windows kickoff |

---

## 11. Phasing

**v1.0 (macOS):** PDF, ePub (DRM-detected and rejected, non-DRM imported), TXT, DOCX import; OCR capture; URL import; library management with full-text search, sort by name/type/date added/date last read/progress, and item removal (single and bulk); standard flow reading view with full typography controls; RSVP speed-reading view with speed dial and play/pause; light/dark/sepia/OLED themes with OS-follow; annotations; TTS; all state stored locally on-device.

**v1.1:** Full table rendering in reading views; paginated reading view.

**iOS (follows macOS v1.0):** Same Rust core, same `.xcframework`. Additional iOS-specific work: share extension, camera capture, BackgroundTasks, App Store compliance (~35–45% of the macOS shell effort).

**Future (not committed):** Cross-device sync (mechanism TBD); Android/Web clients; social/sharing features; Windows client.
