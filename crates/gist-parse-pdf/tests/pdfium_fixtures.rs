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
fn huge_string_object_is_bounded_by_limits_or_pdfium() {
    let Some(r) = run(
        "adversarial/huge_string_object.pdf",
        &ParseLimits::default(),
    ) else {
        return;
    };
    // A 3M-character string in a few KB of Flate data. Either our per-page
    // glyph ceiling trips, or pdfium itself bounds what it surfaces; in the
    // latter case the extracted text must still respect the budget. What must
    // never happen is a panic or unbounded growth.
    match r {
        Err(PdfError::ResourceLimitExceeded { .. }) => {}
        Ok(d) => assert!(text(&d).len() <= ParseLimits::default().max_expanded_bytes),
        other => panic!("unexpected {other:?}"),
    }
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

// ── F36: PDF-specific text budget ────────────────────────────────────────────

/// Builds an in-memory PDF of `pages` pages that all share one uncompressed
/// content stream of `lines` lines x `cpl` characters (so the file is about
/// `lines * cpl` bytes, not `pages` times that).
fn text_heavy_pdf(pages: usize, lines: usize, cpl: usize, font_size: f32) -> Vec<u8> {
    let line: String = "lorem ipsum "
        .chars()
        .cycle()
        .take(cpl)
        .collect::<String>()
        .trim_end()
        .to_string();
    let mut content = format!("BT /F1 {font_size} Tf {} TL 20 780 Td\n", font_size * 1.05);
    for _ in 0..lines {
        content.push_str(&format!("({line}) Tj T*\n"));
    }
    content.push_str("ET");
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            (0..pages)
                .map(|k| format!("{} 0 R", 5 + k))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
    ];
    for _ in 0..pages {
        objs.push(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
              /Resources << /Font << /F1 3 0 R >> >> >>"
                .to_vec(),
        );
    }
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (n, body) in objs.iter().enumerate() {
        offs.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", n + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    buf
}

fn run_bytes(bytes: &[u8], limits: &ParseLimits) -> Option<Result<gist_model::Document, PdfError>> {
    let r = parse_pdf(bytes, "generated", limits);
    if let Err(PdfError::LibraryUnavailable(why)) = &r {
        if std::env::var("GIST_REQUIRE_PDFIUM").as_deref() == Ok("1") {
            panic!("pdfium required but unavailable: {why}");
        }
        eprintln!("SKIP generated pdf: pdfium unavailable ({why})");
        return None;
    }
    Some(r)
}

#[test]
fn pdf_text_budget_is_pdf_specific_and_never_exceeds_the_global_limit() {
    use gist_parse_pdf::{text_budget, MAX_PDF_TEXT_BYTES};
    let d = ParseLimits::default();
    assert!(MAX_PDF_TEXT_BYTES < d.max_expanded_bytes);
    assert_eq!(text_budget(&d), MAX_PDF_TEXT_BYTES);
    let small = ParseLimits {
        max_expanded_bytes: 1000,
        ..ParseLimits::default()
    };
    assert_eq!(text_budget(&small), 1000);
    // Headroom: a very text-dense 2000-page book (6000 chars/page, even at
    // 4 bytes per char) is far below the budget.
    const { assert!(2000 * 6000 * 4 < MAX_PDF_TEXT_BYTES) };
}

#[test]
fn text_over_the_budget_is_rejected_with_a_typed_limit_error() {
    // 4 pages x (200 lines x 200 chars) = 160 000 chars, budget 50 000.
    let pdf = text_heavy_pdf(4, 200, 200, 4.0);
    let lim = ParseLimits {
        max_expanded_bytes: 50_000,
        ..ParseLimits::default()
    };
    let Some(r) = run_bytes(&pdf, &lim) else {
        return;
    };
    match r {
        Err(PdfError::ResourceLimitExceeded { kind, .. }) => {
            assert_eq!(kind, gist_model::LimitKind::ExpandedTooLarge)
        }
        other => panic!("expected a limit error, got {other:?}"),
    }
}

#[test]
fn text_under_the_budget_still_imports_in_full() {
    // 3 pages x (50 lines x 70 chars): an ordinary text-dense document.
    let pdf = text_heavy_pdf(3, 50, 70, 10.0);
    let Some(r) = run_bytes(&pdf, &ParseLimits::default()) else {
        return;
    };
    let d = r.expect("an ordinary document must still import");
    assert!(text(&d).len() > 3 * 50 * 60, "text unexpectedly short");
}

#[test]
fn page_count_bomb_reports_the_too_many_pages_kind() {
    let Some(r) = run("adversarial/page_count_bomb.pdf", &ParseLimits::default()) else {
        return;
    };
    match r {
        Err(PdfError::ResourceLimitExceeded { kind, .. }) => {
            assert_eq!(kind, gist_model::LimitKind::TooManyPages)
        }
        other => panic!("expected a limit error, got {other:?}"),
    }
}
