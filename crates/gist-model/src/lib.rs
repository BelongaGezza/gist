use serde::{Deserialize, Serialize};

// ── Resource limits ────────────────────────────────────────────────────────

/// Coarse, path-free classification of which resource limit an importer hit.
/// Carried alongside the free-form `limit` text by every importer's
/// `ResourceLimitExceeded` so callers (and the FFI boundary) can present a
/// specific message without ever parsing strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// Input file/body larger than `max_bytes` (or the web 50 MB cap).
    TooLarge,
    /// More pages / spine items than `max_pages`.
    TooManyPages,
    /// More archive entries than `max_zip_entries`.
    TooManyEntries,
    /// Structure nested deeper than `max_nesting_depth`.
    TooDeeplyNested,
    /// Extracted/decompressed content larger than the expanded-size budget.
    ExpandedTooLarge,
    /// A table with too many rows or columns.
    TableTooLarge,
    /// Any other limit (e.g. redirect cap).
    Other,
}

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
    ///
    /// **Merged cells (ADR-019 addendum 2, M7/R7).** `rows` stays the plain
    /// grid; a merged region keeps its text in its top-left slot and every
    /// other slot it covers holds `""`, so column positions are always
    /// aligned and [`Block::plain_text`] is unchanged. [`CellSpan`]s in
    /// `spans` record only the cells that merge (`colspan > 1` or
    /// `rowspan > 1`); an unmerged table has an empty `spans`, which is not
    /// serialised at all (byte-identical to the M6 shape).
    Table {
        rows: Vec<Vec<String>>,
        /// `true` if the first row is a header row (`<th>`/`w:tblHeader`).
        header_row: bool,
        /// Merged cells only. `#[serde(default)]` so blobs written before
        /// this field existed (every M6 table) still load.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        spans: Vec<CellSpan>,
    },
}

/// A merged table cell: the slot `(row, col)` holds the text and the cell
/// covers `rowspan` rows by `colspan` columns starting there. Always
/// `rowspan >= 1`, `colspan >= 1`, and at least one of them `> 1` (a 1x1
/// cell is never recorded). Consumers must tolerate out-of-range values from
/// hand-edited blobs (clamp to the grid; ignore what does not fit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellSpan {
    pub row: u32,
    pub col: u32,
    pub rowspan: u32,
    pub colspan: u32,
}

// ── Table layout (shared by the DOCX / ePub / web parsers) ───────────────────

/// One source cell before layout. `colspan`/`rowspan` are the *declared*
/// values (already parsed by [`parse_span_attr`]); `0` is treated as `1`.
/// `v_merge_continue` is DOCX's `w:vMerge` continuation cell (present in the
/// row as a real, empty cell): it extends the cell above in the same grid
/// column instead of standing alone.
#[derive(Debug, Clone)]
pub struct RawCell {
    pub text: String,
    pub colspan: u32,
    pub rowspan: u32,
    pub v_merge_continue: bool,
}

impl RawCell {
    /// An unmerged cell.
    pub fn plain(text: impl Into<String>) -> Self {
        RawCell {
            text: text.into(),
            colspan: 1,
            rowspan: 1,
            v_merge_continue: false,
        }
    }
}

/// The table would exceed `max_table_rows` / `max_table_cols`. Parsers map
/// this to their own `ResourceLimitExceeded { kind: LimitKind::TableTooLarge }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLayoutError {
    pub limit: String,
    pub attempted: usize,
}

/// Parse a `colspan` / `rowspan` / `w:gridSpan` attribute value. Only a run
/// of ASCII digits is a number; anything else (empty, negative, `+2`, `1.5`,
/// `abc`) is `1`. `0` is `1` too (HTML's "rowspan=0 = to the end of the row
/// group" is not supported). Overflow saturates to `u32::MAX` so a hostile
/// value is *large* (and then rejected/clamped by [`layout_table`]) rather
/// than silently treated as `1`.
pub fn parse_span_attr(s: &str) -> u32 {
    let s = s.trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return 1;
    }
    match s.parse::<u64>() {
        Ok(0) => 1,
        Ok(n) => u32::try_from(n).unwrap_or(u32::MAX),
        Err(_) => u32::MAX,
    }
}

/// Lay raw source rows out into the IR grid plus the merged-cell spans.
///
/// Rules (documented in ADR-019 addendum 2):
/// - A cell occupies the next free column of its row; slots already covered
///   by a `rowspan` from an earlier row are skipped (HTML table model).
/// - Its text lives in its top-left slot; every other slot it covers is `""`.
///   Every row is padded with `""` out to the last slot any cell covers, so
///   columns stay aligned.
/// - **Columns:** a cell that would reach past `max_table_cols` is rejected
///   (`TableTooLarge`), not clamped, before anything is allocated for it. A
///   `colspan` that would run into a slot already covered from above is
///   truncated at that slot (spans never overlap).
/// - **Rows:** `rowspan` is clamped to the rows that exist (a span past the
///   last row ends at the last row), and the row count is checked against
///   `max_table_rows` first.
/// - A `v_merge_continue` cell extends the cell directly above when that cell
///   starts in the same column with the same width and ends on the previous
///   row; otherwise it is an ordinary empty cell (orphan continuation).
///
/// Memory is bounded by `max_table_rows * max_table_cols` slots because spans
/// never overlap.
pub fn layout_table(
    raw: Vec<Vec<RawCell>>,
    limits: &ParseLimits,
) -> Result<(Vec<Vec<String>>, Vec<CellSpan>), TableLayoutError> {
    let nrows = raw.len();
    if nrows > limits.max_table_rows {
        return Err(TableLayoutError {
            limit: format!("max_table_rows={}", limits.max_table_rows),
            attempted: nrows,
        });
    }
    let max_cols = limits.max_table_cols;
    let cols_err = |attempted: usize| TableLayoutError {
        limit: format!("max_table_cols={max_cols}"),
        attempted,
    };

    let mut grid: Vec<Vec<String>> = vec![Vec::new(); nrows];
    // owner[r][c] = index into `placed` of the cell covering that slot.
    let mut owner: Vec<Vec<Option<usize>>> = vec![Vec::new(); nrows];
    let mut placed: Vec<CellSpan> = Vec::new();

    fn ensure(grid: &mut [Vec<String>], owner: &mut [Vec<Option<usize>>], r: usize, w: usize) {
        if grid[r].len() < w {
            grid[r].resize(w, String::new());
        }
        if owner[r].len() < w {
            owner[r].resize(w, None);
        }
    }

    for (r, row) in raw.into_iter().enumerate() {
        let mut c = 0usize;
        for cell in row {
            while owner[r].get(c).copied().flatten().is_some() {
                c += 1;
            }
            let want = cell.colspan.max(1) as usize;
            // `want` may be hostile (u32::MAX); compare without overflow.
            if c >= max_cols || want > max_cols - c {
                return Err(cols_err(c.saturating_add(want)));
            }
            let mut cs = 1usize;
            while cs < want && owner[r].get(c + cs).copied().flatten().is_none() {
                cs += 1;
            }

            if cell.v_merge_continue && r > 0 {
                if let Some(Some(oid)) = owner[r - 1].get(c).copied() {
                    let o = placed[oid];
                    if o.col as usize == c
                        && o.colspan as usize == cs
                        && (o.row + o.rowspan) as usize == r
                    {
                        placed[oid].rowspan += 1;
                        ensure(&mut grid, &mut owner, r, c + cs);
                        for slot in &mut owner[r][c..c + cs] {
                            *slot = Some(oid);
                        }
                        c += cs;
                        continue;
                    }
                }
            }

            let rs = (cell.rowspan.max(1) as usize).min(nrows - r);
            let id = placed.len();
            placed.push(CellSpan {
                row: r as u32,
                col: c as u32,
                rowspan: rs as u32,
                colspan: cs as u32,
            });
            for rr in r..r + rs {
                ensure(&mut grid, &mut owner, rr, c + cs);
                for slot in &mut owner[rr][c..c + cs] {
                    *slot = Some(id);
                }
            }
            grid[r][c] = cell.text;
            c += cs;
        }
    }

    let spans = placed
        .into_iter()
        .filter(|s| s.rowspan > 1 || s.colspan > 1)
        .collect();
    Ok((grid, spans))
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
            spans: vec![],
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
                spans: vec![],
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

    // ── Merged cells (ADR-019 addendum 2, M7/R7) ─────────────────────────

    fn rc(text: &str, cs: u32, rs: u32) -> RawCell {
        RawCell {
            text: text.into(),
            colspan: cs,
            rowspan: rs,
            v_merge_continue: false,
        }
    }

    /// The merged fixture table as the HTML parsers see it.
    fn merged_raw() -> Vec<Vec<RawCell>> {
        vec![
            vec![rc("Sales", 2, 1), rc("Notes", 1, 1)],
            vec![rc("North", 1, 1), rc("100", 1, 1), rc("Strong", 1, 2)],
            vec![rc("South", 1, 1), rc("80", 1, 1)],
            vec![rc("Grand total", 2, 1), rc("180", 1, 1)],
        ]
    }

    #[test]
    fn layout_places_merged_cells_top_left_and_pads_covered_slots() {
        let (rows, spans) = layout_table(merged_raw(), &ParseLimits::default()).unwrap();
        assert_eq!(
            rows,
            vec![
                vec!["Sales", "", "Notes"],
                vec!["North", "100", "Strong"],
                vec!["South", "80", ""],
                vec!["Grand total", "", "180"],
            ]
        );
        let sp = |row, col, rowspan, colspan| CellSpan {
            row,
            col,
            rowspan,
            colspan,
        };
        assert_eq!(spans, vec![sp(0, 0, 1, 2), sp(1, 2, 2, 1), sp(3, 0, 1, 2)]);
    }

    /// SECOND shared golden (the Swift `FlowTableTests` pins the identical
    /// literal): a merged region flattens with covered slots empty, so the
    /// tab/newline structure keeps columns aligned.
    #[test]
    fn merged_table_golden_plain_text_matches_swift() {
        let (rows, spans) = layout_table(merged_raw(), &ParseLimits::default()).unwrap();
        let t = Block::Table {
            rows,
            header_row: true,
            spans,
        };
        assert_eq!(
            t.plain_text(),
            "Sales\t\tNotes\nNorth\t100\tStrong\nSouth\t80\t\nGrand total\t\t180"
        );
    }

    #[test]
    fn unmerged_layout_has_no_spans_and_matches_input() {
        let raw = vec![
            vec![RawCell::plain("a"), RawCell::plain("b")],
            vec![RawCell::plain("c")],
        ];
        let (rows, spans) = layout_table(raw, &ParseLimits::default()).unwrap();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["c"]]);
        assert!(spans.is_empty());
    }

    #[test]
    fn v_merge_continue_extends_the_cell_above() {
        let cont = |cs| RawCell {
            text: String::new(),
            colspan: cs,
            rowspan: 1,
            v_merge_continue: true,
        };
        let raw = vec![
            vec![rc("A", 1, 1), rc("S", 1, 1)],
            vec![rc("B", 1, 1), cont(1)],
            vec![rc("C", 1, 1), cont(1)],
        ];
        let (rows, spans) = layout_table(raw, &ParseLimits::default()).unwrap();
        assert_eq!(rows[2], vec!["C", ""]);
        assert_eq!(
            spans,
            vec![CellSpan {
                row: 0,
                col: 1,
                rowspan: 3,
                colspan: 1
            }]
        );
    }

    #[test]
    fn orphan_v_merge_continue_is_an_ordinary_empty_cell() {
        let raw = vec![vec![RawCell {
            text: String::new(),
            colspan: 1,
            rowspan: 1,
            v_merge_continue: true,
        }]];
        let (rows, spans) = layout_table(raw, &ParseLimits::default()).unwrap();
        assert_eq!(rows, vec![vec![""]]);
        assert!(spans.is_empty());
    }

    #[test]
    fn hostile_colspan_is_rejected_before_allocating() {
        let raw = vec![vec![rc("x", u32::MAX, 1)]];
        let err = layout_table(raw, &ParseLimits::default()).unwrap_err();
        assert!(err.limit.contains("max_table_cols"));
    }

    #[test]
    fn hostile_rowspan_is_clamped_to_the_rows_that_exist() {
        let raw = vec![vec![rc("x", 1, u32::MAX)], vec![RawCell::plain("y")]];
        let (rows, spans) = layout_table(raw, &ParseLimits::default()).unwrap();
        assert_eq!(rows, vec![vec!["x"], vec!["", "y"]]);
        assert_eq!(spans[0].rowspan, 2);
    }

    #[test]
    fn overlapping_colspan_is_truncated_never_overlaps() {
        // Row 0's rowspan=2 cell covers col 1 of row 1; row 1's colspan=3
        // cell at col 0 must stop before it.
        let raw = vec![
            vec![rc("a", 1, 1), rc("b", 1, 2)],
            vec![rc("c", 3, 1)],
        ];
        let (rows, spans) = layout_table(raw, &ParseLimits::default()).unwrap();
        assert_eq!(rows[1], vec!["c", ""]);
        assert!(spans.iter().all(|s| s.col != 0 || s.row != 1));
    }

    #[test]
    fn layout_enforces_row_and_column_caps() {
        let l = ParseLimits {
            max_table_rows: 2,
            max_table_cols: 3,
            ..ParseLimits::default()
        };
        let three_rows = vec![vec![RawCell::plain("x")]; 3];
        assert!(layout_table(three_rows, &l).is_err());
        let wide = vec![vec![RawCell::plain("x"); 4]];
        assert!(layout_table(wide, &l).is_err());
        // A rowspan that pushes a later row's cell past the cap is rejected.
        let pushed = vec![vec![rc("a", 3, 2)], vec![RawCell::plain("b")]];
        assert!(layout_table(pushed, &l).is_err());
    }

    #[test]
    fn parse_span_attr_handles_hostile_values() {
        assert_eq!(parse_span_attr("2"), 2);
        assert_eq!(parse_span_attr(" 3 "), 3);
        assert_eq!(parse_span_attr("0"), 1);
        assert_eq!(parse_span_attr(""), 1);
        assert_eq!(parse_span_attr("-2"), 1);
        assert_eq!(parse_span_attr("+2"), 1);
        assert_eq!(parse_span_attr("1.5"), 1);
        assert_eq!(parse_span_attr("abc"), 1);
        assert_eq!(parse_span_attr("4294967295"), u32::MAX);
        assert_eq!(parse_span_attr("99999999999999999999999"), u32::MAX);
    }

    #[test]
    fn spans_are_omitted_when_empty_and_default_when_absent() {
        let json = serde_json::to_string(&sample_table()).unwrap();
        assert!(!json.contains("spans"), "{json}");
        let old = r#"{"Table":{"rows":[["a"]],"header_row":false}}"#;
        let b: Block = serde_json::from_str(old).unwrap();
        assert!(matches!(b, Block::Table { ref spans, .. } if spans.is_empty()));
        let (rows, spans) = layout_table(merged_raw(), &ParseLimits::default()).unwrap();
        let t = Block::Table {
            rows,
            header_row: true,
            spans,
        };
        let back: Block = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.plain_text(), t.plain_text());
    }
}
