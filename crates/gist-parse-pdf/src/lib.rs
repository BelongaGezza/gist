//! PDF import for GIST (ADR-002).
//!
//! Layering:
//! - [`PdfParser`] is the swappable backend trait: given bytes and limits it
//!   streams per-page glyph data to a sink. The only implementation is
//!   [`PdfiumParser`] (pdfium, loaded dynamically at runtime).
//! - [`layout`] is pure, backend-independent analysis: reading order (column
//!   detection), header/footer/page-number stripping, heading inference from
//!   font size, paragraph reconstruction, `Document` assembly.
//!
//! Security posture (mirrors `gist-parse-epub`):
//! - `max_bytes` is checked before anything else; `max_pages` is checked as
//!   soon as the page count is known and **before any page is loaded**.
//! - Extracted text is bounded by `min(max_expanded_bytes, MAX_PDF_TEXT_BYTES)` incrementally, and a
//!   per-page glyph ceiling is checked against pdfium's reported character
//!   count *before* any per-character allocation.
//! - Password-protected / permission-restricted PDFs are rejected with
//!   [`PdfError::Encrypted`]; no password is ever tried or bypassed
//!   (same posture as ADR-004's DRM rule).
//! - A PDF with no extractable text yields [`PdfError::NoTextLayer`], the
//!   typed signal the app uses to route to OCR.
//! - A missing pdfium library is a typed [`PdfError::LibraryUnavailable`],
//!   never a panic. No `unsafe` in this crate.
//! - `max_nesting_depth` is not applicable: this crate does no recursive
//!   descent of its own (pdfium owns structure traversal).

#![forbid(unsafe_code)]

pub mod layout;
mod pdfium_backend;

use gist_model::{Document, Metadata, ParseLimits};

pub use layout::{Glyph, RawPage};
pub use pdfium_backend::{set_library_path, PdfiumParser};

/// Hard ceiling on glyphs read from a single page, independent of
/// `ParseLimits` (a legitimate page holds at most tens of thousands).
pub const MAX_GLYPHS_PER_PAGE: usize = 2_000_000;

/// PDF-specific ceiling on total extracted text, in bytes (F36), applied as
/// `min(limits.max_expanded_bytes, MAX_PDF_TEXT_BYTES)`.
///
/// Why a PDF-specific number: the global `max_expanded_bytes` (512 MiB) is
/// sized for zip expansion, but for a PDF the text is not the only cost -- it
/// is amplified into the `Document`'s word-token stream (~12-14 bytes of
/// resident memory per extracted byte) before anything is stored. Measured on
/// hostile text-dense synthetic PDFs (no real-document corpus exists, decision
/// D6): 48 M chars -> ~800 MiB peak, 242 M chars -> ~3.5 GiB, from inputs of
/// 30-160 KiB (a flate-compressed content stream shared by every page, so the
/// 256 MiB input cap gives no protection). 64 MiB bounds the parse itself to
/// roughly 1 GiB and still leaves ~5x headroom over a very text-dense
/// 2000-page book (~6000 chars/page = ~12 MB).
pub const MAX_PDF_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// Effective total-text budget for one PDF import.
pub fn text_budget(limits: &ParseLimits) -> usize {
    limits.max_expanded_bytes.min(MAX_PDF_TEXT_BYTES)
}

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("this PDF is password-protected or has restricted permissions and cannot be imported")]
    Encrypted,
    #[error("this PDF has no extractable text layer (scanned or image-only)")]
    NoTextLayer,
    #[error("PDF support is unavailable: {0}")]
    LibraryUnavailable(String),
    #[error("resource limit exceeded: {limit} ({attempted} attempted)")]
    ResourceLimitExceeded {
        limit: String,
        attempted: usize,
        kind: gist_model::LimitKind,
    },
    #[error("malformed PDF: {0}")]
    Malformed(String),
}

/// Document-level facts reported by a backend after streaming all pages.
#[derive(Debug, Clone, Default)]
pub struct RawMeta {
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Swappable PDF text-extraction backend (ADR-002).
pub trait PdfParser {
    /// Extract glyphs page by page, calling `sink` once per page in order.
    /// Implementations must enforce `limits.max_pages` before loading any
    /// page and stop at the first error returned by `sink`.
    fn extract(
        &self,
        bytes: &[u8],
        limits: &ParseLimits,
        sink: &mut dyn FnMut(RawPage) -> Result<(), PdfError>,
    ) -> Result<RawMeta, PdfError>;
}

/// Parse a PDF with the default (pdfium) backend.
pub fn parse_pdf(bytes: &[u8], stem: &str, limits: &ParseLimits) -> Result<Document, PdfError> {
    parse_pdf_with(&PdfiumParser::new(), bytes, stem, limits)
}

/// Parse a PDF with an explicit backend.
pub fn parse_pdf_with(
    parser: &dyn PdfParser,
    bytes: &[u8],
    stem: &str,
    limits: &ParseLimits,
) -> Result<Document, PdfError> {
    // 1. Size gate, before anything touches the bytes.
    if bytes.len() > limits.max_bytes {
        return Err(PdfError::ResourceLimitExceeded {
            limit: format!("max_bytes={}", limits.max_bytes),
            kind: gist_model::LimitKind::TooLarge,
            attempted: bytes.len(),
        });
    }
    // 2. Cheap magic check (PDF allows up to 1024 bytes of leading junk).
    let head = &bytes[..bytes.len().min(1024)];
    if !head.windows(5).any(|w| w == b"%PDF-") {
        return Err(PdfError::Malformed("missing %PDF- header".into()));
    }

    // 3. Stream pages -> lines, dropping each page's glyphs immediately.
    let mut pages = Vec::new();
    let mut text_bytes = 0usize;
    let meta = parser.extract(bytes, limits, &mut |page| {
        let lines = layout::page_to_lines(&page);
        for l in &lines.lines {
            text_bytes = text_bytes.saturating_add(l.text.len());
        }
        if text_bytes > text_budget(limits) {
            return Err(PdfError::ResourceLimitExceeded {
                limit: format!("pdf_text_bytes={}", text_budget(limits)),
                kind: gist_model::LimitKind::ExpandedTooLarge,
                attempted: text_bytes,
            });
        }
        pages.push(lines);
        Ok(())
    })?;

    // 4. Assemble.
    let title = meta
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| stem.to_string());
    let mut metadata = Metadata::minimal(title);
    metadata.author = meta
        .author
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty());
    metadata.source_type = "pdf".to_string();
    layout::build_document(pages, metadata, limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that never touches pdfium, for limit-ordering tests.
    struct Fake {
        pages: usize,
        loaded: std::cell::Cell<usize>,
    }
    impl PdfParser for Fake {
        fn extract(
            &self,
            _b: &[u8],
            limits: &ParseLimits,
            sink: &mut dyn FnMut(RawPage) -> Result<(), PdfError>,
        ) -> Result<RawMeta, PdfError> {
            if self.pages > limits.max_pages {
                return Err(PdfError::ResourceLimitExceeded {
                    limit: format!("max_pages={}", limits.max_pages),
                    kind: gist_model::LimitKind::TooManyPages,
                    attempted: self.pages,
                });
            }
            for _ in 0..self.pages {
                self.loaded.set(self.loaded.get() + 1);
                sink(RawPage {
                    width: 612.0,
                    height: 792.0,
                    glyphs: vec![],
                })?;
            }
            Ok(RawMeta::default())
        }
    }

    #[test]
    fn rejects_oversized_input_before_parsing() {
        let lim = ParseLimits {
            max_bytes: 10,
            ..ParseLimits::default()
        };
        let r = parse_pdf_with(
            &Fake {
                pages: 1,
                loaded: 0.into(),
            },
            &[b'x'; 11],
            "t",
            &lim,
        );
        assert!(matches!(r, Err(PdfError::ResourceLimitExceeded { .. })));
    }

    #[test]
    fn rejects_non_pdf_bytes_without_pdfium() {
        let r = parse_pdf(b"hello, not a pdf", "t", &ParseLimits::default());
        assert!(matches!(r, Err(PdfError::Malformed(_))), "{r:?}");
    }

    #[test]
    fn empty_input_is_malformed_not_panic() {
        assert!(matches!(
            parse_pdf(b"", "t", &ParseLimits::default()),
            Err(PdfError::Malformed(_))
        ));
    }

    #[test]
    fn page_count_bomb_rejected_before_any_page_loads() {
        let fake = Fake {
            pages: 5_000,
            loaded: 0.into(),
        };
        let r = parse_pdf_with(&fake, b"%PDF-1.4", "t", &ParseLimits::default());
        assert!(matches!(r, Err(PdfError::ResourceLimitExceeded { .. })));
        assert_eq!(fake.loaded.get(), 0);
    }

    #[test]
    fn zero_text_pages_are_no_text_layer() {
        let r = parse_pdf_with(
            &Fake {
                pages: 3,
                loaded: 0.into(),
            },
            b"%PDF-1.4",
            "t",
            &ParseLimits::default(),
        );
        assert!(matches!(r, Err(PdfError::NoTextLayer)));
    }
}
