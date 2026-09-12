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

        // Stamp source path into metadata.
        doc.metadata.source_ref = Some(path.to_string_lossy().into_owned());

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

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

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

        // Stamp source path.
        doc.metadata.source_ref = Some(path.to_string_lossy().into_owned());
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
    /// original imported file (`source_path`) is only deleted when
    /// `delete_source_files` is true, since that file may be a
    /// user-managed document living outside app storage.
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
            if let Err(e) = std::fs::remove_file(&item.doc_path) {
                tracing::debug!(
                    "gist-core: failed to delete document blob {}: {}",
                    item.doc_path,
                    e
                );
            }

            let tokens_path = item
                .doc_path
                .strip_suffix(".json")
                .map(|s| format!("{s}.tokens.json"))
                .unwrap_or_else(|| format!("{}.tokens.json", item.doc_path));
            if let Err(e) = std::fs::remove_file(&tokens_path) {
                tracing::debug!(
                    "gist-core: failed to delete tokens blob {}: {}",
                    tokens_path,
                    e
                );
            }

            if delete_source_files {
                if let Some(source_path) = &item.source_path {
                    if let Err(e) = std::fs::remove_file(source_path) {
                        tracing::debug!(
                            "gist-core: failed to delete source file {}: {}",
                            source_path,
                            e
                        );
                    }
                }
            }
        }

        Ok(())
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

    #[test]
    fn remove_items_with_delete_source_files_true_deletes_source() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();

        let txt = dir.path().join("source_true.txt");
        std::fs::write(&txt, b"Delete my source file too.").unwrap();
        let id = core.import_file(&txt, &NullObserver).unwrap();

        core.remove_items(&[id], true).unwrap();

        assert!(
            !txt.exists(),
            "source file should be deleted when delete_source_files=true"
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

        core.remove_items(&[id], false).unwrap();

        assert!(
            txt.exists(),
            "source file should survive when delete_source_files=false"
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
}
