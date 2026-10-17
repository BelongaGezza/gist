//! Encryption key handling for the GTK spike, kept free of GTK so it can be
//! tested headlessly (ADR-031, `docs/linux-development-plan.md` D4).
//!
//! Policy:
//! * The key store is **not touched at startup** unless the library already
//!   holds encrypted items; a user who never encrypts never has a key created.
//! * A library with encrypted items opens the key with `open_existing`, which
//!   never creates one: a lost key is reported, not silently replaced.
//! * A key is created only when the user explicitly encrypts an item in a
//!   library that has no encrypted items yet.
//! * Every failure is a [`KeyError`] turned into a message that says what
//!   still works and what the user can do.

use std::path::PathBuf;
use std::sync::Arc;

use gist_core::Core;
use gist_linux_keyring_spike::{lock_path, KeyError, LinuxKeyProvider, SecretStore};

const PAGE: usize = 200;

/// Where the library lives on disk.
#[derive(Clone, Debug)]
pub struct Layout {
    pub data_dir: PathBuf,
}

impl Layout {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("gist.sqlite3")
    }
    pub fn storage_dir(&self) -> PathBuf {
        self.data_dir.join("storage")
    }
    pub fn key_lock(&self) -> PathBuf {
        lock_path(&self.data_dir)
    }
}

/// A core that can read encrypted items, plus the provider that backs it.
pub struct Keyed {
    pub core: Arc<Core>,
    pub provider: Arc<LinuxKeyProvider>,
}

/// Does any library item have encrypted content? Pages through the library.
pub fn any_encrypted(core: &Core) -> Result<bool, String> {
    let mut offset = 0;
    loop {
        let page = core
            .list_items(offset, PAGE)
            .map_err(|error| format!("Could not read the library: {error}"))?;
        if page.iter().any(|item| item.content_encrypted) {
            return Ok(true);
        }
        if page.len() < PAGE {
            return Ok(false);
        }
        offset += PAGE;
    }
}

/// Open a key-backed core when one is needed.
///
/// * library has encrypted items -> existing key only (never created);
/// * else if `create` (the user is encrypting) -> create on first use;
/// * else -> `Ok(None)`: the key store is not touched at all.
pub fn open_keyed(
    layout: &Layout,
    store: &dyn SecretStore,
    library_has_encrypted: bool,
    create: bool,
) -> Result<Option<Keyed>, KeyError> {
    let provider = if library_has_encrypted {
        LinuxKeyProvider::open_existing(store, &layout.key_lock())?
    } else if create {
        LinuxKeyProvider::open(store, &layout.key_lock())?
    } else {
        return Ok(None);
    };
    let provider = Arc::new(provider);
    let core = Core::init_with_read_key(&layout.db_path(), &layout.storage_dir(), provider.clone())
        .map_err(|error| KeyError::Store(format!("could not open the library: {error}")))?;
    Ok(Some(Keyed {
        core: Arc::new(core),
        provider,
    }))
}

/// What went wrong when encrypting one item.
#[derive(Debug)]
pub enum EncryptError {
    Key(KeyError),
    Other(String),
}

/// Result of a successful encrypt: the key-backed core to switch to.
pub struct Encrypted {
    pub keyed: Keyed,
    pub already: bool,
}

/// Encrypt one item, creating the key first only if the library has none yet.
pub fn encrypt_item(
    layout: &Layout,
    store: &dyn SecretStore,
    current: &Core,
    item_id: &str,
) -> Result<Encrypted, EncryptError> {
    let has_encrypted = any_encrypted(current).map_err(EncryptError::Other)?;
    let keyed = open_keyed(layout, store, has_encrypted, true)
        .map_err(EncryptError::Key)?
        .ok_or_else(|| EncryptError::Other("no key was opened".into()))?;
    let outcome = keyed
        .core
        .encrypt_items(&[item_id.to_string()], keyed.provider.clone())
        .into_iter()
        .next()
        .ok_or_else(|| EncryptError::Other("encryption returned no result".into()))?;
    match outcome.result {
        Ok(gist_store::EncryptOutcome::Encrypted) => Ok(Encrypted {
            keyed,
            already: false,
        }),
        Ok(gist_store::EncryptOutcome::AlreadyEncrypted) => Ok(Encrypted {
            keyed,
            already: true,
        }),
        Err(error) => Err(EncryptError::Other(format!(
            "Could not encrypt the item: {error}"
        ))),
    }
}

/// Banner text for a key problem: what happened, what still works, what to do.
pub fn key_message(error: &KeyError) -> String {
    match error {
        KeyError::Unavailable(_) => "No desktop key service was found, so encryption is unavailable. \
             Unencrypted items still work. Start GNOME Keyring, KWallet or KeePassXC (Secret Service) to use encryption."
            .into(),
        KeyError::Locked => "Your key store is locked. Unlock it (usually by logging in again or opening your \
             keyring) and retry. Encrypted items cannot be opened until then."
            .into(),
        KeyError::Denied => "Access to your key store was declined. Encrypted items cannot be opened until you allow it; \
             unencrypted items still work."
            .into(),
        KeyError::Missing => "Your encrypted items need a key that is no longer in your key store. They cannot be opened, \
             and GIST will not create a replacement key because that would hide the problem. \
             Restore your keyring from a backup if you have one."
            .into(),
        KeyError::Corrupt(_) => "The stored encryption key is damaged. GIST will not replace it, so encrypted items cannot be opened. \
             Unencrypted items still work."
            .into(),
        KeyError::Lock(_) => "GIST could not take the key creation lock in its data folder. Check that the folder is writable."
            .into(),
        KeyError::Store(_) => "The key store reported an error, so encrypted items cannot be opened right now. \
             Unencrypted items still work."
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gist_linux_keyring_spike::Entry;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeStore {
        entries: Mutex<Vec<Entry>>,
        touched: AtomicUsize,
        fail_with: Mutex<Option<fn() -> KeyError>>,
    }

    impl FakeStore {
        fn touches(&self) -> usize {
            self.touched.load(Ordering::SeqCst)
        }
        fn len(&self) -> usize {
            self.entries.lock().unwrap().len()
        }
    }

    impl SecretStore for FakeStore {
        fn list(&self) -> Result<Vec<Entry>, KeyError> {
            self.touched.fetch_add(1, Ordering::SeqCst);
            if let Some(make) = *self.fail_with.lock().unwrap() {
                return Err(make());
            }
            Ok(self.entries.lock().unwrap().clone())
        }
        fn add(&self, secret: &[u8]) -> Result<(), KeyError> {
            self.touched.fetch_add(1, Ordering::SeqCst);
            let mut entries = self.entries.lock().unwrap();
            let id = format!("/item/{}", entries.len());
            entries.push(Entry {
                created: 1,
                id,
                secret: secret.to_vec(),
            });
            Ok(())
        }
    }

    struct Fixture {
        root: PathBuf,
        layout: Layout,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "gist-gtk-keys-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(root.join("storage")).unwrap();
            Self {
                layout: Layout::new(&root),
                root,
            }
        }

        fn keyless_core(&self) -> Core {
            Core::init(&self.layout.db_path(), &self.layout.storage_dir()).unwrap()
        }

        fn import(&self, core: &Core, text: &str) -> String {
            let path = self.root.join(format!("{}.txt", text.len()));
            std::fs::write(&path, text).unwrap();
            core.import_file(&path, &gist_core::NullObserver).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn the_key_store_is_not_touched_when_nothing_is_encrypted_and_the_user_is_not_encrypting() {
        let fx = Fixture::new("untouched");
        let core = fx.keyless_core();
        fx.import(&core, "just plain reading");
        let store = FakeStore::default();
        let has = any_encrypted(&core).unwrap();
        assert!(!has);
        assert!(open_keyed(&fx.layout, &store, has, false)
            .unwrap()
            .is_none());
        assert_eq!(store.touches(), 0);
    }

    #[test]
    fn encrypting_creates_a_key_then_the_item_reads_through_the_keyed_core() {
        let fx = Fixture::new("encrypt");
        let core = fx.keyless_core();
        let id = fx.import(&core, "zyxwvutsrq lighthouse");
        let store = FakeStore::default();

        let done = encrypt_item(&fx.layout, &store, &core, &id).unwrap();
        assert!(!done.already);
        assert_eq!(store.len(), 1, "one key created");
        assert!(done
            .keyed
            .core
            .get_document(&id)
            .unwrap()
            .contains("zyxwvutsrq"));
        assert!(any_encrypted(&done.keyed.core).unwrap());
        // The keyless core can no longer read it: encryption really happened.
        assert!(core.get_document(&id).is_err());
    }

    #[test]
    fn a_library_with_encrypted_items_reopens_with_the_existing_key() {
        let fx = Fixture::new("reopen");
        let core = fx.keyless_core();
        let id = fx.import(&core, "second launch reading");
        let store = FakeStore::default();
        encrypt_item(&fx.layout, &store, &core, &id).unwrap();
        drop(core);

        let fresh = fx.keyless_core();
        let has = any_encrypted(&fresh).unwrap();
        assert!(has);
        let keyed = open_keyed(&fx.layout, &store, has, false).unwrap().unwrap();
        assert!(keyed
            .core
            .get_document(&id)
            .unwrap()
            .contains("second launch"));
        assert_eq!(store.len(), 1, "no second key");
    }

    #[test]
    fn a_lost_key_is_reported_and_never_replaced() {
        let fx = Fixture::new("lost");
        let core = fx.keyless_core();
        let id = fx.import(&core, "needs the old key");
        let original = FakeStore::default();
        encrypt_item(&fx.layout, &original, &core, &id).unwrap();

        // The keyring was wiped: a different, empty store.
        let wiped = FakeStore::default();
        let fresh = fx.keyless_core();
        let has = any_encrypted(&fresh).unwrap();
        let err = open_keyed(&fx.layout, &wiped, has, false).err().unwrap();
        assert!(matches!(err, KeyError::Missing), "{err:?}");
        assert_eq!(wiped.len(), 0, "no replacement key");

        // Encrypting another item in that library must not invent a key either.
        let other = fx.import(&fresh, "another document entirely here");
        match encrypt_item(&fx.layout, &wiped, &fresh, &other) {
            Err(EncryptError::Key(KeyError::Missing)) => {}
            other => panic!("expected Missing, got {:?}", other.err()),
        }
        assert_eq!(wiped.len(), 0);
    }

    #[test]
    fn key_failures_do_not_encrypt_or_create_anything() {
        let fx = Fixture::new("failures");
        let core = fx.keyless_core();
        let id = fx.import(&core, "stays plaintext");
        for make in [
            (|| KeyError::Unavailable("no bus".into())) as fn() -> KeyError,
            || KeyError::Locked,
            || KeyError::Denied,
        ] {
            let store = FakeStore::default();
            *store.fail_with.lock().unwrap() = Some(make);
            match encrypt_item(&fx.layout, &store, &core, &id) {
                Err(EncryptError::Key(_)) => {}
                other => panic!("expected a key error, got {:?}", other.err()),
            }
            assert_eq!(store.len(), 0);
        }
        assert!(!any_encrypted(&core).unwrap());
        assert!(core.get_document(&id).unwrap().contains("stays plaintext"));
    }

    #[test]
    fn encrypting_twice_reports_already_encrypted() {
        let fx = Fixture::new("twice");
        let core = fx.keyless_core();
        let id = fx.import(&core, "encrypt me once only");
        let store = FakeStore::default();
        let first = encrypt_item(&fx.layout, &store, &core, &id).unwrap();
        let second = encrypt_item(&fx.layout, &store, &first.keyed.core, &id).unwrap();
        assert!(second.already);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn every_key_error_has_an_actionable_message_without_internals() {
        for error in [
            KeyError::Unavailable("zbus detail".into()),
            KeyError::Locked,
            KeyError::Denied,
            KeyError::Missing,
            KeyError::Corrupt(7),
            KeyError::Lock("denied".into()),
            KeyError::Store("detail".into()),
        ] {
            let text = key_message(&error);
            assert!(text.len() > 40, "{error:?}: {text}");
            assert!(!text.contains("zbus") && !text.contains("detail"), "{text}");
        }
        assert!(key_message(&KeyError::Missing).contains("will not create a replacement"));
        assert!(key_message(&KeyError::Unavailable(String::new()))
            .contains("Unencrypted items still work"));
    }
}
