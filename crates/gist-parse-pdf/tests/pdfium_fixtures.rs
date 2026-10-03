//! End-to-end tests against real pdfium and the synthetic fixtures.
//!
//! These need `libpdfium` (run `tools/fetch-pdfium.sh`; macOS only today).
//! Where the library is unavailable (Linux/Windows CI, or fetch not run) each
//! test prints a notice and returns - unless `GIST_REQUIRE_PDFIUM=1`, which
//! turns "library missing" into a hard failure so a macOS gate can't pass by
//! silently skipping.

use std::path::PathBuf;

use gist_model::{Block, ParseLimits};
use gist_parse_pdf::{parse_pdf, PdfError};

fn fixture(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf")
        .join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Returns None (skip) when pdfium isn't loadable.
fn run(rel: &str, limits: &ParseLimits) -> Option<Result<gist_model::Document, PdfError>> {
    let r = parse_pdf(&fixture(rel), "fixture", limits);
    if let Err(PdfError::LibraryUnavailable(why)) = &r {
        if std::env::var("GIST_REQUIRE_PDFIUM").as_deref() == Ok("1") {
            panic!("pdfium required but unavailable: {why}");
        }
        eprintln!("SKIP {rel}: pdfium unavailable ({why})");
        return None;
    }
    Some(r)
}

fn text(d: &gist_model::Document) -> String {
    d.sections
        .iter()
        .flat_map(|s| &s.blocks)
        .map(Block::plain_text)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn plain_text_extracts_headings_paragraphs_and_strips_furniture() {
    let Some(r) = run("plain_text.pdf", &ParseLimits::default()) else {
        return;
    };
    let d = r.expect("plain_text.pdf must parse");
    assert_eq!(d.metadata.title, "A Short Synthetic Report");
    assert_eq!(d.metadata.author.as_deref(), Some("GIST Fixtures"));
    assert_eq!(d.metadata.source_type, "pdf");
    let t = text(&d);
    assert!(t.contains("A Short Synthetic Report"), "{t}");
    assert!(t.contains("Section 2"), "{t}");
    assert!(t.contains("steady pace lets the eye settle"), "{t}");
    // Running header on every page and bare page numbers are stripped.
    assert!(
        !t.contains("GIST Synthetic Report"),
        "header not stripped: {t}"
    );
    assert!(
        !t.lines().any(|l| matches!(l.trim(), "1" | "2" | "3")),
        "page numbers not stripped: {t}"
    );
    assert!(d.sections.iter().any(|s| s.heading.is_some()));
    assert!(d.metadata.word_count > 100);
}

#[test]
fn two_column_reads_left_column_before_right() {
    let Some(r) = run("two_column.pdf", &ParseLimits::default()) else {
        return;
    };
    let t = text(&r.expect("two_column.pdf must parse"));
    let right_start = t
        .find("Right column text begins here")
        .expect("right col text");
    // The whole left column (BODY twice) must be read before the right
    // column's first line.
    let before = &t[..right_start];
    assert_eq!(
        before.matches("get in the way of that rhythm.").count(),
        2,
        "columns interleaved:\n{t}"
    );
}

#[test]
fn image_only_is_no_text_layer() {
    let Some(r) = run("image_only.pdf", &ParseLimits::default()) else {
        return;
    };
    assert!(matches!(r, Err(PdfError::NoTextLayer)), "{r:?}");
}

#[test]
fn password_protected_is_encrypted_error() {
    let Some(r) = run(
        "adversarial/encrypted_password.pdf",
        &ParseLimits::default(),
    ) else {
        return;
    };
    assert!(matches!(r, Err(PdfError::Encrypted)), "{r:?}");
}

#[test]
fn truncated_and_garbage_are_malformed_or_recovered_never_panic() {
    for f in [
        "adversarial/truncated.pdf",
        "adversarial/garbage_after_header.pdf",
    ] {
        let Some(r) = run(f, &ParseLimits::default()) else {
            return;
        };
        match r {
            // pdfium may legitimately recover text from a truncated file.
            Ok(_) | Err(PdfError::Malformed(_)) | Err(PdfError::NoTextLayer) => {}
            other => panic!("{f}: unexpected {other:?}"),
        }
    }
}

#[test]
fn page_count_bomb_rejected_by_max_pages() {
    let Some(r) = run("adversarial/page_count_bomb.pdf", &ParseLimits::default()) else {
        return;
    };
    assert!(
        matches!(r, Err(PdfError::ResourceLimitExceeded { .. })),
        "{r:?}"
    );
}

#[test]
fn absurd_declared_page_count_does_not_hang_or_allocate() {
    let Some(r) = run(
        "adversarial/huge_declared_count.pdf",
        &ParseLimits::default(),
    ) else {
        return;
    };
    match r {
        Ok(_)
        | Err(PdfError::ResourceLimitExceeded { .. })
        | Err(PdfError::Malformed(_))
        | Err(PdfError::NoTextLayer) => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn expanded_text_budget_applies_to_real_extraction() {
    let lim = ParseLimits {
        max_expanded_bytes: 200,
        ..ParseLimits::default()
    };
    let Some(r) = run("plain_text.pdf", &lim) else {
        return;
    };
    assert!(
        matches!(r, Err(PdfError::ResourceLimitExceeded { .. })),
        "{r:?}"
    );
}

#[test]
fn max_bytes_checked_before_pdfium() {
    let lim = ParseLimits {
        max_bytes: 100,
        ..ParseLimits::default()
    };
    // Does not need pdfium at all.
    let r = parse_pdf(&fixture("plain_text.pdf"), "x", &lim);
    assert!(matches!(r, Err(PdfError::ResourceLimitExceeded { .. })));
}
