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

## Addendum 2026-10-04 — distribution, linking, loading and extraction (M6 R1)

**Binary distribution.** pdfium is a **pinned prebuilt** from
`github.com/bblanchon/pdfium-binaries`, never compiled from source and never
committed to git. `tools/fetch-pdfium.sh` downloads one specific release asset
(`chromium/8076`, `pdfium-mac-univ.tgz`, a universal2 arm64+x86_64 dylib),
verifies the pinned SHA-256
(`3bdb93e229298dfdf083dc8ccc7d1a8cf87790b6917e5073335504fe2ff0bdc1`, computed
independently from the downloaded asset and equal to the digest GitHub
publishes), and **fails closed** on mismatch; it extracts to the gitignored
`artifacts/pdfium/`. It never uses "latest". Bumping means changing tag and
hash together in a reviewed PR. Run by the Xcode pre-build phase, `apple-build`,
`core-test` (macOS leg) and the macOS fuzz job.

**Linking: dynamic, embedded, signed with the app (N8).** The macOS archive
ships only `libpdfium.dylib` (install name `./libpdfium.dylib`; no static
archive), so static linking is not available. The dylib is embedded at
`GIST.app/Contents/Frameworks/libpdfium.dylib` via an `embed: true, link:
false, codeSign: true` dependency in `apps/apple/project.yml`. It is
deliberately **not linked** at build time: the Rust core `dlopen`s it by an
absolute path resolved relative to the running executable
(`<exe>/../Frameworks/libpdfium.dylib`), so there is no `-l` search-path
ambiguity of the kind N8 describes. Because it is code-signed on copy with the
same identity as the app, Hardened Runtime library validation (same Team ID)
accepts it; `com.apple.security.cs.disable-library-validation` is **not** used
and must not be added. `pdfium-render` is used with `default-features = false`
(no `static`, no `image`), features `pdfium_latest` + `thread_safe`.

**Loading is typed, not panicking.** `gist-parse-pdf` (`#![forbid(unsafe_code)]`)
returns `PdfError::LibraryUnavailable` if the library is absent or fails to
load; `gist-core` maps it to `ImportError::PdfUnavailable` and `gist-ffi` to
`GistError::PdfUnavailable`. A test-only convenience (`GIST_PDFIUM_LIBRARY`
env var and the in-repo `artifacts/pdfium` path) is compiled **only under
`debug_assertions`**, so a release build cannot be steered to a different
library by its environment. pdfium-render's global bindings block forever if a
handle is created before binding; the loader only creates handles after a
successful first bind.

**Extraction pipeline.** The `PdfParser` trait streams per-page glyph data
(char, bounds, font size) to a sink; all layout analysis is backend-independent
and pure (`gist_parse_pdf::layout`): glyph runs -> fragments -> column
detection by vertical-whitespace projection profile (full-width fragments act
as band separators) -> lines in reading order -> margin-band (top/bottom 10%)
header/footer removal when a digit-masked line repeats on >=35% of pages (min
3 pages) or is a page number -> heading inference (font size >= 1.15x the
char-weighted body size, ranked into levels 1-4; levels 1-2 start a new
`Section`, which gets `Section.heading` for the TOC and a leading
`Block::Heading`) -> paragraph breaks from line pitch / short sentence-final
lines, with cross-page continuation and de-hyphenation. Known limits:
rotated pages/text, right-to-left scripts, tables and footnotes are not
specially handled.

**Safety.** `max_bytes` before anything; `%PDF-` header check; `max_pages`
after the page count is known and before any page loads; per-page glyph ceiling
(2,000,000) and cumulative `max_expanded_bytes` checked against pdfium's
reported character count *before* per-character allocation; no recursion of our
own (`max_nesting_depth` n/a). A document pdfium can only open with a password
is `PdfError::Encrypted`; an encrypted document that opens with an empty user
password is additionally rejected if its permission bits forbid text
extraction. No password is ever tried (ADR-004 posture). A document with no
extractable text is `PdfError::NoTextLayer` -> `GistError::PdfNoTextLayer`, the
signal for the app to route to OCR (R2).

**Not verified in this environment.** The *signed* half of the above — that
the embedded dylib is accepted by Hardened Runtime library validation in a
real Developer-ID-signed, notarized Release build — cannot be verified here
(no signing identities or credentials). It remains open under N8 and
`docs/v1.0-release-checklist.md`. What *was* verified: an unsigned build embeds
the dylib in `Contents/Frameworks/` and the test host (`GIST.app`) imports a
real PDF through it.
