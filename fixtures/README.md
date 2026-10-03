# GIST Parser Corpus — Fixture Catalogue

All fixtures are **synthetically generated** — no third-party content is embedded.
They are designed to exercise the four parser crates (`gist-parse-txt`,
`gist-parse-epub`, `gist-parse-docx`, `gist-web`) through the corpus integration
test at `crates/gist-core/tests/corpus.rs`.

> **Generating adversarial fixtures on demand**
> `tools/gen-fixtures` generates adversarial cases on demand (zip-bomb candidates,
> oversized ePubs, fuzz-derived crash reproducers, etc.).  Run it before a release
> cut to refresh the adversarial sub-directories.

---

## txt/

Plain-text files consumed by `gist-parse-txt::parse(bytes, limits)`.

| File | Size (approx) | Description | Expected behaviour |
|---|---|---|---|
| `basic_ascii.txt` | ~1 KB | Three paragraphs of ASCII Lorem-style prose. BOM-less UTF-8. | `Ok(Document)` with non-empty text content. |
| `unicode_utf8.txt` | ~1 KB | Mixed-script prose: CJK, Arabic, Greek, accented Latin, emoji. BOM-less UTF-8. | `Ok(Document)` with all scripts preserved. |
| `empty.txt` | 0 bytes | Zero-byte file. | `Ok(Document)` with empty or minimal content (not a panic, not an error). |
| `single_line.txt` | 50,000 bytes | One line of 50 000 ASCII `'A'` characters, no trailing newline. | `Ok(Document)` — parser must not over-allocate per-line structures. |
| `bench_20page.txt` | ~58 KB, ~8,900 words | 172 Lorem-ipsum paragraphs, blank-line-separated — a realistic stand-in for §9.1's "20-page document" performance target (not part of the corpus test; used by `crates/gist-parse-txt/benches/parse_bench.rs`, M4 R2). | `Ok(Document)` — used for `cargo bench`, not correctness. |
| `line_endings.txt` | ~340 bytes | Five paragraphs, each separated by a differently-spelled "blank line": CRLF-CRLF, bare-CR-CR, a line with a single space, a line with a single tab. Regression fixture for issue #76 (paragraphs previously only split on a literal `"\n\n"`). | `Ok(Document)` with exactly 5 non-empty paragraph blocks. |
| `adversarial/all_null_bytes.bin` | 1,024 bytes | 1 024 null bytes.  Extension is `.bin` so `Core::import_file` returns `ImportError::UnsupportedType`; pass bytes directly to `parse()` to test that path. | When called directly: `Ok` or a recognised `ParseError`, never a panic. |
| `adversarial/max_lines.txt` | ~285 KB | 5 000 short lines ("Line 00000: The quick brown fox…"). | `Ok(Document)` — stress test line-count handling without hitting memory limits. |

---

## epub/

ePub 2 ZIP archives consumed by `gist-parse-epub::parse(bytes, limits)`.
All valid ePubs carry the required structure:
`mimetype` (uncompressed, first entry) + `META-INF/container.xml` +
`OEBPS/content.opf` + `OEBPS/toc.ncx` + one or more XHTML content files.

| File | Description | Expected behaviour |
|---|---|---|
| `minimal_valid.epub` | Single chapter, ~200 words ASCII prose. | `Ok(Document)` with non-empty content. |
| `multi_chapter.epub` | Five chapters with h1–h3 headings, unordered and ordered lists, em/strong emphasis. | `Ok(Document)` — heading hierarchy and list items preserved as Blocks. |
| `utf8_content.epub` | Single chapter with Greek, accented Latin, emoji and mixed-script prose. | `Ok(Document)` — all Unicode code-points round-trip correctly. |
| `with_table.epub` | Synthetic (M6 R3, 2026-10-03; derived from `minimal_valid.epub`). One chapter: `h1`, paragraph, a `<table>` with `thead`/`th` header, bare `<td>` text, an empty `<td></td>`, and a cell holding two `<p>`s, then a paragraph. | `Ok(Document)` with 4 blocks incl. one `Block::Table`. Before M6 R3 bare `<td>` text was dropped entirely. |
| `adversarial/drm_protected.epub` | Valid ePub structure **plus** `META-INF/encryption.xml` whose `EncryptionMethod` uses `http://www.w3.org/2001/04/xmlenc#aes256-cbc` (commercial DRM). | `Err(ParseError::DrmProtected)` — must not attempt to decode encrypted content. |
| `adversarial/idpf_obfuscation.epub` | Valid ePub **plus** `META-INF/encryption.xml` using **only** the IDPF font-obfuscation scheme (`http://www.idpf.org/2008/embedding`), applied to a fake font file. | `Ok(Document)` — IDPF font obfuscation is not content DRM; content is readable and the parser must not treat it as DRM-protected. |
| `adversarial/empty_spine.epub` | Valid OPF with a manifest but a completely empty `<spine>` element (no `<itemref>`). | `Ok(Document)` with no blocks (empty document acceptable), or a recognised `ParseError`; must not panic. |
| `adversarial/missing_content_file.epub` | OPF spine references `missing_chapter.xhtml` which is absent from the ZIP. | `Ok(Document)` with partial content from the present chapter, or a recognised `ParseError`; must not panic. |

---

## docx/

DOCX (Office Open XML) ZIP archives consumed by `gist-parse-docx::parse(bytes, limits)`.
Minimum required ZIP entries: `[Content_Types].xml`, `_rels/.rels`,
`word/_rels/document.xml.rels`, `word/document.xml`.

| File | Description | Expected behaviour |
|---|---|---|
| `minimal_valid.docx` | A few normal paragraphs, no styles file. | `Ok(Document)` with non-empty text content. |
| `headings_and_paragraphs.docx` | Paragraphs with style names "Heading 1", "Heading 2", "Heading 3" interleaved with body text; includes `word/styles.xml`. | `Ok(Document)` — heading styles recognised and mapped to heading Blocks. |
| `with_tracked_changes.docx` | Document body contains `<w:ins>` (tracked insertion) and `<w:del>` (tracked deletion) elements. | `Ok(Document)` — inserted text included, deleted text excluded (or both flagged); must not panic on revision markup. |
| `with_table.docx` | Synthetic (M6 R3, 2026-10-03; built from `headings_and_paragraphs.docx`'s package skeleton with a hand-written `word/document.xml`). Paragraph, one 3x4 `w:tbl` (header row via `w:tblHeader`; row 3 has an empty cell; row 4's middle cell holds two paragraphs), paragraph. | `Ok(Document)` with exactly 3 blocks: paragraph, one `Block::Table`, paragraph. Before M6 R3 each cell paragraph became its own `Paragraph` block and the empty cell vanished. |
| `adversarial/no_document_xml.docx` | ZIP missing `word/document.xml` entirely. `[Content_Types].xml` still references it. | `Err(ParseError::…)` — graceful error, not a panic. |
| `adversarial/deeply_nested.docx` | `word/document.xml` with 250 levels of nested `<w:ins>` elements wrapping the innermost run. | Parser should hit the nesting-depth limit and return an error, or successfully extract text; must **not** stack-overflow or panic. |

---

## web/

Plain HTML files for **direct parser testing** without network I/O.
The corpus test reads these from disk and passes them to the web crate's HTML
extraction function (not `fetch_url`, which requires a live network connection).
Network fetching is tested via integration/fuzz separately.

| File | Description | Expected behaviour |
|---|---|---|
| `article_with_nav.html` | Full page with `<nav>`, `<header>`, `<article>`, `<aside>`, `<footer>`. | Extraction returns the `<article>` content only; `<nav>` and `<header>` excluded. |
| `minimal_body.html` | Bare `<html><body><p>Hello world</p></body></html>`. | Extraction returns "Hello world". |
| `no_article.html` | Page with `<main>` element but no `<article>`. | Extraction falls back to `<main>` content. |
| `table_article.html` | Synthetic (M6 R3, 2026-10-03). `<article>` with `h1`, paragraph, the same 3x4 table as the epub/docx fixtures, paragraph. | `extract_content` returns 4 blocks incl. one `Block::Table` (header row, empty cell preserved). Before M6 R3 each cell became a separate paragraph. |
| `adversarial/empty_body.html` | `<html><body></body></html>` — no content nodes. | Returns empty string or empty block list; must not panic. |
| `adversarial/script_heavy.html` | Many `<script>` and `<style>` tags in head and body, interspersed with article content. | All `<script>` and `<style>` elements stripped; prose content preserved. |
