//! Linux key custody for GIST (ADR-011/014, `docs/linux-development-plan.md` D4).
//!
//! The AES-256 content key lives in the user's Secret Service keyring
//! (GNOME Keyring, KWallet's Secret Service bridge, KeePassXC, ...), never in
//! the library directory.
//!
//! `gist_store::KeyProvider::get_or_create_key` is infallible, so this type
//! resolves the key once, in [`LinuxKeyProvider::open`], where every failure is
//! a typed [`KeyError`]. A provider that exists therefore always holds a valid
//! key. Policy, shared with the Windows and Android providers:
//!
//! * **Fail closed.** A locked, missing, denied or corrupt key service is an
//!   error. A key is never regenerated because the existing one could not be
//!   read, and nothing in the keyring is ever deleted or overwritten.
//! * **Race-safe creation.** Secret Service cannot enforce uniqueness, so
//!   creation is serialised across processes with an advisory lock file, and
//!   the entry is re-read after creation. If non-cooperating writers still
//!   leave several entries, the oldest (then lowest id) wins deterministically.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;

/// Typed failures, so a shell can tell "keyring locked" from "no keyring" from
/// "stored key is damaged" and offer a different action for each.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// No Secret Service on the session bus (headless, minimal desktop,
    /// sandbox without the permission).
    #[error("no Secret Service key store is available: {0}")]
    Unavailable(String),
    /// The keyring exists but is locked and was not unlocked.
    #[error("the key store is locked")]
    Locked,
    /// An unlock prompt was dismissed or the service refused access.
    #[error("access to the key store was denied")]
    Denied,
    /// An entry exists but is not a 32-byte key. It is left untouched.
    #[error("the stored key is corrupt (expected {KEY_LEN} bytes, found {0})")]
    Corrupt(usize),
    /// The creation lock file could not be created or locked.
    #[error("could not take the key creation lock: {0}")]
    Lock(String),
    /// Any other key-store failure.
    #[error("key store error: {0}")]
    Store(String),
}

/// One stored secret, as the keyring reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Creation time in seconds since the epoch.
    pub created: u64,
    /// Stable, unique id (the D-Bus object path for Secret Service).
    pub id: String,
    pub secret: Vec<u8>,
}

/// The slice of a keyring this module needs. Implemented by the Secret Service
/// backend and by in-memory fakes in tests. There is deliberately no delete or
/// replace.
pub trait SecretStore: Send + Sync {
    /// Every entry carrying GIST's content-key attributes.
    fn list(&self) -> Result<Vec<Entry>, KeyError>;
    /// Append a new entry (never replaces an existing one).
    fn add(&self, secret: &[u8]) -> Result<(), KeyError>;
}

/// Resolved content key. Debug output never includes key material.
pub struct LinuxKeyProvider {
    key: Zeroizing<[u8; KEY_LEN]>,
}

impl std::fmt::Debug for LinuxKeyProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxKeyProvider").finish_non_exhaustive()
    }
}

impl LinuxKeyProvider {
    /// Read the content key from `store`, creating it on first use.
    /// `lock_path` is a file in the user's data directory (not a secret); all
    /// processes that may create the key must use the same path.
    pub fn open(store: &dyn SecretStore, lock_path: &Path) -> Result<Self, KeyError> {
        let _guard = CreationLock::acquire(lock_path)?;

        if let Some(key) = oldest_key(store.list()?)? {
            return Ok(Self { key });
        }

        let mut candidate = Zeroizing::new([0u8; KEY_LEN]);
        getrandom::fill(candidate.as_mut_slice())
            .map_err(|e| KeyError::Store(format!("random source failed: {e}")))?;
        store.add(candidate.as_slice())?;

        // Re-read rather than trust our candidate: if a non-cooperating writer
        // got in first, converge on whichever entry wins deterministically.
        let key = oldest_key(store.list()?)?.ok_or_else(|| {
            KeyError::Store("the key store accepted the key but did not return it".into())
        })?;
        Ok(Self { key })
    }
}

impl gist_store::KeyProvider for LinuxKeyProvider {
    fn get_or_create_key(&self) -> [u8; KEY_LEN] {
        *self.key
    }
}

/// Oldest entry wins (ties broken by id); `None` if there are none. A winner of
/// the wrong length is an error, never skipped, so a damaged entry cannot be
/// papered over by a newer one.
fn oldest_key(mut entries: Vec<Entry>) -> Result<Option<Zeroizing<[u8; KEY_LEN]>>, KeyError> {
    entries.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
    let Some(first) = entries.into_iter().next() else {
        return Ok(None);
    };
    let secret = Zeroizing::new(first.secret);
    let len = secret.len();
    let bytes: [u8; KEY_LEN] = secret
        .as_slice()
        .try_into()
        .map_err(|_| KeyError::Corrupt(len))?;
    Ok(Some(Zeroizing::new(bytes)))
}

/// Exclusive advisory lock (`flock`) held while the key is looked up/created.
struct CreationLock(File);

impl CreationLock {
    fn acquire(path: &Path) -> Result<Self, KeyError> {
        let lock_err = |e: std::io::Error| KeyError::Lock(e.to_string());
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(lock_err)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(lock_err)?;
        file.lock().map_err(lock_err)?;
        Ok(Self(file))
    }
}

impl Drop for CreationLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Where the creation lock file lives for a given data root.
pub fn lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keys").join("content-key.lock")
}

// ── Secret Service backend ────────────────────────────────────────────────

const ATTRIBUTES: [(&str, &str); 3] = [
    ("xdg:schema", "org.gist.ContentKey"),
    ("application", "gist"),
    ("purpose", "content-key-v1"),
];

/// Secret Service (D-Bus) backend over the user's default collection.
pub struct SecretServiceStore {
    // Blocking connection; secret-service ties items to its lifetime, so each
    // call reconnects. Key access happens once per launch, not per read.
    _private: (),
}

impl SecretServiceStore {
    pub fn new() -> Self {
        Self { _private: () }
    }

    fn with_collection<T>(
        &self,
        f: impl FnOnce(&secret_service::blocking::Collection<'_>) -> Result<T, KeyError>,
    ) -> Result<T, KeyError> {
        use secret_service::blocking::SecretService;
        use secret_service::EncryptionType;

        let service = SecretService::connect(EncryptionType::Dh).map_err(map_error)?;
        let collection = service.get_default_collection().map_err(map_error)?;
        if collection.is_locked().map_err(map_error)? {
            collection.unlock().map_err(map_error)?;
        }
        f(&collection)
    }
}

impl Default for SecretServiceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for SecretServiceStore {
    fn list(&self) -> Result<Vec<Entry>, KeyError> {
        self.with_collection(|collection| {
            let items = collection
                .search_items(ATTRIBUTES.into_iter().collect())
                .map_err(map_error)?;
            items
                .iter()
                .map(|item| {
                    item.ensure_unlocked().map_err(map_error)?;
                    Ok(Entry {
                        created: item.get_created().map_err(map_error)?,
                        id: item.item_path.to_string(),
                        secret: item.get_secret().map_err(map_error)?,
                    })
                })
                .collect()
        })
    }

    fn add(&self, secret: &[u8]) -> Result<(), KeyError> {
        self.with_collection(|collection| {
            collection
                .create_item(
                    "GIST content key",
                    ATTRIBUTES.into_iter().collect(),
                    secret,
                    false, // never replace an existing entry
                    "application/octet-stream",
                )
                .map(|_| ())
                .map_err(map_error)
        })
    }
}

fn map_error(error: secret_service::Error) -> KeyError {
    use secret_service::Error as E;
    match error {
        E::Locked => KeyError::Locked,
        E::Prompt | E::PromptDisconnected => KeyError::Denied,
        E::Unavailable | E::NoResult => KeyError::Unavailable(error.to_string()),
        E::Zbus(_) | E::ZbusFdo(_) => KeyError::Unavailable(error.to_string()),
        other => KeyError::Store(other.to_string()),
    }
}

/// Convenience for shells: open the real keyring and return an `Arc` ready for
/// `gist_core::Core::init_encrypted`.
pub fn open_default(data_dir: &Path) -> Result<Arc<LinuxKeyProvider>, KeyError> {
    LinuxKeyProvider::open(&SecretServiceStore::new(), &lock_path(data_dir)).map(Arc::new)
}

#[cfg(test)]
mod tests;
