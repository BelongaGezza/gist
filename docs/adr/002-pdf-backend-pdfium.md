# ADR-002: PDF backend — pdfium-render

**Status:** Accepted
**Date:** 2026-09

## Decision

Use `pdfium-render` (bindings to Google's PDFium, BSD-3 licence) as the PDF
parsing backend.

## Alternatives considered

- `lopdf` (pure Rust): ~6–8 weeks to reach acceptable extraction quality vs
  ~2–3 weeks with pdfium. No reading-order extraction.
- `mupdf-rs` / Poppler bindings: AGPL/GPL — incompatible with MIT distribution.

## Consequences

- ~8–10 MB `libpdfium` per architecture in the bundle
- Parser is behind a `PdfParser` trait — backend is swappable without touching
  the rest of the stack
- Reading-order extraction (column detection via whitespace-gap projection
  profiles) is non-trivial regardless of backend; budget 1 week
- Header/footer stripping: compare text in margin bands across pages; strip
  high-similarity repeated runs
