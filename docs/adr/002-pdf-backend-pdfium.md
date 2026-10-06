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

## Addendum (M7 R3, 2026-10-04): PDF total-text budget (F36)

`ParseLimits.max_expanded_bytes` (512 MiB) is sized for zip expansion and was
the only total-text budget for PDFs. For a PDF the extracted text is not the
dominant cost: it is amplified into the `Document`'s word-token stream (about
12-14 bytes of resident memory per extracted byte), and a PDF's input-size cap
gives no protection because a single flate-compressed content stream can be
shared by every page.

**Decision.** `gist-parse-pdf` enforces
`text_budget(limits) = min(limits.max_expanded_bytes, MAX_PDF_TEXT_BYTES)` with
`MAX_PDF_TEXT_BYTES = 64 MiB`, a constant in the crate (no new `ParseLimits`
field, so `gist-model` is unchanged for Windows/wasm32). It is checked before
allocation (pdfium's reported glyph count against the remaining budget) and
again on the laid-out text. Related hardening: page indices are narrowed with a
checked conversion (a limit error, never a silent `u16` wrap); gutter
detection uses sorted extents plus binary search instead of a per-gutter scan
of all fragments (output identical, asserted against the old implementation);
and `build_document` consumes each page's lines as they are folded into
paragraphs so lines and paragraphs are not both resident.

**Evidence and its limits.** There is no real-document corpus (decision D6), so
the number is derived from hostile synthetic PDFs only, generated with stdlib
code (not committed), measured with `/usr/bin/time -l` on
`cargo build --release -p gist-parse-pdf --example parse_file`:

| input | text | peak RSS before | peak RSS after |
|---|---|---|---|
| 2000 pages x 3.5 K chars (a long book) | 7 M chars | 120 MiB | 120 MiB |
| 200 pages x 280 K chars (dense, under the cap) | 56 M chars | 804 MiB | 742 MiB |
| 1000 pages x 280 K chars (hostile, over the cap) | 280 M chars | 3.1-3.7 GiB, completes | 181 MiB, rejected after ~64 M chars |

A very text-dense 2000-page book is about 12 MB, so 64 MiB leaves roughly 5x
headroom while bounding the parse itself to about 0.9 GiB. Layout quality on
real documents remains unmeasured (D6). Extraction with pages dropped
immediately peaks at 92 MiB for the 56 M-character case; retained line text adds
~75 MiB; the remainder is the token stream built by `Document::new`.
