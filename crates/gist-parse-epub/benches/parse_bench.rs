//! Performance benchmarks for `gist-parse-epub::parse`, added for M4 role R2
//! (`docs/m4-agent-roles.md` §2) against product-spec §9.1's "20-page
//! document import+normalisation under 5s" target.
//!
//! No 20-chapter ePub fixture is checked into `fixtures/epub/` (all three
//! existing fixtures are small, correctness-focused, see
//! `fixtures/README.md`) — rather than commit a large generated binary
//! fixture, `build_synthetic_epub` builds a realistic-structure ePub
//! (container.xml + OPF manifest/spine + N XHTML chapters, mirroring the
//! shape `multi_chapter.epub` already exercises at a smaller scale) in code,
//! so the bench's input is transparent/versioned as Rust rather than an
//! opaque committed `.epub` blob.

use criterion::{criterion_group, criterion_main, Criterion};
use gist_model::ParseLimits;
use std::io::Write;
use zip::write::SimpleFileOptions;

const MULTI_CHAPTER: &[u8] = include_bytes!("../../../fixtures/epub/multi_chapter.epub");

/// Builds an in-memory ePub with `chapters` XHTML spine items, each padded
/// with roughly `words_per_chapter` words of paragraph text.
fn build_synthetic_epub(chapters: usize, words_per_chapter: usize) -> Vec<u8> {
    let mut zip_bytes = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut zip_bytes);
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        writer.start_file("mimetype", options).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();

        writer
            .start_file("META-INF/container.xml", options)
            .unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();

        let mut manifest_items = String::new();
        let mut spine_items = String::new();
        for i in 0..chapters {
            manifest_items.push_str(&format!(
                r#"<item id="c{i}" href="chapter{i}.xhtml" media-type="application/xhtml+xml"/>"#
            ));
            spine_items.push_str(&format!(r#"<itemref idref="c{i}"/>"#));
        }

        let opf = format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Synthetic Bench Book</dc:title>
    <dc:creator>Bench Author</dc:creator>
  </metadata>
  <manifest>{manifest_items}</manifest>
  <spine>{spine_items}</spine>
</package>"#
        );
        writer.start_file("OEBPS/content.opf", options).unwrap();
        writer.write_all(opf.as_bytes()).unwrap();

        // Same paragraph body reused for every chapter -- content realism
        // (headings + prose) matters more here than uniqueness per chapter.
        const WORDS_PER_PARAGRAPH: usize = 40;
        let paragraphs_needed = words_per_chapter.div_ceil(WORDS_PER_PARAGRAPH).max(1);
        let mut chapter_body = String::from("<h1>Chapter</h1>");
        for _ in 0..paragraphs_needed {
            chapter_body.push_str("<p>");
            for _ in 0..WORDS_PER_PARAGRAPH {
                chapter_body.push_str("lorem ");
            }
            chapter_body.push_str("</p>");
        }

        for i in 0..chapters {
            let xhtml = format!(
                r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body>{chapter_body}</body></html>"#
            );
            writer
                .start_file(format!("OEBPS/chapter{i}.xhtml"), options)
                .unwrap();
            writer.write_all(xhtml.as_bytes()).unwrap();
        }

        writer.finish().unwrap();
    }
    zip_bytes
}

fn bench_parse_epub(c: &mut Criterion) {
    let limits = ParseLimits::default();
    // 20 chapters * ~500 words/chapter ≈ a 20-page/10,000-word book, the
    // §9.1(a) target document size.
    let synthetic_20_chapter = build_synthetic_epub(20, 500);

    let mut group = c.benchmark_group("parse_epub");
    group.bench_function("multi_chapter_fixture_5ch", |b| {
        b.iter(|| gist_parse_epub::parse(MULTI_CHAPTER, "multi_chapter", &limits).unwrap())
    });
    group.bench_function("synthetic_20_chapter", |b| {
        b.iter(|| {
            gist_parse_epub::parse(&synthetic_20_chapter, "synthetic_20_chapter", &limits).unwrap()
        })
    });
    group.finish();
}

criterion_group!(benches, bench_parse_epub);
criterion_main!(benches);
