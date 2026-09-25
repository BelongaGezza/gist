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

// ── Key provider (ADR-011) ────────────────────────────────────────────────────

/// Callback interface: implemented in Swift/Kotlin (Keychain-backed on
/// Apple), called from Rust whenever `gist-store` needs the AES-256 key it
/// encrypts document content at rest with. Mirrors `OcrEngine` above exactly
/// — same reason (uniffi callback interfaces must be defined where
/// `#[uniffi::export]` runs, i.e. here, not in `gist-core`) and same adapter
/// pattern below.
///
/// Returns raw bytes rather than a fixed-size array — uniffi's supported
/// scalar/collection types don't include const-generic arrays — so
/// `CoreKeyProviderAdapter` validates the length is exactly 32 before
/// handing it to `gist_store::KeyProvider`, which does use `[u8; 32]` since
/// that side is plain Rust-to-Rust.
#[uniffi::export(callback_interface)]
pub trait KeyProvider: Send + Sync {
    /// Return the 32-byte AES-256 key, creating and durably persisting one
    /// (e.g. in the platform Keychain) on first call if none exists yet.
    /// Must return the same key on every call for the life of the app's
    /// data. Must be exactly 32 bytes — a different length is treated as a
    /// fatal misconfiguration (panics, caught by `ffi_catch!` like any other
    /// panic at this boundary, surfacing as `GistError::InternalPanic`).
    fn get_or_create_key(&self) -> Vec<u8>;
}

/// Adapter that makes a uniffi `KeyProvider` callback object usable as a
/// `gist_store::KeyProvider` (re-exported as `gist_core::KeyProvider`) —
/// same circular-dependency-avoidance role as `CoreOcrAdapter` above, and
/// owns the boxed callback object (rather than borrowing it, as
/// `CoreOcrAdapter` does for a single call) because a `Store` holds its
/// `KeyProvider` for its entire lifetime, not just one operation.
struct CoreKeyProviderAdapter(Box<dyn KeyProvider>);

impl gist_core::KeyProvider for CoreKeyProviderAdapter {
    fn get_or_create_key(&self) -> [u8; 32] {
        let bytes = self.0.get_or_create_key();
        let len = bytes.len();
        bytes.try_into().unwrap_or_else(|_| {
            panic!("KeyProvider.get_or_create_key() must return exactly 32 bytes, got {len}")
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
    /// Mirrors `gist_store::LibraryItem::content_encrypted` (ADR-011/014) —
    /// SQL-metadata-only, so it's populated correctly even for an item
    /// whose actual content this `GistCore` instance can no longer decrypt
    /// (see `GistCore::encrypt_items`'s doc comment).
    pub content_encrypted: bool,
}

#[derive(uniffi::Record)]
pub struct FfiCollection {
    pub id: String,
    pub name: String,
    pub created_at: i64,
}

// ── Annotations (ADR-003) ────────────────────────────────────────────────────

/// Mirrors `gist_core::AnnotationKind` (re-exported from `gist-model`) as a
/// uniffi-exportable enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiAnnotationKind {
    Highlight,
    Note,
    Bookmark,
}

impl From<gist_core::AnnotationKind> for FfiAnnotationKind {
    fn from(k: gist_core::AnnotationKind) -> Self {
        match k {
            gist_core::AnnotationKind::Highlight => FfiAnnotationKind::Highlight,
            gist_core::AnnotationKind::Note => FfiAnnotationKind::Note,
            gist_core::AnnotationKind::Bookmark => FfiAnnotationKind::Bookmark,
        }
    }
}

impl From<FfiAnnotationKind> for gist_core::AnnotationKind {
    fn from(k: FfiAnnotationKind) -> Self {
        match k {
            FfiAnnotationKind::Highlight => gist_core::AnnotationKind::Highlight,
            FfiAnnotationKind::Note => gist_core::AnnotationKind::Note,
            FfiAnnotationKind::Bookmark => gist_core::AnnotationKind::Bookmark,
        }
    }
}

/// Mirrors `gist_core::Annotation` (re-exported from `gist-model`) as a
/// uniffi-exportable record. `start`/`len` are `u64` (not `usize`, which
/// uniffi doesn't support) — same convention `list_items`'s `offset`/`limit`
/// already use. `prefix_hash`/`quote_hash` are `u64` directly; uniffi
/// supports unsigned 64-bit scalars natively, unlike `gist-store`'s SQLite
/// layer, which has to bit-cast them through `i64` (see that crate's
/// `create_annotation`).
#[derive(uniffi::Record)]
pub struct FfiAnnotation {
    pub id: String,
    pub item_id: String,
    pub kind: FfiAnnotationKind,
    pub block_id: String,
    pub start: u64,
    pub len: u64,
    pub prefix_hash: u64,
    pub quote_hash: u64,
    pub note_text: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<gist_core::Annotation> for FfiAnnotation {
    fn from(a: gist_core::Annotation) -> Self {
        FfiAnnotation {
            id: a.id,
            item_id: a.item_id,
            kind: a.kind.into(),
            block_id: a.block_id,
            start: a.start as u64,
            len: a.len as u64,
            prefix_hash: a.prefix_hash,
            quote_hash: a.quote_hash,
            note_text: a.note_text,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

// ── Per-item encryption (ADR-014) ──────────────────────────────────────────

/// Mirrors `gist_store::EncryptOutcome` as a uniffi-exportable enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiEncryptOutcome {
    Encrypted,
    AlreadyEncrypted,
}

impl From<gist_core::EncryptOutcome> for FfiEncryptOutcome {
    fn from(o: gist_core::EncryptOutcome) -> Self {
        match o {
            gist_core::EncryptOutcome::Encrypted => FfiEncryptOutcome::Encrypted,
            gist_core::EncryptOutcome::AlreadyEncrypted => FfiEncryptOutcome::AlreadyEncrypted,
        }
    }
}

/// Per-id result of a bulk `GistCore::encrypt_items` call. Exactly one of
/// `outcome`/`error` is set — a uniffi `Record` has no native tagged-union
/// support for "one of two shapes," so this mirrors `gist_core::
/// EncryptItemOutcome`'s `Result` as two `Option` fields instead, which
/// Swift can switch on the same way.
#[derive(uniffi::Record)]
pub struct FfiEncryptItemResult {
    pub id: String,
    pub outcome: Option<FfiEncryptOutcome>,
    pub error: Option<String>,
}

impl From<gist_core::EncryptItemOutcome> for FfiEncryptItemResult {
    fn from(o: gist_core::EncryptItemOutcome) -> Self {
        match o.result {
            Ok(outcome) => FfiEncryptItemResult {
                id: o.id,
                outcome: Some(outcome.into()),
                error: None,
            },
            Err(e) => FfiEncryptItemResult {
                id: o.id,
                outcome: None,
                error: Some(e.to_string()),
            },
        }
    }
}

// ── Removal / sweep outcomes (review Q10) ───────────────────────────────────

/// Mirrors `gist_core::FileDeleteFailureKind` as a uniffi-exportable enum —
/// why one stored file could not be deleted, at the coarsest granularity
/// that is still actionable in a UI.
///
/// Carries no path, filename or OS error text by design: these values are
/// shown to the user in a result summary, and this project logs source paths
/// at `debug!` only. Every variant is a genuine failure worth surfacing —
/// "the file was already gone" is not one of them and is counted separately
/// as `files_missing`, so a pre-ADR-013 item with no checksum sidecars
/// reports zero failures rather than looking broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiFileDeleteFailureKind {
    Locked,
    Permission,
    Other,
}

impl From<gist_core::FileDeleteFailureKind> for FfiFileDeleteFailureKind {
    fn from(k: gist_core::FileDeleteFailureKind) -> Self {
        match k {
            gist_core::FileDeleteFailureKind::Locked => FfiFileDeleteFailureKind::Locked,
            gist_core::FileDeleteFailureKind::Permission => FfiFileDeleteFailureKind::Permission,
            gist_core::FileDeleteFailureKind::Other => FfiFileDeleteFailureKind::Other,
        }
    }
}

/// Result of `GistCore::remove_items_detailed` — what the removal actually
/// managed to do, so a UI never reports "removed" while a stored copy is
/// still on disk (review Q10). See `gist_core::RemoveOutcome`.
#[derive(uniffi::Record)]
pub struct FfiRemoveOutcome {
    /// Ids that matched a row and were removed from the database. Ids that
    /// matched nothing are absent.
    pub removed_ids: Vec<String>,
    /// Stored files deleted: the `.json`/`.tokens.json` blobs, their
    /// `.blake3` checksum sidecars, and (only when `delete_source_files` is
    /// true) the ADR-006 sandboxed original copy and its sidecar. Never the
    /// user's own file.
    pub files_deleted: u32,
    /// How many stored copies were kept on purpose because another item that
    /// survived this removal shares the same content-addressed file (ADR-006
    /// dedup). **Not a failure and not a missing file** — no deletion was
    /// attempted. Removing the last item that shares the file deletes it.
    pub shared_copies_kept: u32,
    /// Files that were already gone, so there was nothing to delete. **Not
    /// a failure** — the ordinary case is an item imported before ADR-013
    /// added checksum sidecars, which has no `.blake3` files to remove. Do
    /// not surface this as a problem; it exists so the tally adds up.
    pub files_missing: u32,
    /// How many deletions genuinely failed. Equals `failure_kinds.len()`.
    /// **This is the only count worth showing the user as a warning.**
    pub files_failed: u32,
    /// One coarse kind per failed deletion, in attempt order.
    pub failure_kinds: Vec<FfiFileDeleteFailureKind>,
}

impl From<gist_core::RemoveOutcome> for FfiRemoveOutcome {
    fn from(o: gist_core::RemoveOutcome) -> Self {
        FfiRemoveOutcome {
            removed_ids: o.removed_ids,
            files_deleted: o.files_deleted,
            shared_copies_kept: o.shared_copies_kept,
            files_missing: o.files_missing,
            files_failed: o.files_failed,
            failure_kinds: o.failure_kinds.into_iter().map(Into::into).collect(),
        }
    }
}

/// Result of `GistCore::sweep_orphaned_files` — counts only, never which
/// files were touched. See `gist_core::SweepOutcome`.
#[derive(uniffi::Record)]
pub struct FfiSweepOutcome {
    pub files_scanned: u32,
    pub files_deleted: u32,
    /// Files that vanished between this sweep listing the directory and
    /// acting on it. Rare, benign, not a failure — same meaning as
    /// `FfiRemoveOutcome.files_missing`.
    pub files_missing: u32,
    pub files_failed: u32,
    pub failure_kinds: Vec<FfiFileDeleteFailureKind>,
}

impl From<gist_core::SweepOutcome> for FfiSweepOutcome {
    fn from(o: gist_core::SweepOutcome) -> Self {
        FfiSweepOutcome {
            files_scanned: o.files_scanned,
            files_deleted: o.files_deleted,
            files_missing: o.files_missing,
            files_failed: o.files_failed,
            failure_kinds: o.failure_kinds.into_iter().map(Into::into).collect(),
        }
    }
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

    /// Encrypted counterpart to `new` (ADR-011): document content, token
    /// streams, and original-file copies are encrypted at rest under a key
    /// supplied by `key_provider` (Keychain-backed on Apple). See
    /// `gist_core::Core::init_encrypted`/`gist_store::Store::open_encrypted`
    /// for the migration story — an existing store opened this way keeps its
    /// pre-existing plaintext rows readable; only content written after this
    /// call is encrypted.
    #[uniffi::constructor]
    pub fn new_encrypted(
        db_path: String,
        storage_dir: String,
        key_provider: Box<dyn KeyProvider>,
    ) -> Result<Arc<Self>, GistError> {
        ffi_catch!({
            let adapter: Arc<dyn gist_core::KeyProvider> =
                Arc::new(CoreKeyProviderAdapter(key_provider));
            let core = gist_core::Core::init_encrypted(
                &PathBuf::from(&db_path),
                &PathBuf::from(&storage_dir),
                adapter,
            )
            .map_err(GistError::from)?;
            Ok(Arc::new(Self { inner: core }))
        })
    }

    /// Read-only-decryption counterpart to `new` (ADR-014): new imports
    /// through this `GistCore` still land plaintext by default, exactly
    /// like plain `new` — but `key_provider` lets already-encrypted items
    /// (written via `encrypt_items`, or historically via `new_encrypted`)
    /// be read back through this instance instead of failing with
    /// `MissingKeyProvider`. See `gist_core::Core::init_with_read_key`/
    /// `gist_store::Store::open_with_read_key` for the full rationale —
    /// this is the constructor `CoreClient.shared`'s production instance
    /// now uses, so per-item "Encrypt" (`encrypt_items`) no longer locks a
    /// user out of reading the item they just encrypted.
    #[uniffi::constructor]
    pub fn new_with_read_key(
        db_path: String,
        storage_dir: String,
        key_provider: Box<dyn KeyProvider>,
    ) -> Result<Arc<Self>, GistError> {
        ffi_catch!({
            let adapter: Arc<dyn gist_core::KeyProvider> =
                Arc::new(CoreKeyProviderAdapter(key_provider));
            let core = gist_core::Core::init_with_read_key(
                &PathBuf::from(&db_path),
                &PathBuf::from(&storage_dir),
                adapter,
            )
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
                    content_encrypted: i.content_encrypted,
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
                    content_encrypted: i.content_encrypted,
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

    /// `remove_items`, but reporting what actually happened (review Q10).
    ///
    /// Additive: `remove_items` above keeps its exact signature and
    /// behaviour (it is this call with the outcome discarded), so existing
    /// callers — including Apple's `CoreClient` — are unaffected.
    ///
    /// Use this where the UI shows a removal result. On Windows a stored
    /// file can genuinely fail to delete while the library row is gone
    /// (antivirus, the search indexer, a backup agent or another app holding
    /// it open without `FILE_SHARE_DELETE`), and reporting a clean removal
    /// in that case is a lie about the user's data. Files left behind this
    /// way are reclaimed by `sweep_orphaned_files` on a later launch.
    pub fn remove_items_detailed(
        &self,
        ids: Vec<String>,
        delete_source_files: bool,
    ) -> Result<FfiRemoveOutcome, GistError> {
        ffi_catch!({
            self.inner
                .remove_items_detailed(&ids, delete_source_files)
                .map(FfiRemoveOutcome::from)
                .map_err(GistError::from)
        })
    }

    /// Delete storage-directory files that no library row references any
    /// more, returning counts only (review Q10). Intended to run at app
    /// launch, as the cleanup pass for anything `remove_items_detailed`
    /// could not delete at the time.
    ///
    /// Never deletes a file any row still references (including a
    /// content-addressed ADR-006 original shared by several items), never
    /// touches anything outside the storage directory, and never follows a
    /// symlink or Windows junction out of it. See
    /// `gist_core::Core::sweep_orphaned_files` for the full contract.
    pub fn sweep_orphaned_files(&self) -> Result<FfiSweepOutcome, GistError> {
        ffi_catch!({
            self.inner
                .sweep_orphaned_files()
                .map(FfiSweepOutcome::from)
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
                    content_encrypted: i.content_encrypted,
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

    /// Return the names of every tag that exists across the library.
    /// See `gist_core::Core::list_all_tags`.
    pub fn list_all_tags(&self) -> Result<Vec<String>, GistError> {
        ffi_catch!({ self.inner.list_all_tags().map_err(GistError::from) })
    }

    /// Return all library items tagged with `tag_name` (newest first).
    /// See `gist_core::Core::list_items_by_tag`.
    pub fn list_items_by_tag(&self, tag_name: String) -> Result<Vec<FfiLibraryItem>, GistError> {
        ffi_catch!({
            let items = self
                .inner
                .list_items_by_tag(&tag_name)
                .map_err(GistError::from)?;
            Ok(items
                .into_iter()
                .map(|i| FfiLibraryItem {
                    id: i.id,
                    title: i.title,
                    authors: i.authors,
                    source_path: i.source_path,
                    cover_path: i.cover_path,
                    content_encrypted: i.content_encrypted,
                })
                .collect())
        })
    }

    /// Create a new annotation (highlight/note/bookmark), anchored per
    /// ADR-003 as `(block_id, start, len, prefix_hash, quote_hash)`. Returns
    /// the generated annotation id. `prefix_hash`/`quote_hash` must be
    /// computed by the caller (the reading view) — this call only persists
    /// them, it never computes or verifies a hash itself.
    /// See `gist_core::Core::create_annotation`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_annotation(
        &self,
        item_id: String,
        kind: FfiAnnotationKind,
        block_id: String,
        start: u64,
        len: u64,
        prefix_hash: u64,
        quote_hash: u64,
        note_text: Option<String>,
    ) -> Result<String, GistError> {
        ffi_catch!({
            self.inner
                .create_annotation(
                    &item_id,
                    kind.into(),
                    &block_id,
                    start as usize,
                    len as usize,
                    prefix_hash,
                    quote_hash,
                    note_text.as_deref(),
                )
                .map_err(GistError::from)
        })
    }

    /// Return all annotations for one item, newest first.
    /// See `gist_core::Core::list_annotations_for_item`.
    pub fn list_annotations_for_item(
        &self,
        item_id: String,
    ) -> Result<Vec<FfiAnnotation>, GistError> {
        ffi_catch!({
            let annotations = self
                .inner
                .list_annotations_for_item(&item_id)
                .map_err(GistError::from)?;
            Ok(annotations.into_iter().map(FfiAnnotation::from).collect())
        })
    }

    /// Update an annotation's note text (e.g. editing a `Note`'s body, or
    /// clearing it by passing `None`). Never touches the anchor fields.
    /// See `gist_core::Core::update_annotation_note`.
    pub fn update_annotation_note(
        &self,
        id: String,
        note_text: Option<String>,
    ) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .update_annotation_note(&id, note_text.as_deref())
                .map_err(GistError::from)
        })
    }

    /// Delete a single annotation. See `gist_core::Core::delete_annotation`.
    pub fn delete_annotation(&self, id: String) -> Result<(), GistError> {
        ffi_catch!({ self.inner.delete_annotation(&id).map_err(GistError::from) })
    }

    /// Delete one or more annotations; unknown ids are silently skipped.
    /// Returns the number of annotations actually deleted.
    /// See `gist_core::Core::delete_annotations`.
    pub fn delete_annotations(&self, ids: Vec<String>) -> Result<u64, GistError> {
        ffi_catch!({
            self.inner
                .delete_annotations(&ids)
                .map(|n| n as u64)
                .map_err(GistError::from)
        })
    }

    /// Retroactively encrypt one or more already-imported items at rest, on
    /// demand (ADR-014) — reuses the same `KeyProvider` callback-interface
    /// machinery `new_encrypted`/`new_with_read_key` use
    /// (`CoreKeyProviderAdapter`), rather than inventing a second mechanism.
    /// See `gist_core::Core::encrypt_items`/`gist_store::Store::
    /// encrypt_item` for the full design. Reading an item's content back
    /// after this call requires the `GistCore` it's read through to have
    /// decryption capability — `new_with_read_key` (what production now
    /// uses) or `new_encrypted` both qualify; a `GistCore` constructed via
    /// plain `new` still correctly cannot decrypt such an item.
    ///
    /// Returns one [`FfiEncryptItemResult`] per id, in the same order as
    /// `ids`, rather than failing the whole call on the first per-id error.
    pub fn encrypt_items(
        &self,
        ids: Vec<String>,
        key_provider: Box<dyn KeyProvider>,
    ) -> Result<Vec<FfiEncryptItemResult>, GistError> {
        ffi_catch!({
            let adapter: Arc<dyn gist_core::KeyProvider> =
                Arc::new(CoreKeyProviderAdapter(key_provider));
            Ok(self
                .inner
                .encrypt_items(&ids, adapter)
                .into_iter()
                .map(FfiEncryptItemResult::from)
                .collect())
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

/// Test-only support: lets a *release-profile* binary prove that `ffi_catch!`
/// really contains panics (i.e. that the release profile does not set
/// `panic = "abort"`). Not part of the uniffi surface; compiled only with the
/// `test-panic` feature, which shipped builds never enable.
#[cfg(feature = "test-panic")]
pub mod test_support {
    use super::*;

    /// Panics inside `ffi_catch!`; a correct build returns `Err(InternalPanic)`.
    pub fn ffi_panic_probe() -> Result<(), GistError> {
        ffi_catch!({ panic!("ffi_panic_probe: deliberate panic") })
    }

    /// The same probe, exposed across the uniffi boundary so a Swift test can
    /// prove the *whole* chain (Rust panic -> `ffi_catch!` -> uniffi Swift
    /// binding -> `GistError.InternalPanic` thrown in Swift), not just the
    /// Rust-internal half the `panic_containment` example already covers.
    /// `#[cfg(feature = "test-panic")]` keeps this out of every build that
    /// doesn't opt in, so it never reaches a shipped xcframework.
    #[uniffi::export]
    pub fn ffi_panic_probe_uniffi() -> Result<(), GistError> {
        ffi_panic_probe()
    }
}
