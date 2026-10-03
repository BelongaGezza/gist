use serde::{Deserialize, Serialize};

// ── Resource limits ────────────────────────────────────────────────────────

/// Shared resource-limit policy enforced by all parsers before allocation.
/// Parsers receive this struct at entry and must check each limit before the
/// corresponding allocation. Exceeding any limit returns
/// `ParseError::ResourceLimitExceeded` without reading further input.
#[derive(Debug, Clone)]
pub struct ParseLimits {
    /// Maximum input file size in bytes (default 256 MB).
    pub max_bytes: usize,
    /// Maximum page / spine-item count (default 2 000).
    pub max_pages: usize,
    /// Maximum XML element nesting depth for DOCX/ePub (default 200).
    pub max_nesting_depth: usize,
    /// Maximum decompressed bytes for zip-based formats during streaming
    /// decompression (default 512 MB). Enforced *before* allocating the
    /// full buffer — provides zip-bomb protection.
    pub max_expanded_bytes: usize,
    /// Maximum number of entries in a zip-based archive (epub/docx), checked
    /// immediately after opening the archive — before any entry's content is
    /// read (default 10 000). Bounds central-directory parsing cost, which
    /// `max_bytes` (the archive's compressed size) doesn't: a crafted archive
    /// with a huge number of near-empty entries can stay well under
    /// `max_bytes` while still being expensive to enumerate.
    pub max_zip_entries: usize,
    /// Maximum rows in one table block (default 2 000). Enforced by the
    /// DOCX/ePub/web parsers *before* a row is appended, so a hostile
    /// table can't make a parser allocate unbounded row vectors.
    pub max_table_rows: usize,
    /// Maximum columns (cells) in any one table row (default 64).
    pub max_table_cols: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        ParseLimits {
            max_bytes: 256 * 1024 * 1024,
            max_pages: 2_000,
            max_nesting_depth: 200,
            max_expanded_bytes: 512 * 1024 * 1024,
            max_zip_entries: 10_000,
            max_table_rows: 2_000,
            max_table_cols: 64,
        }
    }
}

// ── Parse error (shared across image/doc pre-processors) ───────────────────

/// Errors returned by format-specific parsers and pre-processors.
///
/// Lives here (not in `gist-core`, where it originally shipped) so that
/// `gist-imageprep` — which needs this type for `prepare_image`'s `Result`
/// — can depend on it without creating a `gist-imageprep -> gist-core ->
/// gist-imageprep` cycle now that `gist-core::import_image_with_ocr` calls
/// `gist_imageprep::prepare_image` directly (2026-09-26, role R2b). Moved
/// rather than re-implemented, mirroring the `ParseLimits` re-export
/// pattern `gist-core` already uses for exactly this reason.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("resource limit exceeded")]
    ResourceLimitExceeded,
}

// ── Metadata ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub author: Option<String>,
    pub source_type: String,
    /// The original import location (filesystem path or URL), kept for
    /// display/provenance only ("Imported from ~/Downloads/book.epub").
    /// Per ADR-006, this is never used to read or delete files — that's
    /// `source_copy_ref`'s job.
    pub source_ref: Option<String>,
    /// Path to a sandboxed, content-addressed copy of the originally
    /// imported file (ADR-006), made at import time so the app never needs
    /// to read from or delete a location outside its own storage directory.
    /// `None` for URL imports (there is no local file to copy) and for
    /// documents imported before this field existed.
    #[serde(default)]
    pub source_copy_ref: Option<String>,
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
            source_copy_ref: None,
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
    /// A rectangular-ish grid of text cells (ADR-019 addendum, M6/R3).
    ///
    /// Cells are **plain text, not runs**: bold/italic inside a table cell
    /// is dropped. Rationale: every consumer that matters (flow-view grid,
    /// RSVP/TTS linearisation, FTS, annotation anchoring) is text-oriented,
    /// a run model per cell would multiply the size of the persisted IR for
    /// no reading benefit, and `List.items` already sets the precedent of
    /// plain-`String` items. Parsers normalise each cell's whitespace to
    /// single spaces (see [`normalize_cell_text`]) so a cell never contains
    /// [`TABLE_CELL_SEPARATOR`] or [`TABLE_ROW_SEPARATOR`]. Rows may be
    /// ragged (a row may have fewer cells than the widest); an empty cell
    /// is an empty string, never omitted, so column positions are preserved.
    Table {
        rows: Vec<Vec<String>>,
        /// `true` if the first row is a header row (`<th>`/`w:tblHeader`).
        header_row: bool,
    },
}

/// Separator between the cells of one table row in [`Block::plain_text`].
/// Whitespace, so RSVP tokenisation (`split_whitespace`) is unaffected.
/// The Swift mirror (`FlowBlockVM.plainText`) MUST use the same value —
/// annotation anchoring slices the joined text by byte offset (ADR-003).
pub const TABLE_CELL_SEPARATOR: char = '\t';
/// Separator between the rows of a table in [`Block::plain_text`].
pub const TABLE_ROW_SEPARATOR: char = '\n';

/// Collapse all runs of whitespace (including tabs/newlines) in a table
/// cell to single spaces and trim. Parsers must pass every cell through
/// this so cells can't contain the table separators.
pub fn normalize_cell_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
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
            Block::Table { rows, .. } => rows
                .iter()
                .map(|row| row.join(&TABLE_CELL_SEPARATOR.to_string()))
                .collect::<Vec<_>>()
                .join(&TABLE_ROW_SEPARATOR.to_string()),
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

// ── Annotation (ADR-003) ─────────────────────────────────────────────────────

/// The three annotation kinds GIST supports (M3 scope: highlights, notes,
/// bookmarks). Only `Note` carries user-authored text
/// (`Annotation::note_text`); a `Bookmark` reuses the same
/// `(block_id, start, len, ...)` anchor shape as `Highlight` with `len`
/// conventionally `0` (a point, not a span) rather than inventing a second
/// anchor representation just for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationKind {
    Highlight,
    Note,
    Bookmark,
}

/// A user annotation anchored to a span of text per ADR-003:
/// `(block_id, start, len, prefix_hash, quote_hash)`, with re-anchoring on
/// hash mismatch. This type is the durable data shape only — the
/// anchoring/re-anchoring algorithm (verify both hashes on load; on a
/// `prefix_hash` mismatch, search for `quote_hash` within the same block; if
/// that also fails, mark the annotation orphaned and surface it in the UI)
/// is reading-view logic and out of scope for this backend slice. Kept
/// I/O-free like every other type in this crate — must stay compilable to
/// `wasm32-unknown-unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub item_id: String,
    pub kind: AnnotationKind,
    /// The stable id of the `Section`/block this annotation anchors to
    /// (ADR-003's `block_id`).
    pub block_id: String,
    /// Byte offset into the block's plain text at anchoring time.
    pub start: usize,
    /// Length in bytes of the anchored span (conventionally `0` for a
    /// `Bookmark`'s point anchor).
    pub len: usize,
    /// FNV-1a of the 30 characters preceding `start` — detects shifted
    /// context on re-anchoring (ADR-003).
    pub prefix_hash: u64,
    /// FNV-1a of the anchored span's own text (ADR-003).
    pub quote_hash: u64,
    /// User-authored note text. Populated only for `AnnotationKind::Note`
    /// by convention — enforced by callers (`gist-store`'s CRUD), not by
    /// this type, since a plain `Option<String>` round-trips through
    /// serde/uniffi more simply than a payload-carrying enum variant would.
    pub note_text: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
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
        let id = uuid::Uuid::now_v7().to_string();
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
                match block {
                    // Tables linearise row by row: a ParagraphBreak token (a
                    // reading pause) between non-empty rows, so RSVP/TTS
                    // don't run the last cell of one row into the first of
                    // the next. `char_offset` stays relative to `plain`.
                    Block::Table { .. } => {
                        let mut row_start = 0usize;
                        let mut emitted_any = false;
                        for row in plain.split(TABLE_ROW_SEPARATOR) {
                            if row.split_whitespace().next().is_some() {
                                if emitted_any {
                                    tokens.push(Token {
                                        text: String::new(),
                                        kind: TokenKind::ParagraphBreak,
                                        section_idx: si,
                                        block_idx: bi,
                                        char_offset: row_start,
                                    });
                                }
                                push_words(&mut tokens, row, row_start, si, bi);
                                emitted_any = true;
                            }
                            row_start += row.len() + TABLE_ROW_SEPARATOR.len_utf8();
                        }
                    }
                    _ => push_words(&mut tokens, &plain, 0, si, bi),
                }
            }
        }
        tokens
    }
}

/// Push one `Word` token per whitespace-separated word of `text`, whose
/// byte offset within the block's full plain text is `base + (offset in
/// text)`.
fn push_words(tokens: &mut Vec<Token>, text: &str, base: usize, si: usize, bi: usize) {
    let mut search_from = 0usize;
    for word in text.split_whitespace() {
        let pos = text[search_from..].find(word).unwrap_or(0);
        let rel = search_from + pos;
        tokens.push(Token {
            text: word.to_string(),
            kind: TokenKind::Word,
            section_idx: si,
            block_idx: bi,
            char_offset: base + rel,
        });
        search_from = rel + word.len();
    }
}

// ── tests ────────────────────────────────────────────────────────────────

/// Backward/forward-compatibility policy for the IR graph (ADR-019, Q10):
/// no type here sets `#[serde(deny_unknown_fields)]`, and every field added
/// after its containing struct first shipped carries `#[serde(default)]`
/// (see `Metadata::source_copy_ref`). These tests lock in that this is a
/// deliberate, tested policy, not just an absence of an attribute nobody
/// happened to add — `gist-store` (ADR-019's envelope/version-rejection
/// layer) depends on this holding for every type in this crate, not just
/// `Document` itself.
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_document() -> Document {
        let section = Section {
            id: "s0".to_string(),
            heading: Some((1, "Chapter One".to_string())),
            blocks: vec![Block::Paragraph {
                runs: vec![TextRun::plain("hello world")],
            }],
        };
        Document::new(Metadata::minimal("Sample"), vec![section])
    }

    /// A `Document` (and therefore every nested `Section`/`Block`/`Token`)
    /// survives an ordinary JSON round trip unchanged — the baseline this
    /// module's other tests build on.
    #[test]
    fn document_round_trips_through_json() {
        let doc = sample_document();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, doc.id);
        assert_eq!(back.metadata.title, doc.metadata.title);
        assert_eq!(back.sections.len(), doc.sections.len());
        assert_eq!(back.token_stream.len(), doc.token_stream.len());
    }

    /// Forward-compat: a JSON object carrying a field this version of the
    /// type has never heard of must not error — the default serde
    /// behaviour this crate relies on instead of
    /// `#[serde(deny_unknown_fields)]`. Exercised at every level of the IR
    /// graph a single `Document` touches (top-level, and one nested
    /// `Metadata` field), not just the outermost type.
    #[test]
    fn document_json_with_an_extra_unknown_field_still_deserializes() {
        let doc = sample_document();
        let mut value = serde_json::to_value(&doc).unwrap();
        value["a_field_from_a_newer_version"] =
            serde_json::json!("this binary has never seen this key");
        value["metadata"]["another_new_field"] = serde_json::json!(123);
        let back: Document = serde_json::from_value(value).unwrap();
        assert_eq!(back.id, doc.id);
        assert_eq!(back.metadata.title, doc.metadata.title);
    }

    /// Backward-compat: `Metadata::source_copy_ref` (`#[serde(default)]`,
    /// added by ADR-006 after `Metadata` already shipped without it) must
    /// still deserialize from a JSON object that predates the field
    /// entirely — not merely one that sets it to `null`.
    #[test]
    fn metadata_missing_source_copy_ref_key_entirely_defaults_to_none() {
        let json = serde_json::json!({
            "title": "old metadata shape",
            "author": null,
            "source_type": "txt",
            "source_ref": null,
            "import_date": null,
            "language": null,
            "word_count": 0,
        });
        let meta: Metadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.source_copy_ref, None);
    }

    // ── Block::Table (M6/R3, ADR-019 addendum) ───────────────────────────

    fn sample_table() -> Block {
        Block::Table {
            rows: vec![
                vec!["Fruit".into(), "Colour".into(), "Count".into()],
                vec!["Apple".into(), "Red".into(), "3".into()],
                vec!["Banana".into(), String::new(), "12".into()],
            ],
            header_row: true,
        }
    }

    #[test]
    fn table_plain_text_uses_tab_and_newline_separators() {
        assert_eq!(
            sample_table().plain_text(),
            "Fruit\tColour\tCount\nApple\tRed\t3\nBanana\t\t12"
        );
    }

    #[test]
    fn normalize_cell_text_collapses_all_whitespace_including_separators() {
        assert_eq!(normalize_cell_text("  a\tb\n c  "), "a b c");
        assert_eq!(normalize_cell_text("\n\t "), "");
    }

    #[test]
    fn table_tokens_are_row_by_row_with_a_pause_between_rows() {
        let section = Section {
            id: "s0".into(),
            heading: None,
            blocks: vec![
                Block::Paragraph {
                    runs: vec![TextRun::plain("Intro")],
                },
                sample_table(),
            ],
        };
        let doc = Document::new(Metadata::minimal("T"), vec![section]);
        let seq: Vec<String> = doc
            .token_stream
            .iter()
            .map(|t| match t.kind {
                TokenKind::Word => t.text.clone(),
                TokenKind::ParagraphBreak => "PB".into(),
                TokenKind::SectionBreak => "SB".into(),
            })
            .collect();
        assert_eq!(
            seq.join(" "),
            "Intro PB Fruit Colour Count PB Apple Red 3 PB Banana 12"
        );
        // Word offsets are byte offsets into the table's plain_text().
        let plain = doc.sections[0].blocks[1].plain_text();
        for t in doc
            .token_stream
            .iter()
            .filter(|t| t.block_idx == 1 && t.kind == TokenKind::Word)
        {
            assert_eq!(&plain[t.char_offset..t.char_offset + t.text.len()], t.text);
        }
        assert_eq!(doc.metadata.word_count, 9);
    }

    #[test]
    fn table_with_only_empty_cells_emits_no_tokens() {
        let section = Section {
            id: "s0".into(),
            heading: None,
            blocks: vec![Block::Table {
                rows: vec![vec![String::new(), String::new()], vec![String::new()]],
                header_row: false,
            }],
        };
        let doc = Document::new(Metadata::minimal("T"), vec![section]);
        assert!(doc.token_stream.is_empty());
    }

    #[test]
    fn table_round_trips_through_json() {
        let json = serde_json::to_string(&sample_table()).unwrap();
        let back: Block = serde_json::from_str(&json).unwrap();
        assert_eq!(back.plain_text(), sample_table().plain_text());
    }

    /// Pins the rule the Swift side (`FlowBlockVM.plainText`) must mirror:
    /// an image contributes its alt text only, never its caption.
    #[test]
    fn image_plain_text_is_alt_only_never_caption() {
        let img = |alt: Option<&str>, caption: Option<&str>| Block::Image {
            src: "x.png".into(),
            alt: alt.map(str::to_owned),
            caption: caption.map(str::to_owned),
        };
        assert_eq!(img(Some("a dog"), Some("Figure 1")).plain_text(), "a dog");
        assert_eq!(img(None, Some("Figure 1")).plain_text(), "");
        assert_eq!(img(None, None).plain_text(), "");
    }
}
