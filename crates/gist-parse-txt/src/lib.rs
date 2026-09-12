use gist_model::{Block, Document, Metadata, ParseLimits, Section, TextRun};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("utf-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("resource limit exceeded: {limit} ({attempted} bytes attempted)")]
    ResourceLimitExceeded { limit: String, attempted: usize },
}

/// Parse a raw `.txt` byte slice into a [`Document`].
///
/// `stem` is used as the document title.
/// `limits` must be checked before significant allocation.
pub fn parse(bytes: &[u8], stem: &str, limits: &ParseLimits) -> Result<Document, ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::ResourceLimitExceeded {
            limit: format!("max_bytes={}", limits.max_bytes),
            attempted: bytes.len(),
        });
    }

    let text = std::str::from_utf8(bytes)?;

    // Split into paragraphs on blank lines.
    let paragraphs: Vec<Block> = text
        .split("\n\n")
        .map(|para| para.trim())
        .filter(|para| !para.is_empty())
        .map(|para| Block::Paragraph {
            runs: vec![TextRun::plain(para)],
        })
        .collect();

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
