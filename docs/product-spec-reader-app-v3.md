# Product Specification: GIST — General Import & Speed-reading Tool

**Version:** 1.3
**Status:** Draft
**Licence:** MIT (open source)
**Platforms in scope:** Windows, iOS, macOS

---

## 1. Overview

A general-purpose reading application, in the spirit of Reeder, that lets a user import text from a wide range of sources (documents, scanned images, and web articles) into a unified library and read it in a clean, distraction-free, customisable interface. The application is free and open source under the MIT licence. All data is stored locally on-device; there is no account system, no backend, and no cross-device sync in v1.

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

### 3.1 PDF
- Extract text layer where present (preserve reading order, not just raw stream order).
- Where no text layer exists (image-only PDF), route through the OCR pipeline (3.5).
- Preserve: headings, paragraph breaks, footnotes (optional, collapsible), embedded images (optional).
- Strip: headers/footers, page numbers, watermarks (heuristic-based, user-correctable).

### 3.2 ePub
- Parse OPF/spine and NCX/nav for chapter structure and reading order.
- Re-flow into the app's own typography engine rather than rendering the book's own stylesheet.
- Respect any DRM boundary: detect DRM and surface a clear error rather than a silent failure.

### 3.3 TXT
- Encoding detection (UTF-8/16, legacy code pages) with fallback prompt if ambiguous.
- Paragraph detection from blank-line/line-break heuristics, with a manual "re-flow" option.

### 3.4 DOCX
- Extract text via the document's own structure (headings, paragraphs, lists, tables).
- Images: extracted and optionally retained inline or stripped, per user preference.
- Track changes / comments: ignored by default; flag if the doc contains unresolved tracked changes.

### 3.5 Scanned documents (OCR)
- Input: camera capture (mobile) or imported image files (all platforms).
- Pipeline: image pre-processing → OCR → text reconstruction → normalisation.
- Platform OCR engines: Vision framework (iOS/macOS); Windows.Media.Ocr or Tesseract (Windows).
- Multi-page capture flow for mobile.
- Confidence scoring: low-confidence OCR regions flagged for user review.

### 3.6 Web articles / URL import
- On-device fetch + readability-style content extraction.
- Store the canonical source URL and retrieved date.
- Respect robots.txt/paywall boundaries — no bypass of paywalls or authentication.

### 3.7 Common post-import normalisation
All sources are converted into one internal semantic document model (see §7).

---

## 4. Library Management

- **Views:** grid and list, sortable by title, author/source, date added, date last read, reading progress.
- **Organisation:** folders/collections (user-defined) and tags; smart collections (Unread, In progress, Finished, Web articles).
- **Search:** metadata search always; full-text search as a v1.1 stretch goal.
- **Metadata:** title, author/source, source type, date added, date last opened, reading progress %, word count/estimated reading time, cover/thumbnail.
- **Storage management:** per-item file size shown; option to keep only extracted text and discard original source file.

---

## 5. Reading Interface

### 5.1 Reading modes
- **Standard flow view** (default): continuous, re-flowable text.
- **Paginated view**: optional, page-turn style (v1.1).
- **RSVP (speed-reading) view**: key feature; words presented one at a time in a fixed position.

#### 5.1.1 RSVP controls
- **Play/pause:** primary control; pausing holds on the current word.
- **Speed dial:** rotary-style control for WPM (100–1000, default ~250). Adjustments take effect immediately.
- **Scrub/seek:** jump backward/forward within the RSVP sequence.
- **Punctuation-aware pacing:** longer hold on sentence-ending punctuation and paragraph breaks (configurable).
- **Exit to flow view:** one-tap/click return to standard flow view at the same reading position.
- **Session stats:** words read, elapsed time, effective WPM shown at end of session.

### 5.2 Typography & display controls
- Font family, font size, line height, paragraph spacing, margins/text width — all independently adjustable.
- Justification and hyphenation toggle.

### 5.2.1 Theme / dark mode
- Follow OS theme (default); manual override to light, dark, sepia, or OLED.
- Theme preference is persisted locally on-device.

### 5.3 Navigation & interaction
- Chapter/section jump list (TOC), progress bar, search within document.
- Tap/swipe and keyboard/trackpad navigation (platform-appropriate).

### 5.4 Annotation
- Highlights (multiple colours) and margin notes.
- Bookmarks.
- Export of highlights/notes (Markdown or plain text).

### 5.5 Accessibility
- Full support for platform screen readers (VoiceOver on iOS/macOS, Narrator on Windows).
- Dynamic Type / OS-level text scaling respected.
- Text-to-speech read-aloud as a v1 feature.
- Sufficient colour contrast in all themes.

---

## 6. Local Persistence

All application state is stored on-device only. No data leaves the device except during URL import fetch (§3.6).

- **Library database:** SQLite (via the Rust core).
- **Source file storage:** original imported files in the app's sandboxed storage directory.
- **Progress & annotations:** stored immediately on change.
- **Preferences:** persisted locally per-device.
- **Backup:** via standard OS mechanisms (Time Machine on macOS, etc.) — outside the app's remit.
- **Export:** annotations/highlights/progress as Markdown or plain text via standard OS file-save dialogs.

---

## 7. Internal Document Model (source-agnostic)

```
Document
├── metadata (title, author, source type, source reference, import date, language)
├── sections[] (ordered)
│   ├── heading (level, text) [optional]
│   └── blocks[] (ordered)
│       ├── paragraph (text runs with inline emphasis)
│       ├── image (reference, alt text, caption)
│       ├── list (ordered/unordered, items[])
│       └── table [optional, v1.1]
└── ocrConfidence[] [only present for OCR-sourced documents]
```

---

## 8. Architecture: Rust Core + Native UI Shells

### 8.1 Approach
A single shared Rust core owns all platform-independent logic. Each platform ships a thin native UI shell.

**Rust core:** import/parsing, document model, RSVP pacing engine, SQLite persistence, URL fetch+extraction.
**Native shells:** SwiftUI (iOS/macOS), WinUI 3 (Windows). OCR, TTS, file integrations remain platform-native.

### 8.2 Rationale
- Parsing, normalisation, and RSVP timing are CPU-bound and deterministic — plays to Rust's strengths.
- Avoids three independent reimplementations of the document model drifting out of sync.
- Keeps UI fully native per platform; RSVP dial needs native low-latency responsiveness.
- Leaves a future WASM build of the core as a realistic option (§11).

### 8.3 Platform Integration Summary

| Concern | iOS | macOS | Windows |
|---|---|---|---|
| Shared core | Rust core via uniffi-rs Swift bindings | Rust core via uniffi-rs Swift bindings | Rust core via C ABI (C#/WinRT) |
| UI framework | SwiftUI | SwiftUI/AppKit | WinUI 3 |
| Local storage | SQLite via Rust core | SQLite via Rust core | SQLite via Rust core |
| OCR | Vision framework | Vision framework | Windows.Media.Ocr or Tesseract |
| TTS | AVSpeechSynthesizer | AVSpeechSynthesizer | Windows Speech/SAPI |
| Distribution | App Store | Direct DMG (no App Store required) | Direct installer / Microsoft Store |

### 8.4 Trade-offs
- Three UI codebases still need building — the Rust core reduces duplicate logic, not duplicate UI work.
- FFI boundary adds engineering overhead.
- Team skill requirements include Rust in addition to Swift/C#.

---

## 9. Non-Functional Requirements

- **Performance:** import + normalisation of a typical 20-page PDF/DOCX in under 5 seconds; library with 1,000+ items must remain responsive.
- **Privacy:** no content transmitted off-device except URL import fetch; no telemetry or analytics.
- **Security:** local storage sandboxed per platform norms; no network listener.
- **Reliability:** import failures must produce a clear, specific error rather than a silent drop.
- **Localisation:** UI text externalised for translation from v1 (English UK ships initially).
- **Open source hygiene:** CONTRIBUTING.md, per-platform build instructions, and CI required before first public release.

---

## 10. Open Questions / Decisions Needed

1. ~~**Cross-platform strategy**~~ — resolved: Rust core + native UI shells. FFI = uniffi.
2. ~~**Sync backend**~~ — resolved: no sync in v1. All state is local.
3. ~~**Monetisation model**~~ — resolved: free and open source, MIT licence.
4. **Full-text search:** v1 or v1.1? (Development plan recommends v1.)
5. **Table support in DOCX/PDF imports:** parse+persist in v1, flatten at render until v1.1?
6. ~~**MVP platform**~~ — resolved: macOS first.

---

## 11. Phasing

**v1 (MVP — macOS):** PDF, ePub, TXT, DOCX import; OCR capture; URL import; library management; standard flow reading view with full typography controls; RSVP speed-reading view with speed dial and play/pause; light/dark/sepia/OLED themes; annotations; TTS; all state stored locally on-device.

**v1.1:** Full-text search across library; table support in the document model; paginated reading view.

**Future (not committed):** Cross-device sync; iOS; Windows; Android/Web clients; social/sharing features.
