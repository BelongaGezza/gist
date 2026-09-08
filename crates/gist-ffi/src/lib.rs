use std::path::PathBuf;
use std::sync::Arc;

uniffi::setup_scaffolding!();

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum GistError {
    #[error("{0}")]
    Core(String),
}

impl From<gist_core::CoreError> for GistError {
    fn from(e: gist_core::CoreError) -> Self {
        GistError::Core(e.to_string())
    }
}

// ── OCR types ─────────────────────────────────────────────────────────────────

/// Per-page OCR result returned by the Swift/Kotlin callback.
#[derive(Debug, uniffi::Record)]
pub struct OcrPageResult {
    /// Zero-based page index.
    pub page_index: u32,
    /// Extracted plain text for the page (empty string if page is blank).
    pub text: String,
    /// Confidence score 0.0–1.0 as reported by the platform OCR engine.
    pub confidence: f32,
}

/// Callback interface: implemented in Swift/Kotlin, called from Rust per page.
/// uniffi generates the necessary bridge glue.
#[uniffi::export(callback_interface)]
pub trait OcrEngine: Send + Sync {
    /// Recognise text on one page.
    /// `image_bytes` is a raw PNG-encoded page image.
    /// Return `None` to signal cancellation.
    fn recognize_page(&self, page_index: u32, image_bytes: Vec<u8>) -> Option<OcrPageResult>;
}

/// Adapter that makes a uniffi `OcrEngine` callback object usable as a
/// `gist_core::OcrEngine`.  This breaks the otherwise circular dependency:
///   gist-ffi → gist-core (ok) but gist-core must NOT depend on gist-ffi.
struct CoreOcrAdapter<'a>(&'a dyn OcrEngine);

impl gist_core::OcrEngine for CoreOcrAdapter<'_> {
    fn recognize_page(
        &self,
        page_index: u32,
        image_bytes: Vec<u8>,
    ) -> Option<gist_core::OcrPageResult> {
        self.0
            .recognize_page(page_index, image_bytes)
            .map(|r| gist_core::OcrPageResult {
                page_index: r.page_index,
                text: r.text,
                confidence: r.confidence,
            })
    }
}

// ── Record types ─────────────────────────────────────────────────────────────

#[derive(uniffi::Record)]
pub struct FfiLibraryItem {
    pub id: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub source_path: Option<String>,
    pub cover_path: Option<String>,
}

// ── GistCore object ──────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct GistCore {
    inner: gist_core::Core,
}

#[uniffi::export]
impl GistCore {
    #[uniffi::constructor]
    pub fn new(db_path: String, storage_dir: String) -> Result<Arc<Self>, GistError> {
        let core = gist_core::Core::init(&PathBuf::from(&db_path), &PathBuf::from(&storage_dir))
            .map_err(GistError::from)?;
        Ok(Arc::new(Self { inner: core }))
    }

    pub fn health(&self) -> Result<(), GistError> {
        self.inner.health().map_err(GistError::from)
    }

    pub fn import_txt(&self, path: String) -> Result<String, GistError> {
        self.inner
            .import_txt(&PathBuf::from(path))
            .map_err(GistError::from)
    }

    pub fn list_items(&self, offset: u64, limit: u64) -> Result<Vec<FfiLibraryItem>, GistError> {
        let items = self
            .inner
            .list_items(offset as usize, limit as usize)
            .map_err(GistError::from)?;
        Ok(items
            .into_iter()
            .map(|i| FfiLibraryItem {
                id: i.id,
                title: i.title,
                authors: i.authors,
                source_path: i.source_path,
                cover_path: i.cover_path,
            })
            .collect())
    }

    pub fn start_rsvp(&self, item_id: String, wpm: u32) -> Result<String, GistError> {
        let config = gist_rsvp::Config {
            wpm,
            ..Default::default()
        };
        self.inner
            .start_rsvp(&item_id, config)
            .map_err(GistError::from)
    }

    pub fn save_progress(&self, item_id: String, token_index: u64) -> Result<(), GistError> {
        self.inner
            .save_progress(&item_id, token_index as usize)
            .map_err(GistError::from)
    }

    /// Import an image file and run OCR using the provided engine.
    /// Returns the document ID on success.
    pub fn import_image_with_ocr(
        &self,
        path: String,
        engine: Box<dyn OcrEngine>,
    ) -> Result<String, GistError> {
        let adapter = CoreOcrAdapter(engine.as_ref());
        self.inner
            .import_image_with_ocr(&path, &adapter)
            .map(|doc| doc.id)
            .map_err(|e| GistError::Core(e.to_string()))
    }
}
