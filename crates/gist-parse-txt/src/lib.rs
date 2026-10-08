use gist_model::{Block, Document, Metadata, ParseLimits, Section, TextRun};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("utf-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("resource limit exceeded: {limit} ({attempted} bytes attempted)")]
    ResourceLimitExceeded {
        limit: String,
        attempted: usize,
        kind: gist_model::LimitKind,
    },
}

/// Parse a raw `.txt` byte slice into a [`Document`].
///
/// `stem` is used as the document title.
/// `limits` must be checked before significant allocation.
pub fn parse(bytes: &[u8], stem: &str, limits: &ParseLimits) -> Result<Document, ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::ResourceLimitExceeded {
            limit: format!("max_bytes={}", limits.max_bytes),
            kind: gist_model::LimitKind::TooLarge,
            attempted: bytes.len(),
        });
    }

    let text = std::str::from_utf8(bytes)?;

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

    Ok(Document::new(metadata, vec![section]))
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
}
