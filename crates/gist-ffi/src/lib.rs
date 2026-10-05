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
    /// Mirrors `gist_store::StoreError::ChecksumMismatch` (ADR-013 /
    /// security register `A4`, `R3`) as its own variant, for the same
    /// reason `DrmProtected` is one: so Swift can switch on
    /// `.ChecksumMismatch` for a dedicated "this file was corrupted,
    /// consider re-importing it" presentation instead of parsing `Core`'s
    /// generic message text. Reachable from any call that reads document
    /// content (`get_document_json`, `start_rsvp`, …), not just the
    /// explicit `verify_item_integrity`/`verify_library_integrity` checks
    /// below — a real read hitting corrupted content should surface just
    /// as distinctly as an explicit integrity check finding it. `path` is
    /// this app's own internal storage path (e.g. `<id>.json` under the
    /// sandboxed storage directory), never the user's original file — see
    /// `CLAUDE.md`'s "Source paths" policy for why that distinction
    /// matters; `source_ref`/`source_path` never appear here.
    #[error("checksum mismatch for {path}: file appears corrupted on disk")]
    ChecksumMismatch { path: String },
    /// Password-protected PDF (never bypassed). Own variant so Swift can show
    /// a dedicated message, same reason as `DrmProtected`.
    #[error("this PDF is password-protected and cannot be imported")]
    PdfEncrypted,
    /// PDF with no extractable text (scan / image-only): Swift routes this to
    /// the OCR flow instead of showing an error.
    #[error("this PDF has no text layer (it appears to be a scan)")]
    PdfNoTextLayer,
    /// pdfium could not be loaded in this build/bundle.
    #[error("PDF support is unavailable in this build")]
    PdfUnavailable,
    // ── Typed resource-limit errors (M7 R3) ────────────────────────────────
    // One flat variant per `gist_model::LimitKind` (uniffi `flat_error`
    // cannot carry a payload enum), so Swift/C# switch on the case and show
    // an honest, specific message. Messages are fixed text: no paths, no
    // limit numbers, no panic payloads.
    /// The input is larger than GIST's per-file size limit.
    #[error("this file is too large for GIST's import limits")]
    ResourceLimitTooLarge,
    /// More pages (or chapters) than GIST's page limit.
    #[error("this document has too many pages for GIST's import limits")]
    ResourceLimitTooManyPages,
    /// More internal archive entries than GIST's limit (epub/docx).
    #[error("this file contains too many internal parts for GIST's import limits")]
    ResourceLimitTooManyEntries,
    /// Structure nested deeper than GIST's limit.
    #[error("this document is nested too deeply for GIST's import limits")]
    ResourceLimitTooDeeplyNested,
    /// Extracted/decompressed content exceeds GIST's size budget.
    #[error("this document's content is too large for GIST's import limits")]
    ResourceLimitContentTooLarge,
    /// A table with too many rows or columns.
    #[error("this document contains a table that is too large for GIST's import limits")]
    ResourceLimitTableTooLarge,
    /// Any other limit.
    #[error("this document exceeds GIST's import limits")]
    ResourceLimitOther,
    #[error("internal error")]
    InternalPanic,
}

/// Maps the importers' typed [`gist_model::LimitKind`] to the matching flat
/// `GistError` case. Exhaustive on purpose: adding a `LimitKind` forces a
/// decision here.
fn limit_kind_to_error(kind: gist_model::LimitKind) -> GistError {
    use gist_model::LimitKind as K;
    match kind {
        K::TooLarge => GistError::ResourceLimitTooLarge,
        K::TooManyPages => GistError::ResourceLimitTooManyPages,
        K::TooManyEntries => GistError::ResourceLimitTooManyEntries,
        K::TooDeeplyNested => GistError::ResourceLimitTooDeeplyNested,
        K::ExpandedTooLarge => GistError::ResourceLimitContentTooLarge,
        K::TableTooLarge => GistError::ResourceLimitTableTooLarge,
        K::Other => GistError::ResourceLimitOther,
    }
}

impl From<gist_core::CoreError> for GistError {
    fn from(e: gist_core::CoreError) -> Self {
        if let Some(kind) = e.limit_kind() {
            return limit_kind_to_error(kind);
        }
        match e {
            gist_core::CoreError::Store(gist_store::StoreError::ChecksumMismatch { path }) => {
                GistError::ChecksumMismatch { path }
            }
            other => GistError::Core(other.to_string()),
        }
    }
}

impl From<gist_core::ImportError> for GistError {
    fn from(e: gist_core::ImportError) -> Self {
        match e {
            gist_core::ImportError::DrmProtected => GistError::DrmProtected,
            gist_core::ImportError::PdfEncrypted => GistError::PdfEncrypted,
            gist_core::ImportError::PdfNoTextLayer => GistError::PdfNoTextLayer,
            gist_core::ImportError::PdfUnavailable(_) => GistError::PdfUnavailable,
            gist_core::ImportError::ResourceLimitExceeded { kind, .. } => limit_kind_to_error(kind),
            gist_core::ImportError::Store(gist_store::StoreError::ChecksumMismatch { path }) => {
                GistError::ChecksumMismatch { path }
            }
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

/// Result of a multi-page OCR import — mirrors `gist_core::OcrImportResult`.
/// `page_confidences` is in page order (`ocrConfidence[]`); the OCR review
/// screen (M3 role `R8`) uses it to highlight low-confidence pages without a
/// second FFI round trip.
#[derive(Debug, uniffi::Record)]
pub struct FfiOcrImportResult {
    pub item_id: String,
    pub page_confidences: Vec<f32>,
}

impl From<gist_core::OcrImportResult> for FfiOcrImportResult {
    fn from(r: gist_core::OcrImportResult) -> Self {
        FfiOcrImportResult {
            item_id: r.item_id,
            page_confidences: r.page_confidences,
        }
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
    /// Normalised source format: "txt" / "epub" / "docx" / "web" / "pdf" /
    /// "ocr", or "" if unknown (ADR-021).
    pub source_type: String,
    /// Unix milliseconds of the last time either reader opened the item;
    /// `None` = never opened (ADR-021).
    pub last_opened_at: Option<i64>,
    /// Derived RSVP progress in `0.0..=1.0` (ADR-021). Flow-view-only items
    /// report 0.0 — accepted limitation.
    pub progress_fraction: f64,
}

impl From<gist_store::LibraryItem> for FfiLibraryItem {
    fn from(i: gist_store::LibraryItem) -> Self {
        FfiLibraryItem {
            id: i.id,
            title: i.title,
            authors: i.authors,
            source_path: i.source_path,
            cover_path: i.cover_path,
            content_encrypted: i.content_encrypted,
            source_type: i.source_type,
            last_opened_at: i.last_opened_at,
            progress_fraction: i.progress_fraction,
        }
    }
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

// ── Annotation re-anchoring (ADR-003) ───────────────────────────────────────

/// Mirrors `gist_core::anchoring::AnchorStatus` as a uniffi-exportable enum.
/// Kept data-free, like every other uniffi enum in this file, since a
/// uniffi enum with a per-variant payload doesn't map as cleanly to Swift
/// as a flat enum plus an `Option` field on the containing record (the same
/// pattern `FfiEncryptItemResult` above uses) — `FfiAnnotationAnchorResult
/// .old_start` carries `Reanchored`'s only payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiAnchorStatus {
    /// `prefix_hash`/`quote_hash` both matched at the stored position —
    /// nothing changed.
    Valid,
    /// The stored anchor didn't verify in place, but the quoted text was
    /// found elsewhere in the same block and the anchor was rewritten.
    Reanchored,
    /// The quoted text could not be found anywhere in the block. Left
    /// exactly as stored — never deleted or silently moved.
    Orphaned,
}

impl From<&gist_core::AnchorStatus> for FfiAnchorStatus {
    fn from(s: &gist_core::AnchorStatus) -> Self {
        match s {
            gist_core::AnchorStatus::Valid => FfiAnchorStatus::Valid,
            gist_core::AnchorStatus::Reanchored { .. } => FfiAnchorStatus::Reanchored,
            gist_core::AnchorStatus::Orphaned => FfiAnchorStatus::Orphaned,
        }
    }
}

/// Result of re-anchoring one annotation via
/// `GistCore::reanchor_annotations` (ADR-003): the annotation's state after
/// the check — already rewritten in the store if `status == .reanchored` —
/// paired with what happened. `old_start` is `Some` only when
/// `status == .reanchored`, letting a reading view explain the move (e.g.
/// "this highlight moved") without a second round trip.
#[derive(uniffi::Record)]
pub struct FfiAnnotationAnchorResult {
    pub annotation: FfiAnnotation,
    pub status: FfiAnchorStatus,
    pub old_start: Option<u64>,
}

impl From<gist_core::AnnotationAnchorResult> for FfiAnnotationAnchorResult {
    fn from(r: gist_core::AnnotationAnchorResult) -> Self {
        let status = FfiAnchorStatus::from(&r.status);
        let old_start = match r.status {
            gist_core::AnchorStatus::Reanchored { old_start, .. } => Some(old_start as u64),
            _ => None,
        };
        FfiAnnotationAnchorResult {
            annotation: r.annotation.into(),
            status,
            old_start,
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

// ── At-rest integrity verification (ADR-013 / security register `A4`, `R3`) ─

/// Mirrors `gist_core::ChecksumStatus` (re-exported from `gist-store`) as a
/// uniffi-exportable enum. Kept data-free like every other enum in this
/// file, so `FfiItemIntegrityReport`'s fields carry it directly rather than
/// needing a companion `Option` payload field the way
/// `FfiAnnotationAnchorResult.old_start` does for `Reanchored`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiChecksumStatus {
    /// A checksum sidecar exists and matches the file's current bytes.
    Verified,
    /// The file exists but has no checksum sidecar (pre-`A4` data). Not an
    /// error — see `gist_store::verify_checksum`'s backfill policy.
    Unverified,
    /// A checksum sidecar exists but disagrees with the file's current
    /// bytes — genuine corruption.
    Mismatch,
    /// The file itself does not exist on disk at all.
    Missing,
}

impl From<gist_core::ChecksumStatus> for FfiChecksumStatus {
    fn from(s: gist_core::ChecksumStatus) -> Self {
        match s {
            gist_core::ChecksumStatus::Verified => FfiChecksumStatus::Verified,
            gist_core::ChecksumStatus::Unverified => FfiChecksumStatus::Unverified,
            gist_core::ChecksumStatus::Mismatch => FfiChecksumStatus::Mismatch,
            gist_core::ChecksumStatus::Missing => FfiChecksumStatus::Missing,
        }
    }
}

/// Mirrors `gist_core::IntegrityStatus` as a uniffi-exportable enum — the
/// three-state verdict `GistCore::verifyItemIntegrity`/
/// `verifyLibraryIntegrity` (`R4`'s "Verify Library Integrity" action)
/// present per item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiIntegrityStatus {
    /// Every file this item has matched its recorded checksum.
    Pass,
    /// No file disagreed with its checksum, but at least one has no
    /// checksum to check against — not confirmed bad, not confirmed good.
    Unverified,
    /// At least one file's bytes no longer match its recorded checksum —
    /// genuine corruption.
    Failed,
}

impl From<gist_core::IntegrityStatus> for FfiIntegrityStatus {
    fn from(s: gist_core::IntegrityStatus) -> Self {
        match s {
            gist_core::IntegrityStatus::Pass => FfiIntegrityStatus::Pass,
            gist_core::IntegrityStatus::Unverified => FfiIntegrityStatus::Unverified,
            gist_core::IntegrityStatus::Failed => FfiIntegrityStatus::Failed,
        }
    }
}

/// Mirrors `gist_core::ItemIntegrityReport`'s underlying
/// `gist_store::ItemIntegrityReport`: the per-file breakdown one
/// `FfiItemIntegrityOutcome.status` was aggregated from, for a caller that
/// wants more detail than the three-state summary (e.g. "your original
/// copy is missing but your reading copy is fine").
#[derive(uniffi::Record)]
pub struct FfiItemIntegrityReport {
    pub doc_status: FfiChecksumStatus,
    pub tokens_status: FfiChecksumStatus,
    /// `None` when this item has no ADR-006 sandboxed original copy (a URL
    /// import, or one that predates ADR-006).
    pub original_copy_status: Option<FfiChecksumStatus>,
}

impl From<gist_core::ItemIntegrityReport> for FfiItemIntegrityReport {
    fn from(r: gist_core::ItemIntegrityReport) -> Self {
        FfiItemIntegrityReport {
            doc_status: r.doc_status.into(),
            tokens_status: r.tokens_status.into(),
            original_copy_status: r.original_copy_status.map(FfiChecksumStatus::from),
        }
    }
}

/// Per-item result of `GistCore::verifyItemIntegrity`/
/// `verifyLibraryIntegrity` (`R3`, ADR-013 / `A4`).
#[derive(uniffi::Record)]
pub struct FfiItemIntegrityOutcome {
    pub id: String,
    pub status: FfiIntegrityStatus,
    pub report: FfiItemIntegrityReport,
}

impl From<gist_core::ItemIntegrityOutcome> for FfiItemIntegrityOutcome {
    fn from(o: gist_core::ItemIntegrityOutcome) -> Self {
        FfiItemIntegrityOutcome {
            id: o.id,
            status: o.status.into(),
            report: o.report.into(),
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

// ── RSVP pacing (docs/windows-development-plan.md §4.3) ─────────────────────

/// Mirrors `gist_rsvp::PlayState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiPlayState {
    Playing,
    Paused,
}

impl From<&gist_rsvp::PlayState> for FfiPlayState {
    fn from(s: &gist_rsvp::PlayState) -> Self {
        match s {
            gist_rsvp::PlayState::Playing => FfiPlayState::Playing,
            gist_rsvp::PlayState::Paused => FfiPlayState::Paused,
        }
    }
}

/// Mirrors `gist_model::TokenKind` — what a token in the RSVP stream is, so
/// a reader view can render a paragraph/section break differently from a
/// word without re-deriving it from the token text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiTokenKind {
    Word,
    ParagraphBreak,
    SectionBreak,
}

impl From<&gist_model::TokenKind> for FfiTokenKind {
    fn from(k: &gist_model::TokenKind) -> Self {
        match k {
            gist_model::TokenKind::Word => FfiTokenKind::Word,
            gist_model::TokenKind::ParagraphBreak => FfiTokenKind::ParagraphBreak,
            gist_model::TokenKind::SectionBreak => FfiTokenKind::SectionBreak,
        }
    }
}

/// Mirrors `gist_rsvp::SessionStats`.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct FfiSessionStats {
    pub words_shown: u32,
    /// Pace actually achieved, which differs from the configured WPM once
    /// punctuation pauses and back-word rewinds are factored in. `0.0` while
    /// no play time has elapsed, never a division by zero.
    pub estimated_wpm: f32,
    /// Total play time in ms, excluding paused gaps.
    pub duration_ms: u64,
}

impl From<gist_rsvp::SessionStats> for FfiSessionStats {
    fn from(s: gist_rsvp::SessionStats) -> Self {
        FfiSessionStats {
            words_shown: s.words_shown,
            estimated_wpm: s.estimated_wpm,
            duration_ms: s.duration_ms,
        }
    }
}

/// A word split at its Optimal Recognition Point, ready to render as three
/// runs (the middle one highlighted/aligned).
///
/// Deliberately pre-split rather than exposing `gist_rsvp::orp_index`'s
/// return value directly: that is a **byte** offset into UTF-8, which is
/// meaningless in C# (UTF-16) and error-prone in Swift, and slicing a string
/// at a wrong offset is exactly the class of bug the ORP code already goes
/// to trouble to avoid (it works in grapheme clusters so it never splits an
/// emoji, flag or accented letter). Handing over the three pieces makes that
/// impossible to get wrong in a client and means ORP does not become a
/// fourth hand-port of engine logic.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiOrpSplit {
    /// Text before the focus cluster. Empty when the focus is the first
    /// cluster of the word.
    pub before: String,
    /// The single grapheme cluster to highlight. Empty only for an empty
    /// word.
    pub focus: String,
    /// Text after the focus cluster.
    pub after: String,
}

impl FfiOrpSplit {
    fn of(word: &str) -> Self {
        let (before, focus, after) = gist_rsvp::orp_split(word);
        FfiOrpSplit {
            before: before.to_owned(),
            focus: focus.to_owned(),
            after: after.to_owned(),
        }
    }
}

/// Everything a reader view needs to draw one frame, in a single FFI call.
///
/// Returned by [`FfiRsvpSession::frame_at_elapsed`]. The individual
/// accessors next to it return the same values one at a time; this exists so
/// the per-tick path is one call and one lock acquisition rather than five,
/// which matters for W4's "10-minute soak at 600 WPM" exit criterion.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRsvpFrame {
    /// Index of the token to display.
    pub index: u64,
    pub text: String,
    pub kind: FfiTokenKind,
    /// `text` split at its ORP. For a break token (empty text) every field
    /// is empty.
    pub orp: FfiOrpSplit,
    /// How long this token should stay on screen, in ms.
    pub duration_ms: u64,
    /// Elapsed-ms value (on the same clock as `frame_at_elapsed`'s argument)
    /// at which `index` stops being the token to display. A client's timer
    /// should wake at this, not after a fixed interval, and re-ask — which
    /// is what keeps a late or coalesced tick from compounding into drift.
    ///
    /// When `is_last` is true there is nothing to advance to, so this is
    /// simply when the final token's own display time runs out: a client
    /// should stop playback there rather than scheduling another tick.
    pub next_boundary_ms: u64,
    /// True when `index` is the last token in the stream, i.e. playback has
    /// nothing left to advance to.
    pub is_last: bool,
    /// Total tokens in the stream, so a progress readout needs no second
    /// call.
    pub token_count: u64,
}

/// The RSVP pacing engine, live, as an object a UI drives directly.
///
/// This exists so no native shell has to re-implement pacing.
/// `docs/windows-development-plan.md` §4.3: Apple's `RsvpPlayer`/
/// `RsvpWallClockEngine` is already a hand-port of `token_duration_ms` and
/// its punctuation helpers (and `RsvpStats` a hand-port of the stats
/// arithmetic), kept in sync by hand; **Windows must not become a third
/// copy.** A client drives this by re-anchoring to a monotonic clock
/// (`Stopwatch`/`DispatcherQueueTimer` on Windows) and asking
/// [`frame_at_elapsed`](FfiRsvpSession::frame_at_elapsed) what to show for
/// the elapsed time it measured — never by sleeping a fixed interval per
/// token and trusting the sleep.
///
/// # Threading
///
/// `gist_rsvp::RsvpSession`'s mutators take `&mut self` while every uniffi
/// export takes `&self`, so the session lives behind a `Mutex`. Every
/// acquisition is poison-tolerant (`.unwrap_or_else(|p| p.into_inner())`)
/// per this project's mutex policy: a panic inside one method must not
/// permanently brick the reader, and `ffi_catch!` has already converted
/// that panic into a returned error for the caller.
///
/// # Index arguments
///
/// Indices cross this boundary as `u64` (the file's existing convention,
/// cf. `save_progress`) and are converted with a saturating cast, never a
/// bare `as`. Nothing here indexes the token slice with a client-supplied
/// integer: out-of-range reads go through `Option`/clamping, matching the
/// engine's own behaviour.
#[derive(uniffi::Object)]
pub struct FfiRsvpSession {
    inner: std::sync::Mutex<gist_rsvp::RsvpSession>,
}

/// `u64` from a client -> `usize`, saturating rather than truncating. On a
/// 32-bit target a huge value becomes `usize::MAX`, which every engine
/// entry point already clamps; it must never wrap round to a small
/// in-range index.
fn ffi_index(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// `usize` -> `u64` for a return value. Saturating for symmetry; cannot
/// actually saturate on any target this ships to.
fn ffi_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

impl FfiRsvpSession {
    fn new(session: gist_rsvp::RsvpSession) -> Arc<Self> {
        Arc::new(Self {
            inner: std::sync::Mutex::new(session),
        })
    }

    /// Poison-tolerant lock, per this project's mutex policy. A bare
    /// `.lock().unwrap()` is forbidden.
    fn lock(&self) -> std::sync::MutexGuard<'_, gist_rsvp::RsvpSession> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[uniffi::export]
impl FfiRsvpSession {
    // ── Read-only accessors ───────────────────────────────────────────────

    /// Total number of tokens in the stream (words plus breaks).
    pub fn token_count(&self) -> Result<u64, GistError> {
        ffi_catch!({ Ok(ffi_u64(self.lock().tokens.len())) })
    }

    /// Index the current play window starts from — where the last
    /// `pause`/`seek`/`back_words`/`set_wpm` left the reader, and the point
    /// `elapsed_ms` arguments are measured relative to.
    pub fn cursor(&self) -> Result<u64, GistError> {
        ffi_catch!({ Ok(ffi_u64(self.lock().cursor)) })
    }

    pub fn play_state(&self) -> Result<FfiPlayState, GistError> {
        ffi_catch!({ Ok((&self.lock().state).into()) })
    }

    /// The effective words-per-minute, already clamped to 100–1000 by the
    /// core (`docs/windows-ui-spec.md` §7.1: "Range enforced by the core,
    /// not just the control").
    pub fn wpm(&self) -> Result<u32, GistError> {
        ffi_catch!({ Ok(self.lock().config.wpm) })
    }

    /// Text of the token at `index`, or `None` if `index` is out of range.
    pub fn token_text(&self, index: u64) -> Result<Option<String>, GistError> {
        ffi_catch!({
            Ok(self
                .lock()
                .tokens
                .get(ffi_index(index))
                .map(|t| t.text.clone()))
        })
    }

    /// Kind of the token at `index`, or `None` if `index` is out of range.
    pub fn token_kind(&self, index: u64) -> Result<Option<FfiTokenKind>, GistError> {
        ffi_catch!({
            Ok(self
                .lock()
                .tokens
                .get(ffi_index(index))
                .map(|t| (&t.kind).into()))
        })
    }

    /// Whether `index` is the last token in the stream. `false` for an
    /// out-of-range index and for an empty stream — there is no last token
    /// to be.
    pub fn is_last_token(&self, index: u64) -> Result<bool, GistError> {
        ffi_catch!({
            let session = self.lock();
            let len = session.tokens.len();
            Ok(len > 0 && ffi_index(index) == len - 1)
        })
    }

    /// `text` of the token at `index`, split at its Optimal Recognition
    /// Point. `None` if `index` is out of range.
    pub fn orp_split(&self, index: u64) -> Result<Option<FfiOrpSplit>, GistError> {
        ffi_catch!({
            Ok(self
                .lock()
                .tokens
                .get(ffi_index(index))
                .map(|t| FfiOrpSplit::of(&t.text)))
        })
    }

    // ── Pacing ────────────────────────────────────────────────────────────

    /// How long the token at `index` should be displayed, in ms. `0` for an
    /// out-of-range index (matching the engine).
    pub fn token_duration_ms(&self, index: u64) -> Result<u64, GistError> {
        ffi_catch!({ Ok(self.lock().token_duration_ms(ffi_index(index))) })
    }

    /// Index of the token that should be on screen `elapsed_ms` after the
    /// last [`resume`](FfiRsvpSession::resume), clamped to the last token
    /// once the stream runs out. Pure — does not advance the cursor.
    pub fn token_at_elapsed(&self, elapsed_ms: u64) -> Result<u64, GistError> {
        ffi_catch!({ Ok(ffi_u64(self.lock().token_at_elapsed(elapsed_ms))) })
    }

    /// Everything needed to draw one frame at `elapsed_ms`, in one call.
    /// `None` only when the token stream is empty.
    ///
    /// This is the per-tick entry point: a client measures elapsed time on a
    /// monotonic clock, calls this, renders, and sleeps until
    /// `next_boundary_ms` before asking again. Because the answer is
    /// recomputed from measured elapsed time every time rather than
    /// accumulated, a late or coalesced tick self-corrects instead of
    /// compounding into drift.
    pub fn frame_at_elapsed(&self, elapsed_ms: u64) -> Result<Option<FfiRsvpFrame>, GistError> {
        ffi_catch!({
            let session = self.lock();
            let len = session.tokens.len();
            if len == 0 {
                return Ok(None);
            }
            let index = session.token_at_elapsed(elapsed_ms);
            let Some(token) = session.tokens.get(index) else {
                // Unreachable: `token_at_elapsed` clamps into range for a
                // non-empty stream. Reported rather than panicked.
                return Ok(None);
            };
            // Asked of the engine rather than summed here: the engine has it
            // memoised, so this stays O(log k) instead of walking every
            // token since the cursor on every tick.
            let next_boundary_ms = session.elapsed_at_token_end(index);
            Ok(Some(FfiRsvpFrame {
                index: ffi_u64(index),
                text: token.text.clone(),
                kind: (&token.kind).into(),
                orp: FfiOrpSplit::of(&token.text),
                duration_ms: session.token_duration_ms(index),
                next_boundary_ms,
                is_last: index == len - 1,
                token_count: ffi_u64(len),
            }))
        })
    }

    // ── Mutations ─────────────────────────────────────────────────────────

    /// Jump to `index`, clamped to the last token. The caller must reset its
    /// elapsed counter afterwards.
    pub fn seek(&self, index: u64) -> Result<(), GistError> {
        ffi_catch!({
            self.lock().seek(ffi_index(index));
            Ok(())
        })
    }

    /// Pause at `elapsed_ms`: pins the cursor to the token that was on
    /// screen and adds `elapsed_ms` to the session's accumulated play time,
    /// so a pause gap never counts as reading time.
    pub fn pause(&self, elapsed_ms: u64) -> Result<(), GistError> {
        ffi_catch!({
            self.lock().pause(elapsed_ms);
            Ok(())
        })
    }

    /// Resume playing. The caller must restart its elapsed counter from 0 at
    /// the same moment.
    pub fn resume(&self) -> Result<(), GistError> {
        ffi_catch!({
            self.lock().resume();
            Ok(())
        })
    }

    /// Jump back `n` **word** tokens from the position current at
    /// `elapsed_ms` (paragraph/section breaks are skipped over, not
    /// counted). Saturates at the start of the stream. The caller must reset
    /// its elapsed counter afterwards.
    pub fn back_words(&self, n: u64, elapsed_ms: u64) -> Result<(), GistError> {
        ffi_catch!({
            self.lock().back_words(ffi_index(n), elapsed_ms);
            Ok(())
        })
    }

    /// Change speed mid-session. The position current at `elapsed_ms` is
    /// pinned **under the old speed** before the new one is applied, so a
    /// speed change never jumps the reader. `wpm` is clamped to 100–1000
    /// here, in the core — a UI control's own range is not relied on. The
    /// caller must reset its elapsed counter afterwards.
    pub fn set_wpm(&self, wpm: u32, elapsed_ms: u64) -> Result<(), GistError> {
        ffi_catch!({
            self.lock().set_wpm(wpm, elapsed_ms);
            Ok(())
        })
    }

    // ── Stats ─────────────────────────────────────────────────────────────

    /// Live stats at `elapsed_ms`: words actually shown, play time
    /// excluding paused gaps, and the pace achieved. While paused,
    /// `elapsed_ms` is ignored and the pinned cursor is used.
    ///
    /// This is what a stats readout should call. [`stats`](FfiRsvpSession::stats)
    /// reports the engine's caller-maintained counter instead, which nothing
    /// in this stack increments.
    pub fn stats_at_elapsed(&self, elapsed_ms: u64) -> Result<FfiSessionStats, GistError> {
        ffi_catch!({ Ok(self.lock().stats_at_elapsed(elapsed_ms).into()) })
    }

    /// `gist_rsvp::RsvpSession::stats()` verbatim.
    ///
    /// Its `words_shown`/`estimated_wpm` come from the engine's
    /// `session_words_shown` field, which **nothing in this stack ever
    /// increments** — so they read `0` unless a caller maintains it, which
    /// no caller can from here. Exposed for completeness of the session
    /// surface; use [`stats_at_elapsed`](FfiRsvpSession::stats_at_elapsed)
    /// for anything a user sees.
    pub fn stats(&self) -> Result<FfiSessionStats, GistError> {
        ffi_catch!({ Ok(self.lock().stats().into()) })
    }

    /// Number of **word** tokens strictly before `index` — the words a
    /// reader has already been shown when `index` is on screen. Clamped, so
    /// an out-of-range index returns the stream's whole word count.
    pub fn words_shown_through(&self, index: u64) -> Result<u32, GistError> {
        ffi_catch!({ Ok(self.lock().words_shown_through(ffi_index(index))) })
    }
}

/// Split an arbitrary word at its Optimal Recognition Point, without a
/// session. Same engine function the reader uses, exposed for previews and
/// settings screens that render a sample word.
#[uniffi::export]
pub fn rsvp_orp_split(word: String) -> Result<FfiOrpSplit, GistError> {
    ffi_catch!({ Ok(FfiOrpSplit::of(&word)) })
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
            Ok(items.into_iter().map(FfiLibraryItem::from).collect())
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
            Ok(items.into_iter().map(FfiLibraryItem::from).collect())
        })
    }

    /// Create an RSVP session for `item_id` and return it serialised as
    /// JSON, with the reader's saved progress restored.
    ///
    /// Clients that take this have to drive pacing themselves from the JSON,
    /// which is what `docs/windows-development-plan.md` §4.3 identifies as
    /// the problem: Apple's shipped `RsvpPlayer` does exactly that and
    /// hand-ports `token_duration_ms` and its punctuation helpers to do it.
    /// Prefer [`GistCore::open_rsvp_session`], which hands over the engine
    /// itself. Kept — and kept byte-compatible — because Apple's shipped
    /// `CoreClient.startRsvp` depends on this exact shape.
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

    /// Open a live RSVP pacing session for `item_id`, with the reader's
    /// saved progress restored.
    ///
    /// The object-returning counterpart to [`GistCore::start_rsvp`], and the
    /// one a reader view should use: pacing stays in the core instead of
    /// being re-implemented per platform (`docs/windows-development-plan.md`
    /// §4.3). Both go through `gist_core::Core::new_rsvp_session`, so
    /// progress restore cannot drift between them.
    ///
    /// `wpm` is clamped to 100–1000 by the core.
    pub fn open_rsvp_session(
        &self,
        item_id: String,
        wpm: u32,
    ) -> Result<Arc<FfiRsvpSession>, GistError> {
        ffi_catch!({
            let config = gist_rsvp::Config {
                // `Config::wpm` is only clamped where it is *used*
                // (`token_duration_ms`) and by `set_wpm`; clamp here too so
                // `FfiRsvpSession::wpm()` never reports a value the engine
                // would not honour.
                wpm: wpm.clamp(100, 1000),
                ..Default::default()
            };
            let session = self
                .inner
                .new_rsvp_session(&item_id, config)
                .map_err(GistError::from)?;
            Ok(FfiRsvpSession::new(session))
        })
    }

    pub fn save_progress(&self, item_id: String, token_index: u64) -> Result<(), GistError> {
        ffi_catch!({
            self.inner
                .save_progress(&item_id, token_index as usize)
                .map_err(GistError::from)
        })
    }

    /// Record that a reader (RSVP or flow) opened `item_id` just now — feeds
    /// the library's "date last read" sort (ADR-021). Idempotent; an unknown
    /// id is a no-op. Returns the Unix-millisecond timestamp written.
    pub fn mark_item_opened(&self, item_id: String) -> Result<i64, GistError> {
        ffi_catch!({
            self.inner
                .mark_item_opened(&item_id)
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
            Ok(items.into_iter().map(FfiLibraryItem::from).collect())
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
            Ok(items.into_iter().map(FfiLibraryItem::from).collect())
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

    /// Verify every annotation on `item_id` against the document's current
    /// text and re-anchor/orphan as needed (ADR-003). Call this when
    /// opening a reading view, before rendering highlights/notes/
    /// bookmarks, so annotations created before a re-import or content
    /// edit still point at the right text, or are clearly flagged when
    /// they can't be found any more. A `.reanchored` result has already
    /// been persisted by this call; `list_annotations_for_item` afterward
    /// would return the same corrected position. See
    /// `gist_core::Core::reanchor_annotations`.
    pub fn reanchor_annotations(
        &self,
        item_id: String,
    ) -> Result<Vec<FfiAnnotationAnchorResult>, GistError> {
        ffi_catch!({
            let results = self
                .inner
                .reanchor_annotations(&item_id)
                .map_err(GistError::from)?;
            Ok(results
                .into_iter()
                .map(FfiAnnotationAnchorResult::from)
                .collect())
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

    /// Check one library item's on-disk integrity (ADR-013 / security
    /// register `A4`, `R3`): its document/tokens blobs and, if it has one,
    /// its ADR-006 sandboxed original copy, each checked against its
    /// BLAKE3 checksum sidecar. Returns `None` if `item_id` matches no
    /// item. See `gist_core::Core::verify_item_integrity` for the
    /// pass/fail/unverified aggregation policy.
    pub fn verify_item_integrity(
        &self,
        item_id: String,
    ) -> Result<Option<FfiItemIntegrityOutcome>, GistError> {
        ffi_catch!({
            self.inner
                .verify_item_integrity(&item_id)
                .map(|maybe_outcome| maybe_outcome.map(FfiItemIntegrityOutcome::from))
                .map_err(GistError::from)
        })
    }

    /// `verify_item_integrity`, for every item in the library — the
    /// primitive a "Verify Library Integrity" action (`R4`) calls to build
    /// a pass/fail/unverified summary. Returns one
    /// [`FfiItemIntegrityOutcome`] per item, in no guaranteed order.
    pub fn verify_library_integrity(&self) -> Result<Vec<FfiItemIntegrityOutcome>, GistError> {
        ffi_catch!({
            self.inner
                .verify_library_integrity()
                .map(|outcomes| {
                    outcomes
                        .into_iter()
                        .map(FfiItemIntegrityOutcome::from)
                        .collect()
                })
                .map_err(GistError::from)
        })
    }

    /// Import one or more page images (one raw file per page) and run OCR
    /// using the provided engine. Returns the new document's id plus each
    /// page's OCR confidence, in page order. See
    /// `gist_core::Core::import_image_with_ocr` for the full pipeline and
    /// the ADR-009 addendum (`[A7]`) for the byte-size cap enforced before
    /// any page's bytes are read.
    pub fn import_image_with_ocr(
        &self,
        paths: Vec<String>,
        engine: Box<dyn OcrEngine>,
    ) -> Result<FfiOcrImportResult, GistError> {
        ffi_catch!({
            let adapter = CoreOcrAdapter(engine.as_ref());
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            self.inner
                .import_image_with_ocr(&paths, &adapter)
                .map(FfiOcrImportResult::from)
                .map_err(GistError::from)
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

    // ── At-rest integrity verification (ADR-013 / `A4`, `R3`) ────────────

    /// `GistError::from(CoreError)` must surface a checksum mismatch as its
    /// own distinct variant, not folded into the generic `Core(String)`
    /// case — so Swift can pattern-match `.ChecksumMismatch` the same way
    /// it already does `.DrmProtected`, instead of string-matching an
    /// error message.
    #[test]
    fn checksum_mismatch_core_error_maps_to_its_own_gisterror_variant() {
        let core_err = gist_core::CoreError::Store(gist_store::StoreError::ChecksumMismatch {
            path: "/storage/some-id.json".to_string(),
        });
        let ffi_err = GistError::from(core_err);
        match ffi_err {
            GistError::ChecksumMismatch { path } => {
                assert_eq!(path, "/storage/some-id.json");
            }
            other => panic!("expected GistError::ChecksumMismatch, got {other:?}"),
        }
    }

    /// Every other `CoreError` must still fall through to the generic
    /// `Core(String)` case — this new mapping must not swallow unrelated
    /// errors.
    #[test]
    fn non_checksum_core_error_still_maps_to_generic_core_variant() {
        let core_err = gist_core::CoreError::NotFound("missing-id".to_string());
        let ffi_err = GistError::from(core_err);
        assert!(matches!(ffi_err, GistError::Core(_)));
    }

    // ── Typed resource-limit errors (M7 R3) ──────────────────────────────

    /// Every `LimitKind` must reach its own flat `GistError` case, and the
    /// message must be fixed text (no limit numbers, no paths).
    #[test]
    fn every_limit_kind_maps_to_a_distinct_path_free_gist_error() {
        use gist_model::LimitKind as K;
        let cases = [
            (K::TooLarge, "ResourceLimitTooLarge"),
            (K::TooManyPages, "ResourceLimitTooManyPages"),
            (K::TooManyEntries, "ResourceLimitTooManyEntries"),
            (K::TooDeeplyNested, "ResourceLimitTooDeeplyNested"),
            (K::ExpandedTooLarge, "ResourceLimitContentTooLarge"),
            (K::TableTooLarge, "ResourceLimitTableTooLarge"),
            (K::Other, "ResourceLimitOther"),
        ];
        let mut seen = std::collections::HashSet::new();
        for (kind, name) in cases {
            let ffi_err = GistError::from(gist_core::ImportError::ResourceLimitExceeded {
                limit: "max_pages=2000 /Users/someone/secret.pdf".to_string(),
                attempted: 123_456,
                kind,
            });
            let dbg = format!("{ffi_err:?}");
            assert_eq!(dbg, name);
            let msg = ffi_err.to_string();
            assert!(!msg.contains("/Users") && !msg.contains("123456"), "{msg}");
            assert!(seen.insert(dbg));
        }
    }

    /// End to end through `GistCore`: an oversized plain-text file is
    /// rejected with the typed variant, not `Core(String)`.
    #[test]
    fn import_txt_oversized_file_surfaces_typed_limit_error_over_ffi() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let big = dir.join("big.txt");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(gist_model::ParseLimits::default().max_bytes as u64 + 1)
            .unwrap();
        let core = GistCore::new(
            dir.join("db.sqlite").to_string_lossy().into_owned(),
            dir.join("store").to_string_lossy().into_owned(),
        )
        .unwrap();
        let err = core
            .import_file(big.to_string_lossy().into_owned())
            .unwrap_err();
        assert!(matches!(err, GistError::ResourceLimitTooLarge), "{err:?}");
    }

    fn corrupt_file(path: &std::path::Path) {
        let mut bytes = std::fs::read(path).unwrap();
        assert!(!bytes.is_empty(), "cannot corrupt an empty file");
        let idx = bytes.len() / 2;
        bytes[idx] ^= 0xFF;
        std::fs::write(path, bytes).unwrap();
    }

    /// End-to-end through the real `GistCore` FFI surface: a corrupted
    /// item's document blob must make `verify_item_integrity` report
    /// `.failed`, and a normal content read through the same corrupted
    /// item (`get_document_json`) must surface the dedicated
    /// `GistError::ChecksumMismatch`, not an opaque `Core(String)`.
    #[test]
    fn verify_item_integrity_and_a_real_read_both_surface_a_corrupted_item_distinctly() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = GistCore::new(
            db.to_string_lossy().into_owned(),
            storage.to_string_lossy().into_owned(),
        )
        .unwrap();

        let txt = dir.path().join("corrupt_me.txt");
        std::fs::write(&txt, b"this item is about to be corrupted on disk").unwrap();
        let id = core
            .import_file(txt.to_string_lossy().into_owned())
            .unwrap();

        let doc_path = storage.join(format!("{id}.json"));
        corrupt_file(&doc_path);

        let outcome = core
            .verify_item_integrity(id.clone())
            .unwrap()
            .expect("corrupted item must still be found");
        assert!(matches!(outcome.status, FfiIntegrityStatus::Failed));
        assert!(matches!(
            outcome.report.doc_status,
            FfiChecksumStatus::Mismatch
        ));

        let read_result = core.get_document_json(id);
        assert!(
            matches!(read_result, Err(GistError::ChecksumMismatch { .. })),
            "expected a real read of corrupted content to surface \
             GistError::ChecksumMismatch, got {read_result:?}"
        );
    }

    #[test]
    fn verify_item_integrity_returns_none_for_an_unknown_id() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = GistCore::new(
            db.to_string_lossy().into_owned(),
            storage.to_string_lossy().into_owned(),
        )
        .unwrap();

        assert!(core
            .verify_item_integrity("not-a-real-id".to_string())
            .unwrap()
            .is_none());
    }

    #[test]
    fn verify_library_integrity_reports_all_pass_for_a_clean_library() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = GistCore::new(
            db.to_string_lossy().into_owned(),
            storage.to_string_lossy().into_owned(),
        )
        .unwrap();

        let txt = dir.path().join("clean.txt");
        std::fs::write(&txt, b"a perfectly ordinary imported document").unwrap();
        core.import_file(txt.to_string_lossy().into_owned())
            .unwrap();

        let outcomes = core.verify_library_integrity().unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].status, FfiIntegrityStatus::Pass));
    }

    // ── RSVP pacing over FFI (W4 role R1, plan §4.3) ─────────────────────

    /// A real `GistCore` over a real temp-dir store, with `body` imported as
    /// a .txt item, plus the item's id.
    fn rsvp_fixture(body: &str) -> (tempfile::TempDir, Arc<GistCore>, String) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = GistCore::new(
            db.to_string_lossy().into_owned(),
            storage.to_string_lossy().into_owned(),
        )
        .unwrap();
        let txt = dir.path().join("rsvp.txt");
        std::fs::write(&txt, body.as_bytes()).unwrap();
        let id = core
            .import_file(txt.to_string_lossy().into_owned())
            .unwrap();
        (dir, core, id)
    }

    const TWELVE_WORDS: &str = "alpha bravo charlie delta echo foxtrot golf hotel india juliett \
                                kilo lima";

    /// The session has to come up with real tokens, the requested speed, and
    /// the whole read-only surface answering consistently.
    #[test]
    fn open_rsvp_session_exposes_the_whole_read_only_surface() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap();

        let count = session.token_count().unwrap();
        assert!(count >= 12, "{count} tokens");
        assert_eq!(session.cursor().unwrap(), 0);
        assert_eq!(session.wpm().unwrap(), 600);
        assert_eq!(session.play_state().unwrap(), FfiPlayState::Paused);

        assert_eq!(session.token_text(0).unwrap().as_deref(), Some("alpha"));
        assert_eq!(session.token_kind(0).unwrap(), Some(FfiTokenKind::Word));
        assert!(!session.is_last_token(0).unwrap());
        assert!(session.is_last_token(count - 1).unwrap());
        assert!(session.token_duration_ms(0).unwrap() > 0);

        let orp = session.orp_split(0).unwrap().expect("token 0 exists");
        assert_eq!(
            format!("{}{}{}", orp.before, orp.focus, orp.after),
            "alpha",
            "the three pieces must reassemble the word exactly"
        );
        assert_eq!(orp.focus.chars().count(), 1);
    }

    /// Everything that takes an index must clamp or return `None` for an
    /// out-of-range value — never panic, and never index the token slice
    /// with a client-supplied integer.
    #[test]
    fn out_of_range_indices_are_clamped_not_fatal() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap();
        let count = session.token_count().unwrap();

        for index in [count, count + 1, u64::MAX, u64::MAX - 1] {
            assert_eq!(session.token_text(index).unwrap(), None, "{index}");
            assert_eq!(session.token_kind(index).unwrap(), None, "{index}");
            assert!(session.orp_split(index).unwrap().is_none(), "{index}");
            assert!(!session.is_last_token(index).unwrap(), "{index}");
            assert_eq!(session.token_duration_ms(index).unwrap(), 0, "{index}");
            // Seeking past the end lands on the last token, not out of range.
            session.seek(index).unwrap();
            assert_eq!(session.cursor().unwrap(), count - 1, "{index}");
            session.seek(0).unwrap();
        }

        // And an absurd elapsed value clamps to the last token.
        session.resume().unwrap();
        assert_eq!(session.token_at_elapsed(u64::MAX).unwrap(), count - 1);
        let frame = session
            .frame_at_elapsed(u64::MAX)
            .unwrap()
            .expect("non-empty stream");
        assert_eq!(frame.index, count - 1);
        assert!(frame.is_last);
    }

    /// A paused gap must not count: elapsed passed after a resume is
    /// measured from the resume, and the cursor stays where the pause
    /// pinned it.
    #[test]
    fn pause_then_resume_excludes_paused_time() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap(); // 100 ms/word
        session.resume().unwrap();
        assert_eq!(session.play_state().unwrap(), FfiPlayState::Playing);

        assert_eq!(session.token_at_elapsed(550).unwrap(), 5);
        session.pause(550).unwrap();
        assert_eq!(session.cursor().unwrap(), 5);
        assert_eq!(session.play_state().unwrap(), FfiPlayState::Paused);
        assert_eq!(session.stats_at_elapsed(0).unwrap().duration_ms, 550);

        // However long the pause lasted, the caller restarts its clock at
        // the resume, so elapsed 0 must still be token 5.
        session.resume().unwrap();
        assert_eq!(session.token_at_elapsed(0).unwrap(), 5);
        assert_eq!(session.token_at_elapsed(99).unwrap(), 5);
        assert_eq!(session.token_at_elapsed(100).unwrap(), 6);

        session.pause(250).unwrap();
        assert_eq!(session.cursor().unwrap(), 7);
        assert_eq!(
            session.stats_at_elapsed(0).unwrap().duration_ms,
            800,
            "play time only, no pause gap"
        );
    }

    /// `set_wpm` must pin the position under the *old* speed before applying
    /// the new one. Apple's `RsvpWallClockEngine` mirrors this exactly; a
    /// regression silently makes every speed change jump the reader.
    #[test]
    fn set_wpm_pins_the_cursor_under_the_old_speed() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap(); // 100 ms/word
        session.resume().unwrap();

        // 650 ms at 600 wpm is token 6. At the new 120 ms/word it would be
        // token 5 — so a cursor of 5 would mean the new speed was applied
        // before the position was pinned.
        session.set_wpm(500, 650).unwrap();
        assert_eq!(session.cursor().unwrap(), 6);
        assert_eq!(session.wpm().unwrap(), 500);
        assert_eq!(session.token_at_elapsed(0).unwrap(), 6);
        assert_eq!(
            session.token_at_elapsed(120).unwrap(),
            7,
            "120 ms/word at 500 wpm"
        );
    }

    /// `docs/windows-ui-spec.md` §7.1: "Range enforced by the core, not just
    /// the control." Both the opening speed and a later change must clamp.
    #[test]
    fn wpm_is_clamped_to_100_1000_by_the_core() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);

        assert_eq!(
            core.open_rsvp_session(id.clone(), 0)
                .unwrap()
                .wpm()
                .unwrap(),
            100
        );
        assert_eq!(
            core.open_rsvp_session(id.clone(), 7)
                .unwrap()
                .wpm()
                .unwrap(),
            100
        );
        assert_eq!(
            core.open_rsvp_session(id.clone(), u32::MAX)
                .unwrap()
                .wpm()
                .unwrap(),
            1000
        );

        let session = core.open_rsvp_session(id, 400).unwrap();
        session.set_wpm(1, 0).unwrap();
        assert_eq!(session.wpm().unwrap(), 100);
        session.set_wpm(99_999, 0).unwrap();
        assert_eq!(session.wpm().unwrap(), 1000);
    }

    #[test]
    fn back_words_from_the_start_does_not_underflow() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap();
        session.resume().unwrap();

        session.back_words(5, 0).unwrap();
        assert_eq!(session.cursor().unwrap(), 0);
        session.back_words(u64::MAX, 0).unwrap();
        assert_eq!(session.cursor().unwrap(), 0);

        // And from mid-stream it walks back exactly that many words.
        session.seek(9).unwrap();
        session.back_words(4, 0).unwrap();
        assert_eq!(session.cursor().unwrap(), 5);
    }

    /// `frame_at_elapsed` is the per-tick path: it must agree with the
    /// one-at-a-time accessors, and `next_boundary_ms` must be the elapsed
    /// value at which the frame actually changes (that is what lets a
    /// client's timer wake at a boundary instead of drifting).
    #[test]
    fn frame_at_elapsed_agrees_with_the_individual_accessors() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap();
        session.resume().unwrap();

        for elapsed in [0u64, 1, 99, 100, 101, 550, 1_000] {
            let frame = session
                .frame_at_elapsed(elapsed)
                .unwrap()
                .expect("non-empty stream");
            let index = session.token_at_elapsed(elapsed).unwrap();
            assert_eq!(frame.index, index, "elapsed {elapsed}");
            assert_eq!(
                frame.text,
                session.token_text(index).unwrap().unwrap_or_default()
            );
            assert_eq!(Some(frame.kind), session.token_kind(index).unwrap());
            assert_eq!(
                frame.duration_ms,
                session.token_duration_ms(index).unwrap(),
                "elapsed {elapsed}"
            );
            assert_eq!(frame.is_last, session.is_last_token(index).unwrap());
            assert_eq!(frame.token_count, session.token_count().unwrap());
            assert_eq!(
                format!("{}{}{}", frame.orp.before, frame.orp.focus, frame.orp.after),
                frame.text
            );

            // The boundary is in the future, and is exactly where the frame
            // changes.
            assert!(frame.next_boundary_ms > elapsed, "elapsed {elapsed}");
            assert_eq!(
                session
                    .token_at_elapsed(frame.next_boundary_ms - 1)
                    .unwrap(),
                index,
                "frame must still be current 1ms before its boundary"
            );
            if !frame.is_last {
                assert_eq!(
                    session.token_at_elapsed(frame.next_boundary_ms).unwrap(),
                    index + 1,
                    "frame must change exactly at its boundary"
                );
            }
        }
    }

    /// Simulates the W4 exit criterion's shape: a long uninterrupted run
    /// where the client's tick arrives late or early each time. The reported
    /// position must track measured elapsed time exactly, never accumulate.
    #[test]
    fn jittery_ticks_never_accumulate_drift() {
        let body = (0..600)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let (_dir, core, id) = rsvp_fixture(&body);
        let session = core.open_rsvp_session(id, 600).unwrap(); // 100 ms/word
        session.resume().unwrap();

        // Ticks land at the nominal boundary plus a varying lateness.
        let jitter = [0u64, 7, 23, 1, 91, 48, 3];
        for word in 0..500u64 {
            let nominal = word * 100;
            let late = jitter[(word as usize) % jitter.len()];
            let frame = session
                .frame_at_elapsed(nominal + late)
                .unwrap()
                .expect("non-empty stream");
            assert_eq!(
                frame.index, word,
                "a tick {late}ms late at {nominal}ms must still report word {word}"
            );
            assert_eq!(frame.text, format!("word{word}"));
        }

        // Including a tick so late it skips a token entirely — the answer
        // follows the clock rather than advancing by one.
        let frame = session.frame_at_elapsed(50_250).unwrap().unwrap();
        assert_eq!(frame.index, 502);
    }

    #[test]
    fn stats_at_elapsed_is_live_and_stats_reports_the_unmaintained_counter() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap();
        session.resume().unwrap();

        let at_start = session.stats_at_elapsed(0).unwrap();
        assert_eq!(at_start.words_shown, 0);
        assert_eq!(at_start.duration_ms, 0);
        assert_eq!(at_start.estimated_wpm, 0.0);

        let mid = session.stats_at_elapsed(550).unwrap();
        assert_eq!(mid.words_shown, 5);
        assert_eq!(mid.duration_ms, 550);
        assert!(
            mid.estimated_wpm > 500.0 && mid.estimated_wpm < 600.0,
            "5 words in 550ms is ~545 wpm, got {}",
            mid.estimated_wpm
        );
        assert_eq!(session.words_shown_through(5).unwrap(), 5);
        assert_eq!(session.words_shown_through(u64::MAX).unwrap(), 12);

        // `stats()` reflects the engine's caller-maintained counter, which
        // nothing in this stack increments — pinned so the difference
        // between the two is deliberate and visible.
        assert_eq!(session.stats().unwrap().words_shown, 0);
    }

    /// A zero-token stream has to answer every accessor sanely. Reached in
    /// practice by importing a file with no extractable words.
    #[test]
    fn an_empty_token_stream_never_panics() {
        let (_dir, core, id) = rsvp_fixture("   \n\n   \n");
        let session = core.open_rsvp_session(id, 600).unwrap();
        assert_eq!(
            session.token_count().unwrap(),
            0,
            "fixture must actually produce no tokens for this test to mean anything"
        );
        session.resume().unwrap();

        assert_eq!(session.cursor().unwrap(), 0);
        assert_eq!(session.token_text(0).unwrap(), None);
        assert_eq!(session.token_kind(0).unwrap(), None);
        assert!(!session.is_last_token(0).unwrap());
        assert_eq!(session.token_duration_ms(0).unwrap(), 0);
        assert_eq!(session.token_at_elapsed(0).unwrap(), 0);
        assert_eq!(session.token_at_elapsed(u64::MAX).unwrap(), 0);
        assert!(session.frame_at_elapsed(0).unwrap().is_none());
        assert!(session.frame_at_elapsed(u64::MAX).unwrap().is_none());
        assert!(session.orp_split(0).unwrap().is_none());
        assert_eq!(session.words_shown_through(u64::MAX).unwrap(), 0);
        session.seek(u64::MAX).unwrap();
        session.back_words(u64::MAX, u64::MAX).unwrap();
        session.pause(u64::MAX).unwrap();
        session.set_wpm(0, u64::MAX).unwrap();
        assert_eq!(session.cursor().unwrap(), 0);
        assert_eq!(session.stats_at_elapsed(0).unwrap().words_shown, 0);
    }

    #[test]
    fn a_single_token_stream_is_immediately_the_last_token() {
        let (_dir, core, id) = rsvp_fixture("solitary");
        let session = core.open_rsvp_session(id, 600).unwrap();
        assert_eq!(session.token_count().unwrap(), 1);
        session.resume().unwrap();

        assert!(session.is_last_token(0).unwrap());
        let frame = session.frame_at_elapsed(0).unwrap().expect("one token");
        assert_eq!(frame.index, 0);
        assert_eq!(frame.text, "solitary");
        assert!(frame.is_last);
        assert_eq!(session.token_at_elapsed(u64::MAX).unwrap(), 0);
        session.back_words(3, 0).unwrap();
        assert_eq!(session.cursor().unwrap(), 0);
    }

    /// ORP splitting must never cut a grapheme cluster — the whole reason
    /// the pieces are handed over pre-split rather than as a byte offset.
    #[test]
    fn orp_split_handles_multi_byte_and_degenerate_words() {
        for word in [
            "",
            "a",
            "Hello",
            "naïve",
            "re\u{0301}sume\u{0301}",
            "👍🏽",
            "🇬🇧",
            "👨\u{200D}👩x",
            "日本語のテキスト",
            "...",
        ] {
            let split = rsvp_orp_split(word.to_string()).unwrap();
            assert_eq!(
                format!("{}{}{}", split.before, split.focus, split.after),
                word,
                "{word:?} did not reassemble"
            );
            if word.is_empty() {
                assert!(split.focus.is_empty());
            } else {
                assert!(!split.focus.is_empty(), "{word:?} has an empty focus");
                // One grapheme cluster, however many bytes or chars it is.
                assert!(
                    split.focus.chars().count() >= 1,
                    "{word:?} focus {:?}",
                    split.focus
                );
            }
        }
    }

    /// Progress restore must be the *same* restore `start_rsvp` does — one
    /// implementation in `gist_core::Core::new_rsvp_session`, per plan §4.3.
    #[test]
    fn open_rsvp_session_restores_the_same_saved_progress_as_start_rsvp() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        core.save_progress(id.clone(), 7).unwrap();

        let session = core.open_rsvp_session(id.clone(), 600).unwrap();
        assert_eq!(session.cursor().unwrap(), 7);
        assert_eq!(session.token_at_elapsed(0).unwrap(), 7);

        let json = core.start_rsvp(id.clone(), 600).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["cursor"], serde_json::json!(7));

        // Out-of-range saved progress is clamped by both.
        core.save_progress(id.clone(), 9_999).unwrap();
        let clamped = core.open_rsvp_session(id.clone(), 600).unwrap();
        assert_eq!(
            clamped.cursor().unwrap(),
            clamped.token_count().unwrap() - 1
        );
        let json = core.start_rsvp(id, 600).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["cursor"],
            serde_json::json!(clamped.token_count().unwrap() - 1)
        );
    }

    #[test]
    fn open_rsvp_session_on_an_unknown_id_is_an_error_not_a_panic() {
        let (_dir, core, _id) = rsvp_fixture(TWELVE_WORDS);
        let result = core.open_rsvp_session("not-a-real-id".to_string(), 600);
        assert!(
            matches!(result, Err(GistError::Core(_))),
            "{:?}",
            result.map(|_| "unexpected session")
        );
    }

    /// The session is behind a `Mutex` and uniffi hands the same `Arc` to
    /// however many threads a client cares to use it from. Hammer it from
    /// several at once: no deadlock, no poisoned-lock failure, and the
    /// invariants still hold afterwards.
    #[test]
    fn concurrent_use_from_several_threads_is_safe() {
        let (_dir, core, id) = rsvp_fixture(TWELVE_WORDS);
        let session = core.open_rsvp_session(id, 600).unwrap();
        session.resume().unwrap();

        std::thread::scope(|scope| {
            for t in 0..8u64 {
                let session = Arc::clone(&session);
                scope.spawn(move || {
                    for i in 0..200u64 {
                        let _ = session.frame_at_elapsed(i * 13 + t).unwrap();
                        let _ = session.stats_at_elapsed(i).unwrap();
                        if i % 7 == 0 {
                            session.set_wpm(100 + (i as u32 % 900), i).unwrap();
                        }
                        if i % 11 == 0 {
                            session.seek(i % 12).unwrap();
                        }
                    }
                });
            }
        });

        let count = session.token_count().unwrap();
        assert!(session.cursor().unwrap() < count);
        assert!((100..=1000).contains(&session.wpm().unwrap()));
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

#[cfg(test)]
mod reading_state_tests {
    use super::*;

    /// ADR-021: the new DTO fields cross the FFI boundary, and
    /// `mark_item_opened` is callable and idempotent.
    #[test]
    fn reading_state_fields_and_mark_item_opened_cross_the_ffi() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = GistCore::new(
            db.to_string_lossy().into_owned(),
            storage.to_string_lossy().into_owned(),
        )
        .unwrap();
        let txt = dir.path().join("book.txt");
        std::fs::write(&txt, b"alpha beta gamma delta epsilon zeta eta theta").unwrap();
        let id = core
            .import_file(txt.to_string_lossy().into_owned())
            .unwrap();

        let item = core.list_items(0, 10).unwrap().remove(0);
        assert_eq!(item.source_type, "txt");
        assert_eq!(item.last_opened_at, None);
        assert_eq!(item.progress_fraction, 0.0);

        let ts = core.mark_item_opened(id.clone()).unwrap();
        core.mark_item_opened("missing".to_string()).unwrap();
        core.save_progress(id, 3).unwrap();
        let item = core.list_items(0, 10).unwrap().remove(0);
        assert!(item.last_opened_at.is_some_and(|t| t >= ts));
        assert!(item.progress_fraction > 0.0 && item.progress_fraction <= 1.0);
    }
}
