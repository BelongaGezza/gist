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
}

impl Default for ParseLimits {
    fn default() -> Self {
        ParseLimits {
            max_bytes: 256 * 1024 * 1024,
            max_pages: 2_000,
            max_nesting_depth: 200,
            max_expanded_bytes: 512 * 1024 * 1024,
            max_zip_entries: 10_000,
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
}
