use aes_gcm::aead::{Aead, Generate, KeyInit, Nonce};
use aes_gcm::Aes256Gcm;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ── Error ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("item not found: {0}")]
    NotFound(String),
    #[error("database schema version {found} is newer than this app's known version {expected}; upgrade the app")]
    SchemaTooNew { found: i64, expected: i64 },
    /// A row/file is marked `content_encrypted` but this `Store` was opened
    /// via [`Store::open`] (no [`KeyProvider`]), so there's no key to decrypt
    /// it with. Opening the same on-disk store via [`Store::open_encrypted`]
    /// with the same key resolves this.
    #[error("encrypted content found but no KeyProvider is configured for this Store")]
    MissingKeyProvider,
    /// Decryption failed — either the wrong key, or the ciphertext/nonce was
    /// corrupted or truncated. Deliberately doesn't wrap the underlying
    /// `aes_gcm::Error`, which carries no useful detail by design (AEAD
    /// implementations intentionally avoid distinguishing failure reasons,
    /// since doing so can leak information useful to an attacker).
    #[error("decryption failed (wrong key or corrupted data) for {0}")]
    DecryptionFailed(String),
    /// The BLAKE3 checksum recorded for a file at write time (ADR-013 /
    /// security register `A4`) no longer matches the bytes read back from
    /// disk — the file has been corrupted (bit rot, disk fault, manual
    /// tampering) since it was written. Deliberately a hard error rather
    /// than a silent pass-through: this app has no way to tell "corrupted"
    /// from "tampered", and returning corrupted content to the RSVP engine
    /// or FTS indexer silently is worse than surfacing a clear failure. Only
    /// raised when a checksum sidecar file actually exists for `path` — see
    /// ADR-013's backfill policy for what happens when one doesn't.
    #[error("checksum mismatch for {path}: file appears corrupted on disk")]
    ChecksumMismatch { path: String },
    /// The `annotations.kind` column held something other than
    /// `"highlight"`/`"note"`/`"bookmark"` — a corrupt row, since every
    /// write path (`Store::create_annotation`) only ever writes one of
    /// those three via `annotation_kind_to_sql`.
    #[error("invalid annotation kind stored in database: {0}")]
    InvalidAnnotationKind(String),
    /// An IR blob's (`<id>.json`/`<id>.tokens.json`, ADR-007) `ir_version`
    /// envelope field (ADR-019) is newer than this binary's
    /// `CURRENT_IR_VERSION` — the blob was written by a newer version of
    /// GIST whose IR shape this binary does not know how to interpret.
    /// Mirrors [`StoreError::SchemaTooNew`]'s shape for the SQLite schema,
    /// applied to the per-document JSON envelope instead: refuse to guess
    /// at unknown-shape fields rather than attempting a best-effort parse
    /// that could silently misinterpret or drop data.
    #[error("IR blob version {found} is newer than this app's known version {expected}; upgrade the app")]
    IrVersionTooNew { found: u32, expected: u32 },
}

// ── Encryption at rest (ADR-011) ─────────────────────────────────────────────

/// Supplies the AES-256 key used to encrypt/decrypt document blobs and
/// original-file copies at rest (ADR-011). Defined here, not in `gist-core`,
/// because `gist-store` — not `gist-core` — owns the actual file read/write
/// boundary this key is used at, and `gist-core` already depends on
/// `gist-store` (not the reverse), so the trait must live at or below the
/// point of use. `gist-core` re-exports this the same way it already
/// re-exports `gist_model::ParseLimits` — a lower-level crate's type,
/// surfaced for convenience at the facade layer without creating a cycle.
///
/// Real key custody lives outside Rust entirely: the platform layer (Swift
/// on Apple, backed by Keychain) implements this trait and is threaded in via
/// [`Store::open_encrypted`], mirroring how `OcrEngine` is threaded through
/// `gist-ffi` as a uniffi callback interface (see `gist-ffi`'s
/// `CoreOcrAdapter` for the adapter pattern this is expected to follow on the
/// FFI side — a `gist-ffi`-side `KeyProvider` callback interface + adapter,
/// not this trait exported directly over FFI, since uniffi callback
/// interfaces must be defined where the `#[uniffi::export]` macro runs).
pub trait KeyProvider: Send + Sync {
    /// Return the 32-byte AES-256 key for this store, creating and durably
    /// persisting one (in the platform keychain/equivalent) on first call if
    /// none exists yet. Must return the *same* key on every subsequent call
    /// for the life of the app's data — losing this key makes all encrypted
    /// content permanently unreadable, by design (there is no recovery path
    /// other than the platform's own keychain backup/sync, which is outside
    /// this trait's concern).
    fn get_or_create_key(&self) -> [u8; 32];
}

/// Fixed in-memory key for tests. Never use outside `#[cfg(test)]` —
/// there is no persistence and no secrecy; every instance with the same
/// `key` byte returns identical key material.
#[derive(Clone)]
pub struct FakeKeyProvider {
    key: [u8; 32],
}

impl FakeKeyProvider {
    /// Builds a fixed key by repeating `seed` to fill 32 bytes — convenient
    /// for tests that need two provably-different keys (e.g. a
    /// "wrong key" test), not for anything resembling real key derivation.
    pub fn new(seed: u8) -> Self {
        Self { key: [seed; 32] }
    }
}

impl KeyProvider for FakeKeyProvider {
    fn get_or_create_key(&self) -> [u8; 32] {
        self.key
    }
}

/// Encrypts `plaintext` with AES-256-GCM under `key`, returning
/// `nonce || ciphertext` (12-byte random nonce prepended to the AEAD output,
/// which already includes the authentication tag) — the whole thing is what
/// gets written to disk, and `decrypt_at_rest` expects exactly this layout.
/// A fresh random nonce is generated per call (`Aes256Gcm::generate_nonce`),
/// never reused, which AES-GCM requires for its security guarantees to hold.
fn encrypt_at_rest(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::<Aes256Gcm>::generate();
    // Only fails on absurd (>~64 GiB) plaintext sizes for this cipher, which
    // none of this app's document/original-file content can ever reach —
    // ParseLimits caps every import path well below that.
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .expect("AES-256-GCM encryption failed (plaintext far exceeds any realistic size)");
    let mut out = Vec::with_capacity(nonce.len() + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out
}

/// Reverses [`encrypt_at_rest`]: splits the leading 12-byte nonce from
/// `data`, then decrypts the remainder under `key`. `context` is used only
/// to name what failed in the returned error, not for any cryptographic
/// purpose.
fn decrypt_at_rest(key: &[u8; 32], data: &[u8], context: &str) -> Result<Vec<u8>, StoreError> {
    const NONCE_LEN: usize = 12;
    if data.len() < NONCE_LEN {
        return Err(StoreError::DecryptionFailed(context.to_string()));
    }
    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = <&Nonce<Aes256Gcm>>::try_from(nonce_bytes)
        .map_err(|_| StoreError::DecryptionFailed(context.to_string()))?;
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| StoreError::DecryptionFailed(context.to_string()))
}

// ── At-rest integrity checksums (ADR-013 / security register `A4`) ──────────

/// Extension appended to a checksummed file's path to name its checksum
/// sidecar file (e.g. `<id>.json` → `<id>.json.blake3`).
const CHECKSUM_EXT: &str = "blake3";

/// BLAKE3 hex digest of `bytes`. BLAKE3 (not a second cryptographic-integrity
/// scheme, chosen deliberately — see ADR-013): this is a corruption-detection
/// checksum, not a security boundary, and BLAKE3 is fast and already
/// license-compatible with `deny.toml`'s allow-list without any change to it
/// (`CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception` — the
/// `Apache-2.0` arm alone already satisfies the allow-list).
fn checksum_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The checksum sidecar path for `path` — a plain text file, alongside
/// `path`, containing just that file's BLAKE3 hex digest. A sidecar file
/// (not a new `library_items` DB column) was chosen because: (1) it applies
/// uniformly to `<id>.json`/`<id>.tokens.json` (ADR-007) *and*
/// `originals/<hash>.<ext>` (ADR-006) without needing a separate scheme for
/// the latter's content-addressed, potentially-multiply-referenced files;
/// (2) it needs no schema migration/version bump — nothing about this is
/// queried or filtered in SQL, it's purely "does this exact file's content
/// still match what was written"; (3) it keeps with ADR-007's existing
/// ethos of small, independently inspectable plain-text files alongside the
/// content they describe. See ADR-013 for the full reasoning.
fn checksum_sidecar_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(CHECKSUM_EXT);
    PathBuf::from(s)
}

/// Write (or overwrite) the checksum sidecar for `path`, recording the
/// BLAKE3 digest of `bytes` — the exact bytes being written to `path` (i.e.
/// post-encryption ciphertext when this `Store` is encrypted, ADR-011;
/// plaintext otherwise). Checksumming on-disk bytes rather than plaintext
/// means this catches corruption regardless of encryption status, and
/// verification (below) never needs a key.
fn write_checksum_sidecar(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    std::fs::write(checksum_sidecar_path(path), checksum_hex(bytes))?;
    Ok(())
}

/// Tri/four-state classification of one on-disk file's BLAKE3 checksum
/// sidecar (ADR-013 / security register `A4`) against its current bytes —
/// the information [`verify_checksum`]'s plain `Result<(), StoreError>`
/// deliberately collapses (a missing sidecar and a matching one are both
/// "fine to proceed" for the read path) but which an explicit integrity
/// report (`R3`, [`Store::verify_item_integrity`]) needs kept apart, so a
/// user can be told "this predates checksums" separately from "this is
/// confirmed intact" separately from "this is genuinely corrupted."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumStatus {
    /// A sidecar exists and its digest matches the file's current bytes.
    Verified,
    /// The file exists but has no checksum sidecar — written before
    /// ADR-013 landed, or (for an `originals/` copy) deduplicated against a
    /// pre-existing file that predates it. Not an error; there is nothing
    /// to compare against. See [`verify_checksum`]'s backfill policy.
    Unverified,
    /// A sidecar exists but its digest disagrees with the file's current
    /// bytes — the file has been corrupted (bit rot, disk fault, manual
    /// tampering) since it was written.
    Mismatch,
    /// The file itself does not exist on disk at all. Deliberately its own
    /// state, not folded into `Unverified`: a missing *sidecar* is routine
    /// (pre-`A4` data) but a missing *content file* means the row's own
    /// data is gone, which is a stronger signal — see
    /// [`Store::verify_item_integrity`]'s aggregation for how a caller is
    /// expected to weigh it.
    Missing,
}

/// Classify `bytes` (already read from `path`) against `path`'s checksum
/// sidecar, if one exists. Never returns [`ChecksumStatus::Missing`] — the
/// caller already has `bytes` in hand, so `path` plainly exists; that state
/// is only produced by [`checksum_status_of_file`], which does the read.
fn checksum_status(path: &str, bytes: &[u8]) -> Result<ChecksumStatus, StoreError> {
    let sidecar = checksum_sidecar_path(Path::new(path));
    let expected = match std::fs::read_to_string(&sidecar) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ChecksumStatus::Unverified)
        }
        Err(e) => return Err(StoreError::Io(e)),
    };
    if expected.trim() != checksum_hex(bytes) {
        Ok(ChecksumStatus::Mismatch)
    } else {
        Ok(ChecksumStatus::Verified)
    }
}

/// Read `path` fresh from disk and classify it against its checksum
/// sidecar. Unlike [`checksum_status`], this can report
/// [`ChecksumStatus::Missing`] when `path` itself doesn't exist. Used by
/// [`Store::verify_item_integrity`]/[`Store::verify_library_integrity`]
/// (`R3`) — an explicit integrity check, not the ordinary read path, so a
/// missing file is reported rather than treated as "nothing to verify."
fn checksum_status_of_file(path: &str) -> Result<ChecksumStatus, StoreError> {
    match std::fs::read(path) {
        Ok(bytes) => checksum_status(path, &bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ChecksumStatus::Missing),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Verify `bytes` (freshly read from `path`) against `path`'s checksum
/// sidecar, if one exists.
///
/// **Backfill/legacy policy (ADR-013):** a missing sidecar is *not* an
/// error — it means `path` was written before this feature existed (or,
/// for an `originals/` copy, was deduplicated against a pre-existing file
/// that predates it), and this function has no basis to assert anything
/// about its integrity either way. Only a *present-but-disagreeing* sidecar
/// produces [`StoreError::ChecksumMismatch`]. This means old data is never
/// spuriously flagged as corrupt, but also never silently credited with an
/// integrity guarantee it doesn't have.
fn verify_checksum(path: &str, bytes: &[u8]) -> Result<(), StoreError> {
    match checksum_status(path, bytes)? {
        ChecksumStatus::Mismatch => Err(StoreError::ChecksumMismatch {
            path: path.to_string(),
        }),
        // `Missing` is unreachable here (bytes were already read
        // successfully) but matched for exhaustiveness rather than `_`, so
        // a future new variant can't silently fall through unconsidered.
        ChecksumStatus::Verified | ChecksumStatus::Unverified | ChecksumStatus::Missing => Ok(()),
    }
}

// ── IR envelope versioning (ADR-019, CLAUDE.md open question Q10) ─────────

/// Current on-disk IR envelope format version. Bump this only when the
/// `Document`/`Token` JSON shape changes in a way an older binary cannot
/// safely interpret un-migrated (see ADR-019's migration story) — a plain
/// additive field on `Document`/`Section`/`Block`/`Token`/`Metadata` does
/// **not** require a bump, since `#[serde(default)]` (and the deliberate
/// absence of `#[serde(deny_unknown_fields)]` anywhere in `gist-model`)
/// already makes those forward/backward compatible without any version
/// check at all.
///
/// **Enum variants are the exception** (ADR-019 addendum, M6/R3): serde's
/// externally-tagged enums *error* on an unknown variant, so an older binary
/// cannot read a blob containing, say, `Block::Table`. The policy is a
/// *conditional* bump — a writer stamps each blob with the **lowest** version
/// able to represent its payload ([`required_ir_version`]), so documents that
/// don't use the new variant stay readable by older binaries, while those
/// that do are rejected by them with the typed
/// [`StoreError::IrVersionTooNew`] rather than an opaque JSON error.
/// `CURRENT_IR_VERSION` is the highest version this binary can *read*.
const CURRENT_IR_VERSION: u32 = IR_VERSION_TABLES;

/// v1: `Block` is `Heading | Paragraph | Image | List`.
const IR_VERSION_BASE: u32 = 1;
/// v2: `Block::Table` may appear in a document blob (ADR-019 addendum).
const IR_VERSION_TABLES: u32 = 2;

/// The lowest IR envelope version that can represent `doc` — see
/// [`CURRENT_IR_VERSION`]. A document with no table is written as v1,
/// byte-compatible with every earlier binary; one containing a
/// `Block::Table` is v2. Add a new arm/check here when a future variant
/// ships; never raise the version of a payload that doesn't need it.
fn required_ir_version(doc: &gist_model::Document) -> u32 {
    let has_table = doc.sections.iter().any(|section| {
        section
            .blocks
            .iter()
            .any(|b| matches!(b, gist_model::Block::Table { .. }))
    });
    if has_table {
        IR_VERSION_TABLES
    } else {
        IR_VERSION_BASE
    }
}

/// The on-disk envelope wrapping every IR blob `gist-store` writes
/// (`<id>.json`/`<id>.tokens.json`, ADR-007) as of ADR-019. Lives here, not
/// in `gist-model`, deliberately: this is an I/O-boundary/storage-format
/// concern, not part of the pure IR model that must stay
/// `wasm32-unknown-unknown`-compilable — `gist_model::Document`/`Token`
/// themselves carry no version field and know nothing about this wrapper.
///
/// `payload` is kept as a borrowed reference on write ([`serialize_ir_blob`])
/// and an owned value on read ([`deserialize_ir_blob`]) — two separate
/// monomorphisations of the same shape rather than one generic struct
/// shared byref/by-value, which would need a lifetime parameter threaded
/// through call sites that don't otherwise need one.
#[derive(Serialize)]
struct IrEnvelopeRef<'a, T> {
    ir_version: u32,
    payload: &'a T,
}

/// Serialize `payload` wrapped in the current [`IrEnvelopeRef`] — what every
/// new write of an IR blob produces from this version of GIST onward
/// (ADR-019). The resulting JSON is still a single flat, human-readable
/// object (`{"ir_version":1,"payload":{...}}`), preserving ADR-007's
/// "inspectable with any editor" property.
fn serialize_ir_blob<T: Serialize>(payload: &T, ir_version: u32) -> Result<String, StoreError> {
    Ok(serde_json::to_string(&IrEnvelopeRef {
        ir_version,
        payload,
    })?)
}

/// Deserialize an IR blob written by either this version of GIST (wrapped
/// in an `{"ir_version": N, "payload": ...}` envelope, ADR-019) or by any
/// version before ADR-019 landed (the bare `Document`/`Vec<Token>` JSON,
/// with no envelope at all — ADR-007's original, still-unversioned format).
///
/// **Forward compatibility:** the version check happens *before* any
/// attempt to deserialize `payload` into `T` — a blob whose `ir_version` is
/// newer than [`CURRENT_IR_VERSION`] is rejected with
/// [`StoreError::IrVersionTooNew`] immediately, never handed to `T`'s
/// `Deserialize` impl at all. This matters: a future format's `payload`
/// might not even be structurally parseable as today's `T`, and attempting
/// it anyway could produce a misleading generic JSON error (or, worse, an
/// incomplete-but-technically-valid `T` that silently drops fields this
/// binary doesn't know about) instead of a clear "upgrade the app" signal.
///
/// **Backward compatibility / legacy blobs:** distinguishing the two shapes
/// needs no separate on-disk marker. A pre-ADR-019 blob is either a JSON
/// object with no `"ir_version"` key (a bare `Document`) or a JSON array (a
/// bare `Vec<Token>`) — `serde_json::Value::get("ir_version")` returns
/// `None` for both (arrays never match a string key; a bare `Document`
/// object simply doesn't have that key), so the absence of the key is
/// itself the signal that this is a legacy, implicitly-version-1 blob. It's
/// deserialized directly as `T`, exactly as every binary before this change
/// already did — this is the "existing users' libraries must not break"
/// guarantee ADR-019 requires, and is covered by a dedicated test reading a
/// blob in exactly this pre-existing shape.
fn deserialize_ir_blob<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
    deserialize_ir_blob_with_max(bytes, CURRENT_IR_VERSION)
}

/// [`deserialize_ir_blob`] with an explicit highest-readable version.
/// Production always passes [`CURRENT_IR_VERSION`]; the parameter exists so
/// tests can play an *older* binary (`max_version = IR_VERSION_BASE`) against
/// a blob written by this one — the forward-compat evidence ADR-019's
/// addendum rests on.
fn deserialize_ir_blob_with_max<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    max_version: u32,
) -> Result<T, StoreError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    match value.get("ir_version") {
        Some(v) => {
            // A malformed/non-numeric ir_version is treated as "at least as
            // new as we can prove it isn't" — fail closed via
            // IrVersionTooNew rather than falling through to a payload
            // parse attempt whose error would be far less clear about what
            // actually went wrong.
            let found = v.as_u64().map(|n| n as u32).unwrap_or(u32::MAX);
            if found > max_version {
                return Err(StoreError::IrVersionTooNew {
                    found,
                    expected: max_version,
                });
            }
            let payload = value
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Ok(serde_json::from_value(payload)?)
        }
        // No `ir_version` key at all: a legacy, pre-ADR-019 blob (or a
        // hypothetical future blob using a different envelope shape
        // entirely — both are handled identically here by falling back to
        // a direct parse, which is correct for the former and merely
        // best-effort, not a security-relevant guarantee, for the latter).
        None => Ok(serde_json::from_value(value)?),
    }
}

// ── Long database paths on Windows (review Q10) ────────────────────────────

/// The path to hand SQLite for the database file.
///
/// Everything else this crate touches goes through `std::fs`, which converts
/// absolute Windows paths to the `\\?\` verbatim form internally and so
/// already works past the legacy 260-character `MAX_PATH` limit. SQLite does
/// not: its Win32 VFS calls `CreateFileW` with the path as given, which stays
/// bound by `MAX_PATH` unless the *process* opts in (a `longPathAware`
/// manifest **and** the machine-wide `LongPathsEnabled` policy). A GIST
/// library under a long packaged `LocalState` path therefore failed to open
/// at all — `CannotOpen` — while every blob, sidecar and original copy beside
/// it read and wrote fine. This normalises the database path so that cannot
/// happen regardless of how the host process is manifested.
///
/// Applied only when the path is close enough to the limit to matter, so the
/// ordinary case is byte-for-byte unchanged. The budget subtracts room for
/// the `-journal`/`-wal`/`-shm` siblings SQLite derives by appending to this
/// name.
#[cfg(windows)]
// [PLATFORM: Windows] ─── begin ───────────────────────────────────────────
fn sqlite_path(db_path: &Path) -> PathBuf {
    const MAX_PATH: usize = 260;
    /// Longest suffix SQLite appends to the database filename (`-journal`).
    const LONGEST_DERIVED_SUFFIX: usize = 8;

    if db_path.as_os_str().len() + LONGEST_DERIVED_SUFFIX < MAX_PATH {
        return db_path.to_owned();
    }
    // `canonicalize` returns the verbatim (`\\?\`) form, but requires the
    // path to exist — the database file itself may not yet, so canonicalise
    // the parent directory (the caller has just created it) and re-attach
    // the file name. If that fails for any reason, fall back to the path as
    // given rather than failing the open here: the caller's own error is
    // more informative than one invented in this helper.
    match (db_path.parent(), db_path.file_name()) {
        (Some(parent), Some(name)) => match std::fs::canonicalize(parent) {
            Ok(canonical) => canonical.join(name),
            Err(_) => db_path.to_owned(),
        },
        _ => db_path.to_owned(),
    }
}
// [PLATFORM: Windows] ─── end ─────────────────────────────────────────────

/// No-op on platforms whose path limits SQLite already satisfies.
#[cfg(not(windows))]
fn sqlite_path(db_path: &Path) -> PathBuf {
    db_path.to_owned()
}

// ── Schema version ────────────────────────────────────────────────────────

// v5 added `content_encrypted` (ADR-011) — see Store::open's migration block.
// No schema change accompanies the at-rest checksums added in the same
// release as this comment (ADR-013 / `A4`) — see that ADR for why a sidecar
// file, not a column, was chosen; `SCHEMA_VERSION` is unaffected.
// v6 added the `annotations` table (ADR-003) — see Store::open's migration
// block.
// v7 added `library_items.last_opened_at` and `idx_tokens_item_idx`
// (ADR-021, reading-state model).
const SCHEMA_VERSION: i64 = 7;

// ── LibraryItem (lightweight row, not the full Document) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub source_path: Option<String>,
    pub cover_path: Option<String>,
    pub created_at: i64,
    // TODO M2: populate from tokens table
    pub token_count: Option<usize>,
    /// Mirrors the row's `content_encrypted` column (ADR-011) — `true` once
    /// [`Store::encrypt_item`]/`insert_item` (under [`Store::open_encrypted`])
    /// has encrypted this item's `.json`/`.tokens.json` blobs at rest. Reads
    /// straight from SQL metadata, so this is populated correctly even by a
    /// plain [`Store::open`] instance that cannot itself decrypt the item's
    /// content — see [`Store::encrypt_item`]'s doc comment for why those are
    /// different things.
    pub content_encrypted: bool,
    /// Normalised source format of the import (`"txt"`, `"epub"`, `"docx"`,
    /// `"web"`, `"pdf"`, `"ocr"`), read from the stored `Metadata.source_type`
    /// (a `docx:tracked-changes` variant is folded into `"docx"`). Empty when
    /// the importer recorded none. ADR-021.
    pub source_type: String,
    /// Unix milliseconds of the last time either reader opened this item;
    /// `None` = never opened (schema v7, ADR-021).
    pub last_opened_at: Option<i64>,
    /// Derived RSVP reading progress in `0.0..=1.0` (ADR-021): saved RSVP
    /// token index over the item's last indexed word position. An item read
    /// only in the flow view has no RSVP position, so this is `0.0` for it
    /// even when `last_opened_at` is set (accepted limitation, D2).
    pub progress_fraction: f64,
}

/// SELECT list + FROM/JOIN shared by every library-row query (ADR-021). The
/// derived progress uses the `idx_tokens_item_idx` index (v7) so the per-row
/// `MAX(token_idx)` is an index seek, and is only evaluated for rows that
/// actually have a saved position.
const LIBRARY_ITEM_SELECT: &str = "SELECT li.id, li.title, li.authors, li.source_path,
        li.cover_path, li.created_at, li.content_encrypted, li.last_opened_at,
        COALESCE(CASE WHEN json_valid(li.metadata_json)
                      THEN json_extract(li.metadata_json, '$.source_type') END, ''),
        -- scalar MAX/MIN return NULL if any argument is NULL, so an item with
        -- no indexed words (no tokens rows) yields NULL -> 0.0, never a divide.
        COALESCE(CASE WHEN COALESCE(rp.token_index, 0) > 0
             THEN MIN(1.0, rp.token_index * 1.0 / MAX(
                    (SELECT MAX(t.token_idx) FROM tokens t WHERE t.item_id = li.id), 1))
             END, 0.0)
     FROM library_items li
     LEFT JOIN reading_progress rp ON rp.item_id = li.id";

/// Maps one row of [`LIBRARY_ITEM_SELECT`] to a [`LibraryItem`].
fn library_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryItem> {
    let authors_json: String = row.get(2)?;
    let authors: Vec<String> = serde_json::from_str(&authors_json).unwrap_or_default();
    let raw_type: String = row.get(8)?;
    let source_type = match raw_type.split(':').next() {
        Some(base) => base.to_owned(),
        None => String::new(),
    };
    Ok(LibraryItem {
        id: row.get(0)?,
        title: row.get(1)?,
        authors,
        source_path: row.get(3)?,
        cover_path: row.get(4)?,
        created_at: row.get(5)?,
        token_count: None, // TODO M2: populate from tokens table
        content_encrypted: row.get(6)?,
        source_type,
        last_opened_at: row.get(7)?,
        progress_fraction: row.get::<_, f64>(9)?.clamp(0.0, 1.0),
    })
}

// ── RemovedItem (returned by remove_items so callers can clean up files) ──

/// Identifies an item removed by [`Store::remove_items`], carrying the file
/// paths the caller needs to delete the on-disk blobs (and, optionally, the
/// original source file) after the DB transaction has committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedItem {
    pub id: String,
    /// Path to the `<id>.json` document blob (derived storage, not the
    /// user's original file — always safe/expected to delete).
    pub doc_path: String,
    /// The original import location (filesystem path or URL), informational
    /// only. Per ADR-006, this is never deleted or otherwise touched by
    /// GIST — it may point outside app storage entirely.
    pub source_path: Option<String>,
    /// Path to the sandboxed, content-addressed copy of the originally
    /// imported file (ADR-006), if one exists. `None` for URL-imported items
    /// (no local file was ever copied) or items imported before this field
    /// existed. This — not `source_path` — is what removal deletes when the
    /// caller asks to discard the source file.
    pub source_copy_path: Option<String>,
    /// `true` when `source_copy_path` is **still referenced by a library row
    /// that survived this removal**, so the caller must not delete that file
    /// (ADR-006 addendum, 2026-09-21).
    ///
    /// Stored copies are content-addressed, so two items imported from
    /// byte-identical files share one file on disk. Deleting it on behalf of
    /// one item would silently take away the other item's copy. Evaluated
    /// inside the same transaction, *after* every id in the batch has been
    /// deleted, which gives the intended batch semantics for free: removing
    /// both sharers in one call deletes the file (nothing references it any
    /// more), removing one keeps it, and removing the second one later
    /// deletes it.
    ///
    /// Always `false` when `source_copy_path` is `None` — there is nothing to
    /// share.
    pub source_copy_still_referenced: bool,
}

/// Key a stored-copy path is compared by when deciding whether a surviving
/// row still references it: the lower-cased file name, or the lower-cased
/// whole path when there is no file name.
///
/// Compares names rather than whole path strings for the same reason
/// `gist_core::Core::sweep_orphaned_files` does: a row's stored path is
/// whatever string `storage_dir` was when the row was written, so prefix and
/// separator normalisation (`C:\x` vs `C:\x\`, `\\?\C:\x`, …) must not be
/// able to turn into data loss. Lower-casing matches Windows' case-insensitive
/// filesystem semantics. Both choices are conservative in the only safe
/// direction: they can only ever make a file look *more* referenced, i.e. keep
/// a file that could have been deleted — never delete one that is still in use.
fn copy_ref_key(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase()
}

// ── ReferencedFiles (what an orphan sweep must never delete) ───────────────

/// The on-disk files one `library_items` row still refers to, as returned by
/// [`Store::list_referenced_files`]. Used by
/// `gist_core::Core::sweep_orphaned_files` (review Q10) to build the
/// keep-list before deleting anything in the storage directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferencedFiles {
    /// Path to the `<id>.json` document blob (ADR-007). The
    /// `<id>.tokens.json` blob and both `.blake3` sidecars are derived from
    /// this name.
    pub doc_path: String,
    /// Path to the sandboxed, content-addressed copy of the original import
    /// (ADR-006), if one exists. Because copies are content-addressed, the
    /// *same* path can appear for more than one row — which is exactly why
    /// the sweep keeps a file while **any** row still references it.
    pub source_copy_path: Option<String>,
}

// ── Item integrity report (ADR-013 / security register `A4`, `R3`) ─────────

/// Per-file [`ChecksumStatus`] breakdown for one `library_items` row, as
/// returned by [`Store::verify_item_integrity`]/
/// [`Store::verify_library_integrity`]. `gist_core::Core` aggregates this
/// into a single pass/fail/unverified verdict; kept here as the full
/// breakdown so a caller that wants more than the three-state summary
/// (e.g. "your original copy is missing but your reading copy is fine")
/// doesn't need a second call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemIntegrityReport {
    pub id: String,
    /// Status of the `<id>.json` document blob (ADR-007).
    pub doc_status: ChecksumStatus,
    /// Status of the `<id>.tokens.json` blob. `Unverified` (not `Missing`)
    /// when this item predates ADR-007's separate tokens file and has none
    /// — see [`Store::verify_item_integrity`]'s implementation note.
    pub tokens_status: ChecksumStatus,
    /// Status of the ADR-006 sandboxed `originals/` copy, or `None` if
    /// this item has no such copy (a URL import, or one that predates
    /// ADR-006).
    pub original_copy_status: Option<ChecksumStatus>,
}

// ── Collection ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub created_at: i64,
}

// ── Store ─────────────────────────────────────────────────────────────────

pub struct Store {
    conn: Mutex<Connection>,
    storage_dir: PathBuf,
    /// `Some` for a `Store` opened via [`Store::open_encrypted`] — governs
    /// whether *newly written* content is encrypted (ADR-011): consulted
    /// only by `insert_item`/`store_original_copy` to decide whether a new
    /// write gets encrypted. `None` (the [`Store::open`] and
    /// [`Store::open_with_read_key`] paths) preserves the pre-ADR-011
    /// plaintext-write behavior exactly, unchanged.
    ///
    /// **Deliberately separate from `read_key_provider` below (ADR-014).**
    /// Early per-item encryption (`Store::encrypt_item`) let a
    /// `content_encrypted = 1` row exist on a `Store` instance with no key
    /// provider at all — e.g. exactly `CoreClient.shared`'s production
    /// instance — which then couldn't decrypt its own write back, a real
    /// data-access bug: an item a user encrypted became permanently
    /// unreadable in the running app. This field and `read_key_provider`
    /// decouple "can this `Store` decrypt content on read" from "does this
    /// `Store` auto-encrypt new writes" — two independent questions that
    /// [`Store::open_encrypted`] happens to answer the same way (both
    /// `Some`, same provider) but [`Store::open_with_read_key`] does not
    /// (write stays `None`/plaintext-by-default; read gets a real key).
    key_provider: Option<Arc<dyn KeyProvider>>,
    /// Consulted only by [`Store::read_maybe_encrypted`] to decrypt content
    /// already flagged `content_encrypted = 1` — never consulted by
    /// `insert_item`/`store_original_copy` to decide whether a *new* write
    /// gets encrypted (that's `key_provider`'s job, above). `Some` for both
    /// [`Store::open_encrypted`] (same provider as `key_provider`) and
    /// [`Store::open_with_read_key`] (read-only capability, `key_provider`
    /// stays `None`); `None` only for plain [`Store::open`], which
    /// genuinely cannot decrypt anything (see
    /// `reading_encrypted_item_without_key_provider_fails_cleanly`).
    read_key_provider: Option<Arc<dyn KeyProvider>>,
}

impl Store {
    /// Open (or create) the SQLite database at `db_path`, with document
    /// content stored **unencrypted** — the pre-ADR-011 behavior, unchanged.
    /// Document JSON blobs are stored under `storage_dir`. No key provider
    /// of any kind is configured — this `Store` can neither encrypt new
    /// writes nor decrypt a row some other `Store` instance flagged
    /// encrypted (see [`Store::open_with_read_key`] for that case).
    pub fn open(db_path: &Path, storage_dir: &Path) -> Result<Self, StoreError> {
        Self::open_internal(db_path, storage_dir, None, None)
    }

    /// Open (or create) the SQLite database at `db_path`, encrypting newly
    /// written document blobs, token streams, and original-file copies at
    /// rest under a key from `key_provider` (ADR-011, AES-256-GCM).
    ///
    /// **Migration:** a store that already has rows from before this existed
    /// (or from a plain [`Store::open`] session) keeps working unchanged —
    /// each `library_items` row carries its own `content_encrypted` flag,
    /// set at insert time, so `get_item`/`get_tokens` know per-row whether to
    /// decrypt or read as plaintext. Existing rows are never rewritten or
    /// force-migrated by opening this way; only content inserted *after*
    /// this call is encrypted. This was chosen over an eager bulk
    /// re-encryption pass at open time because a library can be large, import
    /// is otherwise this app's only write path to document content (nothing
    /// currently updates a document in place), and a partial bulk migration
    /// interrupted mid-way is a materially worse failure mode than "old items
    /// stay as they were until re-imported" — see ADR-011 for the full
    /// reasoning.
    pub fn open_encrypted(
        db_path: &Path,
        storage_dir: &Path,
        key_provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, StoreError> {
        Self::open_internal(
            db_path,
            storage_dir,
            Some(key_provider.clone()),
            Some(key_provider),
        )
    }

    /// Open (or create) the SQLite database at `db_path` with **read-only**
    /// decryption capability (ADR-014): `key_provider` is used exclusively
    /// by [`Store::read_maybe_encrypted`] to decrypt rows some `Store`
    /// instance already flagged `content_encrypted = 1` (whether by
    /// [`Store::open_encrypted`]'s auto-write path historically, or by
    /// [`Store::encrypt_item`]'s on-demand per-item path) — it is never
    /// consulted by `insert_item`/`store_original_copy`, so new imports
    /// through this `Store` land plaintext by default, identically to
    /// [`Store::open`].
    ///
    /// This is the fix for the gap [`Store::encrypt_item`] originally
    /// documented as a known consequence: a `Store` with no key provider at
    /// all (e.g. `CoreClient.shared`'s production instance, pre-ADR-014-fix)
    /// could encrypt an item via `encrypt_item` and then never read that
    /// same item's content back through itself. A `Store` opened this way
    /// can do both — encrypt an item on demand (still by passing an
    /// explicit key to `encrypt_item`, unchanged) and then immediately read
    /// it back via `get_item`/`get_tokens` — while every *other* new item it
    /// imports stays plaintext unless separately encrypted the same way.
    pub fn open_with_read_key(
        db_path: &Path,
        storage_dir: &Path,
        key_provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, StoreError> {
        Self::open_internal(db_path, storage_dir, None, Some(key_provider))
    }

    fn open_internal(
        db_path: &Path,
        storage_dir: &Path,
        key_provider: Option<Arc<dyn KeyProvider>>,
        read_key_provider: Option<Arc<dyn KeyProvider>>,
    ) -> Result<Self, StoreError> {
        std::fs::create_dir_all(storage_dir)?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(sqlite_path(db_path))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version > SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                found: version,
                expected: SCHEMA_VERSION,
            });
        }

        // v0 → v1: base schema
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS library_items (
                     id            TEXT PRIMARY KEY,
                     title         TEXT,
                     authors       TEXT,
                     source_path   TEXT,
                     source_url    TEXT,
                     doc_path      TEXT NOT NULL,
                     cover_path    TEXT,
                     created_at    INTEGER NOT NULL,
                     updated_at    INTEGER NOT NULL,
                     metadata_json TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS reading_progress (
                     item_id     TEXT PRIMARY KEY REFERENCES library_items(id) ON DELETE CASCADE,
                     token_index INTEGER NOT NULL DEFAULT 0,
                     updated_at  INTEGER NOT NULL
                 );
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 1");
        }

        // v1 → v2: FTS5 token index
        if version < 2 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS tokens (
                     rowid      INTEGER PRIMARY KEY,
                     item_id    TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                     token_idx  INTEGER NOT NULL,
                     token_text TEXT NOT NULL
                 );
                 CREATE VIRTUAL TABLE IF NOT EXISTS fts_index USING fts5(
                     token_text,
                     content='tokens',
                     content_rowid='rowid',
                     tokenize='porter unicode61'
                 );
                 CREATE TRIGGER IF NOT EXISTS tokens_ai AFTER INSERT ON tokens BEGIN
                     INSERT INTO fts_index(rowid, token_text) VALUES (new.rowid, new.token_text);
                 END;
                 CREATE TRIGGER IF NOT EXISTS tokens_ad AFTER DELETE ON tokens BEGIN
                     INSERT INTO fts_index(fts_index, rowid, token_text)
                         VALUES ('delete', old.rowid, old.token_text);
                 END;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 2 (FTS5 token index)");
        }

        // v2 → v3: collections + tags
        if version < 3 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS collections (
                     id         TEXT PRIMARY KEY,
                     name       TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS item_collections (
                     item_id       TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                     collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                     PRIMARY KEY (item_id, collection_id)
                 );
                 CREATE TABLE IF NOT EXISTS tags (
                     id   TEXT PRIMARY KEY,
                     name TEXT NOT NULL UNIQUE
                 );
                 CREATE TABLE IF NOT EXISTS item_tags (
                     item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                     tag_id  TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                     PRIMARY KEY (item_id, tag_id)
                 );
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 3 (collections + tags)");
        }

        // v3 → v4: sandboxed copy of the originally imported file (ADR-006).
        // `source_path` (from `Metadata.source_ref`) remains the raw,
        // informational-only import location; this new column is the one
        // file-deletion code should ever touch.
        if version < 4 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE library_items ADD COLUMN source_copy_path TEXT;
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 4 (source_copy_path)");
        }

        // v4 → v5: per-item encrypted-at-rest flag (ADR-011). Defaults to 0
        // (unencrypted) for every pre-existing row, which is exactly correct
        // for rows written before this column existed — see
        // `Store::open_encrypted`'s doc comment for why that's sufficient
        // and no bulk re-encryption pass runs here.
        if version < 5 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE library_items ADD COLUMN content_encrypted INTEGER NOT NULL DEFAULT 0;
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 5 (content_encrypted)");
        }

        // v5 → v6: annotations (highlights/notes/bookmarks), anchored per
        // ADR-003 as (block_id, start, len, prefix_hash, quote_hash).
        // `start`/`len` are named `start_offset`/`len_bytes` in SQL only to
        // sidestep any ambiguity with SQL keywords — the Rust-side
        // `gist_model::Annotation` struct still uses `start`/`len`, matching
        // ADR-003's own field names exactly. `prefix_hash`/`quote_hash` are
        // FNV-1a `u64`s stored as `INTEGER` (SQLite's signed 64-bit type) via
        // an `as i64`/`as u64` bit-pattern cast, which is lossless in both
        // directions — see `annotation_kind_to_sql`/`_from_sql` and
        // `create_annotation` below.
        if version < 6 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS annotations (
                     id           TEXT PRIMARY KEY,
                     item_id      TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                     kind         TEXT NOT NULL,
                     block_id     TEXT NOT NULL,
                     start_offset INTEGER NOT NULL,
                     len_bytes    INTEGER NOT NULL,
                     prefix_hash  INTEGER NOT NULL,
                     quote_hash   INTEGER NOT NULL,
                     note_text    TEXT,
                     created_at   INTEGER NOT NULL,
                     updated_at   INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_annotations_item_id ON annotations(item_id);
                 PRAGMA user_version = 6;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 6 (annotations)");
        }

        // v6 → v7: reading-state model (ADR-021). `last_opened_at` is NULL
        // for every existing row (= never opened; nothing is backfilled).
        // The index makes the per-item `MAX(token_idx)` used for derived
        // progress an index seek instead of a scan of the whole tokens table.
        if version < 7 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE library_items ADD COLUMN last_opened_at INTEGER;
                 CREATE INDEX IF NOT EXISTS idx_tokens_item_idx ON tokens(item_id, token_idx);
                 PRAGMA user_version = 7;
                 COMMIT;",
            )?;
            tracing::info!("gist-store: migrated schema to version 7 (last_opened_at)");
        }

        Ok(Self {
            conn: Mutex::new(conn),
            storage_dir: storage_dir.to_owned(),
            key_provider,
            read_key_provider,
        })
    }

    /// The directory this `Store` writes document blobs, token streams,
    /// checksum sidecars and the ADR-006 `originals/` copies into — i.e. the
    /// `storage_dir` it was opened with. Read-only accessor, added so
    /// `gist_core::Core::sweep_orphaned_files` (review Q10) can enumerate
    /// what is actually on disk and compare it against
    /// [`Store::list_referenced_files`]; nothing here mutates state.
    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
    }

    /// Every on-disk file path currently referenced by a `library_items`
    /// row: the `<id>.json` IR blob (ADR-007) and, where one exists, the
    /// sandboxed content-addressed copy of the original import (ADR-006).
    ///
    /// The `<id>.tokens.json` blob and every `.blake3` checksum sidecar are
    /// *derived* from these two paths rather than stored separately, so
    /// callers reconstruct those names themselves (see
    /// `gist_core::Core::sweep_orphaned_files`).
    ///
    /// Deliberately returns `source_copy_path`, never `source_path`: the
    /// latter is the user's real file at its real location, which GIST must
    /// never delete and which the orphan sweep must therefore never even
    /// consider (ADR-006).
    pub fn list_referenced_files(&self) -> Result<Vec<ReferencedFiles>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare("SELECT doc_path, source_copy_path FROM library_items")?;
        let rows = stmt.query_map([], |row| {
            Ok(ReferencedFiles {
                doc_path: row.get(0)?,
                source_copy_path: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// `Some(32-byte key)` if this `Store` was opened via
    /// [`Store::open_encrypted`]; `None` otherwise (including
    /// [`Store::open_with_read_key`] — this key governs auto-encryption of
    /// *new* writes only, see `key_provider`'s field doc comment).
    fn encryption_key(&self) -> Option<[u8; 32]> {
        self.key_provider.as_ref().map(|kp| kp.get_or_create_key())
    }

    /// `Some(32-byte key)` if this `Store` can decrypt already-encrypted
    /// content on read — true for [`Store::open_encrypted`] and
    /// [`Store::open_with_read_key`], `None` only for plain [`Store::open`].
    /// Falls back to `key_provider` so `open_encrypted` (which sets both
    /// fields to the same provider) needs no special-casing here; the two
    /// fields only actually diverge for `open_with_read_key`.
    fn decryption_key(&self) -> Option<[u8; 32]> {
        self.read_key_provider
            .as_ref()
            .or(self.key_provider.as_ref())
            .map(|kp| kp.get_or_create_key())
    }

    /// Persist a [`gist_model::Document`] to disk (JSON blob + token file) and
    /// index its metadata and Word tokens in SQLite.
    ///
    /// When this `Store` was opened via [`Store::open_encrypted`] (ADR-011),
    /// both files are written AES-256-GCM-encrypted and the row's
    /// `content_encrypted` flag is set to 1, so [`Store::get_item`]/
    /// [`Store::get_tokens`] know to decrypt them later. A [`Store::open`]
    /// (no key provider) writes plaintext exactly as before, flag 0.
    ///
    /// Each blob also gets a BLAKE3 checksum sidecar file (ADR-013 / `A4`),
    /// covering the exact bytes written (ciphertext when encrypted,
    /// plaintext otherwise) — [`Store::get_item`]/[`Store::get_tokens`]
    /// verify against it on every read.
    pub fn insert_item(&self, doc: &gist_model::Document) -> Result<(), StoreError> {
        let key = self.encryption_key();

        // Write the full document blob, wrapped in the ADR-019 IR envelope
        // so a future binary can tell what version of the IR shape this is.
        let doc_path = self.storage_dir.join(format!("{}.json", doc.id));
        let doc_json = serialize_ir_blob(doc, required_ir_version(doc))?;
        let doc_bytes = match &key {
            Some(k) => encrypt_at_rest(k, doc_json.as_bytes()),
            None => doc_json.into_bytes(),
        };
        std::fs::write(&doc_path, &doc_bytes)?;
        write_checksum_sidecar(&doc_path, &doc_bytes)?;

        // Write the token stream as a separate file for RSVP / FTS fast path
        // (also ADR-019-enveloped).
        let token_path = self.storage_dir.join(format!("{}.tokens.json", doc.id));
        let tokens_json = serialize_ir_blob(&doc.token_stream, IR_VERSION_BASE)?;
        let tokens_bytes = match &key {
            Some(k) => encrypt_at_rest(k, tokens_json.as_bytes()),
            None => tokens_json.into_bytes(),
        };
        std::fs::write(&token_path, &tokens_bytes)?;
        write_checksum_sidecar(&token_path, &tokens_bytes)?;

        let content_encrypted = key.is_some() as i64;

        let meta = &doc.metadata;
        // gist_model::Metadata has `author: Option<String>` — normalise to a list.
        let authors: Vec<String> = meta.author.iter().cloned().collect();
        let authors_json = serde_json::to_string(&authors)?;
        let meta_json = serde_json::to_string(meta)?;
        let doc_path_str = doc_path.to_string_lossy().into_owned();
        // source_ref holds the origin path/URL (informational only, ADR-006);
        // cover_path is not in M0 Metadata.
        let source_path: Option<&str> = meta.source_ref.as_deref();
        let source_copy_path: Option<&str> = meta.source_copy_ref.as_deref();
        let now_ms = now_millis();

        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT OR REPLACE INTO library_items
             (id, title, authors, source_path, source_url, doc_path, cover_path,
              created_at, updated_at, metadata_json, source_copy_path, content_encrypted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                doc.id,
                meta.title,
                authors_json,
                source_path,
                Option::<String>::None, // source_url — not in M0 Metadata
                doc_path_str,
                Option::<String>::None, // cover_path — not in M0 Metadata
                now_ms,
                now_ms,
                meta_json,
                source_copy_path,
                content_encrypted,
            ],
        )?;

        // Index Word tokens for FTS5.
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tokens (item_id, token_idx, token_text) VALUES (?1, ?2, ?3)",
            )?;
            for (idx, token) in doc.token_stream.iter().enumerate() {
                if token.kind == gist_model::TokenKind::Word {
                    stmt.execute(params![doc.id, idx as i64, token.text])?;
                }
            }
        }

        tx.commit()?;
        tracing::debug!("gist-store: inserted item {}", doc.id);
        Ok(())
    }

    /// Copy `bytes` into this store's sandboxed, content-addressed
    /// `originals/` directory (ADR-006), returning the copy's absolute path
    /// as a string. Callers stamp this onto `Metadata.source_copy_ref` before
    /// calling [`Store::insert_item`].
    ///
    /// The filename is the SHA-256 hex digest of `bytes`, plus `ext` if
    /// non-empty (no leading dot expected). Re-importing byte-identical
    /// content reuses the existing file instead of writing a duplicate — an
    /// existing file at the computed path is left untouched, not rewritten.
    ///
    /// **Known limitation:** because identical content dedupes to one file,
    /// two library items imported from the same bytes share one copy on
    /// disk. Nothing currently reads document content from this path (reads
    /// always go through `doc_path`, the serialised IR), so this is safe
    /// today, but removing one such item with `delete_source_files: true`
    /// will delete the shared file out from under the other — a follow-up
    /// would need reference counting (e.g. only delete when no other
    /// `library_items` row still references the same `source_copy_path`) if
    /// this ever needs to be exposed as a per-item guarantee.
    ///
    /// **Encryption (ADR-011):** when this `Store` was opened via
    /// [`Store::open_encrypted`], the bytes written to disk are
    /// AES-256-GCM-encrypted — but the content-addressed filename is still
    /// the hash of the *plaintext* `bytes`, so byte-identical re-imports keep
    /// deduplicating exactly as before (a fresh random nonce per write means
    /// re-encrypting identical plaintext would produce different ciphertext
    /// anyway, which is exactly why an existing file is left untouched rather
    /// than rewritten — there'd be nothing gained and it would needlessly
    /// invalidate nothing, since nothing keys off the ciphertext itself).
    /// There's no separate `content_encrypted` flag for this file: it's
    /// always written in the same call as the `insert_item` that stamps the
    /// owning row's flag, so that row's flag already describes this file's
    /// encryption state too.
    ///
    /// **Checksums (ADR-013 / `A4`):** a fresh write also gets a BLAKE3
    /// checksum sidecar (same mechanism as [`Store::insert_item`]'s document
    /// blobs), covering the on-disk bytes. A dedup hit (the file already
    /// exists) leaves the existing file *and* its sidecar untouched, exactly
    /// like the content itself — a copy written before this feature existed
    /// stays without a sidecar until it's naturally rewritten, per this
    /// function's existing "never rewrite an existing file" contract; no
    /// separate backfill pass runs here. [`Store::verify_original_copy`]
    /// checks the result later.
    pub fn store_original_copy(&self, bytes: &[u8], ext: &str) -> Result<String, StoreError> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();

        let originals_dir = self.storage_dir.join("originals");
        std::fs::create_dir_all(&originals_dir)?;

        // Every current caller derives `ext` via `Path::extension()` (which
        // can never contain a path separator), but this is a public method —
        // sanitize defensively at its own boundary (F23) rather than relying
        // entirely on that caller discipline holding for every future caller.
        let ext = sanitize_ext(ext);
        let filename = if ext.is_empty() {
            hash
        } else {
            format!("{hash}.{ext}")
        };
        let copy_path = originals_dir.join(filename);

        if !copy_path.exists() {
            let on_disk = match self.encryption_key() {
                Some(key) => encrypt_at_rest(&key, bytes),
                None => bytes.to_vec(),
            };
            std::fs::write(&copy_path, &on_disk)?;
            write_checksum_sidecar(&copy_path, &on_disk)?;
        }

        Ok(copy_path.to_string_lossy().into_owned())
    }

    /// Verify the on-disk integrity of a copy-on-import original at `path`
    /// (ADR-006) against its BLAKE3 checksum sidecar (ADR-013 / `A4`), if one
    /// exists — see [`verify_checksum`]'s backfill policy for what happens
    /// when one doesn't (not an error). Checks the bytes exactly as stored on
    /// disk — ciphertext if this `Store` is encrypted (ADR-011), plaintext
    /// otherwise — so this never needs to decrypt or know this store's key.
    ///
    /// **Not currently called by anything in `gist-core`/`gist-ffi`.** Per
    /// ADR-006, nothing in the app reads content from an original copy's
    /// path today (reads always go through the separate serialised IR at
    /// `doc_path`), so there's no existing "read this original" call site to
    /// hook verification into automatically the way [`Store::get_item`]/
    /// [`Store::get_tokens`] do for the document blobs. This method is the
    /// primitive a future "verify library integrity" maintenance action
    /// would call; it's implemented and tested now so the write side (every
    /// [`Store::store_original_copy`] call already produces a checksum)
    /// isn't left with no way to ever be checked.
    pub fn verify_original_copy(&self, path: &str) -> Result<(), StoreError> {
        let bytes = std::fs::read(path)?;
        verify_checksum(path, &bytes)
    }

    /// Per-item integrity report built from [`ChecksumStatus`], the
    /// primitive [`Store::verify_item_integrity`]/
    /// [`Store::verify_library_integrity`] (`R3`, `A4`) share for a single
    /// `library_items` row's on-disk files: the `<id>.json` document blob,
    /// its `<id>.tokens.json` sibling, and — when one exists — the ADR-006
    /// `originals/` copy. Shared with [`Store::verify_original_copy`]'s
    /// underlying checksum logic, just kept at the finer four-state
    /// granularity ([`ChecksumStatus`]) that an explicit report needs and a
    /// plain `Result<(), StoreError>` read-path gate does not.
    fn integrity_report_for(
        id: &str,
        doc_path: &str,
        source_copy_path: Option<&str>,
    ) -> Result<ItemIntegrityReport, StoreError> {
        let doc_status = checksum_status_of_file(doc_path)?;

        // Mirrors `Store::get_tokens`'s own path derivation exactly, and
        // its fallback: a legacy item with no separate tokens file is not
        // an error (it reads the full document instead at RSVP-time), so a
        // missing tokens file here is `Unverified` (nothing to check), not
        // `Missing` (which would read as "this item's data is gone" — it
        // isn't, it's just stored differently).
        let tokens_path = doc_path
            .strip_suffix(".json")
            .map(|s| format!("{s}.tokens.json"))
            .unwrap_or_else(|| format!("{doc_path}.tokens.json"));
        let tokens_status = if Path::new(&tokens_path).exists() {
            checksum_status_of_file(&tokens_path)?
        } else {
            ChecksumStatus::Unverified
        };

        let original_copy_status = match source_copy_path {
            Some(p) => Some(checksum_status_of_file(p)?),
            None => None,
        };

        Ok(ItemIntegrityReport {
            id: id.to_string(),
            doc_status,
            tokens_status,
            original_copy_status,
        })
    }

    /// Check one library item's on-disk integrity (`R3`, `A4`): its
    /// document/tokens blobs and, if it has one, its ADR-006 sandboxed
    /// original copy — each classified via [`ChecksumStatus`], never
    /// decrypting anything, since checksums cover on-disk bytes
    /// (ciphertext when this `Store` is encrypted, ADR-011) rather than
    /// plaintext (see [`Store::read_maybe_encrypted`]'s doc comment). This
    /// is why, unlike [`Store::get_item`]/[`Store::get_tokens`], this
    /// method needs no `KeyProvider` and cannot fail with
    /// [`StoreError::MissingKeyProvider`].
    ///
    /// Returns `Ok(None)` if `id` matches no row, mirroring
    /// [`Store::get_item_by_id`]. Aggregating the per-file report into a
    /// single pass/fail/unverified verdict is `gist_core::Core`'s job (`R3`
    /// keeps that policy decision at the facade layer, not here).
    pub fn verify_item_integrity(
        &self,
        id: &str,
    ) -> Result<Option<ItemIntegrityReport>, StoreError> {
        let row: Option<(String, Option<String>)> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            conn.query_row(
                "SELECT doc_path, source_copy_path FROM library_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
        };

        match row {
            None => Ok(None),
            Some((doc_path, source_copy_path)) => {
                Self::integrity_report_for(id, &doc_path, source_copy_path.as_deref()).map(Some)
            }
        }
    }

    /// [`Store::verify_item_integrity`], for every item in the library.
    ///
    /// Fails the whole call only on a genuinely unexpected I/O error (e.g.
    /// permission denied reading a file that does exist) — a *missing*
    /// file is not such an error, it's reported as
    /// [`ChecksumStatus::Missing`] within that item's report, so one item's
    /// absent file can never abort the rest of the library's report. This
    /// mirrors this crate's general policy (see [`Store::remove_items`]'s
    /// unknown-id handling) of degrading gracefully per-row rather than
    /// letting one bad row take down a bulk operation, applied here at the
    /// file layer instead of the DB layer.
    pub fn verify_library_integrity(&self) -> Result<Vec<ItemIntegrityReport>, StoreError> {
        let rows: Vec<(String, String, Option<String>)> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            let mut stmt =
                conn.prepare("SELECT id, doc_path, source_copy_path FROM library_items")?;
            let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
            let mut out = Vec::new();
            for r in mapped {
                out.push(r?);
            }
            out
        };

        rows.iter()
            .map(|(id, doc_path, source_copy_path)| {
                Self::integrity_report_for(id, doc_path, source_copy_path.as_deref())
            })
            .collect()
    }

    /// Remove a single item. Thin wrapper around [`Store::remove_items`] so
    /// there is exactly one deletion code path (rather than two that can
    /// drift apart); see that method's doc comment for the ordering,
    /// atomicity, and unknown-id behaviour it implements.
    pub fn delete_item(&self, id: &str) -> Result<(), StoreError> {
        let ids = [id.to_string()];
        self.remove_items(&ids)?;
        Ok(())
    }

    /// Remove one or more items from the library in a single transaction.
    ///
    /// **Atomicity:** every id in `ids` is looked up and deleted inside one
    /// `unchecked_transaction`. If any DB operation errors partway through,
    /// the transaction is rolled back (not committed) and none of the rows
    /// are removed — all-or-nothing on the database side.
    ///
    /// **File deletion ordering (why this function never touches the
    /// filesystem):** this function only removes DB rows and returns the
    /// `doc_path`/`source_path`/`source_copy_path` each removed row held.
    /// Callers (`gist-core`) must delete the corresponding
    /// `.json`/`.tokens.json` blobs — and, optionally, the sandboxed source
    /// copy (`source_copy_path`, ADR-006; never `source_path`, which is the
    /// user's original file at its real location and must never be deleted
    /// by GIST) — only *after* this function returns `Ok`. That ordering (DB
    /// commit, then best-effort file cleanup) means an interruption can only
    /// ever leave behind an orphaned *file* on disk (harmless, reclaimable
    /// later), never an orphaned DB row pointing at files that no longer
    /// exist — the reverse of the ordering bug this replaces.
    ///
    /// **Unknown ids:** an id in `ids` that doesn't match any row is
    /// silently skipped rather than treated as an error, so a bulk remove
    /// is idempotent/best-effort against ids that may already be gone
    /// (e.g. removed concurrently). The returned `Vec<RemovedItem>` contains
    /// only the ids that were actually found and removed, in no guaranteed
    /// order, so callers can tell exactly which ids took effect.
    ///
    /// **Shared stored copies:** each returned row carries
    /// [`RemovedItem::source_copy_still_referenced`], computed after every
    /// deletion in the batch has been applied but before the commit, so the
    /// caller can honour ADR-006's content-addressed dedup instead of
    /// deleting a file another surviving item still points at.
    ///
    /// `reading_progress`, `tokens`, and (via the `tokens_ad` trigger)
    /// `fts_index` rows are cleaned up automatically through
    /// `ON DELETE CASCADE` / triggers — this function never deletes from
    /// those tables directly.
    pub fn remove_items(&self, ids: &[String]) -> Result<Vec<RemovedItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.unchecked_transaction()?;
        let mut removed = Vec::new();

        {
            let mut select_stmt = tx.prepare(
                "SELECT doc_path, source_path, source_copy_path FROM library_items WHERE id = ?1",
            )?;
            let mut delete_stmt = tx.prepare("DELETE FROM library_items WHERE id = ?1")?;

            for id in ids {
                let row: Option<(String, Option<String>, Option<String>)> = select_stmt
                    .query_row(params![id], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })
                    .optional()?;

                if let Some((doc_path, source_path, source_copy_path)) = row {
                    delete_stmt.execute(params![id])?;
                    removed.push(RemovedItem {
                        id: id.clone(),
                        doc_path,
                        source_path,
                        source_copy_path,
                        // Filled in below, once every id in the batch is gone.
                        source_copy_still_referenced: false,
                    });
                }
                // else: unknown id — silently skipped, see doc comment above.
            }
        }

        // ADR-006 shared-copy safety. Deliberately done *after* the whole
        // batch has been deleted and *before* `tx.commit()`, so the answer is
        // "does any row that survives this removal still reference this
        // file?" — the question the caller actually needs answered.
        if removed.iter().any(|r| r.source_copy_path.is_some()) {
            let mut surviving: std::collections::HashSet<String> = Default::default();
            {
                let mut stmt = tx.prepare(
                    "SELECT source_copy_path FROM library_items WHERE source_copy_path IS NOT NULL",
                )?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                for row in rows {
                    surviving.insert(copy_ref_key(&row?));
                }
            }

            for item in &mut removed {
                if let Some(copy) = &item.source_copy_path {
                    item.source_copy_still_referenced = surviving.contains(&copy_ref_key(copy));
                }
            }
        }

        tx.commit()?;
        tracing::debug!("gist-store: removed {} item(s)", removed.len());
        Ok(removed)
    }

    /// Search imported document text and library title/source path.
    /// Text matches are ranked ahead of title/path matches; results are
    /// deduplicated and limited after combining both match sources.
    pub fn search_items(&self, query: &str, limit: usize) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "WITH matches(item_id, rank) AS (
                 SELECT t.item_id, 0
                 FROM tokens t
                 WHERE t.rowid IN (
                     SELECT rowid FROM fts_index WHERE token_text MATCH ?1
                 )
                 UNION ALL
                 SELECT li.id, 1
                 FROM library_items li
                 WHERE lower(COALESCE(li.title, '')) LIKE ?2 ESCAPE '\\'
                    OR lower(COALESCE(li.source_path, '')) LIKE ?2 ESCAPE '\\'
             )
             SELECT item_id
             FROM matches
             GROUP BY item_id
             ORDER BY MIN(rank), item_id
             LIMIT ?3",
        )?;
        let escaped = escape_fts5_query(query);
        let metadata_pattern = format!("%{}%", escape_like_pattern(&query.to_lowercase()));
        let rows = stmt.query_map(params![escaped, metadata_pattern, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Return a paginated list of library items (newest first).
    pub fn list_items(&self, offset: usize, limit: usize) -> Result<Vec<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let sql = format!(
            "{LIBRARY_ITEM_SELECT}
             ORDER BY li.created_at DESC
             LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], library_item_from_row)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    /// Return the lightweight [`LibraryItem`] row for a single `id`, or
    /// `None` if no such item exists.
    pub fn get_item_by_id(&self, id: &str) -> Result<Option<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let sql = format!("{LIBRARY_ITEM_SELECT} WHERE li.id = ?1");
        let row = conn
            .query_row(&sql, params![id], library_item_from_row)
            .optional()?;
        Ok(row)
    }

    /// Record that either reader opened `item_id` just now (ADR-021).
    /// Idempotent and cheap: one indexed `UPDATE`. An unknown id updates
    /// zero rows and is not an error (the item may have been removed
    /// concurrently). Returns the timestamp written (Unix milliseconds).
    pub fn mark_item_opened(&self, item_id: &str) -> Result<i64, StoreError> {
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "UPDATE library_items SET last_opened_at = ?2 WHERE id = ?1",
            params![item_id, now_ms],
        )?;
        Ok(now_ms)
    }

    /// Reads `path`, verifies it against its checksum sidecar if one exists
    /// (ADR-013 / `A4` — [`StoreError::ChecksumMismatch`] if it disagrees)
    /// and, if `content_encrypted` is set, decrypts it (ADR-011) under this
    /// `Store`'s configured *read* key ([`Store::decryption_key`] — ADR-014:
    /// deliberately independent of whether this `Store` auto-encrypts new
    /// writes) before returning. Returns [`StoreError::MissingKeyProvider`]
    /// if the content is flagged encrypted but this `Store` was opened via
    /// plain [`Store::open`] (no key available at all) — this is the
    /// per-row migration check described on [`Store::open_encrypted`].
    ///
    /// Checksum verification runs against the raw on-disk bytes, *before*
    /// any decryption attempt — corruption is reported as
    /// `ChecksumMismatch`, not conflated with `DecryptionFailed` (a wrong
    /// key or corrupted ciphertext that has no checksum sidecar to catch it
    /// early).
    fn read_maybe_encrypted(
        &self,
        path: &str,
        content_encrypted: bool,
    ) -> Result<Vec<u8>, StoreError> {
        let raw = std::fs::read(path)?;
        verify_checksum(path, &raw)?;
        if !content_encrypted {
            return Ok(raw);
        }
        let key = self
            .decryption_key()
            .ok_or(StoreError::MissingKeyProvider)?;
        decrypt_at_rest(&key, &raw, path)
    }

    /// Load and deserialise the full [`gist_model::Document`] for `id`.
    pub fn get_item(&self, id: &str) -> Result<Option<gist_model::Document>, StoreError> {
        let row: Option<(String, bool)> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            conn.query_row(
                "SELECT doc_path, content_encrypted FROM library_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
        };

        match row {
            None => Ok(None),
            Some((path, content_encrypted)) => {
                let bytes = self.read_maybe_encrypted(&path, content_encrypted)?;
                let doc: gist_model::Document = deserialize_ir_blob(&bytes)?;
                Ok(Some(doc))
            }
        }
    }

    /// Load only the token stream for an item (for RSVP / TTS).
    /// Loads from `<id>.tokens.json`; falls back to loading the full document.
    pub fn get_tokens(&self, id: &str) -> Result<Option<Vec<gist_model::Token>>, StoreError> {
        let row: Option<(String, bool)> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            conn.query_row(
                "SELECT doc_path, content_encrypted FROM library_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
        };

        match row {
            None => Ok(None),
            Some((path, content_encrypted)) => {
                let token_path = path
                    .strip_suffix(".json")
                    .map(|s| format!("{s}.tokens.json"))
                    .unwrap_or_else(|| format!("{path}.tokens.json"));

                if std::path::Path::new(&token_path).exists() {
                    let bytes = self.read_maybe_encrypted(&token_path, content_encrypted)?;
                    let tokens: Vec<gist_model::Token> = deserialize_ir_blob(&bytes)?;
                    Ok(Some(tokens))
                } else {
                    // Fallback: load full document and extract token stream.
                    let bytes = self.read_maybe_encrypted(&path, content_encrypted)?;
                    let doc: gist_model::Document = deserialize_ir_blob(&bytes)?;
                    Ok(Some(doc.token_stream))
                }
            }
        }
    }

    /// Upsert the reading position (token index) for an item.
    pub fn save_progress(&self, item_id: &str, token_index: usize) -> Result<(), StoreError> {
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "INSERT INTO reading_progress (item_id, token_index, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(item_id) DO UPDATE SET
               token_index = excluded.token_index,
               updated_at  = excluded.updated_at",
            params![item_id, token_index as i64, now_ms],
        )?;
        Ok(())
    }

    /// Return the last saved token index, or 0 if none.
    pub fn get_progress(&self, item_id: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let idx: Option<i64> = conn
            .query_row(
                "SELECT token_index FROM reading_progress WHERE item_id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(idx.unwrap_or(0) as usize)
    }

    // ── Collections ──────────────────────────────────────────────────────

    /// Create a new collection and return its generated id.
    pub fn create_collection(&self, name: &str) -> Result<String, StoreError> {
        let id = Uuid::now_v7().to_string();
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "INSERT INTO collections (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, now_ms],
        )?;
        Ok(id)
    }

    /// Return all collections, newest first.
    pub fn list_collections(&self) -> Result<Vec<Collection>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt =
            conn.prepare("SELECT id, name, created_at FROM collections ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;
        let mut collections = Vec::new();
        for row in rows {
            collections.push(row?);
        }
        Ok(collections)
    }

    /// Add an item to a collection. Idempotent — adding twice is a no-op.
    pub fn add_item_to_collection(
        &self,
        item_id: &str,
        collection_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "INSERT OR IGNORE INTO item_collections (item_id, collection_id) VALUES (?1, ?2)",
            params![item_id, collection_id],
        )?;
        Ok(())
    }

    /// Remove an item from a collection.
    pub fn remove_item_from_collection(
        &self,
        item_id: &str,
        collection_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "DELETE FROM item_collections WHERE item_id = ?1 AND collection_id = ?2",
            params![item_id, collection_id],
        )?;
        Ok(())
    }

    /// Return all library items belonging to a collection (newest first).
    pub fn list_items_in_collection(
        &self,
        collection_id: &str,
    ) -> Result<Vec<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let sql = format!(
            "{LIBRARY_ITEM_SELECT}
             JOIN item_collections ic ON ic.item_id = li.id
             WHERE ic.collection_id = ?1
             ORDER BY li.created_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![collection_id], library_item_from_row)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    // ── Tags ─────────────────────────────────────────────────────────────

    /// Attach a tag (by name) to an item, creating the tag if it doesn't
    /// already exist. Idempotent — adding the same tag twice is a no-op.
    pub fn add_tag(&self, item_id: &str, tag_name: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.unchecked_transaction()?;

        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![tag_name],
                |row| row.get(0),
            )
            .optional()?;

        let tag_id = match existing {
            Some(id) => id,
            None => {
                let id = Uuid::now_v7().to_string();
                tx.execute(
                    "INSERT INTO tags (id, name) VALUES (?1, ?2)",
                    params![id, tag_name],
                )?;
                id
            }
        };

        tx.execute(
            "INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
            params![item_id, tag_id],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Detach a tag (by name) from an item. Does not delete the tag itself.
    pub fn remove_tag(&self, item_id: &str, tag_name: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "DELETE FROM item_tags
             WHERE item_id = ?1
               AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
            params![item_id, tag_name],
        )?;
        Ok(())
    }

    /// Return the names of all tags attached to an item.
    pub fn list_tags_for_item(&self, item_id: &str) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "SELECT t.name
             FROM tags t
             JOIN item_tags it ON it.tag_id = t.id
             WHERE it.item_id = ?1
             ORDER BY t.name ASC",
        )?;
        let rows = stmt.query_map(params![item_id], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    }

    /// Return the names of every tag currently attached to at least one item,
    /// alphabetically -- used to populate a filter menu, not scoped to any
    /// one item (see `list_tags_for_item` for that).
    pub fn list_all_tags(&self) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "SELECT t.name FROM tags t
             WHERE EXISTS (SELECT 1 FROM item_tags it WHERE it.tag_id = t.id)
             ORDER BY t.name ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    }

    /// Return all library items tagged with `tag_name` (newest first).
    /// Mirrors `list_items_in_collection`'s query shape.
    pub fn list_items_by_tag(&self, tag_name: &str) -> Result<Vec<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let sql = format!(
            "{LIBRARY_ITEM_SELECT}
             JOIN item_tags it ON it.item_id = li.id
             JOIN tags t ON t.id = it.tag_id
             WHERE t.name = ?1
             ORDER BY li.created_at DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tag_name], library_item_from_row)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    // ── Annotations (ADR-003: block_id/start/len/prefix_hash/quote_hash) ─

    /// Create a new annotation and return its generated id.
    /// `prefix_hash`/`quote_hash` are FNV-1a digests computed by the caller
    /// (the reading view, which has the block's plain text available) per
    /// ADR-003 — this method only persists whatever it's given, it never
    /// computes or verifies a hash itself.
    #[allow(clippy::too_many_arguments)]
    pub fn create_annotation(
        &self,
        item_id: &str,
        kind: gist_model::AnnotationKind,
        block_id: &str,
        start: usize,
        len: usize,
        prefix_hash: u64,
        quote_hash: u64,
        note_text: Option<&str>,
    ) -> Result<String, StoreError> {
        let id = Uuid::now_v7().to_string();
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "INSERT INTO annotations
             (id, item_id, kind, block_id, start_offset, len_bytes, prefix_hash,
              quote_hash, note_text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                item_id,
                annotation_kind_to_sql(kind),
                block_id,
                start as i64,
                len as i64,
                prefix_hash as i64,
                quote_hash as i64,
                note_text,
                now_ms,
                now_ms,
            ],
        )?;
        Ok(id)
    }

    /// Return all annotations for one item, newest first.
    pub fn list_annotations_for_item(
        &self,
        item_id: &str,
    ) -> Result<Vec<gist_model::Annotation>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, item_id, kind, block_id, start_offset, len_bytes, prefix_hash,
                    quote_hash, note_text, created_at, updated_at
             FROM annotations
             WHERE item_id = ?1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![item_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;

        let mut annotations = Vec::new();
        for row in rows {
            let (
                id,
                item_id,
                kind_sql,
                block_id,
                start,
                len,
                prefix_hash,
                quote_hash,
                note_text,
                created_at,
                updated_at,
            ) = row?;
            annotations.push(gist_model::Annotation {
                id,
                item_id,
                kind: annotation_kind_from_sql(&kind_sql)?,
                block_id,
                start: start as usize,
                len: len as usize,
                prefix_hash: prefix_hash as u64,
                quote_hash: quote_hash as u64,
                note_text,
                created_at,
                updated_at,
            });
        }
        Ok(annotations)
    }

    /// Update an annotation's note text (e.g. editing a `Note`'s body) and
    /// bump `updated_at`. Pass `None` to clear the text (e.g. converting a
    /// `Note` back to a plain `Highlight`). The anchor fields
    /// (`block_id`/`start`/`len`/`prefix_hash`/`quote_hash`) are never
    /// touched by this method — see `update_annotation_anchor` below for
    /// the write path ADR-003 re-anchoring (`gist_core::Core::
    /// reanchor_annotations`) uses instead.
    pub fn update_annotation_note(
        &self,
        id: &str,
        note_text: Option<&str>,
    ) -> Result<(), StoreError> {
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let affected = conn.execute(
            "UPDATE annotations SET note_text = ?1, updated_at = ?2 WHERE id = ?3",
            params![note_text, now_ms, id],
        )?;
        if affected == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// Persist a corrected anchor for annotation `id` after ADR-003
    /// re-anchoring (`gist_core::Core::reanchor_annotations`, via
    /// `gist_core::anchoring::reanchor`) found its `quote_hash` at a new
    /// position within the same block. Rewrites every anchor field
    /// (`block_id`/`start`/`len`/`prefix_hash`/`quote_hash`) and bumps
    /// `updated_at` — the complement to `update_annotation_note` above,
    /// which deliberately never touches these fields. `quote_hash` is
    /// accepted (not recomputed here) so this method stays a pure
    /// persist-what-I'm-given operation, matching `create_annotation`'s own
    /// convention; the caller is responsible for the hash actually matching
    /// the text at `start`.
    #[allow(clippy::too_many_arguments)]
    pub fn update_annotation_anchor(
        &self,
        id: &str,
        block_id: &str,
        start: usize,
        len: usize,
        prefix_hash: u64,
        quote_hash: u64,
    ) -> Result<(), StoreError> {
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let affected = conn.execute(
            "UPDATE annotations
             SET block_id = ?1, start_offset = ?2, len_bytes = ?3, prefix_hash = ?4,
                 quote_hash = ?5, updated_at = ?6
             WHERE id = ?7",
            params![
                block_id,
                start as i64,
                len as i64,
                prefix_hash as i64,
                quote_hash as i64,
                now_ms,
                id,
            ],
        )?;
        if affected == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// Delete a single annotation. Errors with `NotFound` if `id` doesn't
    /// match a row — unlike `delete_annotations`'s bulk form below, which
    /// silently skips unknown ids (mirroring `remove_items`'s convention,
    /// for the same reason: a bulk multi-select shouldn't fail wholesale
    /// over one stale id).
    pub fn delete_annotation(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let affected = conn.execute("DELETE FROM annotations WHERE id = ?1", params![id])?;
        if affected == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// Delete one or more annotations in a single transaction, mirroring
    /// `remove_items`'s multi-id delete: an id with no matching row is
    /// silently skipped, not an error. Returns the number of rows actually
    /// deleted. `annotations` rows are also cleaned up automatically via
    /// `ON DELETE CASCADE` whenever the owning `library_items` row is
    /// removed (`Store::remove_items`) — this method is for deleting
    /// annotations directly, independent of item removal.
    pub fn delete_annotations(&self, ids: &[String]) -> Result<usize, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.unchecked_transaction()?;
        let mut deleted = 0usize;
        {
            let mut stmt = tx.prepare("DELETE FROM annotations WHERE id = ?1")?;
            for id in ids {
                deleted += stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    // ── Per-item encryption (ADR-014) ───────────────────────────────────

    /// Retroactively encrypts one already-imported item's `.json`/
    /// `.tokens.json` blobs at rest — a per-item, opt-in follow-up to
    /// ADR-011's whole-store `Store::open_encrypted` (see ADR-014 for the
    /// product decision this implements: no global "encrypt my library"
    /// toggle, just an explicit per-item action).
    ///
    /// **`key` is supplied explicitly by the caller, not read from
    /// `self.key_provider`.** This is the core design choice that lets a
    /// `Store` opened via plain [`Store::open`] (no key provider at all)
    /// still encrypt a specific item on demand, without requiring the whole
    /// `Store` to have been opened via [`Store::open_encrypted`]. It works
    /// identically regardless of which constructor `self` came from.
    ///
    /// **Read-after-encrypt (ADR-014, fixed):** encrypting doesn't require
    /// `self.key_provider`, so it's possible to produce a
    /// `content_encrypted = 1` row on a `Store` instance with no
    /// *decryption* capability at all — reading that row's content back
    /// (`get_item`/`get_tokens`, and anything built on them, e.g. RSVP or
    /// flow view) through such an instance still fails with
    /// [`StoreError::MissingKeyProvider`]. This used to be true even for
    /// `CoreClient.shared`'s production instance, which made "Encrypt" a
    /// data-access bug in practice — a user could permanently lock
    /// themselves out of a book through the running app. Fixed by
    /// decoupling read-decrypt capability from write-auto-encrypt behavior:
    /// [`Store::open_with_read_key`] gives a `Store` a real decryption key
    /// (checked by [`Store::read_maybe_encrypted`] via
    /// [`Store::decryption_key`]) without making it auto-encrypt new writes,
    /// so a production instance opened that way can call `encrypt_item` and
    /// then immediately read the same item back through itself — see that
    /// constructor's doc comment. Only a `Store` opened via plain
    /// [`Store::open`] (truly no key of any kind — not what
    /// `CoreClient.shared` uses since this fix) still hits
    /// `MissingKeyProvider` here, and that remains correct: it genuinely has
    /// no key to decrypt with. The item's SQL-backed metadata
    /// (title/authors/tags, used by `list_items`/`search_items`/etc.) was
    /// always readable regardless — those never touch the encrypted blob.
    ///
    /// **Scope — `originals/` is deliberately untouched.** Only the two IR
    /// blobs (`<id>.json`/`<id>.tokens.json`, ADR-007) are encrypted here.
    /// `source_copy_path` (ADR-006's sandboxed `originals/` copy) is never
    /// read or written by this method: that file is content-addressed by a
    /// hash of its *plaintext* bytes and may be shared by more than one
    /// `library_items` row (the same dedup limitation already documented on
    /// [`Store::store_original_copy`] and tracked as security register
    /// `A5`) — encrypting it in place would silently corrupt whatever other
    /// item still expects to find plaintext at that shared path. Nothing in
    /// the app currently reads document *content* from an original copy's
    /// path (only its existence, for deletion), so leaving it alone is
    /// safe, but it is a real, tracked gap: an item "encrypted" through this
    /// method can still have a plaintext copy of its original file on disk.
    ///
    /// **Idempotency:** a row whose `content_encrypted` is already `1`
    /// returns [`EncryptOutcome::AlreadyEncrypted`] immediately — no file
    /// I/O, no DB write — so callers driving a bulk selection of mixed
    /// already-encrypted/not-yet-encrypted items can call this once per id
    /// without checking first.
    ///
    /// **Integrity (ADR-013):** the current bytes are read back through
    /// [`Store::read_maybe_encrypted`] (with `content_encrypted = false`,
    /// since that's confirmed by the row lookup above), which verifies each
    /// blob against its existing checksum sidecar before anything is
    /// encrypted — this never silently encrypts over already-corrupted
    /// data. Once written, each new ciphertext blob gets its checksum
    /// sidecar recomputed and rewritten; the old sidecar (covering the
    /// prior plaintext bytes) would otherwise mismatch every future read.
    ///
    /// **Ordering / crash-window note:** like [`Store::insert_item`], the
    /// file rewrites happen before the DB row's `content_encrypted` flag is
    /// committed — there's no filesystem transaction to join the DB write
    /// to. An interruption between the file rewrite and the DB commit would
    /// leave the row still flagged unencrypted while the on-disk bytes are
    /// already ciphertext, which would then surface as a
    /// `ChecksumMismatch`/deserialisation failure on the next read rather
    /// than silently wrong content. A narrow window, accepted under this
    /// codebase's existing local-single-user threat model (see `F13`'s
    /// similarly-accepted TOCTOU window), not eliminated here.
    pub fn encrypt_item(&self, id: &str, key: &[u8; 32]) -> Result<EncryptOutcome, StoreError> {
        let row: Option<(String, bool)> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            conn.query_row(
                "SELECT doc_path, content_encrypted FROM library_items WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
        };

        let (doc_path, content_encrypted) = match row {
            None => return Err(StoreError::NotFound(id.to_string())),
            Some(row) => row,
        };

        if content_encrypted {
            return Ok(EncryptOutcome::AlreadyEncrypted);
        }

        let token_path = doc_path
            .strip_suffix(".json")
            .map(|s| format!("{s}.tokens.json"))
            .unwrap_or_else(|| format!("{doc_path}.tokens.json"));

        // Read + verify (ADR-013) the current plaintext bytes for both
        // blobs before touching either. `content_encrypted` is known false
        // here (checked above), so this never attempts decryption.
        let doc_plaintext = self.read_maybe_encrypted(&doc_path, false)?;
        let tokens_plaintext = self.read_maybe_encrypted(&token_path, false)?;

        let doc_ciphertext = encrypt_at_rest(key, &doc_plaintext);
        let tokens_ciphertext = encrypt_at_rest(key, &tokens_plaintext);

        std::fs::write(&doc_path, &doc_ciphertext)?;
        write_checksum_sidecar(Path::new(&doc_path), &doc_ciphertext)?;
        std::fs::write(&token_path, &tokens_ciphertext)?;
        write_checksum_sidecar(Path::new(&token_path), &tokens_ciphertext)?;

        // A single UPDATE statement is already atomic at the SQLite level —
        // no explicit `unchecked_transaction` needed here, unlike
        // `add_tag`/`remove_items`, which combine multiple statements.
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute(
            "UPDATE library_items SET content_encrypted = 1 WHERE id = ?1",
            params![id],
        )?;

        tracing::debug!("gist-store: encrypted item {} at rest", id);
        Ok(EncryptOutcome::Encrypted)
    }
}

/// Outcome of a single [`Store::encrypt_item`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptOutcome {
    /// The item was plaintext; its blobs have now been rewritten as
    /// AES-256-GCM ciphertext and its row's `content_encrypted` set to 1.
    Encrypted,
    /// The item's row already had `content_encrypted = 1` — no-op, no files
    /// or DB rows were touched.
    AlreadyEncrypted,
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Escapes `query` for safe use as an FTS5 `MATCH` argument by wrapping the
/// entire input in a single quoted phrase, doubling any embedded `"` (F19).
/// Without this, free-text search input is interpreted by FTS5's own query
/// grammar — `AND`/`OR`/`NOT`, `NEAR/n`, `column:` filters, a trailing prefix
/// `*` — so unbalanced quotes throw an unsanitized syntax error back toward
/// the caller, and adversarial input can build expensive query graphs.
/// Quoting the whole thing as one phrase means a search box's contents can
/// never be read as anything but literal text to match.
///
/// The trailing `*` is appended by us, outside the quotes, never from
/// `query` -- FTS5 treats `"phrase"*` as a prefix match on the phrase's
/// final token, which is what live search-as-you-type needs: typing "Gen"
/// should find "General" without waiting for the whole word. It's still
/// injection-safe because the `*` is a fixed literal we control, not
/// user-controlled syntax reaching outside the quotes; a bare `"phrase"`
/// (what this returned before) instead requires an exact whole-token match,
/// which is what caused short/partial-word searches to silently find
/// nothing even when the full word was right there in the document.
///
/// Note this only ever meaningfully affects a query's *last* word: per
/// ADR-008, `fts_index` holds one row per single `Word` token (not whole
/// documents or lines), so a multi-word query like `"hello world"*` can
/// only match a row whose entire indexed text happens to equal that full
/// phrase -- which never happens, since every row is one token. Multi-word
/// search isn't wired up to work here at all today; that's a pre-existing,
/// separate gap from the one this fixes, not a regression from adding `*`.
fn escape_fts5_query(query: &str) -> String {
    if query.is_empty() {
        return "\"\"".to_string();
    }
    format!("\"{}\"*", query.replace('"', "\"\""))
}

fn escape_like_pattern(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Reduces `ext` to ASCII alphanumeric characters only, for safe use in a
/// content-addressed filename (F23, used by [`Store::store_original_copy`]).
/// A real file extension never legitimately needs `/`, `\`, `..`, or any
/// other punctuation, so this drops such characters rather than trying to
/// escape them — there's no path-construction meaning left to preserve.
fn sanitize_ext(ext: &str) -> String {
    ext.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// The exact string each `gist_model::AnnotationKind` variant is stored as
/// in the `annotations.kind` column. Kept as free functions (rather than a
/// `rusqlite::ToSql`/`FromSql` impl on the type itself) because
/// `gist-model` must stay free of I/O/SQL dependencies (wasm32 target) —
/// see that crate's conventions.
fn annotation_kind_to_sql(kind: gist_model::AnnotationKind) -> &'static str {
    match kind {
        gist_model::AnnotationKind::Highlight => "highlight",
        gist_model::AnnotationKind::Note => "note",
        gist_model::AnnotationKind::Bookmark => "bookmark",
    }
}

/// Reverses [`annotation_kind_to_sql`]. Every write path
/// (`Store::create_annotation`) only ever writes one of the three known
/// strings, so any other value means the row is corrupt.
fn annotation_kind_from_sql(s: &str) -> Result<gist_model::AnnotationKind, StoreError> {
    match s {
        "highlight" => Ok(gist_model::AnnotationKind::Highlight),
        "note" => Ok(gist_model::AnnotationKind::Note),
        "bookmark" => Ok(gist_model::AnnotationKind::Bookmark),
        other => Err(StoreError::InvalidAnnotationKind(other.to_string())),
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    /// Compatibility pin (review: aes-gcm 0.10 -> 0.11). This blob was produced by
    /// `encrypt_at_rest` under aes-gcm 0.10.3 with key 0x42*32. It must keep
    /// decrypting under any later aes-gcm, or every already-encrypted user item
    /// becomes unreadable. Never regenerate it to make this test pass.
    #[test]
    fn decrypts_blob_written_by_aes_gcm_0_10() {
        let key = [0x42u8; 32];
        let hex = "bfda90f08972ffeab5a5186262b59df3edf7e461de4677e163e8a3fa56b72844574611cae08c2ee95c2f8e7c5f5723cb6a4b887fa1ecefb9e8374e75";
        let blob: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let plain = super::decrypt_at_rest(&key, &blob, "kat").expect("0.10 blob must decrypt");
        assert_eq!(plain, b"GIST at-rest known-answer vector");
        let mut bad = blob.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(super::decrypt_at_rest(&key, &bad, "kat").is_err());
    }

    use super::*;
    use gist_model::{Document, Metadata};

    fn open_test_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        let store = Store::open(&db, &storage).unwrap();
        (dir, store)
    }

    fn insert_test_item(store: &Store) -> String {
        let doc = Document::new(Metadata::minimal("Test Title"), vec![]);
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();
        id
    }

    fn doc_with_title(title: &str) -> gist_model::Document {
        let section = gist_model::Section {
            id: "s0".to_string(),
            heading: None,
            blocks: vec![gist_model::Block::Paragraph {
                runs: vec![gist_model::TextRun::plain(format!("hello from {title}"))],
            }],
        };
        let mut meta = gist_model::Metadata::minimal(title);
        meta.source_ref = Some(format!("/tmp/{title}.txt"));
        gist_model::Document::new(meta, vec![section])
    }

    #[test]
    fn fresh_open_migrates_to_latest_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("fresh.db");
        let storage = dir.path().join("storage");
        let _store = Store::open(&db, &storage).unwrap();

        let conn = Connection::open(&db).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    /// `list_referenced_files` is what the orphan sweep builds its keep-list
    /// from (review Q10), so it must report exactly the live rows' blob and
    /// ADR-006 copy paths — and must never report `source_path`, the user's
    /// own file, which no cleanup pass is allowed to consider.
    #[test]
    fn list_referenced_files_reports_blob_and_copy_paths_but_never_the_users_own_file() {
        let (_dir, store) = open_test_store();
        let id = insert_test_item(&store);
        let copy = store.store_original_copy(b"original bytes", "txt").unwrap();

        // `insert_test_item` stamps a `source_ref` but no `source_copy_ref`;
        // re-insert the same document with the copy attached.
        let mut doc = store.get_item(&id).unwrap().unwrap();
        doc.metadata.source_copy_ref = Some(copy.clone());
        store.insert_item(&doc).unwrap();

        let referenced = store.list_referenced_files().unwrap();
        assert_eq!(referenced.len(), 1);
        assert_eq!(
            referenced[0].doc_path,
            store
                .storage_dir()
                .join(format!("{id}.json"))
                .to_string_lossy()
        );
        assert_eq!(
            referenced[0].source_copy_path.as_deref(),
            Some(copy.as_str())
        );

        store.remove_items(&[id]).unwrap();
        assert!(store.list_referenced_files().unwrap().is_empty());
    }

    /// A database path past the legacy 260-character `MAX_PATH` must open.
    ///
    /// Unguarded deliberately: the assertion ("a long path works") is the
    /// same everywhere, and only the Windows leg can regress it — SQLite's
    /// Win32 VFS passes the path to `CreateFileW` unprefixed, so before
    /// `sqlite_path` existed this failed with `CannotOpen` even though every
    /// `std::fs` write beside it succeeded. Found by the `gist-core`
    /// long-path test (review Q10); pinned here at the layer that owns it.
    #[test]
    fn a_database_path_past_legacy_max_path_opens_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = dir.path().to_path_buf();
        while root.as_os_str().len() < 300 {
            root = root.join("a_directory_with_a_long_name");
        }
        std::fs::create_dir_all(&root).unwrap();

        let db = root.join("long.db");
        let storage = root.join("storage");
        assert!(db.as_os_str().len() > 260);

        let store = Store::open(&db, &storage).unwrap();
        let id = insert_test_item(&store);
        // Exercise a write-then-read cycle, which is what actually forces
        // the journal/WAL sibling files (whose names SQLite derives by
        // appending to the database path) into existence.
        assert!(store.get_item(&id).unwrap().is_some());
        assert_eq!(store.list_items(0, 10).unwrap().len(), 1);
    }

    #[test]
    fn collection_create_add_list_roundtrip() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let collection_id = store.create_collection("Favorites").unwrap();

        store
            .add_item_to_collection(&item_id, &collection_id)
            .unwrap();

        let items = store.list_items_in_collection(&collection_id).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, item_id);

        let collections = store.list_collections().unwrap();
        assert!(collections.iter().any(|c| c.id == collection_id));
    }

    #[test]
    fn removing_item_from_collection_leaves_item_and_collection_intact() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let collection_id = store.create_collection("Later").unwrap();

        store
            .add_item_to_collection(&item_id, &collection_id)
            .unwrap();
        store
            .remove_item_from_collection(&item_id, &collection_id)
            .unwrap();

        let items = store.list_items_in_collection(&collection_id).unwrap();
        assert!(items.is_empty());

        // The item and collection themselves are unaffected.
        assert!(store.get_item_by_id(&item_id).unwrap().is_some());
        assert!(store
            .list_collections()
            .unwrap()
            .iter()
            .any(|c| c.id == collection_id));
    }

    #[test]
    fn tag_add_is_idempotent_and_listable() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);

        store.add_tag(&item_id, "sci-fi").unwrap();
        store.add_tag(&item_id, "sci-fi").unwrap(); // duplicate — must not error

        let tags = store.list_tags_for_item(&item_id).unwrap();
        assert_eq!(tags, vec!["sci-fi".to_string()]);
    }

    #[test]
    fn tag_remove_leaves_other_tags_intact() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);

        store.add_tag(&item_id, "sci-fi").unwrap();
        store.add_tag(&item_id, "favorite").unwrap();
        store.remove_tag(&item_id, "sci-fi").unwrap();

        let tags = store.list_tags_for_item(&item_id).unwrap();
        assert_eq!(tags, vec!["favorite".to_string()]);
    }

    #[test]
    fn list_all_tags_returns_distinct_names_alphabetically() {
        let (_dir, store) = open_test_store();
        let item_a = insert_test_item(&store);
        let item_b = insert_test_item(&store);

        store.add_tag(&item_a, "sci-fi").unwrap();
        store.add_tag(&item_b, "sci-fi").unwrap(); // shared tag, must not duplicate
        store.add_tag(&item_b, "favorite").unwrap();

        assert_eq!(
            store.list_all_tags().unwrap(),
            vec!["favorite".to_string(), "sci-fi".to_string()]
        );
    }

    #[test]
    fn list_all_tags_drops_tag_after_last_use_removed() {
        let (_dir, store) = open_test_store();
        let a = insert_test_item(&store);
        let b = insert_test_item(&store);
        store.add_tag(&a, "shared").unwrap();
        store.add_tag(&b, "shared").unwrap();
        store.add_tag(&a, "solo").unwrap();

        store.remove_tag(&a, "solo").unwrap();
        assert_eq!(store.list_all_tags().unwrap(), vec!["shared".to_string()]);

        // Still used by another item: stays after one link is removed.
        store.remove_tag(&a, "shared").unwrap();
        assert_eq!(store.list_all_tags().unwrap(), vec!["shared".to_string()]);
        store.remove_tag(&b, "shared").unwrap();
        assert!(store.list_all_tags().unwrap().is_empty());
    }

    #[test]
    fn list_all_tags_drops_tag_when_last_tagged_item_removed() {
        let (_dir, store) = open_test_store();
        let a = insert_test_item(&store);
        store.add_tag(&a, "gone").unwrap();
        assert_eq!(store.list_all_tags().unwrap(), vec!["gone".to_string()]);

        store.remove_items(std::slice::from_ref(&a)).unwrap();
        assert!(store.list_all_tags().unwrap().is_empty());
    }

    #[test]
    fn readding_orphaned_tag_name_works_and_reappears() {
        let (_dir, store) = open_test_store();
        let a = insert_test_item(&store);
        let b = insert_test_item(&store);
        store.add_tag(&a, "again").unwrap();
        store.remove_tag(&a, "again").unwrap();
        assert!(store.list_all_tags().unwrap().is_empty());

        // The orphaned `tags` row is reused, not duplicated.
        store.add_tag(&b, "again").unwrap();
        assert_eq!(store.list_all_tags().unwrap(), vec!["again".to_string()]);
        let conn = store.conn.lock().unwrap_or_else(|p| p.into_inner());
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE name = 'again'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn list_items_by_tag_returns_only_tagged_items() {
        let (_dir, store) = open_test_store();
        let tagged = insert_test_item(&store);
        let untagged = insert_test_item(&store);

        store.add_tag(&tagged, "favorite").unwrap();

        let items = store.list_items_by_tag("favorite").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, tagged);
        assert!(items.iter().all(|i| i.id != untagged));

        assert!(store
            .list_items_by_tag("nonexistent-tag")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn deleting_item_cascades_to_collection_and_tag_join_rows() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let collection_id = store.create_collection("Archive").unwrap();

        store
            .add_item_to_collection(&item_id, &collection_id)
            .unwrap();
        store.add_tag(&item_id, "classic").unwrap();

        store.delete_item(&item_id).unwrap();

        // No orphaned join rows for either table.
        let conn = store.conn.lock().unwrap_or_else(|p| p.into_inner());
        let item_collections_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_collections WHERE item_id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .unwrap();
        let item_tags_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_tags WHERE item_id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(item_collections_count, 0);
        assert_eq!(item_tags_count, 0);

        // The collection and tag themselves survive — only the join rows go.
        let collection_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM collections WHERE id = ?1",
                params![collection_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(collection_count, 1);
    }

    // ── Annotations (ADR-003) ────────────────────────────────────────────

    /// Round-trips the full ADR-003 anchor tuple through SQLite storage,
    /// including the two FNV-1a hashes at values that exercise both the
    /// sign-bit-flip risk of an `as i64`/`as u64` cast (a hash whose top bit
    /// is set looks negative as a bare `i64`) and `u64::MAX` itself.
    #[test]
    fn annotation_create_then_list_round_trips_every_anchor_field_exactly() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);

        let id = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Highlight,
                "block-42",
                17,
                9,
                0xFFFF_FFFF_FFFF_FFFF, // u64::MAX — top bit set
                0x8000_0000_0000_0001, // top bit set, not all-ones
                None,
            )
            .unwrap();

        let annotations = store.list_annotations_for_item(&item_id).unwrap();
        assert_eq!(annotations.len(), 1);
        let a = &annotations[0];
        assert_eq!(a.id, id);
        assert_eq!(a.item_id, item_id);
        assert_eq!(a.kind, gist_model::AnnotationKind::Highlight);
        assert_eq!(a.block_id, "block-42");
        assert_eq!(a.start, 17);
        assert_eq!(a.len, 9);
        assert_eq!(a.prefix_hash, 0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(a.quote_hash, 0x8000_0000_0000_0001);
        assert_eq!(a.note_text, None);
        assert_eq!(a.created_at, a.updated_at);

        // The same struct also round-trips through JSON exactly, including
        // both hashes — the "serialization" half of this test, independent
        // of SQLite.
        let json = serde_json::to_string(a).unwrap();
        let back: gist_model::Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, a);
    }

    #[test]
    fn annotation_note_kind_carries_note_text() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);

        store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Note,
                "block-1",
                0,
                5,
                1,
                2,
                Some("a margin note"),
            )
            .unwrap();

        let annotations = store.list_annotations_for_item(&item_id).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].kind, gist_model::AnnotationKind::Note);
        assert_eq!(annotations[0].note_text.as_deref(), Some("a margin note"));
    }

    #[test]
    fn annotation_bookmark_kind_uses_zero_length_point_anchor() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);

        store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Bookmark,
                "block-7",
                123,
                0,
                42,
                42,
                None,
            )
            .unwrap();

        let annotations = store.list_annotations_for_item(&item_id).unwrap();
        assert_eq!(annotations[0].kind, gist_model::AnnotationKind::Bookmark);
        assert_eq!(annotations[0].len, 0);
    }

    #[test]
    fn list_annotations_for_item_only_returns_that_items_annotations() {
        let (_dir, store) = open_test_store();
        let item_a = insert_test_item(&store);
        let item_b = insert_test_item(&store);

        store
            .create_annotation(
                &item_a,
                gist_model::AnnotationKind::Highlight,
                "b1",
                0,
                1,
                1,
                1,
                None,
            )
            .unwrap();
        store
            .create_annotation(
                &item_b,
                gist_model::AnnotationKind::Highlight,
                "b2",
                0,
                1,
                2,
                2,
                None,
            )
            .unwrap();

        let a_annotations = store.list_annotations_for_item(&item_a).unwrap();
        let b_annotations = store.list_annotations_for_item(&item_b).unwrap();
        assert_eq!(a_annotations.len(), 1);
        assert_eq!(b_annotations.len(), 1);
        assert_eq!(a_annotations[0].block_id, "b1");
        assert_eq!(b_annotations[0].block_id, "b2");
    }

    #[test]
    fn update_annotation_note_changes_text_and_bumps_updated_at_only() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let id = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Note,
                "block-1",
                0,
                5,
                1,
                2,
                Some("first draft"),
            )
            .unwrap();
        let before = store.list_annotations_for_item(&item_id).unwrap().remove(0);

        // Ensure a strictly later millisecond timestamp is observable.
        std::thread::sleep(std::time::Duration::from_millis(5));
        store
            .update_annotation_note(&id, Some("edited text"))
            .unwrap();

        let after = store.list_annotations_for_item(&item_id).unwrap().remove(0);
        assert_eq!(after.note_text.as_deref(), Some("edited text"));
        assert_eq!(after.created_at, before.created_at);
        assert!(after.updated_at >= before.updated_at);
        // Anchor fields are untouched by a note-only update.
        assert_eq!(after.block_id, before.block_id);
        assert_eq!(after.start, before.start);
        assert_eq!(after.len, before.len);
        assert_eq!(after.prefix_hash, before.prefix_hash);
        assert_eq!(after.quote_hash, before.quote_hash);
    }

    #[test]
    fn update_annotation_note_can_clear_text() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let id = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Note,
                "block-1",
                0,
                5,
                1,
                2,
                Some("will be cleared"),
            )
            .unwrap();

        store.update_annotation_note(&id, None).unwrap();

        let annotations = store.list_annotations_for_item(&item_id).unwrap();
        assert_eq!(annotations[0].note_text, None);
    }

    #[test]
    fn update_annotation_note_on_unknown_id_returns_not_found() {
        let (_dir, store) = open_test_store();
        let result = store.update_annotation_note("does-not-exist", Some("x"));
        assert!(matches!(result, Err(StoreError::NotFound(ref id)) if id == "does-not-exist"));
    }

    #[test]
    fn update_annotation_anchor_rewrites_every_anchor_field_and_bumps_updated_at() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let id = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Highlight,
                "s0",
                10,
                5,
                111,
                222,
                None,
            )
            .unwrap();
        let before = store.list_annotations_for_item(&item_id).unwrap().remove(0);

        // Ensure a strictly later millisecond timestamp is observable.
        std::thread::sleep(std::time::Duration::from_millis(5));
        store
            .update_annotation_anchor(&id, "s1", 42, 7, 333, 444)
            .unwrap();

        let after = store.list_annotations_for_item(&item_id).unwrap().remove(0);
        assert_eq!(after.block_id, "s1");
        assert_eq!(after.start, 42);
        assert_eq!(after.len, 7);
        assert_eq!(after.prefix_hash, 333);
        assert_eq!(after.quote_hash, 444);
        assert_eq!(after.created_at, before.created_at);
        assert!(after.updated_at >= before.updated_at);
        // Note text is untouched by an anchor-only update.
        assert_eq!(after.note_text, before.note_text);
    }

    #[test]
    fn update_annotation_anchor_on_unknown_id_returns_not_found() {
        let (_dir, store) = open_test_store();
        let result = store.update_annotation_anchor("does-not-exist", "s0", 0, 1, 1, 1);
        assert!(matches!(result, Err(StoreError::NotFound(ref id)) if id == "does-not-exist"));
    }

    #[test]
    fn delete_annotation_removes_it_and_errors_on_unknown_id() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let id = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Highlight,
                "block-1",
                0,
                5,
                1,
                2,
                None,
            )
            .unwrap();

        store.delete_annotation(&id).unwrap();
        assert!(store
            .list_annotations_for_item(&item_id)
            .unwrap()
            .is_empty());

        let result = store.delete_annotation(&id);
        assert!(matches!(result, Err(StoreError::NotFound(ref got)) if *got == id));
    }

    #[test]
    fn delete_annotations_bulk_removes_valid_ids_and_ignores_unknown_ids() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        let id_a = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Highlight,
                "b1",
                0,
                1,
                1,
                1,
                None,
            )
            .unwrap();
        let id_b = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Highlight,
                "b2",
                0,
                1,
                2,
                2,
                None,
            )
            .unwrap();
        let id_c = store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Highlight,
                "b3",
                0,
                1,
                3,
                3,
                None,
            )
            .unwrap();

        let deleted = store
            .delete_annotations(&[id_a.clone(), "not-a-real-id".to_string(), id_b.clone()])
            .unwrap();
        assert_eq!(deleted, 2);

        let remaining = store.list_annotations_for_item(&item_id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, id_c);
    }

    #[test]
    fn deleting_item_cascades_to_its_annotations() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        store
            .create_annotation(
                &item_id,
                gist_model::AnnotationKind::Bookmark,
                "b1",
                0,
                0,
                1,
                1,
                None,
            )
            .unwrap();

        store.delete_item(&item_id).unwrap();

        let conn = store.conn.lock().unwrap_or_else(|p| p.into_inner());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM annotations WHERE item_id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    /// `annotations.kind` should only ever hold one of the three strings
    /// `create_annotation` writes; this locks in the round-trip for all
    /// three so a future kind-string typo/rename is caught immediately.
    #[test]
    fn annotation_kind_round_trips_for_every_variant() {
        let (_dir, store) = open_test_store();
        let item_id = insert_test_item(&store);
        for (kind, block) in [
            (gist_model::AnnotationKind::Highlight, "h"),
            (gist_model::AnnotationKind::Note, "n"),
            (gist_model::AnnotationKind::Bookmark, "b"),
        ] {
            store
                .create_annotation(&item_id, kind, block, 0, 1, 1, 1, None)
                .unwrap();
        }
        let annotations = store.list_annotations_for_item(&item_id).unwrap();
        assert_eq!(annotations.len(), 3);
        assert!(annotations
            .iter()
            .any(|a| a.kind == gist_model::AnnotationKind::Highlight));
        assert!(annotations
            .iter()
            .any(|a| a.kind == gist_model::AnnotationKind::Note));
        assert!(annotations
            .iter()
            .any(|a| a.kind == gist_model::AnnotationKind::Bookmark));
    }

    /// `remove_items` treats an id that matches no row as a no-op for that
    /// id — not an error for the whole call — so a mixed batch of valid and
    /// invalid ids removes exactly the valid ones and reports only those in
    /// its return value. This test locks in that exact behaviour.
    #[test]
    fn remove_items_removes_valid_ids_and_ignores_unknown_ids_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        let store = Store::open(&db, &storage).unwrap();

        let doc1 = doc_with_title("alpha");
        let doc2 = doc_with_title("beta");
        store.insert_item(&doc1).unwrap();
        store.insert_item(&doc2).unwrap();

        let doc1_path = storage.join(format!("{}.json", doc1.id));
        let doc1_tokens_path = storage.join(format!("{}.tokens.json", doc1.id));
        let doc2_path = storage.join(format!("{}.json", doc2.id));
        assert!(doc1_path.exists());
        assert!(doc2_path.exists());

        let ids = vec![doc1.id.clone(), "does-not-exist".to_string()];
        let removed = store.remove_items(&ids).unwrap();

        // Only the valid id is reported as removed.
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, doc1.id);
        assert_eq!(removed[0].doc_path, doc1_path.to_string_lossy());
        assert_eq!(removed[0].source_path.as_deref(), Some("/tmp/alpha.txt"));
        // doc_with_title never sets source_copy_ref (that's gist-core's job,
        // via store_original_copy), so there's nothing to delete here.
        assert_eq!(removed[0].source_copy_path, None);

        // The DB row for doc1 is gone; doc2's row is untouched.
        assert!(store.get_item_by_id(&doc1.id).unwrap().is_none());
        assert!(store.get_item_by_id(&doc2.id).unwrap().is_some());

        // remove_items does not touch the filesystem itself — both blobs
        // still exist on disk after the call; that's the caller's job.
        assert!(doc1_path.exists());
        assert!(doc1_tokens_path.exists());
        assert!(doc2_path.exists());
    }

    // ── Shared stored copies (ADR-006 addendum, 2026-09-21) ───────────────

    /// Two documents pointed at one content-addressed stored copy, as
    /// ADR-006's dedup produces for two imports of byte-identical files.
    fn two_docs_sharing_one_copy(store: &Store) -> (String, String, String) {
        let copy = store
            .store_original_copy(b"one set of bytes, two library items", "txt")
            .unwrap();

        let mut a = doc_with_title("sharer-a");
        a.metadata.source_copy_ref = Some(copy.clone());
        let mut b = doc_with_title("sharer-b");
        b.metadata.source_copy_ref = Some(copy.clone());

        store.insert_item(&a).unwrap();
        store.insert_item(&b).unwrap();
        (a.id, b.id, copy)
    }

    /// Removing one of two sharers must report the copy as still referenced,
    /// so the caller keeps it. This is the flag that stops removal deleting
    /// a surviving item's stored copy.
    #[test]
    fn remove_items_flags_a_stored_copy_another_surviving_row_still_references() {
        let (_dir, store) = open_test_store();
        let (id_a, id_b, copy) = two_docs_sharing_one_copy(&store);

        let removed = store.remove_items(&[id_a]).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].source_copy_path.as_deref(), Some(copy.as_str()));
        assert!(
            removed[0].source_copy_still_referenced,
            "the surviving row still points at this copy"
        );

        // …and once the last sharer goes, it is no longer referenced.
        let removed = store.remove_items(&[id_b]).unwrap();
        assert!(
            !removed[0].source_copy_still_referenced,
            "nothing references the copy any more, so it is the caller's to delete"
        );
    }

    /// Both sharers in **one batch**: the question is asked after every
    /// deletion in the batch has been applied, so neither row is flagged as
    /// still referenced and the caller deletes the copy once.
    #[test]
    fn remove_items_does_not_flag_a_copy_shared_only_within_the_same_batch() {
        let (_dir, store) = open_test_store();
        let (id_a, id_b, _copy) = two_docs_sharing_one_copy(&store);

        let removed = store.remove_items(&[id_a, id_b]).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(
            removed.iter().all(|r| !r.source_copy_still_referenced),
            "a row that is itself being removed must not count as a survivor: {removed:?}"
        );
    }

    /// An item with no stored copy at all (a URL import, or one predating
    /// ADR-006) must never be flagged — there is nothing to share.
    #[test]
    fn remove_items_never_flags_an_item_without_a_stored_copy() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("no-copy");
        store.insert_item(&doc).unwrap();

        let removed = store.remove_items(&[doc.id]).unwrap();
        assert_eq!(removed[0].source_copy_path, None);
        assert!(!removed[0].source_copy_still_referenced);
    }

    /// Windows filesystems are case-insensitive, so a row recorded as
    /// `…/ABC.txt` and one recorded as `…/abc.txt` name the *same file*.
    /// Comparing the two paths case-sensitively would miss the match and
    /// report the copy as unreferenced — i.e. fail in the direction that
    /// deletes a live item's stored copy. `copy_ref_key` lower-cases both
    /// sides; this pins that.
    #[test]
    fn remove_items_matches_a_shared_copy_whose_recorded_path_differs_only_by_case() {
        let (_dir, store) = open_test_store();
        let copy = store
            .store_original_copy(b"case-insensitive", "txt")
            .unwrap();

        let mut a = doc_with_title("lower");
        a.metadata.source_copy_ref = Some(copy.clone());
        let mut b = doc_with_title("upper");
        b.metadata.source_copy_ref = Some(copy.to_uppercase());
        store.insert_item(&a).unwrap();
        store.insert_item(&b).unwrap();

        let removed = store.remove_items(&[a.id]).unwrap();
        assert!(
            removed[0].source_copy_still_referenced,
            "a surviving row naming the same file in a different case must still count"
        );
    }

    #[test]
    fn store_original_copy_is_content_addressed_and_deduplicates() {
        let (_dir, store) = open_test_store();

        let path1 = store.store_original_copy(b"hello world", "txt").unwrap();
        let path2 = store.store_original_copy(b"hello world", "txt").unwrap();
        assert_eq!(
            path1, path2,
            "identical bytes + extension must hash to the same path"
        );
        assert!(std::path::Path::new(&path1).exists());
        assert!(path1.ends_with(".txt"));

        let different = store.store_original_copy(b"goodbye world", "txt").unwrap();
        assert_ne!(path1, different);

        // No extension is fine too (e.g. an extensionless source file).
        let no_ext = store.store_original_copy(b"no extension here", "").unwrap();
        assert!(!no_ext.ends_with('.'));
    }

    // ── ext sanitization (F23) ───────────────────────────────────────────

    #[test]
    fn sanitize_ext_strips_path_traversal_characters() {
        assert_eq!(sanitize_ext("txt"), "txt");
        assert_eq!(sanitize_ext("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_ext("../../../evil"), "evil");
        assert_eq!(sanitize_ext("txt/../evil"), "txtevil");
        assert_eq!(sanitize_ext(r"..\..\windows"), "windows");
        assert_eq!(sanitize_ext(""), "");
    }

    #[test]
    fn store_original_copy_confines_traversal_attempt_ext_to_originals_dir() {
        let (dir, store) = open_test_store();

        let path = store
            .store_original_copy(b"malicious payload", "../../../../etc/passwd")
            .unwrap();
        let path = std::path::Path::new(&path);

        // The resulting file must land inside <storage_dir>/originals/, not
        // have escaped it via the traversal sequence in `ext`.
        let originals_dir = dir.path().join("storage").join("originals");
        assert_eq!(
            path.parent().map(|p| p.canonicalize().unwrap()),
            Some(originals_dir.canonicalize().unwrap()),
            "traversal-attempt ext must not escape the originals directory"
        );
        assert!(path.exists());
    }

    #[test]
    fn source_copy_path_round_trips_through_insert_and_remove() {
        let (_dir, store) = open_test_store();

        let copy_path = store.store_original_copy(b"quokka content", "txt").unwrap();
        let mut doc = doc_with_title("delta");
        doc.metadata.source_copy_ref = Some(copy_path.clone());
        store.insert_item(&doc).unwrap();

        let removed = store.remove_items(&[doc.id.clone()]).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(
            removed[0].source_copy_path.as_deref(),
            Some(copy_path.as_str())
        );
        // source_path (informational only) is unaffected by the copy path.
        assert_eq!(removed[0].source_path.as_deref(), Some("/tmp/delta.txt"));
    }

    #[test]
    fn delete_item_is_equivalent_to_remove_items_of_one() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        let store = Store::open(&db, &storage).unwrap();

        let doc = doc_with_title("gamma");
        store.insert_item(&doc).unwrap();

        store.delete_item(&doc.id).unwrap();
        assert!(store.get_item_by_id(&doc.id).unwrap().is_none());

        // Deleting an id that no longer exists is not an error.
        store.delete_item(&doc.id).unwrap();
    }

    // ── FTS5 query sanitization (F19) ────────────────────────────────────

    #[test]
    fn escape_fts5_query_wraps_and_doubles_quotes() {
        assert_eq!(escape_fts5_query("marsupial"), "\"marsupial\"*");
        assert_eq!(escape_fts5_query("say \"hi\""), "\"say \"\"hi\"\"\"*");
        assert_eq!(escape_fts5_query(""), "\"\"");
    }

    #[test]
    fn search_items_treats_operators_as_literal_text_not_syntax() {
        let (_dir, store) = open_test_store();
        store.insert_item(&doc_with_title("marsupial")).unwrap();

        // Before F19's fix, an unbalanced quote threw an unsanitized FTS5
        // syntax error, and OR/NEAR/column-filter/prefix syntax was
        // interpreted as query structure rather than literal search text.
        // All of these must now return Ok, not propagate a syntax error.
        for adversarial in [
            "\"unterminated",
            "marsupial OR *",
            "a NEAR/2 b",
            "col:marsupial",
            "marsupial\" --",
        ] {
            let result = store.search_items(adversarial, 10);
            assert!(
                result.is_ok(),
                "expected Ok for adversarial query {:?}, got {:?}",
                adversarial,
                result
            );
        }
    }

    #[test]
    fn search_items_still_finds_plain_word_match() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("marsupial");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let results = store.search_items("marsupial", 10).unwrap();
        assert!(
            results.contains(&id),
            "expected plain-word search to still find the match, got {:?}",
            results
        );
    }

    /// Regression test for a real user-reported bug: typing a short prefix
    /// of a word (as live search-as-you-type naturally does before the user
    /// finishes typing) found nothing, even though the full word was right
    /// there in the document -- `escape_fts5_query`'s exact-phrase quoting
    /// required a complete-token match, silently disabling prefix search
    /// entirely regardless of query length. Covers both a short (3-char)
    /// and very short (1-char) prefix.
    #[test]
    fn search_items_finds_short_prefix_of_a_longer_word() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("marsupial");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        for prefix in ["mar", "m"] {
            let results = store.search_items(prefix, 10).unwrap();
            assert!(
                results.contains(&id),
                "expected prefix {:?} to find \"marsupial\", got {:?}",
                prefix,
                results
            );
        }
    }

    #[test]
    fn search_items_finds_partial_filename_and_title_matches() {
        let (_dir, store) = open_test_store();
        let mut doc = doc_with_title("quarterly-report");
        doc.metadata.title = "Annual Reading Notes".into();
        doc.metadata.source_ref = Some("/books/Quarterly_Report_2025.txt".into());
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        for query in ["report_20", "READING"] {
            let results = store.search_items(query, 10).unwrap();
            assert!(
                results.contains(&id),
                "expected query {query:?} to find title or filename, got {results:?}"
            );
        }
    }

    #[test]
    fn search_items_treats_like_wildcards_as_literal_metadata_text() {
        let (_dir, store) = open_test_store();
        let mut doc = doc_with_title("literal-percent");
        doc.metadata.title = "100%_ready_report".into();
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();
        store.insert_item(&doc_with_title("unrelated")).unwrap();

        assert_eq!(store.search_items("%_ready", 10).unwrap(), vec![id]);
    }

    // ── Encryption at rest (ADR-011) ─────────────────────────────────────

    fn open_test_store_encrypted(seed: u8) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        let store =
            Store::open_encrypted(&db, &storage, Arc::new(FakeKeyProvider::new(seed))).unwrap();
        (dir, store)
    }

    #[test]
    fn encrypted_store_round_trips_item_and_tokens() {
        let (_dir, store) = open_test_store_encrypted(1);
        let doc = doc_with_title("ciphertext-roundtrip");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let loaded = store.get_item(&id).unwrap().expect("item must be found");
        assert_eq!(loaded.metadata.title, "ciphertext-roundtrip");

        let tokens = store
            .get_tokens(&id)
            .unwrap()
            .expect("tokens must be found");
        assert_eq!(tokens.len(), doc.token_stream.len());
    }

    /// The whole point of ADR-011: the bytes actually on disk must not be
    /// readable as plaintext. Encrypts a document containing a distinctive
    /// string and asserts that string does not appear anywhere in the raw
    /// file bytes — proving this isn't just "round-trips through the API"
    /// but that the on-disk representation is genuinely not plaintext.
    #[test]
    fn encrypted_content_is_not_plaintext_on_disk() {
        let (dir, store) = open_test_store_encrypted(2);
        let needle = "MARSUPIAL_CANARY_STRING_0xB33F";
        let doc = doc_with_title(needle);
        store.insert_item(&doc).unwrap();

        let storage_dir = dir.path().join("storage");
        let doc_path = storage_dir.join(format!("{}.json", doc.id));
        let tokens_path = storage_dir.join(format!("{}.tokens.json", doc.id));

        let doc_bytes = std::fs::read(&doc_path).unwrap();
        let tokens_bytes = std::fs::read(&tokens_path).unwrap();

        assert!(
            !doc_bytes
                .windows(needle.len())
                .any(|w| w == needle.as_bytes()),
            "canary string must not appear in the encrypted document blob on disk"
        );
        assert!(
            !tokens_bytes
                .windows(needle.len())
                .any(|w| w == needle.as_bytes()),
            "canary string must not appear in the encrypted tokens blob on disk"
        );
    }

    /// `store_original_copy`'s output must also be ciphertext, not plaintext,
    /// when a key provider is configured.
    #[test]
    fn encrypted_original_copy_is_not_plaintext_on_disk() {
        let (_dir, store) = open_test_store_encrypted(3);
        let needle = b"MARSUPIAL_ORIGINAL_FILE_CONTENTS";
        let copy_path = store.store_original_copy(needle, "txt").unwrap();

        let on_disk = std::fs::read(&copy_path).unwrap();
        assert!(
            !on_disk
                .windows(needle.len())
                .any(|w| w == needle.as_slice()),
            "original bytes must not appear in the encrypted copy on disk"
        );

        // And it must still decrypt back to exactly the original bytes,
        // proving this isn't just "different bytes" but genuinely the same
        // plaintext recoverable under the right key.
        let key = store.encryption_key().unwrap();
        let recovered = decrypt_at_rest(&key, &on_disk, "test").unwrap();
        assert_eq!(recovered, needle);
    }

    /// Decrypting with the wrong key must fail cleanly (an `Err`), never
    /// panic and never silently return garbage that happens to parse.
    #[test]
    fn wrong_key_fails_to_decrypt_without_panicking() {
        let key_a = FakeKeyProvider::new(0xAA).get_or_create_key();
        let key_b = FakeKeyProvider::new(0xBB).get_or_create_key();

        let ciphertext = encrypt_at_rest(&key_a, b"hello from key A");
        let result = decrypt_at_rest(&key_b, &ciphertext, "test");

        assert!(
            matches!(result, Err(StoreError::DecryptionFailed(_))),
            "expected DecryptionFailed for the wrong key, got {:?}",
            result
        );
    }

    /// Truncated/corrupted ciphertext (shorter than a nonce) must also fail
    /// cleanly rather than panicking on an out-of-bounds slice.
    #[test]
    fn corrupted_short_ciphertext_fails_to_decrypt_without_panicking() {
        let key = FakeKeyProvider::new(0xCC).get_or_create_key();
        let result = decrypt_at_rest(&key, b"short", "test");
        assert!(matches!(result, Err(StoreError::DecryptionFailed(_))));
    }

    /// Migration: a store opened without encryption (existing behavior) then
    /// re-opened *with* encryption must still be able to read the old,
    /// plaintext row — and any newly inserted item after that point is
    /// encrypted. This is `Store::open_encrypted`'s documented migration
    /// model: no bulk re-encryption, per-row `content_encrypted` flag.
    #[test]
    fn migration_old_plaintext_item_readable_after_switching_to_encrypted_store() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");

        // Phase 1: plain, unencrypted store — as if from before ADR-011.
        let legacy_id = {
            let store = Store::open(&db, &storage).unwrap();
            let doc = doc_with_title("legacy-plaintext-item");
            let id = doc.id.clone();
            store.insert_item(&doc).unwrap();
            id
        };

        // Phase 2: same on-disk store, now reopened with encryption enabled.
        let store =
            Store::open_encrypted(&db, &storage, Arc::new(FakeKeyProvider::new(9))).unwrap();

        // The old plaintext row must still be readable, unmodified.
        let legacy = store
            .get_item(&legacy_id)
            .unwrap()
            .expect("legacy plaintext item must still be readable");
        assert_eq!(legacy.metadata.title, "legacy-plaintext-item");
        let legacy_doc_path = storage.join(format!("{}.json", legacy_id));
        let legacy_bytes = std::fs::read(&legacy_doc_path).unwrap();
        assert!(
            deserialize_ir_blob::<gist_model::Document>(&legacy_bytes).is_ok(),
            "legacy item's on-disk blob must still be plain, un-re-encrypted JSON \
             (ADR-019-enveloped, not encrypted)"
        );

        // A newly inserted item, after switching to open_encrypted, must be
        // encrypted (its content_encrypted flag set, its bytes not plaintext).
        let new_doc = doc_with_title("new-encrypted-item");
        let new_id = new_doc.id.clone();
        store.insert_item(&new_doc).unwrap();

        let new_doc_path = storage.join(format!("{}.json", new_id));
        let new_bytes = std::fs::read(&new_doc_path).unwrap();
        assert!(
            serde_json::from_slice::<gist_model::Document>(&new_bytes).is_err(),
            "newly inserted item's on-disk blob must be ciphertext, not plain JSON"
        );

        let loaded_new = store
            .get_item(&new_id)
            .unwrap()
            .expect("new encrypted item must be readable through the API");
        assert_eq!(loaded_new.metadata.title, "new-encrypted-item");
    }

    /// Reading encrypted content back through a plain (no key provider)
    /// `Store::open` must fail with `MissingKeyProvider`, not silently
    /// return garbage or panic.
    #[test]
    fn reading_encrypted_item_without_key_provider_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");

        let id = {
            let store =
                Store::open_encrypted(&db, &storage, Arc::new(FakeKeyProvider::new(5))).unwrap();
            let doc = doc_with_title("encrypted-item");
            let id = doc.id.clone();
            store.insert_item(&doc).unwrap();
            id
        };

        // Reopen the same on-disk store without a key provider.
        let plain_store = Store::open(&db, &storage).unwrap();
        let result = plain_store.get_item(&id);
        assert!(
            matches!(result, Err(StoreError::MissingKeyProvider)),
            "expected MissingKeyProvider, got {:?}",
            result
        );
    }

    #[test]
    fn schema_migration_adds_content_encrypted_column_defaulting_to_unencrypted() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");

        // Simulate a pre-ADR-011 v4 database: open at v4 by calling
        // Store::open before content_encrypted existed isn't possible
        // directly since the code always migrates to SCHEMA_VERSION, so
        // instead this test just confirms a fresh store's rows default to
        // unencrypted and the column exists and is queryable.
        let store = Store::open(&db, &storage).unwrap();
        let id = insert_test_item(&store);

        let conn = Connection::open(&db).unwrap();
        let content_encrypted: i64 = conn
            .query_row(
                "SELECT content_encrypted FROM library_items WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            content_encrypted, 0,
            "items inserted via a plain Store::open must default to unencrypted"
        );
    }

    // ── At-rest integrity checksums (ADR-013 / `A4`) ─────────────────────

    /// Flips a byte roughly in the middle of `path`'s content — enough to
    /// invalidate a BLAKE3 checksum (or an AES-GCM auth tag) without
    /// depending on exactly which byte gets hit.
    fn corrupt_file(path: &std::path::Path) {
        let mut bytes = std::fs::read(path).unwrap();
        assert!(!bytes.is_empty(), "cannot corrupt an empty file");
        let idx = bytes.len() / 2;
        bytes[idx] ^= 0xFF;
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn insert_item_writes_matching_checksum_sidecars_for_doc_and_tokens() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("epsilon");
        store.insert_item(&doc).unwrap();

        let storage = dir.path().join("storage");
        let doc_path = storage.join(format!("{}.json", doc.id));
        let tokens_path = storage.join(format!("{}.tokens.json", doc.id));

        for path in [&doc_path, &tokens_path] {
            let sidecar = checksum_sidecar_path(path);
            assert!(
                sidecar.exists(),
                "expected a checksum sidecar at {sidecar:?}"
            );
            let recorded = std::fs::read_to_string(&sidecar).unwrap();
            let actual = checksum_hex(&std::fs::read(path).unwrap());
            assert_eq!(
                recorded, actual,
                "sidecar checksum must match the file's actual on-disk content"
            );
        }

        // And a normal read still succeeds — the round trip this all exists
        // to not break.
        assert!(store.get_item(&doc.id).unwrap().is_some());
        assert!(store.get_tokens(&doc.id).unwrap().is_some());
    }

    #[test]
    fn corrupted_doc_blob_fails_checksum_verification_on_read() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("zeta");
        store.insert_item(&doc).unwrap();

        let doc_path = dir.path().join("storage").join(format!("{}.json", doc.id));
        corrupt_file(&doc_path);

        let result = store.get_item(&doc.id);
        assert!(
            matches!(result, Err(StoreError::ChecksumMismatch { .. })),
            "expected ChecksumMismatch for a corrupted doc blob, got {:?}",
            result
        );
    }

    #[test]
    fn corrupted_tokens_blob_fails_checksum_verification_on_read() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("eta");
        store.insert_item(&doc).unwrap();

        let tokens_path = dir
            .path()
            .join("storage")
            .join(format!("{}.tokens.json", doc.id));
        corrupt_file(&tokens_path);

        let result = store.get_tokens(&doc.id);
        assert!(
            matches!(result, Err(StoreError::ChecksumMismatch { .. })),
            "expected ChecksumMismatch for a corrupted tokens blob, got {:?}",
            result
        );
    }

    /// A row/file written before this feature existed has no checksum
    /// sidecar at all — simulated here by deleting it after the fact.
    /// Reads must succeed normally, per ADR-013's backfill policy: a
    /// missing sidecar is "unverified," never treated as corruption.
    #[test]
    fn missing_checksum_sidecar_is_treated_as_legacy_and_skipped_not_an_error() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("theta");
        store.insert_item(&doc).unwrap();

        let storage = dir.path().join("storage");
        let doc_path = storage.join(format!("{}.json", doc.id));
        let tokens_path = storage.join(format!("{}.tokens.json", doc.id));
        std::fs::remove_file(checksum_sidecar_path(&doc_path)).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&tokens_path)).unwrap();

        let loaded = store.get_item(&doc.id).unwrap();
        assert!(
            loaded.is_some(),
            "a legacy item with no checksum sidecar must still read successfully"
        );
        assert!(store.get_tokens(&doc.id).unwrap().is_some());
    }

    #[test]
    fn store_original_copy_writes_verifiable_checksum_sidecar() {
        let (_dir, store) = open_test_store();
        let path = store.store_original_copy(b"iota content", "txt").unwrap();

        let sidecar = checksum_sidecar_path(std::path::Path::new(&path));
        assert!(sidecar.exists());

        store
            .verify_original_copy(&path)
            .expect("freshly written original copy must verify cleanly");
    }

    #[test]
    fn corrupted_original_copy_fails_verification() {
        let (_dir, store) = open_test_store();
        let path = store.store_original_copy(b"kappa content", "txt").unwrap();
        corrupt_file(std::path::Path::new(&path));

        let result = store.verify_original_copy(&path);
        assert!(
            matches!(result, Err(StoreError::ChecksumMismatch { .. })),
            "expected ChecksumMismatch for a corrupted original copy, got {:?}",
            result
        );
    }

    /// Same backfill policy as document blobs: an original copy with no
    /// checksum sidecar (simulating one written before this feature, or —
    /// per `store_original_copy`'s documented dedup behaviour — one that was
    /// never rewritten because identical content already existed) verifies
    /// as Ok, not as corrupted.
    #[test]
    fn verify_original_copy_without_sidecar_is_not_an_error() {
        let (_dir, store) = open_test_store();
        let path = store.store_original_copy(b"lambda content", "txt").unwrap();
        std::fs::remove_file(checksum_sidecar_path(std::path::Path::new(&path))).unwrap();

        store
            .verify_original_copy(&path)
            .expect("a missing sidecar must be treated as unverified, not an error");
    }

    /// `store_original_copy`'s content-addressed dedup means a second import
    /// of byte-identical content reuses the existing file untouched — this
    /// checks that a sidecar deliberately absent from that first write (a
    /// stand-in for "written before checksums existed") stays absent after a
    /// dedup hit, rather than being silently backfilled. Locks in the
    /// documented "no backfill pass" decision (ADR-013) rather than letting
    /// it drift.
    #[test]
    fn dedup_hit_does_not_backfill_a_missing_sidecar() {
        let (_dir, store) = open_test_store();
        let path = store.store_original_copy(b"mu content", "txt").unwrap();
        let sidecar = checksum_sidecar_path(std::path::Path::new(&path));
        std::fs::remove_file(&sidecar).unwrap();

        // Re-import the exact same bytes — a dedup hit, since the file
        // already exists at the content-addressed path.
        let path2 = store.store_original_copy(b"mu content", "txt").unwrap();
        assert_eq!(path, path2);
        assert!(
            !sidecar.exists(),
            "a dedup hit must not backfill a checksum sidecar for a pre-existing file"
        );
    }

    /// Checksum verification runs on the raw on-disk bytes *before* any
    /// decryption attempt, so corruption of encrypted content is reported as
    /// `ChecksumMismatch`, not conflated with a decryption failure.
    #[test]
    fn checksum_mismatch_detected_before_decryption_is_attempted() {
        let (dir, store) = open_test_store_encrypted(42);
        let doc = doc_with_title("nu-encrypted");
        store.insert_item(&doc).unwrap();

        let doc_path = dir.path().join("storage").join(format!("{}.json", doc.id));
        corrupt_file(&doc_path);

        let result = store.get_item(&doc.id);
        assert!(
            matches!(result, Err(StoreError::ChecksumMismatch { .. })),
            "expected ChecksumMismatch (checked before decryption) for corrupted \
             encrypted content, got {:?}",
            result
        );
    }

    // ── Item integrity reports (`R3`, ADR-013 / `A4`) ────────────────────

    #[test]
    fn verify_item_integrity_reports_verified_for_a_clean_item() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("xi");
        store.insert_item(&doc).unwrap();

        let report = store
            .verify_item_integrity(&doc.id)
            .unwrap()
            .expect("just-inserted item must be found");
        assert_eq!(report.id, doc.id);
        assert_eq!(report.doc_status, ChecksumStatus::Verified);
        assert_eq!(report.tokens_status, ChecksumStatus::Verified);
        assert_eq!(
            report.original_copy_status, None,
            "doc_with_title stamps no source_copy_ref"
        );
    }

    #[test]
    fn verify_item_integrity_returns_none_for_an_unknown_id() {
        let (_dir, store) = open_test_store();
        assert!(store
            .verify_item_integrity("not-a-real-id")
            .unwrap()
            .is_none());
    }

    #[test]
    fn verify_item_integrity_reports_mismatch_for_a_corrupted_doc_blob() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("omicron");
        store.insert_item(&doc).unwrap();

        let doc_path = dir.path().join("storage").join(format!("{}.json", doc.id));
        corrupt_file(&doc_path);

        let report = store.verify_item_integrity(&doc.id).unwrap().unwrap();
        assert_eq!(report.doc_status, ChecksumStatus::Mismatch);
        assert_eq!(
            report.tokens_status,
            ChecksumStatus::Verified,
            "corrupting the doc blob must not affect the (untouched) tokens blob's status"
        );
    }

    #[test]
    fn verify_item_integrity_reports_unverified_for_a_pre_a4_item_with_no_sidecar() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("pi");
        store.insert_item(&doc).unwrap();

        let storage = dir.path().join("storage");
        let doc_path = storage.join(format!("{}.json", doc.id));
        let tokens_path = storage.join(format!("{}.tokens.json", doc.id));
        std::fs::remove_file(checksum_sidecar_path(&doc_path)).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&tokens_path)).unwrap();

        let report = store.verify_item_integrity(&doc.id).unwrap().unwrap();
        assert_eq!(
            report.doc_status,
            ChecksumStatus::Unverified,
            "a missing sidecar must be reported as unverified, never as corrupt"
        );
        assert_eq!(report.tokens_status, ChecksumStatus::Unverified);
    }

    #[test]
    fn verify_item_integrity_covers_the_original_copy_when_one_exists() {
        let (_dir, store) = open_test_store();
        let copy = store.store_original_copy(b"rho content", "txt").unwrap();
        let mut doc = doc_with_title("rho");
        doc.metadata.source_copy_ref = Some(copy.clone());
        store.insert_item(&doc).unwrap();

        let report = store.verify_item_integrity(&doc.id).unwrap().unwrap();
        assert_eq!(report.original_copy_status, Some(ChecksumStatus::Verified));

        corrupt_file(std::path::Path::new(&copy));
        let report = store.verify_item_integrity(&doc.id).unwrap().unwrap();
        assert_eq!(report.original_copy_status, Some(ChecksumStatus::Mismatch));
    }

    #[test]
    fn verify_library_integrity_reports_one_entry_per_item_including_the_corrupted_one() {
        let (dir, store) = open_test_store();
        let clean = doc_with_title("sigma");
        let corrupted = doc_with_title("tau");
        store.insert_item(&clean).unwrap();
        store.insert_item(&corrupted).unwrap();

        let corrupted_path = dir
            .path()
            .join("storage")
            .join(format!("{}.json", corrupted.id));
        corrupt_file(&corrupted_path);

        let mut reports = store.verify_library_integrity().unwrap();
        reports.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(reports.len(), 2);

        let clean_report = reports.iter().find(|r| r.id == clean.id).unwrap();
        assert_eq!(clean_report.doc_status, ChecksumStatus::Verified);

        let corrupted_report = reports.iter().find(|r| r.id == corrupted.id).unwrap();
        assert_eq!(corrupted_report.doc_status, ChecksumStatus::Mismatch);
    }

    #[test]
    fn verify_library_integrity_on_an_empty_library_returns_an_empty_vec() {
        let (_dir, store) = open_test_store();
        assert_eq!(store.verify_library_integrity().unwrap(), Vec::new());
    }

    // ── Per-item encryption (ADR-014) ────────────────────────────────────

    /// The core round trip: a plain `Store::open` instance encrypts an item
    /// via `encrypt_item`; a *separately opened* `Store::open_encrypted`
    /// instance pointed at the same on-disk files (same db, same storage
    /// dir, and — crucially — the same key) can then read it back correctly
    /// via the normal `get_item`/`get_tokens` API. This is what "is this
    /// item now really encrypted" has to mean here: the originating `Store`
    /// (no key provider at all) structurally cannot decrypt its own write
    /// back (see `encrypt_item`'s doc comment), so verifying the write
    /// requires a second, keyed instance.
    #[test]
    fn encrypt_item_round_trips_through_a_separately_opened_encrypted_store() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");

        let plain_store = Store::open(&db, &storage).unwrap();
        let doc = doc_with_title("xi-plaintext-then-encrypted");
        let id = doc.id.clone();
        plain_store.insert_item(&doc).unwrap();

        let key = FakeKeyProvider::new(0x77).get_or_create_key();
        let outcome = plain_store.encrypt_item(&id, &key).unwrap();
        assert_eq!(outcome, EncryptOutcome::Encrypted);

        // The on-disk blob must now genuinely be ciphertext, not plain JSON.
        let doc_path = storage.join(format!("{id}.json"));
        let on_disk = std::fs::read(&doc_path).unwrap();
        assert!(
            serde_json::from_slice::<gist_model::Document>(&on_disk).is_err(),
            "encrypt_item must leave ciphertext on disk, not plain JSON"
        );

        // The originating (unkeyed) Store can no longer read the content.
        assert!(matches!(
            plain_store.get_item(&id),
            Err(StoreError::MissingKeyProvider)
        ));

        // A separately opened, same-key encrypted Store CAN read it back.
        let encrypted_store =
            Store::open_encrypted(&db, &storage, Arc::new(FakeKeyProvider::new(0x77))).unwrap();
        let loaded = encrypted_store
            .get_item(&id)
            .unwrap()
            .expect("item must be readable through a same-key encrypted Store");
        assert_eq!(loaded.metadata.title, "xi-plaintext-then-encrypted");

        let tokens = encrypted_store
            .get_tokens(&id)
            .unwrap()
            .expect("tokens must be readable through a same-key encrypted Store");
        assert_eq!(tokens.len(), doc.token_stream.len());
    }

    /// **Closes the read-after-encrypt gap (ADR-014).** A `Store` opened via
    /// `Store::open_with_read_key` — the constructor `CoreClient.shared`'s
    /// production instance now uses — can encrypt an item via `encrypt_item`
    /// and then immediately read that same item's actual content back
    /// through *itself*, unlike the plain-`Store::open` instance in
    /// `encrypt_item_round_trips_through_a_separately_opened_encrypted_store`
    /// above, which structurally cannot (and correctly still can't — see
    /// the second half of this test). This is the concrete scenario the bug
    /// report described: a user selects a book, clicks "Encrypt," and must
    /// still be able to open it afterward in the same running app.
    #[test]
    fn encrypt_item_then_read_through_same_read_capable_store_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");
        let key_provider = Arc::new(FakeKeyProvider::new(0x42));

        let store =
            Store::open_with_read_key(&db, &storage, key_provider.clone() as Arc<dyn KeyProvider>)
                .unwrap();

        // New writes through this Store stay plaintext by default —
        // read-only key capability must never flip on auto-encryption.
        let plain_doc = doc_with_title("upsilon-still-plaintext-on-import");
        let plain_id = plain_doc.id.clone();
        store.insert_item(&plain_doc).unwrap();
        let plain_on_disk = std::fs::read(storage.join(format!("{plain_id}.json"))).unwrap();
        assert!(
            deserialize_ir_blob::<gist_model::Document>(&plain_on_disk).is_ok(),
            "a Store opened via open_with_read_key must still write NEW imports as plaintext \
             (ADR-019-enveloped, not encrypted)"
        );

        // Encrypt a separate item on demand, then read it back through the
        // SAME Store instance — this is the gap that used to fail with
        // MissingKeyProvider.
        let doc = doc_with_title("phi-encrypted-then-read-back");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let key = key_provider.get_or_create_key();
        let outcome = store.encrypt_item(&id, &key).unwrap();
        assert_eq!(outcome, EncryptOutcome::Encrypted);

        let loaded = store
            .get_item(&id)
            .expect("read-after-encrypt must succeed through a read-capable Store, not MissingKeyProvider")
            .expect("item must be found");
        assert_eq!(loaded.metadata.title, "phi-encrypted-then-read-back");

        let tokens = store
            .get_tokens(&id)
            .expect("token read-after-encrypt must also succeed")
            .expect("tokens must be found");
        assert_eq!(tokens.len(), doc.token_stream.len());

        // The untouched plaintext item is still readable too — read
        // capability doesn't disturb the plaintext path.
        let plain_loaded = store.get_item(&plain_id).unwrap().expect("found");
        assert_eq!(
            plain_loaded.metadata.title,
            "upsilon-still-plaintext-on-import"
        );
    }

    /// A `Store` opened via `open_with_read_key` with the WRONG key cannot
    /// decrypt content encrypted under a different key — read capability
    /// doesn't bypass the actual cryptography, it just makes the right key
    /// reachable.
    #[test]
    fn encrypt_item_then_read_through_read_capable_store_with_wrong_key_fails() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");

        let writer = Store::open(&db, &storage).unwrap();
        let doc = doc_with_title("chi-wrong-key-test");
        let id = doc.id.clone();
        writer.insert_item(&doc).unwrap();
        let right_key = FakeKeyProvider::new(0x51).get_or_create_key();
        writer.encrypt_item(&id, &right_key).unwrap();

        let reader = Store::open_with_read_key(
            &db,
            &storage,
            Arc::new(FakeKeyProvider::new(0x52)) as Arc<dyn KeyProvider>,
        )
        .unwrap();
        assert!(matches!(
            reader.get_item(&id),
            Err(StoreError::DecryptionFailed(_))
        ));
    }

    /// Calling `encrypt_item` on an already-encrypted item is a safe,
    /// cheap no-op: no error, and neither the doc/tokens blobs nor their
    /// checksum sidecars are rewritten (mtimes unchanged).
    #[test]
    fn encrypt_item_on_already_encrypted_item_is_a_no_op() {
        let (dir, store) = open_test_store_encrypted(0x99);
        let doc = doc_with_title("omicron-already-encrypted");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let doc_path = dir.path().join("storage").join(format!("{id}.json"));
        let sidecar = checksum_sidecar_path(&doc_path);
        let before_bytes = std::fs::read(&doc_path).unwrap();
        let before_mtime = std::fs::metadata(&doc_path).unwrap().modified().unwrap();

        let key = store.encryption_key().unwrap();
        let outcome = store.encrypt_item(&id, &key).unwrap();
        assert_eq!(outcome, EncryptOutcome::AlreadyEncrypted);

        let after_bytes = std::fs::read(&doc_path).unwrap();
        let after_mtime = std::fs::metadata(&doc_path).unwrap().modified().unwrap();
        assert_eq!(
            before_bytes, after_bytes,
            "already-encrypted blob must not be rewritten"
        );
        assert_eq!(
            before_mtime, after_mtime,
            "already-encrypted blob's mtime must be untouched"
        );
        assert!(sidecar.exists());

        // Calling it a second time is equally a safe no-op.
        let outcome2 = store.encrypt_item(&id, &key).unwrap();
        assert_eq!(outcome2, EncryptOutcome::AlreadyEncrypted);
    }

    #[test]
    fn encrypt_item_on_unknown_id_returns_not_found() {
        let (_dir, store) = open_test_store();
        let key = FakeKeyProvider::new(1).get_or_create_key();

        let result = store.encrypt_item("does-not-exist", &key);
        assert!(
            matches!(result, Err(StoreError::NotFound(ref id)) if id == "does-not-exist"),
            "expected NotFound for an unknown id, got {:?}",
            result
        );
    }

    /// The ADR-014 scope decision under test: `encrypt_item` must never
    /// touch `source_copy_path` (the ADR-006 `originals/` copy) — its bytes
    /// and checksum sidecar must be byte-for-byte identical before and
    /// after.
    #[test]
    fn encrypt_item_never_touches_the_original_copy() {
        let (dir, store) = open_test_store();

        let copy_path = store
            .store_original_copy(b"pi original file bytes", "txt")
            .unwrap();
        let mut doc = doc_with_title("pi-has-an-original-copy");
        doc.metadata.source_copy_ref = Some(copy_path.clone());
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let copy_sidecar = checksum_sidecar_path(std::path::Path::new(&copy_path));
        let copy_bytes_before = std::fs::read(&copy_path).unwrap();
        let copy_sidecar_before = std::fs::read_to_string(&copy_sidecar).unwrap();

        let key = FakeKeyProvider::new(2).get_or_create_key();
        let outcome = store.encrypt_item(&id, &key).unwrap();
        assert_eq!(outcome, EncryptOutcome::Encrypted);

        let copy_bytes_after = std::fs::read(&copy_path).unwrap();
        let copy_sidecar_after = std::fs::read_to_string(&copy_sidecar).unwrap();
        assert_eq!(
            copy_bytes_before, copy_bytes_after,
            "encrypt_item must never touch the original copy's bytes"
        );
        assert_eq!(
            copy_sidecar_before, copy_sidecar_after,
            "encrypt_item must never touch the original copy's checksum sidecar"
        );

        let _ = dir; // keep TempDir alive for the duration of the test
    }

    /// `encrypt_item` must correctly rewrite the checksum sidecars to cover
    /// the new ciphertext — this test would fail if that step were skipped,
    /// since the *old* (plaintext-covering) sidecar would then mismatch the
    /// new ciphertext bytes.
    #[test]
    fn encrypt_item_rewrites_checksum_sidecars_to_match_new_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let storage = dir.path().join("storage");

        let store = Store::open(&db, &storage).unwrap();
        let doc = doc_with_title("rho-checksum-rewrite");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let doc_path = storage.join(format!("{id}.json"));
        let tokens_path = storage.join(format!("{id}.tokens.json"));
        let doc_sidecar_before = std::fs::read_to_string(checksum_sidecar_path(&doc_path)).unwrap();

        let key = FakeKeyProvider::new(3).get_or_create_key();
        store.encrypt_item(&id, &key).unwrap();

        for path in [&doc_path, &tokens_path] {
            let sidecar = checksum_sidecar_path(path);
            let recorded = std::fs::read_to_string(&sidecar).unwrap();
            let actual = checksum_hex(&std::fs::read(path).unwrap());
            assert_eq!(
                recorded, actual,
                "checksum sidecar for {path:?} must match the new ciphertext, not the old plaintext"
            );
        }

        let doc_sidecar_after = std::fs::read_to_string(checksum_sidecar_path(&doc_path)).unwrap();
        assert_ne!(
            doc_sidecar_before, doc_sidecar_after,
            "the doc blob's checksum must actually change once its bytes change \
             (sanity check that this isn't accidentally comparing two identical, unrewritten sidecars)"
        );

        // And a same-key encrypted Store confirms the ciphertext + sidecar
        // together are genuinely valid and readable, not just self-consistent.
        let encrypted_store =
            Store::open_encrypted(&db, &storage, Arc::new(FakeKeyProvider::new(3))).unwrap();
        assert!(encrypted_store.get_item(&id).unwrap().is_some());
    }

    /// `list_items`'s `content_encrypted` field must reflect an item's
    /// current flag, before and after `encrypt_item` — this is SQL-only
    /// metadata, so it stays correct even read through the unkeyed `Store`
    /// that performed the encryption (see `encrypt_item`'s doc comment on
    /// why that's different from reading the item's actual *content*).
    #[test]
    fn content_encrypted_flag_visible_in_list_items_after_encrypt_item() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("sigma-flag-visibility");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let before = store.list_items(0, 10).unwrap();
        assert!(
            !before
                .iter()
                .find(|i| i.id == id)
                .unwrap()
                .content_encrypted
        );

        let key = FakeKeyProvider::new(4).get_or_create_key();
        store.encrypt_item(&id, &key).unwrap();

        let after = store.list_items(0, 10).unwrap();
        assert!(after.iter().find(|i| i.id == id).unwrap().content_encrypted);
    }

    // ── IR envelope versioning (ADR-019, Q10) ────────────────────────────

    /// A document inserted (and therefore written through the current
    /// `IrEnvelopeRef`/`CURRENT_IR_VERSION` envelope) round-trips through
    /// `get_item`/`get_tokens` exactly as before — the envelope is
    /// transparent to every existing caller.
    #[test]
    fn ir_envelope_current_version_blob_round_trips() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("ir-versioning-round-trip");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let loaded = store
            .get_item(&id)
            .unwrap()
            .expect("current-version item must round-trip");
        assert_eq!(loaded.metadata.title, "ir-versioning-round-trip");
        assert_eq!(loaded.sections.len(), doc.sections.len());

        let tokens = store
            .get_tokens(&id)
            .unwrap()
            .expect("current-version tokens must round-trip");
        assert_eq!(tokens.len(), doc.token_stream.len());

        // And the on-disk shape really is the new envelope, not the bare
        // pre-ADR-019 shape — confirms the test is exercising what it
        // claims to.
        let doc_path = store.storage_dir().join(format!("{id}.json"));
        let raw = std::fs::read(&doc_path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["ir_version"], serde_json::json!(IR_VERSION_BASE));
        assert!(value["payload"]["id"].is_string());
    }

    /// A blob whose `ir_version` is newer than this binary's
    /// `CURRENT_IR_VERSION` is rejected with a typed
    /// `StoreError::IrVersionTooNew` — never a panic, never a generic
    /// `serde_json`/deserialize error, and critically, the payload (which
    /// this binary has no business trying to interpret) is never even
    /// attempted.
    #[test]
    fn ir_envelope_future_version_is_rejected_with_typed_error_not_a_panic() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("ir-versioning-future");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        // Overwrite with a fabricated future-version envelope. The payload
        // is deliberately garbage (not a valid Document at all) to prove
        // the version check runs *before* any attempt to interpret it.
        let doc_path = store.storage_dir().join(format!("{id}.json"));
        let future_blob = serde_json::json!({
            "ir_version": 9999,
            "payload": { "this_is_not": "a valid Document shape" },
        });
        std::fs::write(&doc_path, serde_json::to_vec(&future_blob).unwrap()).unwrap();
        // Checksum sidecar now covers stale bytes; remove it so this test
        // exercises IR-version rejection specifically, not a checksum
        // mismatch (ADR-013 is a separate, already-tested concern).
        std::fs::remove_file(checksum_sidecar_path(&doc_path)).unwrap();

        let result = store.get_item(&id);
        assert!(
            matches!(
                result,
                Err(StoreError::IrVersionTooNew {
                    found: 9999,
                    expected: CURRENT_IR_VERSION
                })
            ),
            "expected a typed IrVersionTooNew error, got {result:?}"
        );
    }

    /// A blob at the current (understood) version whose `payload` carries
    /// an extra, unrecognised JSON field — simulating a hypothetical future
    /// version that's still additively decodable — still deserializes
    /// successfully. Confirms the "unknown fields are tolerated, never a
    /// hard error" policy ADR-019 documents (no type in the IR graph sets
    /// `#[serde(deny_unknown_fields)]`).
    #[test]
    fn ir_envelope_extra_unknown_payload_field_still_deserializes() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("ir-versioning-extra-field");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let doc_path = store.storage_dir().join(format!("{id}.json"));
        let raw = std::fs::read(&doc_path).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        value["payload"]["a_field_this_binary_has_never_heard_of"] =
            serde_json::json!("from a hypothetically newer, still-decodable version");
        std::fs::write(&doc_path, serde_json::to_vec(&value).unwrap()).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&doc_path)).unwrap();

        let loaded = store
            .get_item(&id)
            .unwrap()
            .expect("an extra unknown field in payload must not break deserialization");
        assert_eq!(loaded.metadata.title, "ir-versioning-extra-field");
    }

    /// **The most important test in this role.** A blob written in the
    /// exact pre-ADR-019 on-disk shape — a bare `Document`/`Vec<Token>`
    /// JSON object/array, no `{"ir_version": ..., "payload": ...}` envelope
    /// at all, exactly what `insert_item` produced for this entire
    /// project's history up to and including the immediately-preceding
    /// commit — must still read correctly through `get_item`/`get_tokens`.
    /// Existing users' on-disk libraries must not break when they upgrade
    /// to a binary built from this change.
    #[test]
    fn pre_adr019_unenveloped_blob_is_still_readable() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("legacy-unenveloped-blob");
        let id = doc.id.clone();

        // Insert normally (to get a valid library_items row + tokens
        // indexed for FTS), then overwrite both on-disk blobs with the bare,
        // unenveloped JSON shape every binary before this change wrote —
        // simulating a library that predates ADR-019.
        store.insert_item(&doc).unwrap();

        let storage = dir.path().join("storage");
        let doc_path = storage.join(format!("{id}.json"));
        let tokens_path = storage.join(format!("{id}.tokens.json"));

        let bare_doc_json = serde_json::to_vec(&doc).unwrap();
        std::fs::write(&doc_path, &bare_doc_json).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&doc_path)).unwrap();

        let bare_tokens_json = serde_json::to_vec(&doc.token_stream).unwrap();
        std::fs::write(&tokens_path, &bare_tokens_json).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&tokens_path)).unwrap();

        let loaded = store
            .get_item(&id)
            .unwrap()
            .expect("a pre-ADR-019 unenveloped document blob must still be readable");
        assert_eq!(loaded.metadata.title, "legacy-unenveloped-blob");
        assert_eq!(loaded.sections.len(), doc.sections.len());

        let tokens = store
            .get_tokens(&id)
            .unwrap()
            .expect("a pre-ADR-019 unenveloped tokens blob must still be readable");
        assert_eq!(tokens.len(), doc.token_stream.len());
    }

    /// Same as above, but for the `get_tokens` fallback path (no
    /// `<id>.tokens.json` present at all, so it falls back to loading the
    /// full document and extracting `token_stream`) — confirms that
    /// fallback also tolerates a pre-ADR-019 unenveloped `<id>.json`.
    #[test]
    fn pre_adr019_unenveloped_doc_blob_readable_via_get_tokens_fallback() {
        let (dir, store) = open_test_store();
        let doc = doc_with_title("legacy-unenveloped-fallback");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let storage = dir.path().join("storage");
        let doc_path = storage.join(format!("{id}.json"));
        let tokens_path = storage.join(format!("{id}.tokens.json"));

        std::fs::write(&doc_path, serde_json::to_vec(&doc).unwrap()).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&doc_path)).unwrap();
        // No tokens file at all, forcing get_tokens's document-fallback path.
        std::fs::remove_file(&tokens_path).unwrap();
        std::fs::remove_file(checksum_sidecar_path(&tokens_path)).unwrap();

        let tokens = store
            .get_tokens(&id)
            .unwrap()
            .expect("fallback must still work against a pre-ADR-019 unenveloped doc blob");
        assert_eq!(tokens.len(), doc.token_stream.len());
    }

    /// `deserialize_ir_blob` directly, at the unit level, for the two
    /// legacy shapes IR blobs actually take on disk: a bare JSON object
    /// (`Document`) and a bare JSON array (`Vec<Token>`) — neither has an
    /// `"ir_version"` key, and `serde_json::Value::get("ir_version")` must
    /// return `None` for both (not panic, not misparse) so the fallback
    /// branch in `deserialize_ir_blob` is reached correctly regardless of
    /// whether the legacy payload was an object or an array.
    #[test]
    fn deserialize_ir_blob_falls_back_correctly_for_both_object_and_array_legacy_shapes() {
        let doc = doc_with_title("unit-level-legacy-object");
        let doc_bytes = serde_json::to_vec(&doc).unwrap();
        let decoded: gist_model::Document = deserialize_ir_blob(&doc_bytes).unwrap();
        assert_eq!(decoded.metadata.title, "unit-level-legacy-object");

        let tokens_bytes = serde_json::to_vec(&doc.token_stream).unwrap();
        let decoded_tokens: Vec<gist_model::Token> = deserialize_ir_blob(&tokens_bytes).unwrap();
        assert_eq!(decoded_tokens.len(), doc.token_stream.len());
    }

    /// Backward-compatibility policy for additive fields (ADR-019): a JSON
    /// blob that predates a field's addition — lacking the key entirely —
    /// must still deserialize cleanly via that field's `#[serde(default)]`.
    /// `Metadata::source_copy_ref` (ADR-006, added after `Metadata` already
    /// existed) is a real, already-shipped example of exactly this
    /// scenario, not a hypothetical one: this locks in that a `Metadata`
    /// JSON object with no `source_copy_ref` key at all (the literal shape
    /// every `Metadata` blob had before ADR-006 landed) still deserializes,
    /// defaulting to `None`.
    #[test]
    fn metadata_json_missing_a_field_added_after_the_fact_deserializes_via_serde_default() {
        let json = serde_json::json!({
            "title": "pre-ADR-006 metadata",
            "author": null,
            "source_type": "txt",
            "source_ref": "/tmp/old.txt",
            // Deliberately no "source_copy_ref" key at all.
            "import_date": null,
            "language": null,
            "word_count": 42,
        });
        let meta: gist_model::Metadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.title, "pre-ADR-006 metadata");
        assert_eq!(meta.source_copy_ref, None);
    }

    // ── Block::Table forward-compat evidence (ADR-019 addendum, M6/R3) ───

    /// The IR types exactly as a pre-table binary knew them: `Block` has no
    /// `Table` variant. Everything else is unchanged and reused from
    /// `gist_model`. This is what "an older binary" deserializes into.
    mod pre_table {
        use serde::Deserialize;

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub enum Block {
            Heading {
                level: u8,
                text: String,
            },
            Paragraph {
                runs: Vec<gist_model::TextRun>,
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

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub struct Section {
            pub id: String,
            pub heading: Option<(u8, String)>,
            pub blocks: Vec<Block>,
        }

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub struct Document {
            pub id: String,
            pub metadata: gist_model::Metadata,
            pub sections: Vec<Section>,
            pub token_stream: Vec<gist_model::Token>,
        }
    }

    fn doc_with_table(title: &str) -> gist_model::Document {
        let section = gist_model::Section {
            id: "s0".to_string(),
            heading: None,
            blocks: vec![
                gist_model::Block::Paragraph {
                    runs: vec![gist_model::TextRun::plain("Before the table.")],
                },
                gist_model::Block::Table {
                    rows: vec![
                        vec!["Fruit".into(), "Colour".into()],
                        vec!["Zucchinimarrow".into(), "Green".into()],
                    ],
                    header_row: true,
                    spans: vec![],
                },
            ],
        };
        gist_model::Document::new(gist_model::Metadata::minimal(title), vec![section])
    }

    /// EVIDENCE (the problem): had `Block::Table` shipped *without* an
    /// `ir_version` bump, an older binary would hit serde's unknown-variant
    /// error on the whole document blob. This hand-writes exactly that
    /// scenario (table payload stamped v1) and decodes it with the
    /// pre-table types.
    #[test]
    fn without_a_version_bump_an_old_binary_fails_with_an_opaque_json_error() {
        let doc = doc_with_table("unbumped");
        let blob = serialize_ir_blob(&doc, IR_VERSION_BASE).unwrap(); // what "no bump" would write
        let result =
            deserialize_ir_blob_with_max::<pre_table::Document>(blob.as_bytes(), IR_VERSION_BASE);
        match result {
            Err(StoreError::Serde(e)) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("unknown variant `Table`"),
                    "unexpected message: {msg}"
                );
            }
            other => panic!("expected an opaque Json error, got {other:?}"),
        }
    }

    /// EVIDENCE (the decision): with the conditional bump, the table
    /// document is stamped v2 and the same old reader gets the typed,
    /// designed `IrVersionTooNew` instead.
    #[test]
    fn with_the_conditional_bump_an_old_binary_gets_a_typed_version_error() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_table("bumped");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let raw = std::fs::read(store.storage_dir().join(format!("{id}.json"))).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["ir_version"], serde_json::json!(IR_VERSION_TABLES));

        let old_reader = deserialize_ir_blob_with_max::<pre_table::Document>(&raw, IR_VERSION_BASE);
        assert!(
            matches!(
                old_reader,
                Err(StoreError::IrVersionTooNew {
                    found: 2,
                    expected: 1
                })
            ),
            "{old_reader:?}"
        );
    }

    /// A document with NO table is still stamped v1, so an old binary reads
    /// it exactly as before: the bump has zero blast radius on existing
    /// kinds of content.
    #[test]
    fn documents_without_tables_stay_v1_and_old_binaries_still_read_them() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_title("no-table");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let raw = std::fs::read(store.storage_dir().join(format!("{id}.json"))).unwrap();
        let old: pre_table::Document =
            deserialize_ir_blob_with_max(&raw, IR_VERSION_BASE).expect("old binary reads v1");
        assert_eq!(old.metadata.title, "no-table");
    }

    /// The token-stream blob of a table document stays v1 (tokens contain
    /// no `Block`), so an old binary can still run RSVP on it even though it
    /// cannot open the document blob for the flow view.
    #[test]
    fn tokens_blob_of_a_table_document_is_v1_and_readable_by_old_binaries() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_table("tokens-v1");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let raw = std::fs::read(store.storage_dir().join(format!("{id}.tokens.json"))).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["ir_version"], serde_json::json!(IR_VERSION_BASE));
        let tokens: Vec<gist_model::Token> =
            deserialize_ir_blob_with_max(&raw, IR_VERSION_BASE).unwrap();
        assert_eq!(tokens.len(), doc.token_stream.len());
    }

    /// The current binary round-trips a table document through the store
    /// (document, tokens) and FTS finds a word that exists only inside a
    /// table cell.
    #[test]
    fn table_document_round_trips_and_cell_text_is_searchable() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_table("roundtrip");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();

        let loaded = store.get_item(&id).unwrap().expect("item");
        match &loaded.sections[0].blocks[1] {
            gist_model::Block::Table {
                rows, header_row, ..
            } => {
                assert!(*header_row);
                assert_eq!(rows[1], vec!["Zucchinimarrow", "Green"]);
            }
            other => panic!("expected a table, got {other:?}"),
        }
        assert_eq!(
            store.get_tokens(&id).unwrap().unwrap().len(),
            doc.token_stream.len()
        );
        let hits = store.search_items("zucchinimarrow", 10).unwrap();
        assert_eq!(hits, vec![id]);
    }

    /// Libraries written before this change keep loading: a bare legacy blob
    /// and an explicit v1 envelope both decode with the table-aware types.
    #[test]
    fn legacy_and_v1_blobs_still_load_with_table_aware_types() {
        let doc = doc_with_title("legacy-after-tables");
        let bare = serde_json::to_vec(&doc).unwrap();
        let v1 = serialize_ir_blob(&doc, IR_VERSION_BASE).unwrap();
        for bytes in [bare.as_slice(), v1.as_bytes()] {
            let loaded: gist_model::Document = deserialize_ir_blob(bytes).unwrap();
            assert_eq!(loaded.metadata.title, "legacy-after-tables");
        }
    }

    // ── Merged-cell `spans` forward-compat evidence (ADR-019 addendum 2, M7/R7) ──

    /// The IR types exactly as an M6 (table-aware, span-unaware) binary knew
    /// them: `Block::Table { rows, header_row }` with NO `spans` field.
    mod pre_spans {
        use serde::Deserialize;

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub enum Block {
            Heading {
                level: u8,
                text: String,
            },
            Paragraph {
                runs: Vec<gist_model::TextRun>,
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
            Table {
                rows: Vec<Vec<String>>,
                header_row: bool,
            },
        }

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub struct Section {
            pub id: String,
            pub heading: Option<(u8, String)>,
            pub blocks: Vec<Block>,
        }

        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        pub struct Document {
            pub id: String,
            pub metadata: gist_model::Metadata,
            pub sections: Vec<Section>,
            pub token_stream: Vec<gist_model::Token>,
        }
    }

    fn doc_with_merged_table(title: &str) -> gist_model::Document {
        let section = gist_model::Section {
            id: "s0".to_string(),
            heading: None,
            blocks: vec![gist_model::Block::Table {
                rows: vec![
                    vec!["Sales".into(), "".into(), "Notes".into()],
                    vec!["North".into(), "100".into(), "Strong".into()],
                ],
                header_row: true,
                spans: vec![gist_model::CellSpan {
                    row: 0,
                    col: 0,
                    rowspan: 1,
                    colspan: 2,
                }],
            }],
        };
        gist_model::Document::new(gist_model::Metadata::minimal(title), vec![section])
    }

    /// EVIDENCE: a blob with merged-cell spans is still `ir_version` 2 (a
    /// table document), and an M6 binary — which knows `Table` but not
    /// `spans` — decodes it with its grid intact (serde ignores the unknown
    /// field). So no further version bump is needed.
    #[test]
    fn an_m6_binary_reads_a_spans_blob_with_the_grid_intact_so_no_bump_is_needed() {
        let doc = doc_with_merged_table("spans-fwd");
        assert_eq!(required_ir_version(&doc), IR_VERSION_TABLES);
        let blob = serialize_ir_blob(&doc, required_ir_version(&doc)).unwrap();
        assert!(blob.contains("\"spans\""), "the new field is in the blob");
        let old: pre_spans::Document =
            deserialize_ir_blob_with_max(blob.as_bytes(), IR_VERSION_TABLES)
                .expect("M6 binary decodes a spans blob");
        match &old.sections[0].blocks[0] {
            pre_spans::Block::Table { rows, header_row } => {
                assert!(*header_row);
                assert_eq!(rows[0], vec!["Sales", "", "Notes"]);
                assert_eq!(rows[1], vec!["North", "100", "Strong"]);
            }
            other => panic!("expected a table, got {other:?}"),
        }
    }

    /// The reverse direction: an M6-written table blob (no `spans` key at
    /// all) loads in this binary with empty spans, and an unmerged table
    /// serialises byte-identically to the M6 shape.
    #[test]
    fn m6_table_blobs_load_with_empty_spans_and_unmerged_output_is_unchanged() {
        let m6 = r#"{"ir_version":2,"payload":{"id":"d1","metadata":{"title":"t","author":null,"source_type":"","source_ref":null,"import_date":null,"language":null,"word_count":0},"sections":[{"id":"s0","heading":null,"blocks":[{"Table":{"rows":[["a","b"]],"header_row":false}}]}],"token_stream":[]}}"#;
        let doc: gist_model::Document = deserialize_ir_blob(m6.as_bytes()).unwrap();
        match &doc.sections[0].blocks[0] {
            gist_model::Block::Table { spans, .. } => assert!(spans.is_empty()),
            other => panic!("expected a table, got {other:?}"),
        }
        let unmerged = doc_with_table("plain");
        let blob = serialize_ir_blob(&unmerged, required_ir_version(&unmerged)).unwrap();
        assert!(!blob.contains("spans"), "unmerged tables omit the field");
    }

    #[test]
    fn merged_table_document_round_trips_through_the_store_and_search() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_merged_table("merged-roundtrip");
        let id = doc.id.clone();
        store.insert_item(&doc).unwrap();
        let loaded = store.get_item(&id).unwrap().expect("item");
        match &loaded.sections[0].blocks[0] {
            gist_model::Block::Table { spans, .. } => assert_eq!(spans.len(), 1),
            other => panic!("expected a table, got {other:?}"),
        }
    }

    // ── ADR-021: reading-state model (schema v7) ─────────────────────────

    fn doc_with_words(title: &str, source_type: &str, words: usize) -> gist_model::Document {
        let text = (0..words)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let section = gist_model::Section {
            id: "s0".to_string(),
            heading: None,
            blocks: vec![gist_model::Block::Paragraph {
                runs: vec![gist_model::TextRun::plain(text)],
            }],
        };
        let mut meta = gist_model::Metadata::minimal(title);
        meta.source_type = source_type.to_string();
        gist_model::Document::new(meta, vec![section])
    }

    #[test]
    fn newer_schema_is_rejected_with_schema_too_new() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("future.db");
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
                .unwrap();
        }
        match Store::open(&db, &dir.path().join("s")) {
            Err(StoreError::SchemaTooNew { found, expected }) => {
                assert_eq!(found, SCHEMA_VERSION + 1);
                assert_eq!(expected, SCHEMA_VERSION);
            }
            other => panic!("expected SchemaTooNew, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn never_opened_item_has_null_last_opened_and_zero_progress() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_words("Fresh", "txt", 10);
        store.insert_item(&doc).unwrap();
        let item = store.get_item_by_id(&doc.id).unwrap().unwrap();
        assert_eq!(item.last_opened_at, None);
        assert_eq!(item.progress_fraction, 0.0);
        assert_eq!(item.source_type, "txt");
    }

    #[test]
    fn mark_item_opened_round_trips_is_idempotent_and_ignores_unknown_ids() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_words("Opened", "epub", 5);
        store.insert_item(&doc).unwrap();
        let t1 = store.mark_item_opened(&doc.id).unwrap();
        let got = store.get_item_by_id(&doc.id).unwrap().unwrap();
        assert_eq!(got.last_opened_at, Some(t1));
        let t2 = store.mark_item_opened(&doc.id).unwrap();
        assert!(t2 >= t1);
        assert_eq!(
            store
                .get_item_by_id(&doc.id)
                .unwrap()
                .unwrap()
                .last_opened_at,
            Some(t2)
        );
        // Unknown id: no error, no row created.
        store.mark_item_opened("no-such-id").unwrap();
        assert_eq!(store.list_items(0, 10).unwrap().len(), 1);
        // The list query reports it too.
        assert_eq!(store.list_items(0, 10).unwrap()[0].last_opened_at, Some(t2));
    }

    #[test]
    fn progress_is_derived_from_rsvp_position_and_clamped() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_words("Progress", "txt", 101);
        store.insert_item(&doc).unwrap();
        // 101 words -> last word index 100 (single paragraph, no breaks).
        let last = doc
            .token_stream
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == gist_model::TokenKind::Word)
            .map(|(i, _)| i)
            .max()
            .unwrap();
        store.save_progress(&doc.id, last / 2).unwrap();
        let p = store
            .get_item_by_id(&doc.id)
            .unwrap()
            .unwrap()
            .progress_fraction;
        assert!((p - ((last / 2) as f64 / last as f64)).abs() < 1e-9, "{p}");
        // Position past the last word clamps to 1.0.
        store.save_progress(&doc.id, last + 50).unwrap();
        let p = store.list_items(0, 10).unwrap()[0].progress_fraction;
        assert_eq!(p, 1.0);
        // Position 0 is unstarted.
        store.save_progress(&doc.id, 0).unwrap();
        assert_eq!(store.list_items(0, 10).unwrap()[0].progress_fraction, 0.0);
    }

    #[test]
    fn zero_token_items_never_divide_by_zero() {
        let (_dir, store) = open_test_store();
        let doc = Document::new(Metadata::minimal("Empty"), vec![]);
        store.insert_item(&doc).unwrap();
        store.save_progress(&doc.id, 5).unwrap();
        let item = store.get_item_by_id(&doc.id).unwrap().unwrap();
        assert_eq!(item.progress_fraction, 0.0);
        assert!(item.progress_fraction.is_finite());
    }

    #[test]
    fn flow_only_item_has_last_opened_but_zero_progress() {
        // ADR-021 accepted limitation: opened (flow view) but no RSVP position.
        let (_dir, store) = open_test_store();
        let doc = doc_with_words("FlowOnly", "pdf", 50);
        store.insert_item(&doc).unwrap();
        store.mark_item_opened(&doc.id).unwrap();
        let item = store.get_item_by_id(&doc.id).unwrap().unwrap();
        assert!(item.last_opened_at.is_some());
        assert_eq!(item.progress_fraction, 0.0);
    }

    #[test]
    fn source_type_is_normalised_and_reported_by_every_listing() {
        let (_dir, store) = open_test_store();
        let a = doc_with_words("A", "docx:tracked-changes", 3);
        let b = doc_with_words("B", "web", 3);
        store.insert_item(&a).unwrap();
        store.insert_item(&b).unwrap();
        let coll = store.create_collection("C").unwrap();
        store.add_item_to_collection(&a.id, &coll).unwrap();
        store.add_tag(&a.id, "t").unwrap();

        let by_id = |items: Vec<LibraryItem>, id: &str| {
            items.into_iter().find(|i| i.id == id).unwrap().source_type
        };
        assert_eq!(by_id(store.list_items(0, 10).unwrap(), &a.id), "docx");
        assert_eq!(by_id(store.list_items(0, 10).unwrap(), &b.id), "web");
        assert_eq!(
            by_id(store.list_items_in_collection(&coll).unwrap(), &a.id),
            "docx"
        );
        assert_eq!(by_id(store.list_items_by_tag("t").unwrap(), &a.id), "docx");
        assert_eq!(
            store.get_item_by_id(&a.id).unwrap().unwrap().source_type,
            "docx"
        );
    }

    #[test]
    fn removal_cascades_progress_and_leaves_no_reading_state() {
        let (_dir, store) = open_test_store();
        let doc = doc_with_words("Gone", "txt", 20);
        store.insert_item(&doc).unwrap();
        store.save_progress(&doc.id, 3).unwrap();
        store.mark_item_opened(&doc.id).unwrap();
        store.remove_items(std::slice::from_ref(&doc.id)).unwrap();
        assert!(store.get_item_by_id(&doc.id).unwrap().is_none());
        assert_eq!(store.get_progress(&doc.id).unwrap(), 0);
        // mark on a removed id is a harmless no-op.
        store.mark_item_opened(&doc.id).unwrap();
        assert!(store.list_items(0, 10).unwrap().is_empty());
    }

    #[test]
    fn item_row_queries_use_the_token_index() {
        // The derived-progress subquery must be an index seek, not a scan of
        // the whole tokens table (ADR-021 query-cost reasoning).
        let (_dir, store) = open_test_store();
        let conn = store.conn.lock().unwrap_or_else(|p| p.into_inner());
        let sql = format!("EXPLAIN QUERY PLAN {LIBRARY_ITEM_SELECT}");
        let mut stmt = conn.prepare(&sql).unwrap();
        let plan: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            plan.iter().any(|p| p.contains("idx_tokens_item_idx")),
            "plan: {plan:?}"
        );
    }
}
