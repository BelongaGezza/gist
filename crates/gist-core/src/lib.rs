use std::path::Path;

/// Lower-cased file extension with no leading dot (empty string if none).
/// Shared by every import path that needs to name a sandboxed copy of the
/// original file (ADR-006).
fn file_ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Best-effort delete `path` and its BLAKE3 checksum sidecar (`<path>.blake3`,
/// ADR-013 / `A4`), if either exists. Mirrors `Core::remove_items`'s existing
/// file-cleanup policy exactly: a missing or unremovable file is logged at
/// `debug!` and never fails the overall call, since by the time this runs the
/// owning `library_items` row is already gone. A checksum sidecar with no
/// corresponding blob (or vice versa) is harmless either way — this just
/// avoids leaving one behind after its blob is gone.
fn remove_file_and_checksum_sidecar(path: &str) {
    if let Err(e) = std::fs::remove_file(path) {
        tracing::debug!("gist-core: failed to delete file {}: {}", path, e);
    }
    let sidecar = format!("{path}.blake3");
    if let Err(e) = std::fs::remove_file(&sidecar) {
        tracing::debug!(
            "gist-core: failed to delete checksum sidecar {}: {}",
            sidecar,
            e
        );
    }
}

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

/// Errors that can occur during the import pipeline.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Epub(String),
    #[error("parse: {0}")]
    Docx(String),
    #[error("parse: {0}")]
    Txt(String),
    #[error("image processing: {0}")]
    ImagePrep(String),
    #[error("web fetch: {0}")]
    Web(String),
    #[error("store: {0}")]
    Store(#[from] gist_store::StoreError),
    #[error("unsupported file type: {0}")]
    UnsupportedType(String),
    #[error("import cancelled by observer")]
    Cancelled,
    #[error("resource limit exceeded: {limit} ({attempted} bytes attempted)")]
    ResourceLimitExceeded { limit: String, attempted: usize },
    /// Kept as a distinct variant (not folded into `Epub(String)`) so callers
    /// across the FFI boundary can present DRM as its own UX case instead of
    /// a generic import-failure toast, without string-matching error text.
    #[error("this document is protected by DRM and cannot be imported")]
    DrmProtected,
}

// ── Import observer ─────────────────────────────────────────────────────────

/// Progress/cancellation callback for the import pipeline.
pub trait ImportObserver: Send + Sync {
    /// Called as bytes are consumed; `total` is None if unknown.
    fn on_progress(&self, bytes_read: u64, total: Option<u64>);
    /// Return true to request cancellation. Checked between pipeline stages.
    fn is_cancelled(&self) -> bool;
}

/// No-op observer for callers that don't need progress events.
pub struct NullObserver;
impl ImportObserver for NullObserver {
    fn on_progress(&self, _: u64, _: Option<u64>) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

// ── Resource limits ────────────────────────────────────────────────────────

/// Re-exported from `gist-model` so existing `gist_core::ParseLimits` call
/// sites keep working. Lives in `gist-model` (not here) because parser
/// crates need it and must not depend back on `gist-core`.
pub use gist_model::ParseLimits;

// ── Encryption at rest (ADR-011) ─────────────────────────────────────────────

/// Re-exported from `gist-store` for the same reason `ParseLimits` is
/// re-exported from `gist-model` above: the trait must live where it's
/// actually consumed (`gist-store`'s file read/write boundary), and
/// `gist-core` already depends on `gist-store` (not the reverse), so
/// defining it here instead would make `gist-store` depend back on
/// `gist-core` — a cycle. Callers that only ever see `gist-core`'s facade
/// (like `gist-ffi`) can still refer to `gist_core::KeyProvider` without
/// caring which crate it's actually defined in.
pub use gist_store::{EncryptOutcome, FakeKeyProvider, KeyProvider};

/// Per-id result of a bulk [`Core::encrypt_items`] call (ADR-014). A `Vec`
/// of these — not a single aggregate `Result` — so a failure on one id in a
/// bulk selection doesn't lose information about which of the *other* ids
/// succeeded or were already encrypted.
#[derive(Debug)]
pub struct EncryptItemOutcome {
    pub id: String,
    pub result: Result<EncryptOutcome, gist_store::StoreError>,
}

/// Initialise `Core` with document/original-file content encrypted at rest
/// (ADR-011, AES-256-GCM) using a key from `key_provider` — the encrypted
/// counterpart to [`Core::init`]. See [`gist_store::Store::open_encrypted`]
/// for the full migration story (existing plaintext rows keep working
/// unchanged; only content inserted after this call is encrypted).
impl Core {
    pub fn init_encrypted(
        db_path: &Path,
        storage_dir: &Path,
        key_provider: std::sync::Arc<dyn KeyProvider>,
    ) -> Result<Self, CoreError> {
        let store = gist_store::Store::open_encrypted(db_path, storage_dir, key_provider)?;
        Ok(Self { store })
    }

    /// Initialise `Core` with **read-only** decryption capability (ADR-014):
    /// `key_provider` lets already-encrypted items (written via
    /// [`Core::encrypt_items`], or historically via [`Core::init_encrypted`])
    /// be read back — `get_document`/`start_rsvp` and anything else that
    /// loads document content — but new imports through this `Core` still
    /// land plaintext by default, exactly like [`Core::init`]. See
    /// [`gist_store::Store::open_with_read_key`] for the full rationale:
    /// this is the fix for the bug where an item encrypted through
    /// `CoreClient.shared`'s production (previously keyless) instance
    /// became permanently unreadable in the running app.
    pub fn init_with_read_key(
        db_path: &Path,
        storage_dir: &Path,
        key_provider: std::sync::Arc<dyn KeyProvider>,
    ) -> Result<Self, CoreError> {
        let store = gist_store::Store::open_with_read_key(db_path, storage_dir, key_provider)?;
        Ok(Self { store })
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
        let limits = ParseLimits::default();

        // Check size before reading the file into memory.
        let declared_len = std::fs::metadata(path)?.len() as usize;
        if declared_len > limits.max_bytes {
            return Err(CoreError::Parse(
                gist_parse_txt::ParseError::ResourceLimitExceeded {
                    limit: format!("max_bytes={}", limits.max_bytes),
                    attempted: declared_len,
                },
            ));
        }

        let bytes = std::fs::read(path)?;

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");

        let mut doc = gist_parse_txt::parse(&bytes, stem, &limits)?;

        // Copy into sandboxed storage before stamping metadata (ADR-006) —
        // only for content that parsed successfully.
        let copy_path = self.store.store_original_copy(&bytes, &file_ext(path))?;

        // Stamp source path (informational) and the sandboxed copy path
        // (authoritative for any future file-based operation, e.g. removal).
        doc.metadata.source_ref = Some(path.to_string_lossy().into_owned());
        doc.metadata.source_copy_ref = Some(copy_path);

        // Rebuild token stream (Document::new already did this, but we
        // re-run so changes to metadata are reflected if needed).
        doc.token_stream = doc.build_token_stream();

        let id = doc.id.clone();
        self.store.insert_item(&doc)?;

        tracing::debug!("gist-core: imported as {}", id);
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

    /// Full-text search across all imported document tokens.
    ///
    /// Returns library items ranked by FTS5 relevance, resolved from the ids
    /// `gist_store::Store::search_items` returns. An id that no longer
    /// resolves to a row (e.g. the item was deleted between the FTS match
    /// and this lookup) is silently omitted rather than failing the whole
    /// search.
    pub fn search_items(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<gist_store::LibraryItem>, CoreError> {
        let ids = self.store.search_items(query, limit)?;
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(item) = self.store.get_item_by_id(&id)? {
                items.push(item);
            }
        }
        Ok(items)
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

    /// Return a document's full content (metadata + section/block structure)
    /// serialised as JSON, for reading views that need the parsed block
    /// structure rather than RSVP's flat token stream — e.g. the flow-view
    /// prototypes (M2, Q8). Read-only: doesn't touch progress or rebuild the
    /// token stream. Mirrors `start_rsvp`'s "look up, then serialise" shape.
    pub fn get_document(&self, item_id: &str) -> Result<String, CoreError> {
        let doc = self
            .store
            .get_item(item_id)?
            .ok_or_else(|| CoreError::NotFound(item_id.to_owned()))?;
        Ok(serde_json::to_string(&doc)?)
    }

    /// Import any supported file (ePub, DOCX, TXT) into the library.
    ///
    /// Type detection: magic bytes via `infer`, with file extension as fallback.
    /// All format parsers receive the same `ParseLimits`; limits are currently
    /// the crate default — a per-call override can be added in a later milestone.
    ///
    /// The observer is polled for cancellation after reading bytes and after
    /// parsing. A cancelled import returns `ImportError::Cancelled`; no partial
    /// data is written to the store.
    ///
    /// Returns the new document's id string.
    pub fn import_file(
        &self,
        path: &std::path::Path,
        observer: &dyn ImportObserver,
    ) -> Result<String, ImportError> {
        let limits = ParseLimits::default();

        // Check size before reading the file into memory.
        let declared_len = std::fs::metadata(path)?.len() as usize;
        if declared_len > limits.max_bytes {
            return Err(ImportError::ResourceLimitExceeded {
                limit: format!("max_bytes={}", limits.max_bytes),
                attempted: declared_len,
            });
        }

        let bytes = std::fs::read(path)?;
        observer.on_progress(bytes.len() as u64, Some(bytes.len() as u64));
        if observer.is_cancelled() {
            return Err(ImportError::Cancelled);
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");

        let ext = file_ext(path);

        // Magic-byte detection first, extension as fallback.
        let mime = infer::get(&bytes).map(|t| t.mime_type()).unwrap_or("");

        let mut doc = if mime == "application/epub+zip" || ext == "epub" {
            gist_parse_epub::parse(&bytes, stem, &limits).map_err(|e| match e {
                gist_parse_epub::ParseError::DrmProtected => ImportError::DrmProtected,
                other => ImportError::Epub(other.to_string()),
            })?
        } else if mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            || ext == "docx"
        {
            gist_parse_docx::parse(&bytes, stem, &limits)
                .map_err(|e| ImportError::Docx(e.to_string()))?
        } else if mime.starts_with("text/") || ext == "txt" || ext == "md" || ext == "text" {
            gist_parse_txt::parse(&bytes, stem, &limits)
                .map_err(|e| ImportError::Txt(e.to_string()))?
        } else {
            return Err(ImportError::UnsupportedType(ext));
        };

        if observer.is_cancelled() {
            return Err(ImportError::Cancelled);
        }

        // Copy into sandboxed storage before stamping metadata (ADR-006) —
        // only for content that parsed successfully (DRM-rejected, unsupported,
        // or resource-limited input never reaches this point, so no orphaned
        // copies pile up for files GIST refused to import).
        let copy_path = self.store.store_original_copy(&bytes, &ext)?;

        // Stamp source path (informational) and the sandboxed copy path
        // (authoritative for any future file-based operation, e.g. removal).
        doc.metadata.source_ref = Some(path.to_string_lossy().into_owned());
        doc.metadata.source_copy_ref = Some(copy_path);
        doc.token_stream = doc.build_token_stream();

        let id = doc.id.clone();
        self.store.insert_item(&doc)?;

        tracing::debug!("gist-core: imported as {}", id);
        Ok(id)
    }

    /// Import content fetched from `url` into the library.
    ///
    /// Pipeline:
    /// 1. Fetch and extract readable content via `gist_web::fetch_url`, which
    ///    enforces ADR-005 (TLS-only, robots.txt pre-check, size cap) before
    ///    any parsing happens — there's no separate local size pre-check the
    ///    way `import_file` does, because there's no local file to stat.
    /// 2. Check cancellation. Unlike `import_file`'s separate read/parse
    ///    stages, `fetch_url` is a single blocking call, so there's only one
    ///    natural point to check before it and one after.
    /// 3. Stamp the source URL, then rebuild the token stream.
    /// 4. Insert into the store.
    /// 5. Return the document id.
    ///
    /// Unlike `import_file`/`import_txt`, this never populates
    /// `Metadata.source_copy_ref` (ADR-006's copy-on-import) — there is no
    /// local file to copy, only fetched-and-extracted content. `source_ref`
    /// (the URL) remains the only provenance record, and removal's
    /// `delete_source_files` has nothing to do for URL-imported items.
    pub fn import_url(
        &self,
        url: &str,
        observer: &dyn ImportObserver,
    ) -> Result<String, ImportError> {
        if observer.is_cancelled() {
            return Err(ImportError::Cancelled);
        }

        let mut doc = gist_web::fetch_url(url, &ParseLimits::default())
            .map_err(|e| ImportError::Web(e.to_string()))?;

        if observer.is_cancelled() {
            return Err(ImportError::Cancelled);
        }

        // Stamp source URL (fetch_url's build_document already sets this,
        // but stamp explicitly here too so the pipeline step matches
        // import_file's pattern and doesn't rely on gist-web's internals).
        doc.metadata.source_ref = Some(url.to_owned());
        doc.token_stream = doc.build_token_stream();

        let id = doc.id.clone();
        self.store.insert_item(&doc)?;

        tracing::debug!("gist-core: imported url as {}", id);
        Ok(id)
    }

    /// Remove one or more items from the library.
    ///
    /// Deletes the DB rows first (transactionally, via
    /// `gist_store::Store::remove_items`) and only attempts on-disk file
    /// cleanup after that call returns `Ok` — this ordering is deliberate:
    /// it guarantees a mid-operation interruption leaves at worst an
    /// orphaned file (harmless, reclaimable), never an orphaned library row
    /// pointing at files that no longer exist.
    ///
    /// The internal `.json`/`.tokens.json` blobs (derived storage) are
    /// always deleted for every item the store actually removed. The
    /// sandboxed copy of the original file (`source_copy_path`, ADR-006) is
    /// only deleted when `delete_source_files` is true. This deliberately
    /// never touches `source_path` — the user's original file at its real,
    /// possibly-outside-app-storage location — which GIST must never delete;
    /// an item with no sandboxed copy (a URL import, or one imported before
    /// ADR-006 landed) simply has nothing to delete here.
    ///
    /// File deletion is best-effort: a missing or unremovable file is
    /// logged at `debug!` (per this project's source-path logging policy)
    /// and does not fail the overall call, since the library metadata is
    /// already gone by the time file cleanup runs.
    ///
    /// ids that don't match any library item are silently ignored (see
    /// `gist_store::Store::remove_items`'s doc comment for the exact
    /// semantics this delegates to).
    pub fn remove_items(&self, ids: &[String], delete_source_files: bool) -> Result<(), CoreError> {
        let removed = self.store.remove_items(ids)?;

        for item in removed {
            remove_file_and_checksum_sidecar(&item.doc_path);

            let tokens_path = item
                .doc_path
                .strip_suffix(".json")
                .map(|s| format!("{s}.tokens.json"))
                .unwrap_or_else(|| format!("{}.tokens.json", item.doc_path));
            remove_file_and_checksum_sidecar(&tokens_path);

            if delete_source_files {
                if let Some(source_copy_path) = &item.source_copy_path {
                    remove_file_and_checksum_sidecar(source_copy_path);
                }
                // else: no sandboxed copy exists for this item (URL import,
                // or imported before ADR-006 landed) — nothing to delete.
                // `item.source_path` (the user's real file) is never used
                // here; see the doc comment above.
            }
        }

        Ok(())
    }

    // ── Collections ─────────────────────────────────────────────────────────

    /// Create a new collection. Returns the generated collection id.
    /// See `gist_store::Store::create_collection`.
    pub fn create_collection(&self, name: &str) -> Result<String, CoreError> {
        Ok(self.store.create_collection(name)?)
    }

    /// Return all collections, newest first.
    /// See `gist_store::Store::list_collections`.
    pub fn list_collections(&self) -> Result<Vec<gist_store::Collection>, CoreError> {
        Ok(self.store.list_collections()?)
    }

    /// Add an item to a collection. Idempotent — adding twice is a no-op.
    /// See `gist_store::Store::add_item_to_collection`.
    pub fn add_item_to_collection(
        &self,
        item_id: &str,
        collection_id: &str,
    ) -> Result<(), CoreError> {
        Ok(self.store.add_item_to_collection(item_id, collection_id)?)
    }

    /// Remove an item from a collection.
    /// See `gist_store::Store::remove_item_from_collection`.
    pub fn remove_item_from_collection(
        &self,
        item_id: &str,
        collection_id: &str,
    ) -> Result<(), CoreError> {
        Ok(self
            .store
            .remove_item_from_collection(item_id, collection_id)?)
    }

    /// Return all library items belonging to a collection (newest first).
    /// See `gist_store::Store::list_items_in_collection`.
    pub fn list_items_in_collection(
        &self,
        collection_id: &str,
    ) -> Result<Vec<gist_store::LibraryItem>, CoreError> {
        Ok(self.store.list_items_in_collection(collection_id)?)
    }

    // ── Tags ────────────────────────────────────────────────────────────────

    /// Attach a tag (by name) to an item, creating the tag if it doesn't
    /// already exist. Idempotent — adding the same tag twice is a no-op.
    /// See `gist_store::Store::add_tag`.
    pub fn add_tag(&self, item_id: &str, tag_name: &str) -> Result<(), CoreError> {
        Ok(self.store.add_tag(item_id, tag_name)?)
    }

    /// Detach a tag (by name) from an item. Does not delete the tag itself.
    /// See `gist_store::Store::remove_tag`.
    pub fn remove_tag(&self, item_id: &str, tag_name: &str) -> Result<(), CoreError> {
        Ok(self.store.remove_tag(item_id, tag_name)?)
    }

    /// Return the names of all tags attached to an item.
    /// See `gist_store::Store::list_tags_for_item`.
    pub fn list_tags_for_item(&self, item_id: &str) -> Result<Vec<String>, CoreError> {
        Ok(self.store.list_tags_for_item(item_id)?)
    }

    /// Return the names of every tag that exists across the library.
    /// See `gist_store::Store::list_all_tags`.
    pub fn list_all_tags(&self) -> Result<Vec<String>, CoreError> {
        Ok(self.store.list_all_tags()?)
    }

    /// Return all library items tagged with `tag_name` (newest first).
    /// See `gist_store::Store::list_items_by_tag`.
    pub fn list_items_by_tag(
        &self,
        tag_name: &str,
    ) -> Result<Vec<gist_store::LibraryItem>, CoreError> {
        Ok(self.store.list_items_by_tag(tag_name)?)
    }

    // ── Per-item encryption (ADR-014) ──────────────────────────────────────

    /// Retroactively encrypt one or more already-imported items at rest, on
    /// demand — the per-item, opt-in follow-up to ADR-011's whole-store
    /// [`Core::init_encrypted`] (see ADR-014). Calls
    /// `key_provider.get_or_create_key()` exactly once (not once per item —
    /// `KeyProvider::get_or_create_key` must always return the same key
    /// anyway, so this just avoids redundant calls into what may be a
    /// Keychain round trip on the platform side) and then
    /// [`gist_store::Store::encrypt_item`] once per id.
    ///
    /// Returns one [`EncryptItemOutcome`] per id in `ids`, in the same
    /// order, rather than failing the whole call on the first error — a
    /// bulk selection from the UI (e.g. "select all" in the library) can
    /// contain a mix of ids that succeed, are already encrypted, or fail
    /// (most plausibly [`gist_store::StoreError::NotFound`] if an item was
    /// removed concurrently), and the caller needs to know which is which
    /// to show an accurate summary rather than losing that information
    /// behind one aggregate `Result`.
    ///
    /// See [`gist_store::Store::encrypt_item`]'s doc comment for the full
    /// design (why `key` bypasses `self.store`'s own construction-time
    /// key-provider state entirely, and the `originals/` scope exclusion).
    /// Reading an item's content back after this call requires the `Core`
    /// it's read through to have decryption capability — see
    /// [`Core::init_with_read_key`] (ADR-014), which is what
    /// `CoreClient.shared`'s production instance is now built with for
    /// exactly this reason. A `Core`/`Store` with no key of any kind (plain
    /// [`Core::init`]) still correctly cannot decrypt such an item, since it
    /// genuinely has no key.
    pub fn encrypt_items(
        &self,
        ids: &[String],
        key_provider: std::sync::Arc<dyn KeyProvider>,
    ) -> Vec<EncryptItemOutcome> {
        let key = key_provider.get_or_create_key();
        ids.iter()
            .map(|id| EncryptItemOutcome {
                id: id.clone(),
                result: self.store.encrypt_item(id, &key),
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysCancelObserver;
    impl ImportObserver for AlwaysCancelObserver {
        fn on_progress(&self, _: u64, _: Option<u64>) {}
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    #[test]
    fn unsupported_extension_returns_error() {
        // We need a Core — use a temp dir.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let fake = dir.path().join("file.xyz");
        std::fs::write(&fake, b"not a real document").unwrap();

        let err = core.import_file(&fake, &NullObserver).unwrap_err();
        assert!(matches!(err, ImportError::UnsupportedType(_)));
    }

    #[test]
    fn search_items_finds_imported_document_by_word() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("aardvark.txt");
        std::fs::write(
            &txt,
            b"The quokka is a marsupial found in Western Australia.",
        )
        .unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let results = core.search_items("quokka", 10).unwrap();
        assert!(
            results.iter().any(|item| item.id == id),
            "expected search for 'quokka' to find the imported item, got {:?}",
            results
        );

        let no_match = core.search_items("nonexistentxyzzy", 10).unwrap();
        assert!(no_match.is_empty());
    }

    /// End-to-end check that `Core::init_encrypted` (ADR-011) actually wires
    /// through to `gist-store`'s encryption: import a file, confirm it's
    /// readable/searchable through the normal `Core` API, and confirm its
    /// on-disk blob is not plaintext. The crypto correctness itself
    /// (round-trip, wrong-key, migration) is covered exhaustively in
    /// `gist-store`'s own test suite — this test only proves the wiring
    /// from `gist-core`'s facade down to it is intact.
    #[test]
    fn init_encrypted_imports_and_searches_with_ciphertext_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let key_provider = std::sync::Arc::new(FakeKeyProvider::new(42));
        let core = Core::init_encrypted(&db, &storage, key_provider).unwrap();

        let txt = dir.path().join("wombat.txt");
        let needle = "WOMBAT_CANARY_STRING_ADR011";
        std::fs::write(&txt, format!("A story about a {needle} wombat.")).unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        // Readable and searchable through the normal API, same as an
        // unencrypted Core.
        let results = core.search_items("wombat", 10).unwrap();
        assert!(results.iter().any(|item| item.id == id));

        // But the on-disk blob must not contain the plaintext.
        let doc_path = storage.join(format!("{id}.json"));
        let on_disk = std::fs::read(&doc_path).unwrap();
        assert!(
            !on_disk
                .windows(needle.len())
                .any(|w| w == needle.as_bytes()),
            "canary string must not appear in the encrypted document blob on disk"
        );
    }

    #[test]
    fn cancellation_before_parse_returns_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("hello.txt");
        std::fs::write(&txt, b"Hello world").unwrap();

        // AlwaysCancelObserver returns is_cancelled=true immediately.
        let err = core.import_file(&txt, &AlwaysCancelObserver).unwrap_err();
        assert!(matches!(err, ImportError::Cancelled));
    }

    #[test]
    fn import_url_rejects_non_https_without_network_access() {
        // gist-web enforces HTTPS-only (ADR-005) before ever touching the
        // network, so this exercises the import_url wiring without needing
        // connectivity in CI or this sandbox.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let err = core
            .import_url("http://example.com", &NullObserver)
            .unwrap_err();
        assert!(
            matches!(err, ImportError::Web(_)),
            "expected ImportError::Web for a non-HTTPS URL, got {:?}",
            err
        );
    }

    #[test]
    fn remove_items_removes_one_and_keeps_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt1 = dir.path().join("keep.txt");
        std::fs::write(&txt1, b"This document should survive removal.").unwrap();
        let id_keep = core.import_file(&txt1, &NullObserver).unwrap();

        let txt2 = dir.path().join("gone.txt");
        std::fs::write(&txt2, b"This document should vanish, quokka style.").unwrap();
        let id_gone = core.import_file(&txt2, &NullObserver).unwrap();

        // Blob paths for the item we're about to remove.
        let doc_path = storage.join(format!("{id_gone}.json"));
        let tokens_path = storage.join(format!("{id_gone}.tokens.json"));
        assert!(doc_path.exists());
        assert!(tokens_path.exists());

        core.remove_items(&[id_gone.clone()], false).unwrap();

        // list_items no longer surfaces the removed item, but does surface
        // the other one.
        let items = core.list_items(0, 10).unwrap();
        assert!(!items.iter().any(|i| i.id == id_gone));
        assert!(items.iter().any(|i| i.id == id_keep));

        // search_items no longer surfaces the removed item either.
        let results = core.search_items("quokka", 10).unwrap();
        assert!(!results.iter().any(|i| i.id == id_gone));

        // Blob files are gone from the storage dir.
        assert!(!doc_path.exists());
        assert!(!tokens_path.exists());

        // delete_source_files was false — the original file is untouched.
        assert!(txt2.exists());
    }

    /// Finds the single file under `<storage>/originals/`, asserting there's
    /// exactly one (the sandboxed copy ADR-006 requires `import_file` to
    /// make). Ignores `.blake3` checksum sidecar files (ADR-013 / `A4`) —
    /// every fresh copy gets one alongside it, but this helper is about the
    /// content file itself, not its checksum.
    fn the_one_sandboxed_copy(storage: &std::path::Path) -> std::path::PathBuf {
        let originals_dir = storage.join("originals");
        let copies: Vec<_> = std::fs::read_dir(&originals_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) != Some("blake3"))
            .collect();
        assert_eq!(
            copies.len(),
            1,
            "expected exactly one sandboxed copy in {originals_dir:?}, found {copies:?}"
        );
        copies.into_iter().next().unwrap()
    }

    /// The core ADR-006 guarantee under test: `delete_source_files: true`
    /// must delete the sandboxed copy GIST made at import time, and must
    /// never touch the user's original file at its real location.
    #[test]
    fn remove_items_with_delete_source_files_true_deletes_the_sandboxed_copy_not_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("source_true.txt");
        std::fs::write(&txt, b"Delete my source file too.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let copy_path = the_one_sandboxed_copy(&storage);

        let copy_sidecar = format!("{}.blake3", copy_path.display());
        assert!(
            std::path::Path::new(&copy_sidecar).exists(),
            "a fresh sandboxed copy must have a checksum sidecar (ADR-013 / A4)"
        );

        core.remove_items(&[id], true).unwrap();

        assert!(
            !copy_path.exists(),
            "the sandboxed copy should be deleted when delete_source_files=true"
        );
        assert!(
            !std::path::Path::new(&copy_sidecar).exists(),
            "the sandboxed copy's checksum sidecar should be cleaned up alongside it"
        );
        assert!(
            txt.exists(),
            "GIST must never delete the user's original file (ADR-006)"
        );
    }

    #[test]
    fn remove_items_with_delete_source_files_false_keeps_source() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("source_false.txt");
        std::fs::write(&txt, b"Keep my source file.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let copy_path = the_one_sandboxed_copy(&storage);

        core.remove_items(&[id], false).unwrap();

        assert!(
            txt.exists(),
            "source file should survive when delete_source_files=false"
        );
        assert!(
            copy_path.exists(),
            "sandboxed copy should also survive when delete_source_files=false"
        );
    }

    #[test]
    fn remove_items_bulk_empties_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let mut ids = Vec::new();
        for i in 0..3 {
            let txt = dir.path().join(format!("bulk{i}.txt"));
            std::fs::write(&txt, format!("Bulk removal candidate number {i}.")).unwrap();
            ids.push(core.import_file(&txt, &NullObserver).unwrap());
        }

        assert_eq!(core.list_items(0, 10).unwrap().len(), 3);

        core.remove_items(&ids, false).unwrap();

        assert!(core.list_items(0, 10).unwrap().is_empty());
    }

    #[test]
    fn create_collection_add_item_and_list_contents_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("in_collection.txt");
        std::fs::write(&txt, b"A document that will live in a collection.").unwrap();
        let item_id = core.import_file(&txt, &NullObserver).unwrap();

        let other_txt = dir.path().join("not_in_collection.txt");
        std::fs::write(&other_txt, b"A document that stays uncollected.").unwrap();
        let other_id = core.import_file(&other_txt, &NullObserver).unwrap();

        let collection_id = core.create_collection("Favourites").unwrap();

        let collections = core.list_collections().unwrap();
        assert!(
            collections
                .iter()
                .any(|c| c.id == collection_id && c.name == "Favourites"),
            "expected newly created collection to appear in list_collections, got {:?}",
            collections
        );

        core.add_item_to_collection(&item_id, &collection_id)
            .unwrap();

        let contents = core.list_items_in_collection(&collection_id).unwrap();
        assert!(contents.iter().any(|i| i.id == item_id));
        assert!(!contents.iter().any(|i| i.id == other_id));

        core.remove_item_from_collection(&item_id, &collection_id)
            .unwrap();

        let contents_after_removal = core.list_items_in_collection(&collection_id).unwrap();
        assert!(!contents_after_removal.iter().any(|i| i.id == item_id));
    }

    #[test]
    fn get_document_returns_parsed_blocks_as_json() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("flow.txt");
        std::fs::write(&txt, b"A quokka wandered through the heading-free prose.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let json = core.get_document(&id).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["id"], serde_json::Value::String(id));
        assert!(
            value["sections"].is_array() && !value["sections"].as_array().unwrap().is_empty(),
            "expected at least one section in {value:?}"
        );

        let unknown = core.get_document("not-a-real-id");
        assert!(matches!(unknown, Err(CoreError::NotFound(_))));
    }

    #[test]
    fn add_and_remove_tag_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("tagged.txt");
        std::fs::write(&txt, b"A document that will be tagged.").unwrap();
        let item_id = core.import_file(&txt, &NullObserver).unwrap();

        assert!(core.list_tags_for_item(&item_id).unwrap().is_empty());

        core.add_tag(&item_id, "sci-fi").unwrap();
        core.add_tag(&item_id, "favourite").unwrap();

        // Adding the same tag twice is idempotent.
        core.add_tag(&item_id, "sci-fi").unwrap();

        let tags = core.list_tags_for_item(&item_id).unwrap();
        assert_eq!(tags, vec!["favourite".to_string(), "sci-fi".to_string()]);

        core.remove_tag(&item_id, "sci-fi").unwrap();

        let tags_after_removal = core.list_tags_for_item(&item_id).unwrap();
        assert_eq!(tags_after_removal, vec!["favourite".to_string()]);
    }

    #[test]
    fn list_all_tags_and_list_items_by_tag_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let tagged_path = dir.path().join("tagged.txt");
        std::fs::write(&tagged_path, b"This one gets tagged.").unwrap();
        let tagged_id = core.import_file(&tagged_path, &NullObserver).unwrap();

        let untagged_path = dir.path().join("untagged.txt");
        std::fs::write(&untagged_path, b"This one does not.").unwrap();
        let untagged_id = core.import_file(&untagged_path, &NullObserver).unwrap();

        core.add_tag(&tagged_id, "favourite").unwrap();

        assert_eq!(core.list_all_tags().unwrap(), vec!["favourite".to_string()]);

        let items = core.list_items_by_tag("favourite").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, tagged_id);
        assert!(items.iter().all(|i| i.id != untagged_id));
    }

    // ── Per-item encryption (ADR-014) ───────────────────────────────────

    /// A genuinely keyless `Core::init` instance (no read or write key of
    /// any kind) still correctly cannot decrypt content it encrypts via
    /// `encrypt_items` — that's the right behavior for a truly keyless
    /// instance, and this test still covers it. **This is no longer how
    /// `CoreClient.shared`'s production instance is configured** — see
    /// `encrypt_items_then_read_through_read_capable_core_succeeds` below
    /// for the scenario that matches production today (`Core::
    /// init_with_read_key`), which is the ADR-014 fix for the read-after-
    /// encrypt gap this test used to (mis)represent as production behavior.
    #[test]
    fn encrypt_items_encrypts_unencrypted_and_reports_already_encrypted_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("tau.txt");
        std::fs::write(&txt, b"A document that will be encrypted on demand.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let key_provider: std::sync::Arc<dyn KeyProvider> =
            std::sync::Arc::new(FakeKeyProvider::new(11));

        // First call: item is plaintext -> Encrypted.
        let results = core.encrypt_items(&[id.clone()], key_provider.clone());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
        assert!(matches!(results[0].result, Ok(EncryptOutcome::Encrypted)));

        // Flag is visible through the normal list_items metadata query, even
        // though this Core's own Store has no key and can no longer read
        // the item's actual content (see encrypt_item's doc comment).
        let items = core.list_items(0, 10).unwrap();
        assert!(items.iter().find(|i| i.id == id).unwrap().content_encrypted);
        assert!(matches!(
            core.get_document(&id),
            Err(CoreError::Store(gist_store::StoreError::MissingKeyProvider))
        ));

        // Second call: idempotent no-op -> AlreadyEncrypted, not an error.
        let results2 = core.encrypt_items(&[id.clone()], key_provider);
        assert_eq!(results2.len(), 1);
        assert!(matches!(
            results2[0].result,
            Ok(EncryptOutcome::AlreadyEncrypted)
        ));
    }

    /// **Closes the read-after-encrypt gap (ADR-014) for `gist-core`'s
    /// facade** — the same scenario as `gist-store`'s
    /// `encrypt_item_then_read_through_same_read_capable_store_succeeds`,
    /// but through `Core::init_with_read_key`, the constructor
    /// `CoreClient.shared`'s production instance now actually uses. A book
    /// a user encrypts must stay readable through the same running app.
    #[test]
    fn encrypt_items_then_read_through_read_capable_core_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();

        let key_provider: std::sync::Arc<dyn KeyProvider> =
            std::sync::Arc::new(FakeKeyProvider::new(0x64));
        let core = Core::init_with_read_key(&db, &storage, key_provider.clone()).unwrap();

        // A new import through this Core must still land plaintext by
        // default -- read capability must never flip on auto-encryption.
        let plain_txt = dir.path().join("chi.txt");
        std::fs::write(&plain_txt, b"Stays plaintext unless explicitly encrypted.").unwrap();
        let plain_id = core.import_file(&plain_txt, &NullObserver).unwrap();
        assert!(
            !core
                .list_items(0, 10)
                .unwrap()
                .into_iter()
                .find(|i| i.id == plain_id)
                .unwrap()
                .content_encrypted
        );
        assert!(
            core.get_document(&plain_id).is_ok(),
            "a plaintext import must remain trivially readable"
        );

        let txt = dir.path().join("psi.txt");
        std::fs::write(&txt, b"A document encrypted on demand, then reopened.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let results = core.encrypt_items(&[id.clone()], key_provider);
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].result, Ok(EncryptOutcome::Encrypted)));

        // This is the assertion that used to fail with MissingKeyProvider:
        // the SAME Core instance that just encrypted the item can read its
        // actual content straight back.
        let doc_json = core
            .get_document(&id)
            .expect("read-after-encrypt must succeed through a read-capable Core");
        assert!(doc_json.contains("psi"));

        // The RSVP path (the other real content-reading call site) must
        // also work -- start_rsvp internally calls store.get_item too.
        let rsvp_json = core
            .start_rsvp(&id, gist_rsvp::Config::default())
            .expect("start_rsvp must also succeed after encrypt through a read-capable Core");
        assert!(!rsvp_json.is_empty());
    }

    /// A bulk call spanning a real id and an unknown one must report both
    /// outcomes individually rather than losing the real id's success
    /// behind one aggregate failure.
    #[test]
    fn encrypt_items_reports_per_id_outcomes_for_a_mixed_batch() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("upsilon.txt");
        std::fs::write(&txt, b"Bulk encrypt candidate.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        let key_provider: std::sync::Arc<dyn KeyProvider> =
            std::sync::Arc::new(FakeKeyProvider::new(12));
        let results = core.encrypt_items(&[id.clone(), "not-a-real-id".to_string()], key_provider);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, id);
        assert!(matches!(results[0].result, Ok(EncryptOutcome::Encrypted)));
        assert_eq!(results[1].id, "not-a-real-id");
        assert!(matches!(
            results[1].result,
            Err(gist_store::StoreError::NotFound(_))
        ));
    }
}
