use std::path::PathBuf;
use std::sync::Arc;

uniffi::setup_scaffolding!();

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum GistError {
    #[error("{0}")]
    Core(String),
    /// Kept as its own variant (uniffi's `flat_error` still preserves variant
    /// identity even though associated data is flattened to a display
    /// string) so Swift can switch on `.DrmProtected` for a dedicated DRM
    /// presentation instead of parsing `Core`'s message text.
    #[error("this document is protected by DRM and cannot be imported")]
    DrmProtected,
    #[error("internal error")]
    InternalPanic,
}

impl From<gist_core::CoreError> for GistError {
    fn from(e: gist_core::CoreError) -> Self {
        GistError::Core(e.to_string())
    }
}

impl From<gist_core::ImportError> for GistError {
    fn from(e: gist_core::ImportError) -> Self {
        match e {
            gist_core::ImportError::DrmProtected => GistError::DrmProtected,
            other => GistError::Core(other.to_string()),
        }
    }
}

/// Installs a panic hook that routes the panic message through
/// `tracing::debug!` instead of the default hook's stderr write (F22).
/// `catch_unwind` below already keeps the panic *payload* out of the
/// `GistError` returned to Swift/Kotlin, but without this, the default hook
/// still writes the raw panic message — which can incidentally include path
/// fragments from a dependency's `.unwrap()` — to stderr before the catch
/// runs. Harmless while nothing reads stderr, but a latent leak path the day
/// it's captured into a shared crash log or telemetry pipeline. Installed
/// lazily on first use of `ffi_catch!`, once per process.
fn install_panic_hook_once() {
    // No-op under `cfg(test)`: replacing the panic hook process-wide would
    // silence the default hook's stderr output for every other test in this
    // binary that panics after this one runs (Rust's test harness shares a
    // process across tests by default) — a real regression in test
    // diagnostics for a fix that only matters in the shipped library.
    #[cfg(not(test))]
    {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                tracing::debug!("gist-ffi: internal panic caught at FFI boundary: {info}");
            }));
        });
    }
}

/// Wraps a `#[uniffi::export]` function body in `catch_unwind`. A Rust panic
/// unwinding across the C ABI into Swift/Kotlin is undefined behavior, so
/// every exported function must catch it here and return
/// `GistError::InternalPanic` instead.
macro_rules! ffi_catch {
    ($body:block) => {{
        install_panic_hook_once();
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body)) {
            Ok(result) => result,
            Err(_) => Err(GistError::InternalPanic),
        }
    }};
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

#[derive(uniffi::Record)]
pub struct FfiCollection {
    pub id: String,
    pub name: String,
    pub created_at: i64,
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
        ffi_catch!({
            let core =
                gist_core::Core::init(&PathBuf::from(&db_path), &PathBuf::from(&storage_dir))
                    .map_err(GistError::from)?;
            Ok(Arc::new(Self { inner: core }))
        })
    }

    pub fn health(&self) -> Result<(), GistError> {
        ffi_catch!({ self.inner.health().map_err(GistError::from) })
    }

    pub fn import_txt(&self, path: String) -> Result<String, GistError> {
        ffi_catch!({
            self.inner
                .import_txt(&PathBuf::from(path))
                .map_err(GistError::from)
        })
    }

    /// Import any supported file (ePub, DOCX, TXT), detected by magic bytes
    /// with extension as fallback. Returns the new item's id string.
    pub fn import_file(&self, path: String) -> Result<String, GistError> {
        ffi_catch!({
            self.inner
                .import_file(&PathBuf::from(path), &gist_core::NullObserver)
                .map_err(GistError::from)
        })
    }

    /// Fetch and import readable content from `url`. See `gist_core::Core::import_url`.
    pub fn import_url(&self, url: String) -> Result<String, GistError> {
        ffi_catch!({
            self.inner
                .import_url(&url, &gist_core::NullObserver)
                .map_err(GistError::from)
        })
    }

    pub fn list_items(&self, offset: u64, limit: u64) -> Result<Vec<FfiLibraryItem>, GistError> {
        ffi_catch!({
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
        })
    }

    /// Full-text search across the library. Returns items ranked by FTS5
    /// relevance.
    pub fn search_items(
        &self,
        query: String,
        limit: u64,
    ) -> Result<Vec<FfiLibraryItem>, GistError> {
        ffi_catch!({
            let items = self
                .inner
                .search_items(&query, limit as usize)
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
        })
    }

    pub fn start_rsvp(&self, item_id: String, wpm: u32) -> Result<String, GistError> {
        ffi_catch!({
            let config = gist_rsvp::Config {
                wpm,
                ..Default::default()
            };
            self.inner
                .start_rsvp(&item_id, config)
                .map_err(GistError::from)
        })
    }

    pub fn save_progress(&self, item_id: String, token_index: u64) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .save_progress(&item_id, token_index as usize)
                .map_err(GistError::from)
        })
    }

    /// Return a document's full content (metadata + section/block structure)
    /// as a JSON string, for non-RSVP reading views — e.g. the flow-view
    /// prototypes (M2, Q8) — that need the parsed block structure rather
    /// than RSVP's flat token stream. Read-only.
    pub fn get_document_json(&self, item_id: String) -> Result<String, GistError> {
        ffi_catch!({ self.inner.get_document(&item_id).map_err(GistError::from) })
    }

    /// Remove one or more items from the library (single-item context menu
    /// or bulk multi-select both go through this one call). Deletes the DB
    /// rows transactionally first, then best-effort cleans up the internal
    /// `.json`/`.tokens.json` blobs; when `delete_source_files` is true, the
    /// original imported file is deleted too. See `gist_core::Core::remove_items`
    /// for the full ordering/atomicity contract.
    pub fn remove_items(
        &self,
        ids: Vec<String>,
        delete_source_files: bool,
    ) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .remove_items(&ids, delete_source_files)
                .map_err(GistError::from)
        })
    }

    /// Create a new collection. Returns the generated collection id.
    /// See `gist_core::Core::create_collection`.
    pub fn create_collection(&self, name: String) -> Result<String, GistError> {
        ffi_catch!({ self.inner.create_collection(&name).map_err(GistError::from) })
    }

    /// Return all collections, newest first.
    /// See `gist_core::Core::list_collections`.
    pub fn list_collections(&self) -> Result<Vec<FfiCollection>, GistError> {
        ffi_catch!({
            let collections = self.inner.list_collections().map_err(GistError::from)?;
            Ok(collections
                .into_iter()
                .map(|c| FfiCollection {
                    id: c.id,
                    name: c.name,
                    created_at: c.created_at,
                })
                .collect())
        })
    }

    /// Add an item to a collection. Idempotent — adding twice is a no-op.
    /// See `gist_core::Core::add_item_to_collection`.
    pub fn add_item_to_collection(
        &self,
        item_id: String,
        collection_id: String,
    ) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .add_item_to_collection(&item_id, &collection_id)
                .map_err(GistError::from)
        })
    }

    /// Remove an item from a collection.
    /// See `gist_core::Core::remove_item_from_collection`.
    pub fn remove_item_from_collection(
        &self,
        item_id: String,
        collection_id: String,
    ) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .remove_item_from_collection(&item_id, &collection_id)
                .map_err(GistError::from)
        })
    }

    /// Return all library items belonging to a collection (newest first).
    /// See `gist_core::Core::list_items_in_collection`.
    pub fn list_items_in_collection(
        &self,
        collection_id: String,
    ) -> Result<Vec<FfiLibraryItem>, GistError> {
        ffi_catch!({
            let items = self
                .inner
                .list_items_in_collection(&collection_id)
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
        })
    }

    /// Attach a tag (by name) to an item, creating the tag if it doesn't
    /// already exist. Idempotent — adding the same tag twice is a no-op.
    /// See `gist_core::Core::add_tag`.
    pub fn add_tag(&self, item_id: String, tag_name: String) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .add_tag(&item_id, &tag_name)
                .map_err(GistError::from)
        })
    }

    /// Detach a tag (by name) from an item. Does not delete the tag itself.
    /// See `gist_core::Core::remove_tag`.
    pub fn remove_tag(&self, item_id: String, tag_name: String) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .remove_tag(&item_id, &tag_name)
                .map_err(GistError::from)
        })
    }

    /// Return the names of all tags attached to an item.
    /// See `gist_core::Core::list_tags_for_item`.
    pub fn list_tags_for_item(&self, item_id: String) -> Result<Vec<String>, GistError> {
        ffi_catch!({
            self.inner
                .list_tags_for_item(&item_id)
                .map_err(GistError::from)
        })
    }

    /// Import an image file and run OCR using the provided engine.
    /// Returns the document ID on success.
    pub fn import_image_with_ocr(
        &self,
        path: String,
        engine: Box<dyn OcrEngine>,
    ) -> Result<String, GistError> {
        ffi_catch!({
            let adapter = CoreOcrAdapter(engine.as_ref());
            self.inner
                .import_image_with_ocr(&path, &adapter)
                .map(|doc| doc.id)
                .map_err(|e| GistError::Core(e.to_string()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_panic_hook_once_is_idempotent() {
        // Must be safe to call from every ffi_catch! invocation, not just
        // the first — this should never panic or double-install.
        install_panic_hook_once();
        install_panic_hook_once();
        install_panic_hook_once();
    }

    #[test]
    fn ffi_catch_converts_panic_to_internal_panic_error() {
        fn panics() -> Result<(), GistError> {
            ffi_catch!({
                panic!("deliberate test panic");
            })
        }

        let result = panics();
        assert!(matches!(result, Err(GistError::InternalPanic)));
    }

    #[test]
    fn ffi_catch_passes_through_ok_result() {
        fn succeeds() -> Result<i32, GistError> {
            ffi_catch!({ Ok(42) })
        }

        assert_eq!(succeeds().unwrap(), 42);
    }
}
