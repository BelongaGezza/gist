# GIST — Architecture

GIST uses a **Rust core + native UI shells** architecture.

## Mono-repo layout

```
gist/
├── crates/           # Rust workspace
│   ├── gist-model    # Document IR, token stream, annotation anchors
│   ├── gist-parse-*  # Format parsers (txt, epub, docx, pdf)
│   ├── gist-imageprep # OCR pre/post-processing
│   ├── gist-web      # URL fetch + readability extraction
│   ├── gist-rsvp     # RSVP pacing engine (pure, no I/O)
│   ├── gist-store    # SQLite persistence (rusqlite + FTS5)
│   ├── gist-core     # Facade: import pipeline, library API
│   └── gist-ffi      # uniffi bindings → Swift / Kotlin; C ABI shim → Windows
├── apps/
│   ├── apple/        # SwiftUI (macOS + iOS)
│   ├── windows/      # WinUI 3 (Windows)
│   ├── android/      # Kotlin + Jetpack Compose (planned; ADR-024)
│   └── linux/        # GTK4 (planned)
├── docs/             # Architecture docs, ADRs, build guides
├── fixtures/         # Test corpus (public-domain only)
└── tools/            # Build scripts
```

## Crate dependencies

```
gist-model (no I/O, wasm32 safe)
    └── gist-parse-txt, gist-parse-epub, gist-parse-docx, gist-parse-pdf
    └── gist-imageprep, gist-web, gist-rsvp
    └── gist-store
            └── gist-core
                    └── gist-ffi → Swift / Kotlin shells (UniFFI) / C ABI (Windows)
```

## Key design decisions

See `docs/adr/` for full decision records.

- **FFI:** uniffi 0.32 proc-macro mode (ADR-001). No hand-rolled C ABI for Swift.
- **Android FFI:** upstream UniFFI Kotlin bindings and Android NDK shared libraries are proposed in ADR-025.
- **PDF:** `pdfium-render` (BSD-3) behind a swappable trait (ADR-002).
- **Annotation anchoring:** `(block_id, start, len, prefix_hash, quote_hash)` with re-anchoring (ADR-003).
- **RSVP timing:** pure `(state, elapsed_ms) → token` function; each native shell supplies its own monotonic clock.
- **Persistence:** SQLite via `rusqlite` (bundled, FTS5). WAL mode. No sync, no backend.
- **Distribution:** notarised DMG (macOS); Windows MSIX and Android APK/AAB are planned.

## Local persistence

All user data lives in app-private storage:

- macOS: `~/Library/Application Support/GIST/`
- Windows: the app's local application-data directory
- Android (planned): `context.filesDir` (ADR-027)

The store contains:

- `gist.sqlite` — library metadata, progress, annotations, FTS5 index
- `docs/` — serialised Document JSON files
- `originals/` — content-addressed copy of imported source files, per ADR-006
  (implemented 2026-09-12; file-based imports only — URL imports have no
  local file to copy). `Metadata.source_ref` remains the raw original path,
  informational only; `Metadata.source_copy_ref` points at the copy here.

No telemetry or accounts. Network access is limited to explicit URL imports per ADR-005.

## Token stream

Computed once at import from the document block tree; persisted alongside the
document. Shared substrate for RSVP, TTS, full-text search, and reading-time
estimation.

## OCR architecture

`gist-imageprep` handles pre/post-processing in Rust. Recognition is delegated
to a platform-native engine via a UniFFI callback interface (`OcrEngine` trait):
Vision framework on Apple, `Windows.Media.Ocr` on Windows, and bundled ML Kit on
Android (proposed in ADR-028).
