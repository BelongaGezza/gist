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
│   └── gist-ffi      # uniffi bindings → Swift; C ABI shim → Windows
├── apps/
│   ├── apple/        # SwiftUI (macOS + iOS)
│   └── windows/      # WinUI 3 (future)
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
                    └── gist-ffi → Swift shell / C ABI
```

## Key design decisions

See `docs/adr/` for full decision records.

- **FFI:** uniffi 0.32 proc-macro mode (ADR-001). No hand-rolled C ABI for Swift.
- **PDF:** `pdfium-render` (BSD-3) behind a swappable trait (ADR-002).
- **Annotation anchoring:** `(block_id, start, len, prefix_hash, quote_hash)` with re-anchoring (ADR-003).
- **RSVP timing:** pure `(state, elapsed_ms) → token` function; shell drives from `CVDisplayLink`.
- **Persistence:** SQLite via `rusqlite` (bundled, FTS5). WAL mode. No sync, no backend.
- **Distribution:** notarised DMG via GitHub Actions on `v*` tag. No App Store required.

## Local persistence

All user data lives in `~/Library/Application Support/GIST/` (macOS):

- `gist.sqlite` — library metadata, progress, annotations, FTS5 index
- `docs/` — serialised Document JSON files
- `originals/` — content-addressed copy of imported source files

No data leaves the device. No accounts. No telemetry.

## Token stream

Computed once at import from the document block tree; persisted alongside the
document. Shared substrate for RSVP, TTS, full-text search, and reading-time
estimation.

## OCR architecture

`gist-imageprep` handles pre/post-processing in Rust. Recognition is delegated
to a platform-native engine via a uniffi callback interface (`OcrEngine` trait):
Vision framework on Apple, `Windows.Media.Ocr` on Windows.
