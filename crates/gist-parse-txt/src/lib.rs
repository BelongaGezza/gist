use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};
use gist_model::{Block, Document, Metadata, ParseLimits, Section, TextRun};
use std::borrow::Cow;

/// Maximum bytes fed to the statistical detector. Detection quality does not
/// improve meaningfully past this, and it bounds the cost on huge files.
const DETECT_SAMPLE_BYTES: usize = 1 << 20;

/// Longest encoding label accepted from a caller (labels are short ASCII).
const MAX_LABEL_LEN: usize = 64;

/// Fewer non-ASCII bytes than this in the sample makes a legacy-code-page
/// guess "uncertain" (too little evidence to tell code pages apart).
const MIN_NON_ASCII_FOR_CONFIDENCE: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("utf-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("unsupported or unknown text encoding label")]
    UnknownEncoding,
    #[error("resource limit exceeded: {limit} ({attempted} bytes attempted)")]
    ResourceLimitExceeded {
        limit: String,
        attempted: usize,
        kind: gist_model::LimitKind,
    },
}

/// How the encoding was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingSource {
    /// A byte-order mark identified it (UTF-8, UTF-16LE or UTF-16BE).
    Bom,
    /// The bytes are valid UTF-8 (including pure ASCII).
    Utf8,
    /// Guessed from the bytes (UTF-16 without a BOM, or a legacy code page).
    Detected,
    /// The caller named the encoding explicitly.
    Override,
}

/// What encoding a text file was decoded with, and how sure we are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodingInfo {
    /// Canonical WHATWG name, e.g. `"UTF-8"`, `"UTF-16LE"`, `"windows-1252"`.
    pub name: String,
    pub source: EncodingSource,
    /// `true` when the guess is weak and a UI should offer the user a choice.
    pub uncertain: bool,
    /// `true` when some bytes were malformed for the encoding and were
    /// replaced with U+FFFD.
    pub lossy: bool,
}

/// Which encoding to decode with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingChoice<'a> {
    /// BOM, then UTF-16 / UTF-8 / legacy-code-page heuristics.
    Auto,
    /// A caller-named encoding (any WHATWG label, e.g. `"windows-1251"`).
    /// A BOM matching it is stripped, but never overrides the choice.
    Named(&'a str),
}

/// A parsed document together with the encoding it was decoded with.
#[derive(Debug)]
pub struct ParsedText {
    pub document: Document,
    pub encoding: EncodingInfo,
}

fn expanded_too_large(worst: usize, limits: &ParseLimits) -> ParseError {
    ParseError::ResourceLimitExceeded {
        limit: format!("max_expanded_bytes={}", limits.max_expanded_bytes),
        attempted: worst,
        kind: gist_model::LimitKind::ExpandedTooLarge,
    }
}

/// Heuristic for BOM-less UTF-16 of mostly-Latin text: one byte of each pair
/// is NUL. Returns `(encoding, confident)`.
fn sniff_utf16_without_bom(bytes: &[u8]) -> Option<(&'static Encoding, bool)> {
    let pairs = bytes.len() / 2;
    if pairs == 0 {
        return None;
    }
    let (mut even_nul, mut odd_nul) = (0usize, 0usize);
    for p in bytes.as_chunks::<2>().0 {
        even_nul += usize::from(p[0] == 0);
        odd_nul += usize::from(p[1] == 0);
    }
    let high = (pairs * 2).div_ceil(5); // >= 40% NULs in one position
    let low = pairs / 20; // < 5% in the other
    let enc = if odd_nul >= high && even_nul < low.max(1) {
        UTF_16LE
    } else if even_nul >= high && odd_nul < low.max(1) {
        UTF_16BE
    } else {
        return None;
    };
    Some((enc, pairs >= 4))
}

fn pick_encoding(bytes: &[u8]) -> (&'static Encoding, EncodingSource, bool) {
    if let Some((enc, _)) = Encoding::for_bom(bytes) {
        return (enc, EncodingSource::Bom, false);
    }
    if let Some((enc, confident)) = sniff_utf16_without_bom(bytes) {
        return (enc, EncodingSource::Detected, !confident);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return (UTF_8, EncodingSource::Utf8, false);
    }
    let sample = &bytes[..bytes.len().min(DETECT_SAMPLE_BYTES)];
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    det.feed(sample, sample.len() == bytes.len());
    // Invalid UTF-8 was ruled out above, so never guess UTF-8 here.
    let enc = det.guess(None, chardetng::Utf8Detection::Deny);
    // chardetng exposes no confidence score. Legacy code pages are inherently
    // ambiguous, so treat a guess as uncertain unless there is a reasonable
    // amount of non-ASCII evidence to base it on.
    let non_ascii = sample.iter().filter(|b| !b.is_ascii()).count();
    (
        enc,
        EncodingSource::Detected,
        non_ascii < MIN_NON_ASCII_FOR_CONFIDENCE,
    )
}

/// Decode `bytes` per `choice`, enforcing `limits` before allocating.
fn decode<'a>(
    bytes: &'a [u8],
    choice: EncodingChoice<'_>,
    limits: &ParseLimits,
) -> Result<(Cow<'a, str>, EncodingInfo), ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::ResourceLimitExceeded {
            limit: format!("max_bytes={}", limits.max_bytes),
            attempted: bytes.len(),
            kind: gist_model::LimitKind::TooLarge,
        });
    }

    let (enc, source, uncertain) = match choice {
        EncodingChoice::Auto => pick_encoding(bytes),
        EncodingChoice::Named(label) => {
            let found = if label.len() > MAX_LABEL_LEN {
                None
            } else {
                Encoding::for_label(label.trim().as_bytes())
            };
            match found {
                // `replacement` is not a real text encoding.
                Some(e) if e.name() != "replacement" => (e, EncodingSource::Override, false),
                _ => return Err(ParseError::UnknownEncoding),
            }
        }
    };

    let (text, lossy) = if enc == UTF_8 {
        // Allocation-free path for valid UTF-8 (text unchanged, BOM dropped).
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
        match std::str::from_utf8(body) {
            Ok(s) => (Cow::Borrowed(s), false),
            Err(_) => {
                // Each malformed byte may become a 3-byte U+FFFD.
                let worst = body.len().saturating_mul(3);
                if worst > limits.max_expanded_bytes {
                    return Err(expanded_too_large(worst, limits));
                }
                (UTF_8.decode_without_bom_handling(body).0, true)
            }
        }
    } else {
        // Worst-case UTF-8 output size, checked before the buffer exists.
        let worst = enc
            .new_decoder_without_bom_handling()
            .max_utf8_buffer_length(bytes.len())
            .unwrap_or(usize::MAX);
        if worst > limits.max_expanded_bytes {
            return Err(expanded_too_large(worst, limits));
        }
        let (c, had_errors) = enc.decode_with_bom_removal(bytes);
        (c, had_errors)
    };

    Ok((
        text,
        EncodingInfo {
            name: enc.name().to_string(),
            source,
            uncertain,
            lossy,
        },
    ))
}

/// Report the encoding `parse` would use for `bytes` (or the named one),
/// without building a document. Applies the same limits as `parse`.
pub fn detect_encoding(
    bytes: &[u8],
    choice: EncodingChoice<'_>,
    limits: &ParseLimits,
) -> Result<EncodingInfo, ParseError> {
    decode(bytes, choice, limits).map(|(_, info)| info)
}

/// Parse a raw `.txt` byte slice into a [`Document`], auto-detecting the
/// encoding (BOM, UTF-8, UTF-16, legacy single-byte code pages).
///
/// `stem` is used as the document title.
/// `limits` must be checked before significant allocation.
pub fn parse(bytes: &[u8], stem: &str, limits: &ParseLimits) -> Result<Document, ParseError> {
    parse_with_encoding(bytes, stem, limits, EncodingChoice::Auto).map(|p| p.document)
}

/// Like [`parse`], but lets the caller name the encoding and also returns the
/// encoding that was used (with its confidence) so a UI can prompt.
pub fn parse_with_encoding(
    bytes: &[u8],
    stem: &str,
    limits: &ParseLimits,
    choice: EncodingChoice<'_>,
) -> Result<ParsedText, ParseError> {
    let (text, encoding) = decode(bytes, choice, limits)?;
    Ok(ParsedText {
        document: build_document(&text, stem),
        encoding,
    })
}

fn build_document(text: &str, stem: &str) -> Document {
    // Normalise line endings before splitting, so a blank-line paragraph
    // break is recognised regardless of how the source file spells one:
    // CRLF (Windows), bare CR (classic Mac / Word's paragraph mark), or
    // U+2028 LINE SEPARATOR. U+2029 PARAGRAPH SEPARATOR is treated as an
    // explicit paragraph break in its own right, not just a line break.
    let normalized = text
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}'], "\n")
        .replace('\u{2029}', "\n\n");

    // Group consecutive non-blank lines into paragraphs; a line containing
    // only whitespace (not just an empty line) ends the current paragraph.
    // Lines within a paragraph are rejoined with "\n" so single line breaks
    // inside a paragraph are preserved, matching the previous split("\n\n")
    // behaviour for that case.
    let mut paragraphs: Vec<Block> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in normalized.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(paragraph_from_lines(&current));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(paragraph_from_lines(&current));
    }

    let blocks = if paragraphs.is_empty() {
        vec![Block::Paragraph {
            runs: vec![TextRun::plain("")],
        }]
    } else {
        paragraphs
    };

    let section = Section {
        id: "s0".to_string(),
        heading: None,
        blocks,
    };

    let metadata = Metadata {
        title: stem.to_string(),
        author: None,
        source_type: "txt".to_string(),
        source_ref: None,
        source_copy_ref: None,
        import_date: None,
        language: None,
        word_count: 0, // Document::new recomputes this
    };

    Document::new(metadata, vec![section])
}

fn paragraph_from_lines(lines: &[&str]) -> Block {
    let text = lines.join("\n");
    Block::Paragraph {
        runs: vec![TextRun::plain(text.trim())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraphs_of(bytes: &[u8]) -> Vec<String> {
        let doc = parse(bytes, "test", &ParseLimits::default()).expect("parse should succeed");
        doc.sections[0]
            .blocks
            .iter()
            .map(|b| b.plain_text())
            .collect()
    }

    fn info(bytes: &[u8]) -> EncodingInfo {
        detect_encoding(bytes, EncodingChoice::Auto, &ParseLimits::default()).unwrap()
    }

    fn utf16(s: &str, le: bool, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend(if le { [0xFF, 0xFE] } else { [0xFE, 0xFF] });
        }
        for u in s.encode_utf16() {
            out.extend(if le { u.to_le_bytes() } else { u.to_be_bytes() });
        }
        out
    }

    #[test]
    fn splits_on_lf_blank_line() {
        assert_eq!(
            paragraphs_of(b"First para.\n\nSecond para."),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn splits_on_crlf_blank_line() {
        assert_eq!(
            paragraphs_of(b"First para.\r\n\r\nSecond para."),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn splits_on_bare_cr_blank_line() {
        assert_eq!(
            paragraphs_of(b"First para.\r\rSecond para."),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn splits_on_u2029_paragraph_separator() {
        assert_eq!(
            paragraphs_of("First para.\u{2029}Second para.".as_bytes()),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn splits_on_whitespace_only_blank_line() {
        assert_eq!(
            paragraphs_of(b"First para.\n \nSecond para."),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn splits_on_tab_only_blank_line() {
        assert_eq!(
            paragraphs_of(b"First para.\n\t\nSecond para."),
            vec!["First para.", "Second para."]
        );
    }

    #[test]
    fn u2028_line_separator_stays_within_one_paragraph() {
        assert_eq!(
            paragraphs_of("First line.\u{2028}Second line.".as_bytes()),
            vec!["First line.\nSecond line."]
        );
    }

    #[test]
    fn single_newline_within_a_paragraph_is_preserved() {
        assert_eq!(
            paragraphs_of(b"Line one.\nLine two.\n\nSecond para."),
            vec!["Line one.\nLine two.", "Second para."]
        );
    }

    #[test]
    fn empty_input_yields_one_empty_paragraph() {
        assert_eq!(paragraphs_of(b""), vec![""]);
    }

    #[test]
    fn whitespace_only_input_yields_one_empty_paragraph() {
        assert_eq!(paragraphs_of(b"   \n\t\n  "), vec![""]);
    }

    #[test]
    fn mixed_crlf_and_lf_in_same_file() {
        assert_eq!(
            paragraphs_of(b"First.\r\n\r\nSecond.\n\nThird."),
            vec!["First.", "Second.", "Third."]
        );
    }

    // ── Encoding detection (SC-01) ─────────────────────────────────────────

    #[test]
    fn plain_utf8_is_confident_and_unchanged() {
        let i = info("Héllo wörld — 日本語".as_bytes());
        assert_eq!(i.name, "UTF-8");
        assert_eq!(i.source, EncodingSource::Utf8);
        assert!(!i.uncertain && !i.lossy);
        assert_eq!(
            paragraphs_of("Héllo wörld — 日本語".as_bytes()),
            vec!["Héllo wörld — 日本語"]
        );
    }

    #[test]
    fn empty_input_is_utf8_and_confident() {
        let i = info(b"");
        assert_eq!(i.name, "UTF-8");
        assert!(!i.uncertain);
    }

    #[test]
    fn utf8_bom_is_detected_and_stripped() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend("Hello".as_bytes());
        let i = info(&b);
        assert_eq!((i.name.as_str(), i.source), ("UTF-8", EncodingSource::Bom));
        assert_eq!(paragraphs_of(&b), vec!["Hello"]);
    }

    #[test]
    fn utf16_le_and_be_with_bom() {
        for le in [true, false] {
            let b = utf16("Hello\n\nWörld 日本", le, true);
            let i = info(&b);
            assert_eq!(i.name, if le { "UTF-16LE" } else { "UTF-16BE" });
            assert_eq!(i.source, EncodingSource::Bom);
            assert!(!i.uncertain);
            assert_eq!(paragraphs_of(&b), vec!["Hello", "Wörld 日本"]);
        }
    }

    #[test]
    fn utf16_without_bom_latin_text_is_sniffed() {
        for le in [true, false] {
            let b = utf16("The quick brown fox jumps.\n\nSecond paragraph.", le, false);
            let i = info(&b);
            assert_eq!(i.name, if le { "UTF-16LE" } else { "UTF-16BE" });
            assert_eq!(i.source, EncodingSource::Detected);
            assert!(!i.uncertain);
            assert_eq!(
                paragraphs_of(&b),
                vec!["The quick brown fox jumps.", "Second paragraph."]
            );
        }
    }

    #[test]
    fn tiny_bomless_utf16_is_flagged_uncertain() {
        let i = info(&utf16("A", true, false));
        assert_eq!(i.name, "UTF-16LE");
        assert!(i.uncertain);
    }

    #[test]
    fn legacy_windows_1252_is_decoded() {
        // "café – “quoted”" in windows-1252 (0x96 en dash, 0x93/0x94 quotes).
        let b =
            b"Le caf\xE9 \x96 \x93bonjour\x94, dit-il. Voil\xE0 une longue phrase fran\xE7aise.";
        let i = info(b);
        assert_eq!(i.source, EncodingSource::Detected);
        assert!(!i.lossy);
        let text = paragraphs_of(b).remove(0);
        assert!(text.contains("café"), "{text}");
        assert!(text.contains('\u{2013}'), "{text}");
    }

    #[test]
    fn sparse_legacy_evidence_is_uncertain_and_rich_evidence_is_not() {
        assert!(info(b"caf\xE9").uncertain);
        let rich = b"Voil\xE0 une longue phrase fran\xE7aise avec beaucoup d'accents \xE9\xE8\xEA\xEB \xE0\xE2 \xF4\xFB \xE7 et encore \xE9t\xE9, \xE0 l'\xE9cole d'\xE9t\xE9 pr\xE8s de l'\xEEle.";
        assert!(!info(rich).uncertain);
    }

    #[test]
    fn invalid_utf8_never_errors_and_is_never_utf8_source() {
        let b = [b'a', 0xFF, 0xFE, 0xFD, b'b', 0x80, 0x81, b'\n'];
        let p = parse_with_encoding(&b, "t", &ParseLimits::default(), EncodingChoice::Auto)
            .expect("must not fail");
        assert_ne!(p.encoding.source, EncodingSource::Utf8);
    }

    #[test]
    fn override_names_an_encoding_and_wins_over_detection() {
        // KOI8-R bytes for "Привет" ; forcing windows-1251 yields different text.
        let koi8: &[u8] = &[0xF0, 0xD2, 0xC9, 0xD7, 0xC5, 0xD4];
        let p = parse_with_encoding(
            koi8,
            "t",
            &ParseLimits::default(),
            EncodingChoice::Named("koi8-r"),
        )
        .unwrap();
        assert_eq!(p.encoding.name, "KOI8-R");
        assert_eq!(p.encoding.source, EncodingSource::Override);
        assert!(!p.encoding.uncertain);
        assert_eq!(p.document.sections[0].blocks[0].plain_text(), "Привет");
    }

    #[test]
    fn override_strips_matching_bom_but_ignores_it_for_choice() {
        let b = utf16("Hi", true, true);
        let p = parse_with_encoding(
            &b,
            "t",
            &ParseLimits::default(),
            EncodingChoice::Named("utf-16le"),
        )
        .unwrap();
        assert_eq!(p.document.sections[0].blocks[0].plain_text(), "Hi");
    }

    #[test]
    fn override_on_malformed_bytes_is_lossy_not_an_error() {
        let p = parse_with_encoding(
            &[b'a', 0xFF, b'b'],
            "t",
            &ParseLimits::default(),
            EncodingChoice::Named("utf-8"),
        )
        .unwrap();
        assert!(p.encoding.lossy);
        assert_eq!(p.document.sections[0].blocks[0].plain_text(), "a\u{FFFD}b");
    }

    #[test]
    fn unknown_or_hostile_labels_are_rejected() {
        let l = ParseLimits::default();
        for label in ["", "nope", "replacement", &"x".repeat(10_000)] {
            assert!(matches!(
                parse_with_encoding(b"abc", "t", &l, EncodingChoice::Named(label)),
                Err(ParseError::UnknownEncoding)
            ));
        }
    }

    #[test]
    fn mixed_valid_utf8_then_legacy_bytes_does_not_panic() {
        let mut b = "Grüße ".as_bytes().to_vec();
        b.extend([0xE9, 0xE8, 0xE0, 0x20]);
        let p = parse_with_encoding(&b, "t", &ParseLimits::default(), EncodingChoice::Auto)
            .expect("lossy or guessed, but never an error");
        assert!(!p.document.sections.is_empty());
    }

    #[test]
    fn max_bytes_is_enforced_before_decoding() {
        let limits = ParseLimits {
            max_bytes: 10,
            ..ParseLimits::default()
        };
        let err = parse(&[b'a'; 11], "t", &limits).unwrap_err();
        assert!(matches!(
            err,
            ParseError::ResourceLimitExceeded {
                kind: gist_model::LimitKind::TooLarge,
                ..
            }
        ));
    }

    #[test]
    fn expansion_budget_is_enforced_before_decoding_legacy_text() {
        let limits = ParseLimits {
            max_expanded_bytes: 100,
            ..ParseLimits::default()
        };
        // 50 windows-1252 bytes could expand to 150 UTF-8 bytes in the worst case.
        let r = parse_with_encoding(
            &[b'a'; 50],
            "t",
            &limits,
            EncodingChoice::Named("windows-1252"),
        );
        assert!(matches!(
            r,
            Err(ParseError::ResourceLimitExceeded {
                kind: gist_model::LimitKind::ExpandedTooLarge,
                ..
            })
        ));
        // Valid UTF-8 never expands, so the same budget does not reject it.
        assert!(parse(&[b'a'; 50], "t", &limits).is_ok());
    }

    #[test]
    fn single_bom_only_files_are_empty_documents() {
        assert_eq!(paragraphs_of(&[0xEF, 0xBB, 0xBF]), vec![""]);
        assert_eq!(paragraphs_of(&[0xFF, 0xFE]), vec![""]);
    }

    #[test]
    fn odd_length_utf16_tail_does_not_panic() {
        let mut b = utf16("Hello world", true, true);
        b.push(0x41);
        assert!(parse(&b, "t", &ParseLimits::default()).is_ok());
    }

    #[test]
    fn arbitrary_binary_never_panics() {
        let mut x: u32 = 12345;
        for len in [1usize, 2, 3, 7, 64, 1000] {
            let b: Vec<u8> = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                    (x >> 24) as u8
                })
                .collect();
            assert!(parse(&b, "t", &ParseLimits::default()).is_ok());
        }
    }
}
