# Changelog

All notable user-facing changes to GIST are recorded here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), adapted for a project that has not
tagged a release yet.

This file summarizes what's shipped at a user-facing level. It is derived from — and is not a
substitute for — `CLAUDE.md`'s milestone register (dated history, exact verification status of
every item) and `docs/windows-development-plan.md` (the Windows-specific plan and status). Where
this file says a feature is "done," check those two documents for the precise caveat: on this
project, "done" reliably means "implemented and covered by automated tests," and separately
either "compiler-verified" or "manually click-through verified by a person" — those are not
always the same thing yet, and the distinction matters for a pre-1.0 project. See "Known
limitations" at the bottom of this file for what's explicitly still open.

## [Unreleased]

No version has been tagged and there is no public release yet — this section is a running log of
pre-1.0 work, not a shipped release note.

### Added — Rust core (shared by macOS and Windows)

- Document import: plain text, ePub (with DRM detection via `META-INF/encryption.xml`, never
  circumvented), and DOCX (style resolution, tracked changes). Every parser enforces a shared
  `ParseLimits` guard (max bytes/pages/nesting depth/expanded-zip-bytes/zip-entry-count) before
  allocating based on untrusted input. (PDF: see below.)
- Image import with on-device OCR (`gist-imageprep` + an OCR callback interface implemented by
  each platform shell), with the same size/dimension caps as the other importers.
- URL-paste import, HTTPS-only, with SSRF-safe address resolution (rejects loopback/private/
  link-local/cloud-metadata targets on every connection attempt including redirect hops),
  robots.txt respect, and readability-style content extraction.
- Copy-on-import (ADR-006): file-based imports get a content-addressed local copy, so removing a
  library item — even with "delete original" selected — never touches the user's real file at its
  real location.
- An RSVP (word-at-a-time) pacing engine and a flowing/virtualized document reading model. Each
  platform shell mirrors the Rust pacing engine's cursor/elapsed math client-side, wall-clock-
  driven rather than timer-driven, to avoid playback drift.
- Full-text search (SQLite FTS5), library item removal (single/bulk), collections and tags CRUD,
  sort and tag-filter queries.
- PDF import (macOS): text PDFs are extracted via a pinned, hash-verified build of pdfium
  embedded in the app, with reading-order, heading and header/footer handling. Scanned
  (image-only) PDFs are rendered page by page and routed through the existing on-device OCR
  review screen. Password-protected PDFs are rejected with a clear message and never opened.
  Layout heuristics are so far tuned on synthetic PDFs only.
- Tables are now preserved as real tables (previously dropped in ePub and flattened to loose
  paragraphs in DOCX and web imports) and shown as an accessible, theme-aware grid in the flow
  view. Documents containing a table are stored with a newer internal format version, so an older
  GIST build will report "needs a newer version" for them rather than failing obscurely.
- Fixed: annotations placed after an image that had a caption but no alt text could anchor to
  the wrong place.
- Annotations (highlights, notes, bookmarks) with content-based re-anchoring so an annotation can
  survive a document being re-imported or re-parsed (ADR-003).
- Opt-in, per-item encryption at rest (AES-256-GCM; key custody via the OS keychain on macOS,
  DPAPI on Windows) and at-rest integrity verification (BLAKE3 checksum sidecars, checked before
  any decryption attempt; a mismatch is a typed, surfaced error, never a silent pass-through or a
  silent data loss).
- Supply-chain hardening: every GitHub Actions step pinned to a commit SHA, `cargo-deny` license/
  advisory gating in CI, Dependabot alerts and automated security updates enabled on the GitHub-
  hosted repository.

### Added — macOS app (SwiftUI)

- Library view: list with multi-select, ⌘F-focusable search, sort, tag filter, bulk removal
  (whether removal also deletes the sandboxed copy of the original file is a persisted Settings
  default rather than a per-action choice), "Add to Collection," and a tag editor.
- Library sort by source type, date last read, and progress (alongside date added, title and
  author), plus a small progress bar and "Last read …" line on library rows (ADR-021). The core
  stores a "last opened" time (schema v7) that both readers set; progress is derived from the RSVP
  reading position. Items never opened sort last for "last read".
- Sidebar navigation across the library and user-created collections.
- A theme engine: system-follow, light, dark, sepia, and true-black OLED.
- A flow (continuous document) reading view: virtualized rendering, typography controls (size,
  font design, line spacing), a heading-nested table of contents, in-document search, scroll-
  position persistence, and keyboard/trackpad navigation.
- An RSVP reading view: optimal-recognition-point (ORP) highlighting, a rotary WPM dial, scrub/
  seek and back-5-words controls, a punctuation-pause toggle, session stats, and exit-to-flow-view.
- Annotation UI: highlight/note/bookmark composer sheets, a sidebar with jump-to and export, and
  badges for annotations whose anchor has drifted since the source document changed.
- A multi-page OCR review screen (on-device recognition, editable per-page text, low-confidence
  text highlighted) sitting in front of the single Rust commit call.
- Accessibility work: a VoiceOver label/trait pass, Dynamic Type support, on-device read-aloud
  (`AVSpeechSynthesizer`), and theme colors checked against WCAG AA contrast with a dedicated
  contrast-ratio helper (this caught and fixed one real failing color).
- A Settings scene (Reading, Typography, RSVP, Import, Storage, About), an in-app open-source
  license attribution screen, a storage-usage breakdown, and a "Verify Library Integrity" action.
- App Sandbox entitlements (ADR-012).

### Added — Windows app (WinUI 3) — in progress, not yet at feature parity with the macOS app

- The shared Rust core wired in via a uniffi-generated C# binding, with DPAPI-backed key custody
  for encryption-at-rest (ADR-015, ADR-016).
- A library screen: import, list, multi-select, search, sort, tag filter, and removal — a single,
  unconditional "complete delete" action (one button, deletes everything GIST holds for the item,
  including its sandboxed original copy, but never the user's real file). macOS defaults to the
  same behavior but exposes it as a toggleable Settings default; Windows does not yet.
- Sidebar navigation, collections, a tag editor, and the same five-theme engine as macOS.
- The RSVP reader, the flow reader, an accessibility pass, and MSIX packaging are the remaining
  planned phases and are not built yet — see `docs/windows-development-plan.md`'s W4–W6.

- RSVP reader (W4): play/pause, WPM slider + number box, word stepping, scrubber, progress save/restore. Pacing comes from the shared Rust engine over FFI (no second port). A 10-minute soak at 600 WPM showed no cumulative drift. Visual/Narrator passes still pending.

- Flow reader (W5): virtualised reading view with typography (size, Default/Serif, line spacing), Contents, in-document find (F3/Ctrl+G), progress with position restore, and selectable text. Opened from the Library context menu. Known issues: memory growth across repeated opens of very large documents, and very long single paragraphs are untested.

### Security

GIST tracks security findings from internal and independent reviews in an open register rather
than a private one. See the "Security register" in `CLAUDE.md` and `docs/security-review-v2.md`
for the full, dated list of what's been found and fixed. Headline items: an SSRF gap and an
epub zip-bomb gap in the import paths (both closed the day they were found); FTS5 query-injection
hardening for search input; a macOS App Sandbox with minimal entitlements; and the encryption-at-
rest and at-rest-integrity work described above. To report a new vulnerability, see `SECURITY.md`.

### Known limitations

- **Library "progress" counts RSVP reading only.** A book you have read only in the Flow view
  shows a correct "last read" date but 0% progress, and sorts as unstarted under the progress sort
  (the Flow view and RSVP keep separate position stores). A shared progress value would need a
  later schema change. See ADR-021. The database schema is now v7: an older GIST build cannot open
  a library that a newer build has upgraded.
- **No signed or notarized macOS release exists yet.** The release pipeline
  (`.github/workflows/release-macos.yml`, `tools/build-dmg.sh`) is built and has been exercised
  end-to-end unsigned in this environment; the signed/notarized half is blocked on a human
  configuring real Apple Developer Program credentials.
- **No completed manual QA click-through exists for either platform's UI yet.** Every feature
  listed above is covered by automated (unit/logic, and for macOS, compiler-verified `xcodebuild`)
  tests, but a person driving every screen end-to-end — including VoiceOver navigation, actually
  hearing the read-aloud feature, and a real-display Dynamic Type/contrast check on macOS, plus
  the equivalent Windows Narrator/visual pass — has not happened yet.
- **PDF import is new and lightly tested.** It works end to end (text PDFs and scanned PDFs via OCR), but layout heuristics are tuned on synthetic PDFs only (two-column and simple footnote-free layouts); rotated text, right-to-left scripts, footnotes and tables inside PDFs are not handled, and the signed/notarised build of the embedded pdfium library has not been verified.
- **A paginated (page-turning) reading view is not implemented** — only the continuous flow view
  and RSVP exist today; a paginated view is an open question for a future release.
- **IR/schema forward-compatibility policy is not yet implemented.** The persisted document
  format (`gist-model::Document`, stored as `<id>.json`) has no version field and no defined
  behavior for a future, incompatible format change — tracked as an open question and scheduled
  ahead of the first public beta.
