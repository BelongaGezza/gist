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

/// Extension of the BLAKE3 checksum sidecar `gist-store` writes beside every
/// file it checksums (ADR-013 / `A4`): `<path>` → `<path>.blake3`.
const CHECKSUM_SIDECAR_EXT: &str = "blake3";

// ── Honest file-deletion outcomes (review Q10) ─────────────────────────────

/// Why one file could not be deleted, at the coarsest granularity that is
/// still actionable in a UI.
///
/// Deliberately carries **no path, filename, title or OS error string** — it
/// crosses the FFI boundary into a result summary the user sees, and this
/// project logs source paths at `debug!` only (see `CLAUDE.md`'s
/// "Source paths" policy). The full error, with its path, is logged at
/// `debug!` at the point of failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDeleteFailureKind {
    /// Another open handle prevents deletion. The dominant real-world case on
    /// Windows (`ERROR_SHARING_VIOLATION`/`ERROR_LOCK_VIOLATION`): an
    /// antivirus scan, the search indexer, a backup agent or a reader app
    /// holding the file open *without* delete-sharing. Almost always
    /// transient — the file becomes deletable once the handle closes, which
    /// is what [`Core::sweep_orphaned_files`] is for.
    Locked,
    /// The filesystem refused the delete on permission grounds (read-only
    /// file/attribute, ACL denial).
    Permission,
    /// Anything else (I/O error, path too long for the platform, etc.).
    Other,
}

impl FileDeleteFailureKind {
    /// Classify a `std::fs::remove_file` error that is **known not to be
    /// `NotFound`** — callers must handle a missing file before reaching
    /// here, because "the file was already gone" is not a failure (see
    /// [`RemoveOutcome::files_missing`]). There is deliberately no
    /// `NotFound` variant to return: every value of this enum is something
    /// that genuinely went wrong.
    fn classify(e: &std::io::Error) -> Self {
        // [PLATFORM: Windows] ─── begin ───────────────────────────────────
        // Windows reports "another handle has this file open and did not
        // grant FILE_SHARE_DELETE" as ERROR_SHARING_VIOLATION (32) or
        // ERROR_LOCK_VIOLATION (33). Neither has a distinct `io::ErrorKind`
        // that is stable across the Rust versions this project pins, and
        // both currently decode to `PermissionDenied`, which would be
        // actively misleading in the UI ("check your permissions" for a file
        // that antivirus will release in a second). Raw code first, so the
        // mapping below can never mask it.
        #[cfg(windows)]
        {
            match e.raw_os_error() {
                Some(32) | Some(33) => return FileDeleteFailureKind::Locked,
                Some(5) => return FileDeleteFailureKind::Permission,
                _ => {}
            }
        }
        // [PLATFORM: Windows] ─── end ─────────────────────────────────────
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => FileDeleteFailureKind::Permission,
            // `NotFound` cannot reach here (the caller filters it) and is
            // not a failure if it somehow did, so it falls in with `Other`
            // rather than being given a variant that would then be
            // permanently unreachable.
            _ => FileDeleteFailureKind::Other,
        }
    }
}

/// What [`Core::remove_items_detailed`] actually managed to do.
///
/// The library rows are gone if this returns `Ok` (the DB delete is
/// transactional and happens first); the file counts describe the
/// **best-effort** cleanup that followed, which on Windows genuinely can
/// fail while the removal itself succeeded. Reporting "removed" while a
/// stored copy is still on disk is the dishonesty review finding Q10 is
/// about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveOutcome {
    /// The ids that actually matched a row and were removed from the
    /// database. Ids that matched nothing are silently absent (see
    /// [`gist_store::Store::remove_items`]).
    pub removed_ids: Vec<String>,
    /// How many stored files were deleted: `<id>.json`, `<id>.tokens.json`,
    /// their `.blake3` checksum sidecars, and — only when
    /// `delete_source_files` is true — the ADR-006 sandboxed original copy
    /// and its sidecar. Never counts the user's own file at `source_path`,
    /// which is never touched.
    pub files_deleted: u32,
    /// How many files were already gone, so there was nothing to delete.
    ///
    /// **Not a failure, and deliberately its own counter.** The ordinary
    /// case is an item imported before ADR-013 added checksum sidecars: it
    /// has no `.blake3` files, so a clean removal of it reports
    /// `files_missing: 2` (or 3) with `files_failed: 0`. Folding these into
    /// `files_failed` would make a perfectly good removal look broken in
    /// the UI. A user who tidied the storage directory by hand lands here
    /// too. Kept visible rather than ignored so the tally still adds up
    /// against what was attempted.
    pub files_missing: u32,
    /// How many deletions genuinely failed — `Locked`, `Permission` or
    /// `Other` only. Equals `failure_kinds.len()`. A non-zero value here is
    /// the thing worth telling the user about; everything else in this
    /// struct is bookkeeping.
    pub files_failed: u32,
    /// One entry per failed deletion, in attempt order, coarse kind only —
    /// no paths, no titles. Never contains a "missing file" entry; see
    /// `files_missing`. See [`FileDeleteFailureKind`].
    pub failure_kinds: Vec<FileDeleteFailureKind>,
    /// How many removed items had their ADR-006 stored copy **deliberately
    /// kept** because another library row that survived this removal still
    /// references the same content-addressed file.
    ///
    /// **Not a failure and not a missing file** — the third counter exists
    /// precisely so neither of those has to lie. No deletion was attempted,
    /// so nothing can have failed; the file is still there on purpose, so it
    /// is not missing. Removing the last item that shares the file deletes it
    /// then. See [`gist_store::RemovedItem::source_copy_still_referenced`].
    pub shared_copies_kept: u32,
}

/// What [`Core::sweep_orphaned_files`] found and did. Counts only — the
/// sweep never reports which files it touched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Files examined inside the storage directory (and its `originals/`
    /// subdirectory). Includes files that were kept.
    pub files_scanned: u32,
    /// Orphaned files successfully deleted.
    pub files_deleted: u32,
    /// Orphaned files that turned out to be already gone by the time the
    /// delete ran — i.e. something removed them between this sweep listing
    /// the directory and acting on it. Rare, benign, and not a failure;
    /// carried for the same reason as [`RemoveOutcome::files_missing`], so
    /// deleted + missing + failed accounts for everything attempted.
    pub files_missing: u32,
    /// Orphaned files that genuinely could not be deleted this time
    /// (`Locked`/`Permission`/`Other`). Equals `failure_kinds.len()`; a
    /// later sweep will try again.
    pub files_failed: u32,
    /// One entry per failure, coarse kind only. Never contains a "missing
    /// file" entry. See [`FileDeleteFailureKind`].
    pub failure_kinds: Vec<FileDeleteFailureKind>,
}

/// Accumulates per-file deletion results for [`Core::remove_items_detailed`]
/// and [`Core::sweep_orphaned_files`] so both report the same shape.
#[derive(Default)]
struct DeleteTally {
    deleted: u32,
    missing: u32,
    failure_kinds: Vec<FileDeleteFailureKind>,
}

impl DeleteTally {
    /// Best-effort delete one file, recording the outcome. Never returns an
    /// error: this runs *after* the owning `library_items` row is already
    /// gone, so failing the whole call here would misreport a removal that
    /// genuinely happened. The path (and full OS error) go to `debug!` only,
    /// per this project's source-path logging policy.
    ///
    /// Three outcomes, kept distinct on purpose: deleted, already missing
    /// (not a failure — see [`RemoveOutcome::files_missing`]), or a real
    /// failure with a coarse kind.
    fn try_delete(&mut self, path: &str) {
        match std::fs::remove_file(path) {
            Ok(()) => self.deleted += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!("gist-core: nothing to delete at {}", path);
                self.missing += 1;
            }
            Err(e) => {
                tracing::debug!("gist-core: failed to delete file {}: {}", path, e);
                self.failure_kinds.push(FileDeleteFailureKind::classify(&e));
            }
        }
    }

    /// Delete `path` and its BLAKE3 checksum sidecar (`<path>.blake3`,
    /// ADR-013 / `A4`). A sidecar with no blob (or vice versa) is harmless
    /// either way — this just avoids leaving one behind after its blob is
    /// gone.
    fn try_delete_with_sidecar(&mut self, path: &str) {
        self.try_delete(path);
        self.try_delete(&format!("{path}.{CHECKSUM_SIDECAR_EXT}"));
    }

    fn failed(&self) -> u32 {
        self.failure_kinds.len() as u32
    }
}

/// Is `path` inside `storage_dir`, resolving `..`, `.` and (on Windows)
/// junctions/symlinks first?
///
/// Removal is the one place that deletes a path read back out of the
/// database rather than one it just computed, and since 2026-09-21 it does so
/// unconditionally (ADR-006 addendum). A `source_copy_path` column is only
/// ever written by [`gist_store::Store::store_original_copy`], whose own
/// filename is traversal-safe (`F23`) — but *the column is not the
/// filename*, and a corrupted, hand-edited or otherwise tampered row could
/// name anything at all, including a file the user cares about. This is the
/// check that makes that impossible rather than merely unlikely.
///
/// Both sides are canonicalised, so the two paths are compared in one form
/// (on Windows that means both carry the `\\?\` verbatim prefix, which a
/// plain string comparison would otherwise get wrong) and a reparse point
/// planted inside `originals/` cannot redirect a delete outside it.
///
/// A path that does not exist (`NotFound`) returns `true`: there is nothing
/// to delete, and the caller's ordinary `NotFound` handling should record it
/// as *missing* rather than have it silently disappear from the tally.
/// Any OTHER canonicalisation failure (access denied, a bad component, an I/O
/// error) returns `false`: containment could not be proven, and this check
/// exists precisely so that an unproven path is never deleted.
fn is_inside(storage_dir: &Path, path: &str) -> bool {
    let Ok(storage) = std::fs::canonicalize(storage_dir) else {
        // No storage directory to be inside of: refuse rather than guess.
        return false;
    };
    match std::fs::canonicalize(path) {
        Ok(candidate) => candidate.starts_with(&storage),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// `<id>.json` → `<id>.tokens.json`, matching how
/// `gist_store::Store::insert_item` names the two blobs. Falls back to
/// appending (rather than replacing) the suffix for a path that doesn't end
/// in `.json`, preserving the behaviour removal has always had.
fn tokens_path_for(doc_path: &str) -> String {
    doc_path
        .strip_suffix(".json")
        .map(|s| format!("{s}.tokens.json"))
        .unwrap_or_else(|| format!("{doc_path}.tokens.json"))
}

/// Does `s` have the shape of a document id — a hyphenated UUID (v7, per
/// `CLAUDE.md`'s "Document IDs" convention)? Shape only; this exists so the
/// orphan sweep deletes nothing whose name it cannot positively identify as
/// GIST-generated.
fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Names [`Core::sweep_orphaned_files`] may delete from the top level of the
/// storage directory: `<uuid>.json`, `<uuid>.tokens.json` and the `.blake3`
/// checksum sidecar of either. Everything else — a SQLite file and its
/// `-wal`/`-shm` siblings, a key file, anything a host app or the user put
/// there — fails this test and is left alone.
fn is_sweepable_blob_name(name: &str) -> bool {
    let base = name.strip_suffix(".blake3").unwrap_or(name);
    let stem = match base.strip_suffix(".tokens.json") {
        Some(stem) => stem,
        None => match base.strip_suffix(".json") {
            Some(stem) => stem,
            None => return false,
        },
    };
    looks_like_uuid(stem)
}

/// Names [`Core::sweep_orphaned_files`] may delete from
/// `<storage_dir>/originals/`: a SHA-256-hex filename with an optional
/// sanitised extension, exactly as `gist_store::Store::store_original_copy`
/// writes it (ADR-006), plus its `.blake3` sidecar.
fn is_sweepable_original_name(name: &str) -> bool {
    let base = name.strip_suffix(".blake3").unwrap_or(name);
    let (hash, ext) = match base.split_once('.') {
        Some((hash, ext)) => (hash, Some(ext)),
        None => (base, None),
    };
    let hash_ok = hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    // `store_original_copy` runs `ext` through `sanitize_ext`, which reduces
    // it to ASCII alphanumerics — so a multi-dot or otherwise odd tail is
    // not something this app wrote, and is not swept.
    let ext_ok = match ext {
        None => true,
        Some(ext) => !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric()),
    };
    hash_ok && ext_ok
}

/// One non-recursive pass over `dir`: delete every regular file that is
/// *not* in `keep` and that `is_candidate` positively identifies as a
/// GIST-generated file. See [`Core::sweep_orphaned_files`] for the full
/// safety contract this implements.
fn sweep_dir(
    dir: &Path,
    keep: &std::collections::HashSet<String>,
    is_candidate: fn(&str) -> bool,
    tally: &mut DeleteTally,
    scanned: &mut u32,
) -> Result<(), CoreError> {
    match std::fs::symlink_metadata(dir) {
        // A symlink/junction *as* the directory: never followed, so the
        // sweep can never be redirected outside the storage directory by a
        // link planted in it.
        Ok(md) if md.file_type().is_symlink() => {
            tracing::debug!("gist-core: sweep skipped {:?} (reparse point)", dir);
            return Ok(());
        }
        Ok(md) if !md.is_dir() => return Ok(()),
        Ok(_) => {}
        // `originals/` only exists once a file-based import has happened.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(CoreError::Io(e)),
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        // `DirEntry::file_type` does not follow links, so a symlink or
        // Windows junction reports `is_file() == false` here and is skipped
        // entirely — neither deleted nor traversed. Directories are skipped
        // for the same reason: this sweep never recurses and never removes
        // a directory.
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            // A non-UTF-8 name cannot be one this app wrote (ids and content
            // hashes are ASCII), so there is nothing to do but leave it.
            continue;
        };
        *scanned += 1;
        // Keep-list lookup is case-insensitive (see `sweep_orphaned_files`);
        // the candidate-pattern check uses the name as it really is.
        if keep.contains(&name.to_ascii_lowercase()) || !is_candidate(&name) {
            continue;
        }
        let path = dir.join(&name);
        debug_assert_eq!(
            path.parent(),
            Some(dir),
            "sweep must only ever act on direct children of the directory it is scanning"
        );
        tally.try_delete(&path.to_string_lossy());
    }

    Ok(())
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
    /// **Shared stored copies are never deleted out from under a survivor**
    /// (ADR-006 addendum, 2026-09-21). Copies are content-addressed, so two
    /// items imported from byte-identical files share one file on disk. When
    /// `delete_source_files` is true, a stored copy is deleted only once the
    /// *last* row referencing it goes — so removing one of two sharers keeps
    /// the file, removing both in one call deletes it, and removing the
    /// second one later deletes it then. See
    /// [`RemoveOutcome::shared_copies_kept`].
    ///
    /// **A stored-copy path that does not resolve inside the storage
    /// directory is refused, not deleted** (see [`is_inside`]) — a
    /// corrupted or tampered row cannot make removal reach a file elsewhere
    /// on the user's disk. Such a refusal appears in no counter, for the same
    /// reason `source_path` does not: it is not one of GIST's own files.
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
        self.remove_items_detailed(ids, delete_source_files)
            .map(|_| ())
    }

    /// [`Core::remove_items`], but reporting what actually happened
    /// (review finding Q10).
    ///
    /// Behaviourally identical to `remove_items` — same DB-first ordering,
    /// same best-effort file cleanup, same ADR-006 rule that only the
    /// sandboxed copy is ever deleted and never the user's own file — but it
    /// returns a [`RemoveOutcome`] instead of `()`, so a caller can tell the
    /// user "removed from your library, but 1 file could not be deleted
    /// because something else has it open" rather than claiming a clean
    /// removal. `remove_items` is this function with the outcome discarded,
    /// so there is exactly one code path.
    ///
    /// This matters far more on Windows than on Apple platforms: antivirus,
    /// the search indexer, backup agents and OneDrive routinely hold a file
    /// open without granting `FILE_SHARE_DELETE`, which makes the delete
    /// fail outright rather than unlinking a still-open file the way POSIX
    /// does. A file left behind this way is reclaimed by
    /// [`Core::sweep_orphaned_files`] on a later launch.
    pub fn remove_items_detailed(
        &self,
        ids: &[String],
        delete_source_files: bool,
    ) -> Result<RemoveOutcome, CoreError> {
        let removed = self.store.remove_items(ids)?;

        let mut tally = DeleteTally::default();
        let mut removed_ids = Vec::with_capacity(removed.len());
        let mut shared_copies_kept = 0u32;
        // Stored copies handled already in *this* batch. Removing both items
        // that share one content-addressed copy means two rows name the same
        // file: without this, the second one would re-attempt a delete of a
        // file the first already removed and report it as `files_missing`,
        // which is bookkeeping noise about a perfectly clean removal.
        let mut copies_handled: std::collections::HashSet<String> = Default::default();

        for item in removed {
            tally.try_delete_with_sidecar(&item.doc_path);
            tally.try_delete_with_sidecar(&tokens_path_for(&item.doc_path));

            if delete_source_files {
                if let Some(source_copy_path) = &item.source_copy_path {
                    if !copies_handled.insert(source_copy_path.to_ascii_lowercase()) {
                        // Already dealt with for an earlier item in this same
                        // batch — the two share one file.
                    } else if !is_inside(self.store.storage_dir(), source_copy_path) {
                        // Not one of GIST's own files (see `is_inside`).
                        // Refused outright, and deliberately counted in
                        // nothing: it was never GIST's to delete, exactly
                        // like `source_path`.
                        tracing::debug!(
                            "gist-core: refusing to delete stored copy outside storage: {}",
                            source_copy_path
                        );
                    } else if item.source_copy_still_referenced {
                        // ADR-006 dedup: another surviving row points at this
                        // exact content-addressed file. Deleting it here would
                        // take away that item's stored copy too. Counted, not
                        // attempted — so it lands in neither `files_failed`
                        // nor `files_missing`.
                        shared_copies_kept += 1;
                        tracing::debug!(
                            "gist-core: kept shared stored copy {} (still referenced)",
                            source_copy_path
                        );
                    } else {
                        tally.try_delete_with_sidecar(source_copy_path);
                    }
                }
                // else: no sandboxed copy exists for this item (URL import,
                // or imported before ADR-006 landed) — nothing to delete.
                // `item.source_path` (the user's real file) is never used
                // here; see the doc comment above.
            }

            removed_ids.push(item.id);
        }

        Ok(RemoveOutcome {
            removed_ids,
            files_deleted: tally.deleted,
            files_missing: tally.missing,
            files_failed: tally.failed(),
            failure_kinds: tally.failure_kinds,
            shared_copies_kept,
        })
    }

    /// Delete files in the storage directory that no library row references
    /// any more, and report counts (review finding Q10).
    ///
    /// This is the companion to [`Core::remove_items_detailed`]'s honesty: a
    /// blob that could not be deleted at removal time — because antivirus,
    /// the indexer or another app had it open without delete-sharing, the
    /// common Windows case — is reclaimed the next time this runs, typically
    /// at app launch. Safe to call at any time, including when nothing is
    /// orphaned.
    ///
    /// **What it will delete**, and nothing else:
    /// - `<storage_dir>/<uuid>.json`, `<uuid>.tokens.json` and their
    ///   `.blake3` checksum sidecars, where `<uuid>` is not a live row's id;
    /// - `<storage_dir>/originals/<sha256-hex>[.<ext>]` and its `.blake3`
    ///   sidecar, where no live row's `source_copy_path` names that file.
    ///
    /// **Safety rules, all enforced here rather than assumed:**
    /// - A file referenced by *any* row is kept. Content-addressed originals
    ///   (ADR-006) are shared between items imported from identical bytes,
    ///   so "no longer referenced" means no row at all references it — the
    ///   ADR-006 dedup residual can never be turned into data loss by this
    ///   sweep.
    /// - Only file names matching the generated patterns above are ever
    ///   deleted. A SQLite database, its `-wal`/`-shm` siblings, a key file,
    ///   a user's own file or anything else a host app happens to keep in
    ///   the same directory does not match and is left alone.
    /// - Only the storage directory itself and its `originals/`
    ///   subdirectory are examined, non-recursively, and every path acted on
    ///   is `dir.join(<single file name from that directory's listing>)`.
    /// - Symlinks, Windows junctions and any other reparse point are skipped
    ///   without being deleted or followed, at both the directory and file
    ///   level, so a link planted inside the storage directory cannot make
    ///   the sweep reach outside it.
    /// - Directories are never removed.
    ///
    /// Failures are counted, not propagated: a file still locked at sweep
    /// time just stays for the next sweep.
    pub fn sweep_orphaned_files(&self) -> Result<SweepOutcome, CoreError> {
        let storage_dir = self.store.storage_dir().to_path_buf();
        let referenced = self.store.list_referenced_files()?;

        // Keep-lists are built from file *names* within the two directories
        // the sweep looks at, not full path strings: a row's stored path is
        // whatever string the storage dir was when the row was written, so
        // comparing names avoids any separator/prefix normalisation question
        // (`C:\x` vs `C:\x\`, `\\?\C:\x`, …) becoming a data-loss bug. It is
        // conservative in the only direction that is safe: an unrelated file
        // that happens to share a referenced name is kept.
        //
        // Names are ASCII-lowercased on both sides, because Windows
        // filesystems are case-insensitive: without this, a row recorded as
        // `ABC.json` and a directory entry listed as `abc.json` would be the
        // same file yet fail to match, and the sweep would delete a file
        // that is still referenced. Every name this app generates is already
        // lowercase (UUID and SHA-256 hex, fixed suffixes), so this only
        // ever adds matches — i.e. only ever keeps more.
        let mut keep_blobs: std::collections::HashSet<String> = Default::default();
        let mut keep_originals: std::collections::HashSet<String> = Default::default();

        let keep = |set: &mut std::collections::HashSet<String>, path: &str| {
            if let Some(name) = Path::new(path).file_name().and_then(|n| n.to_str()) {
                let name = name.to_ascii_lowercase();
                set.insert(format!("{name}.{CHECKSUM_SIDECAR_EXT}"));
                set.insert(name);
            }
        };

        for row in &referenced {
            keep(&mut keep_blobs, &row.doc_path);
            keep(&mut keep_blobs, &tokens_path_for(&row.doc_path));
            if let Some(copy) = &row.source_copy_path {
                keep(&mut keep_originals, copy);
                // A copy path that (historically) points somewhere other
                // than `originals/` still contributes its name to the
                // keep-list — again, conservative in the safe direction.
                keep(&mut keep_blobs, copy);
            }
        }

        let mut tally = DeleteTally::default();
        let mut scanned = 0u32;

        sweep_dir(
            &storage_dir,
            &keep_blobs,
            is_sweepable_blob_name,
            &mut tally,
            &mut scanned,
        )?;
        sweep_dir(
            &storage_dir.join("originals"),
            &keep_originals,
            is_sweepable_original_name,
            &mut tally,
            &mut scanned,
        )?;

        tracing::debug!(
            "gist-core: sweep scanned {} file(s), deleted {}, failed {}",
            scanned,
            tally.deleted,
            tally.failed()
        );

        Ok(SweepOutcome {
            files_scanned: scanned,
            files_deleted: tally.deleted,
            files_missing: tally.missing,
            files_failed: tally.failed(),
            failure_kinds: tally.failure_kinds,
        })
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

    // ── Path shapes, file locking, orphan sweep (review Q10) ────────────
    //
    // These cover what removal and import do to real files under path and
    // sharing semantics that differ between platforms. Everything that is
    // genuinely cross-platform is unguarded so it also runs on the ubuntu
    // and macos CI legs; Windows-only behaviour (delete-sharing, reserved
    // names, reparse points) is `#[cfg(windows)]` with the parent
    // CLAUDE.md's platform-guard comment convention.

    /// `(core, db_path, storage_dir)` rooted at `root`, mirroring how every
    /// other test in this module builds a `Core`, but reusable for the path
    /// shapes below where `root` is deliberately awkward.
    fn core_rooted_at(root: &std::path::Path) -> (Core, std::path::PathBuf) {
        std::fs::create_dir_all(root).unwrap();
        let db = root.join("test.db");
        let storage = root.join("storage");
        std::fs::create_dir_all(&storage).unwrap();
        let core = Core::init(&db, &storage).unwrap();
        (core, storage)
    }

    /// Every file a *fresh* (post-ADR-013) file-based import puts on disk
    /// for one item: the two blobs, their two checksum sidecars, and the
    /// sandboxed original copy plus its sidecar — i.e. what
    /// `remove_items_detailed(.., true)` should report deleting.
    const FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY: u32 = 6;
    /// The same, without the sandboxed copy (`delete_source_files: false`).
    const FILES_PER_FRESH_ITEM: u32 = 4;

    /// Long paths: Rust's std uses Windows' `\\?\` verbatim form internally
    /// for absolute paths, so a storage directory and a source file well
    /// past the legacy 260-character `MAX_PATH` work end to end without the
    /// app doing anything special. This proves it rather than assuming it —
    /// Q10 called the long-path case untested, and the packaged
    /// `LocalState` path on Windows eats a large slice of the budget before
    /// the library's own directories are appended.
    ///
    /// **This test found a real bug when first written (fixed in the same
    /// change).** `std::fs` coped with everything, but `Core::init` itself
    /// failed with SQLite `CannotOpen`: SQLite's Win32 VFS passes the
    /// database path to `CreateFileW` unprefixed, so it stays bound by
    /// `MAX_PATH` whatever Rust does around it. The whole library was
    /// unopenable at a path where every blob beside it read and wrote fine.
    /// See `gist_store::sqlite_path`.
    #[test]
    fn long_paths_past_legacy_max_path_import_search_and_remove_cleanly() {
        let dir = tempfile::tempdir().unwrap();

        let mut root = dir.path().to_path_buf();
        while root.as_os_str().len() < 300 {
            // 28 chars per component, comfortably inside the 255-character
            // per-component limit that applies on every platform here.
            root = root.join("a_directory_with_a_long_name");
        }
        let (core, storage) = core_rooted_at(&root);

        let long_stem = "a_file_name_that_is_also_quite_long_".repeat(3);
        let src = root.join(format!("{long_stem}.txt"));
        std::fs::write(&src, b"Long paths must not silently break removal, quokka.").unwrap();
        assert!(
            src.as_os_str().len() > 260,
            "the source path must exceed legacy MAX_PATH for this test to mean anything (got {})",
            src.as_os_str().len()
        );

        let id = core.import_file(&src, &NullObserver).unwrap();

        let doc_path = storage.join(format!("{id}.json"));
        assert!(doc_path.as_os_str().len() > 260);
        assert!(
            doc_path.exists(),
            "the IR blob must be written at a >260-character path"
        );

        assert!(core.get_document(&id).is_ok());
        assert!(core
            .search_items("quokka", 10)
            .unwrap()
            .iter()
            .any(|i| i.id == id));

        let outcome = core.remove_items_detailed(&[id.clone()], true).unwrap();
        assert_eq!(outcome.removed_ids, vec![id]);
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY, 0),
            "every stored file must be deletable at a long path: {outcome:?}"
        );
        assert!(!doc_path.exists());
        assert!(
            src.exists(),
            "GIST must never delete the user's original file (ADR-006)"
        );
    }

    /// Unicode (multi-byte, non-BMP emoji, combining-mark-adjacent scripts)
    /// and spaces in both the storage directory and the source file name.
    /// Spaces catch any accidental shell-style path splitting; non-ASCII
    /// catches an encoding assumption (Windows paths are UTF-16, Rust's are
    /// UTF-8).
    #[test]
    fn unicode_and_space_containing_paths_import_and_remove_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir
            .path()
            .join("Ordner mit Leerzeichen — 日本語 — Ünïcødé 📚");
        let (core, storage) = core_rooted_at(&root);

        let stem = "My Book (draft 1) — 日本語 — çöpy 📖";
        let src = root.join(format!("{stem}.txt"));
        std::fs::write(&src, "A quokka wandered through the Ünïcødé prose.").unwrap();

        let id = core.import_file(&src, &NullObserver).unwrap();

        let item = core
            .list_items(0, 10)
            .unwrap()
            .into_iter()
            .find(|i| i.id == id)
            .expect("the imported item must be listed");
        assert_eq!(
            item.title.as_deref(),
            Some(stem),
            "the title is the file stem, so it must survive the round trip through SQLite byte-for-byte"
        );
        assert_eq!(
            item.source_path.as_deref(),
            Some(src.to_string_lossy().as_ref()),
            "the informational source path must round-trip unchanged"
        );

        let outcome = core.remove_items_detailed(&[id], true).unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY, 0),
            "{outcome:?}"
        );
        assert!(src.exists());
        assert!(
            std::fs::read_dir(storage.join("originals"))
                .unwrap()
                .next()
                .is_none(),
            "the originals directory should be empty after removing the only item"
        );
    }

    /// The honest-outcome contract itself: which ids came out of the
    /// database, and an exact file tally for each `delete_source_files`
    /// state. An id that matches no row must not appear in `removed_ids`.
    #[test]
    fn remove_items_detailed_reports_removed_ids_and_an_exact_file_tally() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let keep_src = dir.path().join("kept.txt");
        std::fs::write(&keep_src, b"Kept behind.").unwrap();
        let kept = core.import_file(&keep_src, &NullObserver).unwrap();

        let src = dir.path().join("removed.txt");
        std::fs::write(&src, b"Removed, source copy kept.").unwrap();
        let id = core.import_file(&src, &NullObserver).unwrap();

        let outcome = core
            .remove_items_detailed(&[id.clone(), "not-a-real-id".to_string()], false)
            .unwrap();
        assert_eq!(
            outcome.removed_ids,
            vec![id],
            "an unknown id must be absent from removed_ids, not reported as removed"
        );
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (FILES_PER_FRESH_ITEM, 0),
            "delete_source_files=false must delete the two blobs and their two sidecars only: {outcome:?}"
        );
        assert!(outcome.failure_kinds.is_empty());
        assert!(
            storage.join("originals").read_dir().unwrap().count() > 0,
            "the sandboxed copy must survive delete_source_files=false"
        );

        let outcome = core.remove_items_detailed(&[kept], true).unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY, 0),
            "{outcome:?}"
        );
    }

    /// A file that is already gone is counted as **missing, not failed**.
    /// The "attempted vs deleted" tally still has to add up, but a missing
    /// file is not something to warn the user about, so it must not inflate
    /// `files_failed` or appear in the failure breakdown.
    #[test]
    fn remove_items_detailed_counts_already_deleted_files_as_missing_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let src = dir.path().join("half_gone.txt");
        std::fs::write(&src, b"Someone deleted my blob already.").unwrap();
        let id = core.import_file(&src, &NullObserver).unwrap();

        let doc_path = storage.join(format!("{id}.json"));
        std::fs::remove_file(&doc_path).unwrap();
        std::fs::remove_file(format!("{}.blake3", doc_path.display())).unwrap();

        let outcome = core.remove_items_detailed(&[id], false).unwrap();
        assert_eq!(
            (
                outcome.files_deleted,
                outcome.files_missing,
                outcome.files_failed
            ),
            (2, 2, 0),
            "the tokens blob and its sidecar are deleted; the two already-gone files are \
             missing, and nothing failed: {outcome:?}"
        );
        assert!(
            outcome.failure_kinds.is_empty(),
            "a missing file must never appear in the failure breakdown: {outcome:?}"
        );
    }

    /// The case the `files_missing`/`files_failed` split exists for: an
    /// item imported before ADR-013 added checksum sidecars has no
    /// `.blake3` files at all, so removing it inevitably finds two of them
    /// absent. That is a completely clean removal and **must report zero
    /// failures** — otherwise every legacy item in a user's library would
    /// warn on removal for no reason.
    #[test]
    fn removing_a_pre_adr013_item_without_checksum_sidecars_reports_no_failures() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let src = dir.path().join("legacy.txt");
        std::fs::write(&src, b"Imported before checksum sidecars existed.").unwrap();
        let id = core.import_file(&src, &NullObserver).unwrap();

        // Simulate the legacy on-disk shape: blobs present, sidecars never
        // written. (The sandboxed copy keeps its sidecar, so this also
        // covers a mixed library rather than an all-or-nothing one.)
        for name in [format!("{id}.json"), format!("{id}.tokens.json")] {
            std::fs::remove_file(storage.join(format!("{name}.blake3"))).unwrap();
        }

        let outcome = core.remove_items_detailed(&[id], true).unwrap();
        assert_eq!(
            (
                outcome.files_deleted,
                outcome.files_missing,
                outcome.files_failed
            ),
            (FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY - 2, 2, 0),
            "a legacy item removes cleanly: only the two never-written sidecars are missing, \
             and nothing failed: {outcome:?}"
        );
        assert!(outcome.failure_kinds.is_empty(), "{outcome:?}");
    }

    /// The sweep deletes GIST-generated files no row references, and leaves
    /// everything else alone — including files belonging to a live item and
    /// any file whose name it cannot positively identify as its own (a
    /// SQLite database and its sidecars being the case that would be
    /// catastrophic to get wrong).
    #[test]
    fn sweep_deletes_orphans_and_leaves_referenced_and_unknown_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let src = dir.path().join("live.txt");
        std::fs::write(&src, b"A live item that must survive the sweep.").unwrap();
        let live = core.import_file(&src, &NullObserver).unwrap();

        // Orphans: shaped exactly like what this app writes, but referenced
        // by no row (e.g. left behind by a removal whose delete failed).
        let orphan_id = "01926f3a-7c2b-7a10-9f3d-2b6c5e4a1d88";
        let orphans = [
            storage.join(format!("{orphan_id}.json")),
            storage.join(format!("{orphan_id}.json.blake3")),
            storage.join(format!("{orphan_id}.tokens.json")),
            storage.join(format!("{orphan_id}.tokens.json.blake3")),
            storage
                .join("originals")
                .join(format!("{}.txt", "ab".repeat(32))),
        ];
        for path in &orphans {
            std::fs::write(path, b"orphan").unwrap();
        }

        // Not ours, or not identifiable as ours: must never be deleted.
        let bystanders = [
            storage.join("gist.db"),
            storage.join("gist.db-wal"),
            storage.join("not-a-uuid.json"),
            storage.join("notes.txt"),
            storage.join("originals").join("README.md"),
        ];
        for path in &bystanders {
            std::fs::write(path, b"not mine").unwrap();
        }

        let outcome = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (orphans.len() as u32, 0),
            "{outcome:?}"
        );
        assert!(outcome.files_scanned >= (orphans.len() + bystanders.len()) as u32);

        for path in &orphans {
            assert!(!path.exists(), "orphan {path:?} should have been swept");
        }
        for path in &bystanders {
            assert!(path.exists(), "the sweep must not touch {path:?}");
        }
        assert!(storage.join(format!("{live}.json")).exists());
        assert!(storage.join(format!("{live}.json.blake3")).exists());
        assert!(storage.join(format!("{live}.tokens.json")).exists());
        assert!(
            core.get_document(&live).is_ok(),
            "the live item must still be readable"
        );

        // Idempotent: a second sweep with nothing orphaned deletes nothing.
        let again = core.sweep_orphaned_files().unwrap();
        assert_eq!((again.files_deleted, again.files_failed), (0, 0));
    }

    /// ADR-006's dedup residual must not become data loss in the sweep: two
    /// items imported from byte-identical content share one
    /// content-addressed copy, so that copy stays while **any** row still
    /// references it, and is only swept once the last one is gone.
    #[test]
    fn sweep_keeps_a_shared_original_copy_while_any_row_still_references_it() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let content = b"Identical bytes imported twice share one copy (ADR-006).";
        let first_src = dir.path().join("first.txt");
        let second_src = dir.path().join("second.txt");
        std::fs::write(&first_src, content).unwrap();
        std::fs::write(&second_src, content).unwrap();

        let first = core.import_file(&first_src, &NullObserver).unwrap();
        let second = core.import_file(&second_src, &NullObserver).unwrap();

        let copy = the_one_sandboxed_copy(&storage);

        // One row gone, the other still referencing the shared copy.
        core.remove_items(&[first], false).unwrap();
        let outcome = core.sweep_orphaned_files().unwrap();
        assert_eq!(outcome.files_deleted, 0, "{outcome:?}");
        assert!(
            copy.exists(),
            "a content-addressed copy referenced by a surviving row must never be swept"
        );

        // Last reference gone: now it is genuinely orphaned.
        core.remove_items(&[second], false).unwrap();
        let outcome = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (2, 0),
            "the unreferenced copy and its checksum sidecar: {outcome:?}"
        );
        assert!(!copy.exists());
    }

    /// The sweep must not follow a link out of the storage directory. On
    /// Unix this is a symlink; the Windows equivalent (a junction or
    /// symlink, both reparse points) is covered by its own guarded test
    /// below, because creating one there needs Developer Mode or elevation.
    // [PLATFORM: macOS] / [PLATFORM: Linux] ─── begin ───────────────────
    #[cfg(unix)]
    #[test]
    fn sweep_does_not_follow_a_symlink_out_of_the_storage_directory() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(&dir.path().join("library"));

        // An outside directory holding a file whose *name* would otherwise
        // match the sweep's delete patterns.
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let bait = outside.join("01926f3a-7c2b-7a10-9f3d-2b6c5e4a1d88.json");
        std::fs::write(&bait, b"must survive").unwrap();

        std::os::unix::fs::symlink(&outside, storage.join("originals")).unwrap();
        std::os::unix::fs::symlink(&bait, storage.join("linked.json")).unwrap();

        let outcome = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (0, 0),
            "{outcome:?}"
        );
        assert!(
            bait.exists(),
            "the sweep must never reach outside the storage directory"
        );
        assert!(outside.exists());
    }
    // [PLATFORM: macOS] / [PLATFORM: Linux] ─── end ─────────────────────

    // [PLATFORM: Windows] ─── begin ─────────────────────────────────────
    // Windows-only file semantics that have no POSIX equivalent, and are
    // exactly what review finding Q10 says is untested: mandatory
    // delete-sharing, reserved device names, trailing-dot components, and
    // junctions as a second flavour of reparse point.

    /// Open a file so that *deleting* it is refused while the handle lives —
    /// `FILE_SHARE_READ` only, no `FILE_SHARE_DELETE`. This is what an
    /// antivirus scanner, the search indexer, a backup agent or another
    /// reader app typically does, and why "remove" can leave a file behind
    /// on Windows when it never would on POSIX (where unlink succeeds
    /// regardless of open handles). Note `std::fs::File::open` will **not**
    /// reproduce it: Rust opens with all three share flags, including
    /// delete.
    #[cfg(windows)]
    fn open_without_delete_sharing(path: &std::path::Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .expect("the blob must be openable before it is locked")
    }

    /// The core Q10 scenario. A stored file is held open by another handle
    /// while the user removes the item:
    ///
    /// - the database row goes away (the removal genuinely happened),
    /// - the file deletion fails,
    /// - and that failure is **observable** — reported as `Locked`, not
    ///   swallowed into a "removed successfully" that is untrue about the
    ///   user's data.
    ///
    /// Then, once the handle is released, a sweep reclaims the file — so a
    /// lock costs a deferred cleanup, never a permanent leak.
    #[cfg(windows)]
    #[test]
    fn a_locked_blob_is_reported_locked_the_row_still_goes_and_a_later_sweep_reclaims_it() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let src = dir.path().join("locked.txt");
        std::fs::write(&src, b"Antivirus is reading this quokka right now.").unwrap();
        let id = core.import_file(&src, &NullObserver).unwrap();

        let doc_path = storage.join(format!("{id}.json"));
        let handle = open_without_delete_sharing(&doc_path);

        let outcome = core.remove_items_detailed(&[id.clone()], true).unwrap();

        // The removal itself happened, in full.
        assert_eq!(outcome.removed_ids, vec![id.clone()]);
        assert!(core.list_items(0, 10).unwrap().is_empty());
        assert!(matches!(
            core.get_document(&id),
            Err(CoreError::NotFound(_))
        ));

        // The lock is visible, not silent.
        assert_eq!(
            outcome.files_failed, 1,
            "exactly the locked blob should fail: {outcome:?}"
        );
        assert_eq!(
            outcome.failure_kinds,
            vec![FileDeleteFailureKind::Locked],
            "a delete refused by another handle must be Locked, not Permission (which would tell \
             the user to go change ACLs for something antivirus will release in a second)"
        );
        assert_eq!(
            outcome.files_deleted,
            FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY - 1,
            "everything except the locked file should still be cleaned up: {outcome:?}"
        );
        assert_eq!(
            outcome.files_missing, 0,
            "a locked file is present-but-undeletable, never 'missing': {outcome:?}"
        );
        assert!(
            doc_path.exists(),
            "the locked file is necessarily still there"
        );

        // A sweep while the handle is still open must also fail honestly,
        // and must not lose the file.
        let blocked = core.sweep_orphaned_files().unwrap();
        assert_eq!(blocked.files_failed, 1, "{blocked:?}");
        assert_eq!(blocked.failure_kinds, vec![FileDeleteFailureKind::Locked]);
        assert!(doc_path.exists());

        drop(handle);

        let swept = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (swept.files_deleted, swept.files_failed),
            (1, 0),
            "once the handle is released the orphan must be reclaimed: {swept:?}"
        );
        assert!(!doc_path.exists());
        assert!(
            src.exists(),
            "the user's own file is never in scope (ADR-006)"
        );
    }

    /// Reserved device names (`CON`, `NUL`, `COM1`, …), trailing dots and
    /// trailing/leading spaces are the classic Windows filename traps.
    /// Which ones are creatable depends on the API path taken and the
    /// Windows build, so this test does not assert *that* they can be
    /// created — it asserts that wherever the OS **does** allow one, the
    /// import either succeeds and then removes completely, or fails with a
    /// clean typed error leaving **nothing** behind in storage. The thing
    /// that must never happen is a half-state: a stored copy or blob with no
    /// row, or a row whose files cannot be cleaned up.
    ///
    /// Observed on Windows 11 (26200) when written: `CON.txt`/`COM1.txt`/
    /// `LPT1.txt`/`AUX.txt` and the space-padded names are creatable through
    /// std's verbatim paths and import normally; `NUL.txt` opens the null
    /// device and is skipped; a name ending in `.` keeps its literal dot, so
    /// it has no file extension at all and is correctly refused as an
    /// unsupported type (plain text carries no magic bytes to fall back on).
    /// That refusal is a legitimate outcome, not a bug — but it must leave
    /// no residue, which is what this asserts.
    #[cfg(windows)]
    #[test]
    fn reserved_like_and_trailing_dot_source_names_round_trip_where_the_os_allows_them() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let candidates = [
            "CON.txt",
            "NUL.txt",
            "COM1.txt",
            "LPT1.txt",
            "AUX.txt",
            "trailing dot..txt",
            "ends with a dot.txt.",
            " leading space.txt",
            "trailing space .txt",
        ];

        let mut exercised = 0;
        let mut imported = 0;
        for name in candidates {
            let src = dir.path().join(name);
            // Create *and* read back: a reserved name can "succeed" as a
            // device rather than a file, which would make everything after
            // it meaningless.
            let content = format!("Reserved-name probe for {name}, quokka.");
            if let Err(e) = std::fs::write(&src, &content) {
                eprintln!("{name}: not creatable on this machine ({e}) — skipped");
                continue;
            }
            if !matches!(std::fs::read(&src), Ok(b) if b == content.as_bytes()) {
                // e.g. `NUL`, which swallows writes and reads back empty:
                // it is a device, not a file, so there is nothing to import.
                eprintln!("{name}: resolved to a device, not a file — skipped");
                let _ = std::fs::remove_file(&src);
                continue;
            }

            exercised += 1;
            match core.import_file(&src, &NullObserver) {
                Ok(id) => {
                    imported += 1;
                    assert!(
                        core.get_document(&id).is_ok(),
                        "{name}: must be readable back"
                    );

                    let outcome = core.remove_items_detailed(&[id], true).unwrap();
                    assert_eq!(
                        (outcome.files_deleted, outcome.files_failed),
                        (FILES_PER_FRESH_ITEM_WITH_SOURCE_COPY, 0),
                        "{name}: removal must clean up completely — {outcome:?}"
                    );
                }
                // A refused import must be a clean refusal: no row, and
                // nothing written under storage (the sandboxed copy is only
                // made after a successful parse, ADR-006).
                Err(e @ ImportError::UnsupportedType(_)) => {
                    eprintln!("{name}: refused as unsupported ({e}) — checking for residue");
                    assert!(core.list_items(0, 10).unwrap().is_empty());
                    let sweep = core.sweep_orphaned_files().unwrap();
                    assert_eq!(
                        (sweep.files_scanned, sweep.files_deleted),
                        (0, 0),
                        "{name}: a refused import must leave no file behind — {sweep:?}"
                    );
                }
                Err(e) => panic!("{name}: unexpected import failure: {e}"),
            }

            assert!(
                src.exists(),
                "{name}: the user's original must survive (ADR-006)"
            );
            std::fs::remove_file(&src).unwrap();
        }

        assert!(
            exercised > 0 && imported > 0,
            "no candidate name was creatable/importable, so this test proved nothing \
             (exercised {exercised}, imported {imported})"
        );
        // Nothing may be left in storage after every probe was removed.
        assert!(storage
            .join("originals")
            .read_dir()
            .unwrap()
            .next()
            .is_none());
    }

    /// The Windows half of "never follow a link out of the storage
    /// directory": a junction (`IO_REPARSE_TAG_MOUNT_POINT`) rather than a
    /// POSIX symlink. Creating one needs Developer Mode or elevation, so the
    /// test skips itself when the OS refuses rather than failing on an
    /// unprivileged machine — it is a real assertion where it can run, and
    /// the guarantee is also covered by the unix test above.
    #[cfg(windows)]
    #[test]
    fn sweep_does_not_follow_a_junction_or_symlink_out_of_the_storage_directory() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(&dir.path().join("library"));

        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let bait = outside.join("01926f3a-7c2b-7a10-9f3d-2b6c5e4a1d88.json");
        std::fs::write(&bait, b"must survive").unwrap();

        if std::os::windows::fs::symlink_dir(&outside, storage.join("originals")).is_err() {
            eprintln!(
                "skipping: creating a directory symlink needs Developer Mode or elevation on \
                 this machine (the same guarantee is asserted by the unix test)"
            );
            return;
        }

        let outcome = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (0, 0),
            "{outcome:?}"
        );
        assert!(
            bait.exists(),
            "the sweep must never reach outside the storage directory through a reparse point"
        );
    }

    /// Windows filesystems are case-insensitive, so `ABC.json` and
    /// `abc.json` are the *same file* — but two different strings. The
    /// sweep's keep-list is matched on file names, so a case mismatch
    /// between what a row recorded and what the directory listing reports
    /// would fail in the **unsafe** direction: a file a live row still
    /// references would look unreferenced and be deleted.
    ///
    /// Reproduced here by renaming a live item's blob to an upper-cased
    /// form of the same name (which on Windows changes only the stored
    /// case — the row still resolves to it, as the `get_document` assertion
    /// below proves) while the database keeps the original lower-case path.
    /// Without the case-insensitive keep-list this deletes a live item's
    /// document blob; with it, the file is kept.
    ///
    /// `#[cfg(windows)]` because the premise is false elsewhere: on a
    /// case-sensitive filesystem the renamed file genuinely *is* a
    /// different, unreferenced file, and sweeping it would be correct.
    #[cfg(windows)]
    #[test]
    fn sweep_keeps_a_referenced_file_whose_name_differs_only_by_case() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let src = dir.path().join("case.txt");
        std::fs::write(&src, b"A live item whose blob gets re-cased on disk.").unwrap();
        let id = core.import_file(&src, &NullObserver).unwrap();

        // Re-case the blob's name on disk; the row still says `<id>.json`.
        let as_written = storage.join(format!("{id}.json"));
        let re_cased = storage.join(format!("{}.json", id.to_ascii_uppercase()));
        std::fs::rename(&as_written, &re_cased).unwrap();

        assert!(
            core.get_document(&id).is_ok(),
            "precondition: on a case-insensitive filesystem the row must still resolve to the \
             re-cased file — otherwise this test is not exercising what it claims to"
        );

        let outcome = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (outcome.files_deleted, outcome.files_failed),
            (0, 0),
            "a live row's file must be kept despite the case difference: {outcome:?}"
        );
        assert!(
            re_cased.exists(),
            "the sweep deleted a file a live library row still references"
        );
        assert!(
            core.get_document(&id).is_ok(),
            "the item must still be readable after the sweep"
        );
    }

    /// The complete-delete path meeting the Windows case that makes removal
    /// fallible at all: the ADR-006 stored copy is held open without
    /// delete-sharing while the user removes its only item.
    ///
    /// The removal must still happen, the locked copy must be reported
    /// `Locked` (so the UI's "in use by another program … will retry next
    /// start" wording is literally true), and the next sweep — once the
    /// handle is gone — must reclaim it. This is what makes it honest to
    /// drop the old "Remove from Library" choice: a stored copy left behind
    /// by a lock is deferred cleanup, never a permanent leak.
    #[cfg(windows)]
    #[test]
    fn a_locked_stored_copy_is_reported_locked_and_reclaimed_by_a_later_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage) = core_rooted_at(dir.path());

        let src = dir.path().join("locked_copy.txt");
        std::fs::write(&src, b"Antivirus is holding the stored copy open.").unwrap();
        let before = sha256_hex(&src);
        let id = core.import_file(&src, &NullObserver).unwrap();

        let copy_path = the_one_sandboxed_copy(&storage);
        let handle = open_without_delete_sharing(&copy_path);

        let outcome = core.remove_items_detailed(&[id.clone()], true).unwrap();

        assert_eq!(outcome.removed_ids, vec![id]);
        assert_eq!(
            outcome.failure_kinds,
            vec![FileDeleteFailureKind::Locked],
            "a stored copy another handle holds open must read as Locked: {outcome:?}"
        );
        assert_eq!(
            outcome.shared_copies_kept, 0,
            "a locked copy is a failure, never a deliberately-kept shared copy: {outcome:?}"
        );
        assert!(
            copy_path.exists(),
            "the locked copy is necessarily still there"
        );

        drop(handle);

        let swept = core.sweep_orphaned_files().unwrap();
        assert!(
            !copy_path.exists(),
            "the sweep must reclaim the now-unlocked orphaned copy: {swept:?}"
        );
        assert!(src.exists() && sha256_hex(&src) == before);
    }
    // [PLATFORM: Windows] ─── end ───────────────────────────────────────

    // ── ADR-006 shared stored copies (complete-delete semantics) ─────────
    // Stored copies are content-addressed, so two items imported from
    // byte-identical files share one file on disk. Since 2026-09-21 removal
    // always deletes GIST's stored copy, which makes "who else still points
    // at this file?" a data-loss question rather than a tidiness one. These
    // tests are deliberately adversarial: every one of them also re-checks,
    // by SHA-256, that the user's own original files are untouched.

    /// SHA-256 of a file's bytes, lower-case hex. Used instead of a bare
    /// `exists()` check so a removal that truncated, emptied or rewrote a
    /// user's original file could not pass.
    fn sha256_hex(path: &std::path::Path) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(std::fs::read(path).expect("file must be readable"));
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Every non-sidecar file under `<storage>/originals/`.
    fn stored_copies(storage: &std::path::Path) -> Vec<std::path::PathBuf> {
        let dir = storage.join("originals");
        match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) != Some("blake3"))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Two items imported from two *differently named* files with identical
    /// bytes, which is exactly what makes ADR-006's content-addressed dedup
    /// give them one shared stored copy. Returns
    /// `(core, storage, [id_a, id_b], [src_a, src_b], shared_copy)`.
    #[allow(clippy::type_complexity)]
    fn two_items_sharing_one_stored_copy(
        dir: &std::path::Path,
    ) -> (
        Core,
        std::path::PathBuf,
        [String; 2],
        [std::path::PathBuf; 2],
        std::path::PathBuf,
    ) {
        let (core, storage) = core_rooted_at(dir);

        // Identical bytes, different file names: same SHA-256, so
        // `store_original_copy` dedups to one file.
        let bytes = b"Two library items, one quokka, one set of bytes on disk.";
        let src_a = dir.join("first name.txt");
        let src_b = dir.join("second name.txt");
        std::fs::write(&src_a, bytes).unwrap();
        std::fs::write(&src_b, bytes).unwrap();

        let id_a = core.import_file(&src_a, &NullObserver).unwrap();
        let id_b = core.import_file(&src_b, &NullObserver).unwrap();
        assert_ne!(id_a, id_b, "two imports must produce two distinct items");

        let copies = stored_copies(&storage);
        assert_eq!(
            copies.len(),
            1,
            "precondition: identical bytes must dedup to one stored copy (ADR-006), found {copies:?}"
        );
        let shared = copies.into_iter().next().unwrap();

        (core, storage, [id_a, id_b], [src_a, src_b], shared)
    }

    /// **The rule this whole change turns on.** Removing one of two items
    /// that share a stored copy must keep the file, because the survivor
    /// still references it — and must say so honestly: kept, not failed,
    /// not missing.
    #[test]
    fn removing_one_of_two_sharers_keeps_the_shared_stored_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage, [id_a, id_b], [src_a, src_b], shared) =
            two_items_sharing_one_stored_copy(dir.path());
        let (hash_a, hash_b) = (sha256_hex(&src_a), sha256_hex(&src_b));

        let outcome = core.remove_items_detailed(&[id_a.clone()], true).unwrap();

        assert_eq!(outcome.removed_ids, vec![id_a]);
        assert!(
            shared.exists(),
            "the surviving item's stored copy must not be deleted on the other item's behalf"
        );
        assert_eq!(
            outcome.shared_copies_kept, 1,
            "the kept copy must be reported as kept: {outcome:?}"
        );
        assert_eq!(
            (outcome.files_failed, outcome.files_missing),
            (0, 0),
            "a deliberately-kept shared copy is neither a failure nor a missing file: {outcome:?}"
        );
        assert_eq!(
            outcome.files_deleted, FILES_PER_FRESH_ITEM,
            "only the removed item's own blobs and sidecars should be deleted: {outcome:?}"
        );
        assert!(
            outcome.failure_kinds.is_empty(),
            "nothing was even attempted for the shared copy: {outcome:?}"
        );

        // The survivor is genuinely intact, not merely listed.
        assert!(core.get_document(&id_b).is_ok());

        // And a sweep must agree: a referenced file is never an orphan.
        let swept = core.sweep_orphaned_files().unwrap();
        assert_eq!(
            (swept.files_deleted, swept.files_failed),
            (0, 0),
            "the sweep must also keep a copy a live row still references: {swept:?}"
        );
        assert!(shared.exists());

        // Neither user file was touched, by content not just existence.
        assert_eq!(sha256_hex(&src_a), hash_a);
        assert_eq!(sha256_hex(&src_b), hash_b);
        assert!(
            storage.join("originals").exists(),
            "the originals directory itself must never be removed"
        );
    }

    /// Removing both sharers **in one batch** must delete the shared copy:
    /// the "still referenced?" question is asked after the whole batch has
    /// been applied, so nothing survives to reference it.
    #[test]
    fn removing_both_sharers_in_one_batch_deletes_the_shared_stored_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage, [id_a, id_b], [src_a, src_b], shared) =
            two_items_sharing_one_stored_copy(dir.path());
        let (hash_a, hash_b) = (sha256_hex(&src_a), sha256_hex(&src_b));
        let shared_sidecar = std::path::PathBuf::from(format!("{}.blake3", shared.display()));

        let outcome = core
            .remove_items_detailed(&[id_a.clone(), id_b.clone()], true)
            .unwrap();

        assert_eq!(outcome.removed_ids.len(), 2, "{outcome:?}");
        assert!(
            !shared.exists(),
            "with no row left referencing it, the shared copy must go"
        );
        assert!(
            !shared_sidecar.exists(),
            "its checksum sidecar must go with it (ADR-013)"
        );
        assert_eq!(
            outcome.shared_copies_kept, 0,
            "nothing was kept — both sharers went: {outcome:?}"
        );
        assert_eq!(
            (outcome.files_failed, outcome.files_missing),
            (0, 0),
            "{outcome:?}"
        );
        assert_eq!(
            outcome.files_deleted,
            // Two items' blobs + sidecars, plus the one shared copy + its
            // sidecar — deleted once, not once per item.
            FILES_PER_FRESH_ITEM * 2 + 2,
            "the shared copy must be deleted exactly once: {outcome:?}"
        );

        assert!(core.list_items(0, 10).unwrap().is_empty());
        assert_eq!(sha256_hex(&src_a), hash_a);
        assert_eq!(sha256_hex(&src_b), hash_b);
    }

    /// The same two items removed **one call at a time**. The first removal
    /// keeps the copy; the second — now the last referencing row — deletes
    /// it. This is the case a naive "delete whatever this row points at"
    /// implementation gets wrong in the direction that loses a live item's
    /// data, and the case a naive "never delete a shared copy" gets wrong in
    /// the direction that leaks forever.
    #[test]
    fn removing_sharers_one_at_a_time_deletes_the_copy_only_with_the_last_one() {
        let dir = tempfile::tempdir().unwrap();
        let (core, _storage, [id_a, id_b], [src_a, src_b], shared) =
            two_items_sharing_one_stored_copy(dir.path());
        let (hash_a, hash_b) = (sha256_hex(&src_a), sha256_hex(&src_b));

        let first = core.remove_items_detailed(&[id_a], true).unwrap();
        assert_eq!(first.shared_copies_kept, 1, "{first:?}");
        assert!(shared.exists(), "one sharer left — keep it");

        let second = core.remove_items_detailed(&[id_b], true).unwrap();
        assert_eq!(
            second.shared_copies_kept, 0,
            "the last sharer's removal must delete, not keep: {second:?}"
        );
        assert_eq!(
            (second.files_failed, second.files_missing),
            (0, 0),
            "{second:?}"
        );
        assert!(
            !shared.exists(),
            "the last referencing row is gone, so the copy must go with it"
        );

        assert_eq!(sha256_hex(&src_a), hash_a);
        assert_eq!(sha256_hex(&src_b), hash_b);
    }

    /// A mixed batch: two sharers plus an unrelated item plus an unknown id.
    /// Removing one sharer alongside the unrelated item must delete the
    /// unrelated item's own copy while keeping the shared one — i.e. the
    /// shared-copy rule is per file, not a blanket "keep everything when
    /// anything is shared".
    #[test]
    fn a_mixed_batch_keeps_only_the_still_shared_copy_and_deletes_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage, [id_a, id_b], [src_a, src_b], shared) =
            two_items_sharing_one_stored_copy(dir.path());

        let src_solo = dir.path().join("solo.txt");
        std::fs::write(&src_solo, b"Nobody else shares these bytes.").unwrap();
        let id_solo = core.import_file(&src_solo, &NullObserver).unwrap();

        let solo_copy = stored_copies(&storage)
            .into_iter()
            .find(|p| p != &shared)
            .expect("the unrelated item must have its own distinct stored copy");

        let hashes = [
            sha256_hex(&src_a),
            sha256_hex(&src_b),
            sha256_hex(&src_solo),
        ];

        let outcome = core
            .remove_items_detailed(
                &[id_a.clone(), id_solo.clone(), "not-a-real-id".to_string()],
                true,
            )
            .unwrap();

        let mut removed = outcome.removed_ids.clone();
        removed.sort();
        let mut expected = vec![id_a, id_solo];
        expected.sort();
        assert_eq!(
            removed, expected,
            "an unknown id must not be reported removed"
        );

        assert_eq!(
            outcome.shared_copies_kept, 1,
            "exactly the shared copy is kept: {outcome:?}"
        );
        assert!(shared.exists(), "still referenced by the surviving item");
        assert!(!solo_copy.exists(), "the unrelated item's own copy must go");
        assert_eq!(
            (outcome.files_failed, outcome.files_missing),
            (0, 0),
            "{outcome:?}"
        );

        assert!(core.get_document(&id_b).is_ok());
        for (src, hash) in [&src_a, &src_b, &src_solo].into_iter().zip(hashes) {
            assert_eq!(sha256_hex(src), hash, "{src:?} must be byte-identical");
        }
    }

    /// After a complete removal, nothing anywhere in the storage tree may
    /// still refer to the removed item — no blob, no tokens blob, no
    /// checksum sidecar, no stored copy — while the *shared* copy the
    /// survivor needs is still there. Walks the directory rather than
    /// checking the handful of paths the implementation happens to know
    /// about, so a file written by some other code path would still be
    /// caught.
    #[test]
    fn no_residue_referencing_a_removed_item_remains_in_the_storage_tree() {
        let dir = tempfile::tempdir().unwrap();
        let (core, storage, [id_a, id_b], [src_a, src_b], shared) =
            two_items_sharing_one_stored_copy(dir.path());
        let hash_a = sha256_hex(&src_a);

        core.remove_items_detailed(&[id_a.clone()], true).unwrap();

        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }

        let mut files = Vec::new();
        walk(&storage, &mut files);

        for path in &files {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                !name.contains(&id_a),
                "a file still named after the removed item survived: {path:?} (all: {files:?})"
            );
        }

        // The one thing that *must* remain: the copy the survivor shares.
        assert!(
            files.iter().any(|p| p == &shared),
            "the shared stored copy must survive for the other item: {files:?}"
        );
        assert!(
            files
                .iter()
                .any(|p| p.file_name().unwrap().to_string_lossy().contains(&id_b)),
            "the surviving item's own blobs must still be there: {files:?}"
        );
        assert_eq!(sha256_hex(&src_a), hash_a);
    }

    /// A stored copy whose *recorded* name looks like a path-traversal
    /// attempt must never make removal act outside the storage directory.
    ///
    /// `store_original_copy` already sanitises the extension it builds the
    /// name from (`F23`), so this attacks the layer above it instead: a row
    /// whose `source_copy_path` column has been rewritten by hand to a
    /// traversal path pointing at a real file outside storage — the shape a
    /// corrupted or tampered database would have. Removal must not delete
    /// that file.
    #[test]
    fn a_traversal_shaped_stored_copy_path_never_deletes_outside_the_storage_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let (core, storage) = core_rooted_at(&root);
        let db = root.join("test.db");

        // A file well outside the storage directory, standing in for
        // anything on the user's disk.
        let outside = dir.path().join("precious.txt");
        std::fs::write(&outside, b"Not GIST's to delete.").unwrap();
        let outside_hash = sha256_hex(&outside);

        let src = dir.path().join("traversal.txt");
        std::fs::write(&src, b"An item whose copy path gets rewritten.").unwrap();
        let src_hash = sha256_hex(&src);
        let id = core.import_file(&src, &NullObserver).unwrap();

        // Rewrite the row's stored-copy path to a traversal string that
        // resolves to `outside`.
        let traversal = storage
            .join("originals")
            .join("..")
            .join("..")
            .join("..")
            .join("precious.txt");
        assert_eq!(
            std::fs::canonicalize(&traversal).unwrap(),
            std::fs::canonicalize(&outside).unwrap(),
            "precondition: the traversal path must really resolve to the outside file"
        );

        // Tamper with the row exactly as a corrupted database would. Done on
        // a second connection so the `Core`'s own store never wrote it — the
        // point is that removal cannot trust this column, not that some code
        // path produces it.
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            let changed = conn
                .execute(
                    "UPDATE library_items SET source_copy_path = ?1 WHERE id = ?2",
                    rusqlite::params![traversal.to_string_lossy().as_ref(), &id],
                )
                .unwrap();
            assert_eq!(changed, 1, "the tampering must actually have hit the row");
        }

        let outcome = core.remove_items_detailed(&[id], true).unwrap();

        assert!(
            outside.exists() && sha256_hex(&outside) == outside_hash,
            "removal must never delete a file outside the storage directory, \
             whatever a row's stored-copy path says: {outcome:?}"
        );
        assert!(
            src.exists() && sha256_hex(&src) == src_hash,
            "the user's own file is never touched either"
        );

        // And the sweep must not act on it either: its keep-list and its
        // candidate patterns are both name-based and confined to the two
        // directories it scans.
        let swept = core.sweep_orphaned_files().unwrap();
        assert!(
            outside.exists() && sha256_hex(&outside) == outside_hash,
            "the sweep must not reach outside the storage directory either: {swept:?}"
        );
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
