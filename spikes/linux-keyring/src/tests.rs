use super::*;
use gist_store::KeyProvider;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Default)]
struct FakeStore {
    entries: Mutex<Vec<Entry>>,
    next: AtomicU64,
    list_error: Mutex<Option<fn() -> KeyError>>,
    add_error: Mutex<Option<fn() -> KeyError>>,
    clock: AtomicU64,
    /// Widens the read-then-write window so unlocked callers interleave.
    list_delay: Mutex<Option<std::time::Duration>>,
}

impl FakeStore {
    fn with(entries: Vec<Entry>) -> Self {
        let store = Self::default();
        *store.entries.lock().unwrap() = entries;
        store
    }
    fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
    fn snapshot(&self) -> Vec<Entry> {
        self.entries.lock().unwrap().clone()
    }
}

impl SecretStore for FakeStore {
    fn list(&self) -> Result<Vec<Entry>, KeyError> {
        if let Some(make) = *self.list_error.lock().unwrap() {
            return Err(make());
        }
        let snapshot = self.snapshot();
        if let Some(delay) = *self.list_delay.lock().unwrap() {
            std::thread::sleep(delay);
        }
        Ok(snapshot)
    }
    fn add(&self, secret: &[u8]) -> Result<(), KeyError> {
        if let Some(make) = *self.add_error.lock().unwrap() {
            return Err(make());
        }
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        self.entries.lock().unwrap().push(Entry {
            created: self.clock.load(Ordering::SeqCst),
            id: format!("/item/{n}"),
            secret: secret.to_vec(),
        });
        Ok(())
    }
}

fn entry(created: u64, id: &str, byte: u8) -> Entry {
    Entry {
        created,
        id: id.into(),
        secret: vec![byte; KEY_LEN],
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gist-keyring-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn first_open_creates_one_random_key_and_later_opens_return_it() {
    let dir = temp_dir("create");
    let store = FakeStore::default();
    let first = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    let key = first.get_or_create_key();
    assert_ne!(key, [0u8; KEY_LEN]);
    assert_eq!(store.len(), 1);

    let second = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    assert_eq!(second.get_or_create_key(), key);
    assert_eq!(store.len(), 1, "an existing key must not be re-created");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn independent_stores_get_different_keys() {
    let dir = temp_dir("distinct");
    let a = LinuxKeyProvider::open(&FakeStore::default(), &lock_path(&dir)).unwrap();
    let b = LinuxKeyProvider::open(&FakeStore::default(), &lock_path(&dir)).unwrap();
    assert_ne!(a.get_or_create_key(), b.get_or_create_key());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_existing_key_is_used_without_writing() {
    let dir = temp_dir("existing");
    let store = FakeStore::with(vec![entry(10, "/item/a", 0x42)]);
    *store.add_error.lock().unwrap() = Some(|| KeyError::Store("must not write".into()));
    let provider = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    assert_eq!(provider.get_or_create_key(), [0x42; KEY_LEN]);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_wrong_length_entry_fails_closed_and_is_left_untouched() {
    let dir = temp_dir("corrupt");
    let bad = Entry {
        created: 1,
        id: "/item/bad".into(),
        secret: vec![7; 16],
    };
    let store = FakeStore::with(vec![bad.clone()]);
    let err = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap_err();
    assert!(matches!(err, KeyError::Corrupt(16)), "{err:?}");
    assert_eq!(store.snapshot(), vec![bad], "no regenerate, no delete");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_newer_valid_entry_does_not_paper_over_a_corrupt_older_one() {
    let dir = temp_dir("corrupt-old");
    let store = FakeStore::with(vec![
        Entry {
            created: 1,
            id: "/item/old".into(),
            secret: vec![],
        },
        entry(2, "/item/new", 9),
    ]);
    let err = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap_err();
    assert!(matches!(err, KeyError::Corrupt(0)), "{err:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn duplicates_resolve_to_the_oldest_then_lowest_id() {
    let dir = temp_dir("dupes");
    let store = FakeStore::with(vec![
        entry(5, "/item/z", 3),
        entry(5, "/item/b", 2),
        entry(9, "/item/a", 1),
    ]);
    let provider = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    assert_eq!(provider.get_or_create_key(), [2; KEY_LEN]);
    assert_eq!(store.len(), 3, "duplicates are never deleted");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn keyring_failures_propagate_typed_and_create_nothing() {
    let dir = temp_dir("errors");
    for (make, check) in [
        (
            (|| KeyError::Locked) as fn() -> KeyError,
            (|e: &KeyError| matches!(e, KeyError::Locked)) as fn(&KeyError) -> bool,
        ),
        (|| KeyError::Denied, |e| matches!(e, KeyError::Denied)),
        (
            || KeyError::Unavailable("no bus".into()),
            |e| matches!(e, KeyError::Unavailable(_)),
        ),
    ] {
        let store = FakeStore::default();
        *store.list_error.lock().unwrap() = Some(make);
        let err = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap_err();
        assert!(check(&err), "{err:?}");
        assert_eq!(store.len(), 0);
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_failed_write_yields_an_error_not_an_unpersisted_key() {
    let dir = temp_dir("add-fails");
    let store = FakeStore::default();
    *store.add_error.lock().unwrap() = Some(|| KeyError::Locked);
    let err = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap_err();
    assert!(matches!(err, KeyError::Locked), "{err:?}");
    assert_eq!(store.len(), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn concurrent_first_opens_converge_on_one_key_and_one_entry() {
    let dir = temp_dir("race");
    let store = Arc::new(FakeStore::default());
    *store.list_delay.lock().unwrap() = Some(std::time::Duration::from_millis(20));
    let lock = lock_path(&dir);
    let keys: Vec<[u8; KEY_LEN]> = (0..8)
        .map(|_| {
            let (store, lock) = (Arc::clone(&store), lock.clone());
            std::thread::spawn(move || {
                LinuxKeyProvider::open(store.as_ref(), &lock)
                    .unwrap()
                    .get_or_create_key()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    assert!(keys.windows(2).all(|w| w[0] == w[1]), "all callers agree");
    assert_eq!(store.len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_unusable_lock_path_is_a_typed_error() {
    let dir = temp_dir("lock-bad");
    let blocker = dir.join("keys");
    fs::write(&blocker, b"a file where a directory is needed").unwrap();
    let err = LinuxKeyProvider::open(&FakeStore::default(), &lock_path(&dir)).unwrap_err();
    assert!(matches!(err, KeyError::Lock(_)), "{err:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn debug_output_never_contains_key_material() {
    let dir = temp_dir("debug");
    let store = FakeStore::with(vec![entry(1, "/item/a", 0xAB)]);
    let provider = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    assert_eq!(format!("{provider:?}"), "LinuxKeyProvider { .. }");
    let _ = fs::remove_dir_all(dir);
}

/// Real Secret Service round trip. Never run against a developer's own
/// keyring: needs a disposable session, e.g.
/// `dbus-run-session -- sh -c 'echo pw | gnome-keyring-daemon --unlock --components=secrets && cargo test -- --ignored'`.
#[test]
#[ignore = "needs an isolated Secret Service daemon on a private session bus"]
fn real_secret_service_round_trip_is_stable_across_opens() {
    let dir = temp_dir("real");
    let store = SecretServiceStore::new();
    let first = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    let second = LinuxKeyProvider::open(&store, &lock_path(&dir)).unwrap();
    assert_eq!(first.get_or_create_key(), second.get_or_create_key());
    assert_eq!(store.list().unwrap().len(), 1);
    let _ = fs::remove_dir_all(dir);
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn encrypted_item_is_unreadable_on_disk_and_readable_after_reopening_with_the_stored_key() {
    use gist_core::{Core, NullObserver};

    let dir = temp_dir("e2e");
    let storage = dir.join("storage");
    fs::create_dir_all(&storage).unwrap();
    let db = dir.join("gist.sqlite3");
    let keyring = FakeStore::default();
    let marker = "zyxwvutsrq";
    let source = dir.join("source.txt");
    fs::write(&source, format!("The {marker} lighthouse keeper waited.")).unwrap();

    let id = {
        let provider = Arc::new(LinuxKeyProvider::open(&keyring, &lock_path(&dir)).unwrap());
        let core = Core::init_with_read_key(&db, &storage, provider.clone()).unwrap();
        let id = core.import_file(&source, &NullObserver).unwrap();
        let outcomes = core.encrypt_items(std::slice::from_ref(&id), provider);
        assert!(outcomes[0].result.is_ok(), "{:?}", outcomes[0].result);
        id
    };

    // `originals/` is deliberately left plaintext by `encrypt_item` (ADR-014,
    // tracked gap A5); the IR blobs are what it protects.
    for file in files_under(&storage)
        .into_iter()
        .filter(|f| !f.components().any(|c| c.as_os_str() == "originals"))
    {
        let bytes = fs::read(&file).unwrap();
        assert!(
            !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
            "plaintext found in {}",
            file.display()
        );
    }

    assert!(!files_under(&storage).is_empty());

    // New process-equivalent: a fresh provider resolved from the same keyring.
    let provider = Arc::new(LinuxKeyProvider::open(&keyring, &lock_path(&dir)).unwrap());
    let core = Core::init_with_read_key(&db, &storage, provider).unwrap();
    assert!(core.get_document(&id).unwrap().contains(marker));

    // Losing the key is not silently repaired: a different keyring cannot read it.
    let other = Arc::new(LinuxKeyProvider::open(&FakeStore::default(), &lock_path(&dir)).unwrap());
    let core = Core::init_with_read_key(&db, &storage, other).unwrap();
    assert!(core.get_document(&id).is_err());
    let _ = fs::remove_dir_all(dir);
}
