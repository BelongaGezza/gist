# ADR 029 — Android PDF text extraction: Pinned prebuilt `libpdfium.so` via `pdfium-render`

**Date:** 2026-10-07
**Status:** Proposed

## Context

GIST supports PDF import (extracting the text layer, reading order heuristics, rejecting encrypted PDFs, and routing image-only scans to OCR per spec §3.1 and ADR-002).

On macOS, `gist-parse-pdf` loads a prebuilt, SHA-256 verified `libpdfium.dylib` (Chromium build 8076) dynamically via `pdfium-render` 0.9.4.

For Android:
- The standard Android SDK `android.graphics.pdf.PdfRenderer` can only render PDF pages as bitmap images for screen display. It does **not** expose APIs for extracting text layers, reading text character streams, or font metrics.
- Therefore, native PDF text layer extraction requires PDFium (or a comparable C++ engine).
- The Rust crate `gist-parse-pdf` already integrates `pdfium-render` with support for dynamic library loading (`Pdfium::bind_to_library_path`).

## Decision

Bundle a pinned, SHA-256 verified prebuilt **`libpdfium.so`** for each supported Android ABI (`arm64-v8a`, `armeabi-v7a`, `x86_64`) within the Android APK/AAB `jniLibs/` package.

Key implementation details:
- **Binary Provenance & Licensing:** Source prebuilt Android NDK binaries from Chromium or the community-standard `bblanchon/pdfium-binaries` release corresponding to the desktop PDFium version.
  - License: PDFium is licensed under BSD-3-Clause (fully permissive, compatible with GIST's MIT license).
  - SHA-256 checksums for each architecture binary are pinned and verified fail-closed during build time in `tools/fetch-pdfium-android.sh`.
- **Dynamic Loading:**
  - Android packages native shared libraries in the app's native library path (`context.applicationInfo.nativeLibraryDir`).
  - During `gist-core` initialization or on first PDF import, pass the path to `libpdfium.so` to the Rust core:
    ```rust
    let pdfium_path = format!("{}/libpdfium.so", native_lib_dir);
    gist_parse_pdf::init_pdfium(&pdfium_path)?;
    ```
- **Fallback / Staged Rollout:** If `libpdfium.so` is omitted or unavailable, the core cleanly returns `GistError::PdfUnavailable`. Image-only PDFs continue to route cleanly to the OCR pipeline (ADR-028).

## Consequences

- Architectural uniformity: PDF parsing logic across macOS, Linux, Windows, and Android remains 100% unified in `gist-parse-pdf`. No Android-specific PDF parser needs to be written.
- All PDF security policies (rejecting encrypted PDFs, streaming memory limits, zip-bomb/malformed stream checks) are automatically enforced by the Rust core.
- Increases APK size by ~5–8 MB per architecture (compressed), managed seamlessly by Android App Bundle (AAB) split APKs.
