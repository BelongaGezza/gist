use serde::{Deserialize, Serialize};

// ── Metadata ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub author: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub import_date: Option<String>,
    pub language: Option<String>,
    pub word_count: u32,
}

impl Metadata {
    pub fn minimal(title: impl Into<String>) -> Self {
        Metadata {
            title: title.into(),
            author: None,
            source_type: String::new(),
            source_ref: None,
            import_date: None,
            language: None,
            word_count: 0,
        }
    }
}

// ── Block / Section ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

impl TextRun {
    pub fn plain(text: impl Into<String>) -> Self {
        TextRun {
            text: text.into(),
            bold: false,
            italic: false,
            code: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        runs: Vec<TextRun>,
    },
    Image {
        src: String,
        alt: Option<String>,
        caption: Option<String>,
    },
    List {
        ordered: bool,
        items: Vec<String>,
    },
}

impl Block {
    pub fn plain_text(&self) -> String {
        match self {
            Block::Heading { text, .. } => text.clone(),
            Block::Paragraph { runs } => runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join(""),
            Block::Image { alt, .. } => alt.clone().unwrap_or_default(),
            Block::List { items, .. } => items.join(" "),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub heading: Option<(u8, String)>,
    pub blocks: Vec<Block>,
}

// ── Token stream ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    ParagraphBreak,
    SectionBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub text: String,
    pub kind: TokenKind,
    pub section_idx: usize,
    pub block_idx: usize,
    pub char_offset: usize,
}

// ── Document ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub metadata: Metadata,
    pub sections: Vec<Section>,
    pub token_stream: Vec<Token>,
}

impl Document {
    pub fn new(metadata: Metadata, sections: Vec<Section>) -> Self {
        let id = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            format!("{ns:016x}")
        };
        let mut doc = Document {
            id,
            metadata,
            sections,
            token_stream: Vec::new(),
        };
        doc.token_stream = doc.build_token_stream();
        let word_count = doc
            .token_stream
            .iter()
            .filter(|t| t.kind == TokenKind::Word)
            .count();
        doc.metadata.word_count = word_count as u32;
        doc
    }

    /// Build the flat token stream from all sections and blocks.
    /// Words are split on whitespace; paragraph/section break tokens are inserted between blocks.
    pub fn build_token_stream(&self) -> Vec<Token> {
        let mut tokens = Vec::new();
        for (si, section) in self.sections.iter().enumerate() {
            if si > 0 {
                tokens.push(Token {
                    text: String::new(),
                    kind: TokenKind::SectionBreak,
                    section_idx: si,
                    block_idx: 0,
                    char_offset: 0,
                });
            }
            for (bi, block) in section.blocks.iter().enumerate() {
                if bi > 0 {
                    tokens.push(Token {
                        text: String::new(),
                        kind: TokenKind::ParagraphBreak,
                        section_idx: si,
                        block_idx: bi,
                        char_offset: 0,
                    });
                }
                let plain = block.plain_text();
                let mut search_from = 0usize;
                for word in plain.split_whitespace() {
                    let pos = plain[search_from..].find(word).unwrap_or(0);
                    let char_offset = search_from + pos;
                    tokens.push(Token {
                        text: word.to_string(),
                        kind: TokenKind::Word,
                        section_idx: si,
                        block_idx: bi,
                        char_offset,
                    });
                    search_from = char_offset + word.len();
                }
            }
        }
        tokens
    }
}
