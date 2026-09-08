use std::path::Path;

// ── Parse error (shared across image/doc parsers) ──────────────────────────

/// Errors returned by format-specific parsers and pre-processors.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("resource limit exceeded")]
    ResourceLimitExceeded,
}

// ── OCR types ──────────────────────────────────────────────────────────────

/// Per-page OCR result produced by the platform OCR engine.
#[derive(Debug, Clone)]
pub struct OcrPageResult {
    /// Zero-based page index.
    pub page_index: u32,
    /// Extracted plain text (empty string if the page is blank).
    pub text: String,
    /// Confidence score 0.0–1.0 as reported by the platform OCR engine.
    pub confidence: f32,
}

/// Abstraction over a platform OCR engine (Vision on Apple, MLKit on Android).
///
/// Implemented on the host side (Swift/Kotlin) and passed into the import
/// pipeline via the FFI callback-interface bridge.  The trait is defined here
/// in `gist-core` so that `gist-imageprep` and other crates can depend on it
/// without creating a circular dependency through `gist-ffi`.
pub trait OcrEngine: Send + Sync {
    /// Recognise text on one page.
    ///
    /// `image_bytes` is a PNG-encoded, pre-processed page image.
    /// Return `None` to signal cancellation.
    fn recognize_page(&self, page_index: u32, image_bytes: Vec<u8>) -> Option<OcrPageResult>;
}

// ── Import error ────────────────────────────────────────────────────────────

/// Errors that can occur during the image-import pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("image processing: {0}")]
    ImagePrep(#[from] ParseError),
    #[error("store: {0}")]
    Store(#[from] gist_store::StoreError),
}

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
}

impl Default for ParseLimits {
    fn default() -> Self {
        ParseLimits {
            max_bytes: 256 * 1024 * 1024,
            max_pages: 2_000,
            max_nesting_depth: 200,
            max_expanded_bytes: 512 * 1024 * 1024,
        }
    }
}

// ── Error ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("store: {0}")]
    Store(#[from] gist_store::StoreError),
    #[error("parse: {0}")]
    Parse(#[from] gist_parse_txt::ParseError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("item not found: {0}")]
    NotFound(String),
}

// ── Core facade ────────────────────────────────────────────────────────────

pub struct Core {
    store: gist_store::Store,
}

impl Core {
    /// Initialise Core, creating the database and storage directory if needed.
    pub fn init(db_path: &Path, storage_dir: &Path) -> Result<Self, CoreError> {
        let store = gist_store::Store::open(db_path, storage_dir)?;
        Ok(Self { store })
    }

    /// Return `Ok(())` if the store is reachable (basic health check).
    pub fn health(&self) -> Result<(), CoreError> {
        self.store.list_items(0, 1)?;
        Ok(())
    }

    /// Import a `.txt` file. Returns the new item's id string.
    ///
    /// Pipeline:
    /// 1. Read bytes from `path`.
    /// 2. Parse with `gist_parse_txt::parse` (which calls `Document::new`,
    ///    which internally calls `build_token_stream`).
    /// 3. Stamp the source path, then explicitly rebuild the token stream so
    ///    the pipeline step is observable.
    /// 4. Insert into the store.
    /// 5. Return the document id.
    pub fn import_txt(&self, path: &Path) -> Result<String, CoreError> {
        let bytes = std::fs::read(path)?;

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");

        let mut doc = gist_parse_txt::parse(&bytes, stem)?;

        // Stamp source path into metadata.
        doc.metadata.source_ref = Some(path.to_string_lossy().into_owned());

        // Rebuild token stream (Document::new already did this, but we
        // re-run so changes to metadata are reflected if needed).
        doc.token_stream = doc.build_token_stream();

        let id = doc.id.clone();
        self.store.insert_item(&doc)?;

        tracing::info!("gist-core: imported '{}' as {}", stem, id);
        Ok(id)
    }

    /// List library items with offset/limit pagination.
    pub fn list_items(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<gist_store::LibraryItem>, CoreError> {
        Ok(self.store.list_items(offset, limit)?)
    }

    /// Create an RSVP session for the given item.
    /// Returns the session serialised as JSON.
    pub fn start_rsvp(
        &self,
        item_id: &str,
        config: gist_rsvp::Config,
    ) -> Result<String, CoreError> {
        let doc = self
            .store
            .get_item(item_id)?
            .ok_or_else(|| CoreError::NotFound(item_id.to_owned()))?;

        let progress = self.store.get_progress(item_id)?;

        // Pass the full token stream — gist-rsvp handles Word, ParagraphBreak
        // and SectionBreak tokens natively.
        let mut session = gist_rsvp::RsvpSession::new(doc.token_stream, config);

        // Restore saved cursor position, clamped to valid range.
        session.cursor = progress.min(session.tokens.len().saturating_sub(1));

        Ok(serde_json::to_string(&session)?)
    }

    /// Persist the current token index for `item_id`.
    pub fn save_progress(&self, item_id: &str, token_index: usize) -> Result<(), CoreError> {
        Ok(self.store.save_progress(item_id, token_index)?)
    }

    /// Import a single image file and run OCR using the provided engine.
    ///
    /// Phase M3 stub — the full pipeline (multi-page PDF tiling, heuristic
    /// de-skew, layout analysis) is deferred.  For now the method signature
    /// is stable so the FFI layer and tests can be wired up.
    ///
    /// Planned pipeline:
    /// 1. Read raw bytes from `path`.
    /// 2. Pre-process with `gist_imageprep::prepare_image` (greyscale + resize).
    /// 3. Call `engine.recognize_page` for each page image.
    /// 4. Assemble a [`gist_model::Document`] from the OCR text.
    /// 5. Insert into the store and return the document.
    pub fn import_image_with_ocr(
        &self,
        _path: &str,
        _engine: &dyn OcrEngine,
    ) -> Result<gist_model::Document, ImportError> {
        todo!("OCR import pipeline — Phase M3")
    }
}
