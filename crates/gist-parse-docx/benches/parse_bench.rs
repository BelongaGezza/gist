//! Performance benchmarks for `gist-parse-docx::parse`, added for M4 role R2
//! (`docs/m4-agent-roles.md` §2) against product-spec §9.1's "20-page
//! document import+normalisation under 5s" target — DOCX is used as the
//! primary evidence for that target since `gist-parse-pdf` is still a stub
//! (see §2's R2 note in the plan).
//!
//! No 20-page DOCX fixture is checked into `fixtures/docx/` (all three
//! existing fixtures are small, correctness-focused — see
//! `fixtures/README.md`). `build_synthetic_docx` builds a minimal-but-valid
//! `word/document.xml` (the only zip entry `parse()` actually reads content
//! from beyond styles/numbering, which are optional and fall back to empty
//! when absent) with N realistic paragraphs, in code rather than as a
//! committed binary blob, so the bench input stays transparent/versioned.

use criterion::{criterion_group, criterion_main, Criterion};
use gist_model::ParseLimits;
use std::io::Write;
use zip::write::SimpleFileOptions;

const HEADINGS_AND_PARAGRAPHS: &[u8] =
    include_bytes!("../../../fixtures/docx/headings_and_paragraphs.docx");

/// Builds an in-memory DOCX whose `word/document.xml` has `paragraphs`
/// paragraphs, each with roughly `words_per_paragraph` words of run text.
fn build_synthetic_docx(paragraphs: usize, words_per_paragraph: usize) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..paragraphs {
        body.push_str("<w:p>");
        if i % 10 == 0 {
            // Every tenth paragraph is a heading, matching real documents'
            // structure more closely than an unbroken wall of body text.
            body.push_str(r#"<w:pPr><w:pStyle w:val="Heading1"/></w:pPr>"#);
        }
        body.push_str("<w:r><w:t>");
        for _ in 0..words_per_paragraph {
            body.push_str("lorem ");
        }
        body.push_str("</w:t></w:r></w:p>");
    }
    let document = format!(
        r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    let styles = r#"<?xml version="1.0"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>
</w:styles>"#;

    let mut zip_bytes = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut zip_bytes);
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        writer.start_file("word/styles.xml", options).unwrap();
        writer.write_all(styles.as_bytes()).unwrap();
        writer.start_file("word/document.xml", options).unwrap();
        writer.write_all(document.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    zip_bytes
}

fn bench_parse_docx(c: &mut Criterion) {
    let limits = ParseLimits::default();
    // 200 paragraphs * 50 words ≈ a 10,000-word / 20-page document, the
    // §9.1(a) target document size.
    let synthetic_20_page = build_synthetic_docx(200, 50);

    let mut group = c.benchmark_group("parse_docx");
    group.bench_function("headings_and_paragraphs_fixture", |b| {
        b.iter(|| {
            gist_parse_docx::parse(HEADINGS_AND_PARAGRAPHS, "headings_and_paragraphs", &limits)
                .unwrap()
        })
    });
    group.bench_function("synthetic_20_page", |b| {
        b.iter(|| gist_parse_docx::parse(&synthetic_20_page, "synthetic_20_page", &limits).unwrap())
    });
    group.finish();
}

criterion_group!(benches, bench_parse_docx);
criterion_main!(benches);
