//! Wire-level tests: the real client against a fake Secret Service served by
//! zbus on a private `dbus-daemon` that the test starts itself. Never touches
//! the user's session bus or keyring.
//!
//! This proves the client follows the Secret Service spec *as read here*; it
//! does not replace running against GNOME Keyring / KWallet / KeePassXC.

use super::*;
use crate::dh::{KeyPair, SessionKey};
use gist_store::KeyProvider;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use zbus::object_server::SignalEmitter;
use zbus::{ObjectPath, OwnedObjectPath, OwnedVariant, Variant};

const COLLECTION: &str = "/org/freedesktop/secrets/collection/login";
const SESSION: &str = "/org/freedesktop/secrets/session/s1";
const PROMPT: &str = "/org/freedesktop/secrets/prompt/p1";
const MAX_ITEMS: usize = 4;

#[derive(Clone, Copy, PartialEq)]
enum Lock {
    Open,
    /// Locked and the service cannot prompt.
    LockedNoPrompt,
    /// Locked; unlocking shows a prompt the user accepts.
    PromptAccept,
    /// Locked; unlocking shows a prompt the user dismisses.
    PromptDismiss,
    /// Accepts calls but never answers them.
    Hang,
}

struct FakeItem {
    created: u64,
    secret: Vec<u8>,
}

struct State {
    lock: Lock,
    items: Vec<FakeItem>,
    key: Option<SessionKey>,
}

type Shared = Arc<Mutex<State>>;

struct Service(Shared);
struct Collection(Shared);
struct Item(Shared, usize);
struct Prompt(Shared);

fn path(s: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(s).unwrap()
}

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Service {
    fn open_session(
        &self,
        algorithm: &str,
        input: Variant<'_>,
    ) -> zbus::fdo::Result<(OwnedVariant, OwnedObjectPath)> {
        if algorithm != dh::ALGORITHM {
            return Err(zbus::fdo::Error::NotSupported(algorithm.into()));
        }
        let client_public =
            Vec::<u8>::try_from(input).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let pair = KeyPair::generate().map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let key = pair
            .derive(&client_public)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        self.0.lock().unwrap().key = Some(key);
        let public = OwnedVariant::try_from(Variant::from(pair.public_bytes().to_vec())).unwrap();
        Ok((public, path(SESSION)))
    }

    fn read_alias(&self, _name: &str) -> OwnedObjectPath {
        if self.0.lock().unwrap().lock == Lock::Hang {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
        path(COLLECTION)
    }

    fn unlock(&self, objects: Vec<ObjectPath<'_>>) -> (Vec<OwnedObjectPath>, OwnedObjectPath) {
        let state = self.0.lock().unwrap();
        match state.lock {
            Lock::Open => (
                objects.iter().map(|o| path(o.as_str())).collect(),
                path("/"),
            ),
            Lock::LockedNoPrompt => (vec![], path("/")),
            Lock::PromptAccept | Lock::PromptDismiss => (vec![], path(PROMPT)),
            Lock::Hang => unreachable!("read_alias hangs first"),
        }
    }
}

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl Collection {
    fn search_items(&self, attributes: HashMap<String, String>) -> Vec<OwnedObjectPath> {
        assert_eq!(
            attributes.get("application").map(String::as_str),
            Some("gist")
        );
        let n = self.0.lock().unwrap().items.len();
        (0..n).map(|i| path(&format!("{COLLECTION}/{i}"))).collect()
    }

    fn create_item(
        &self,
        properties: HashMap<String, OwnedVariant>,
        secret: (OwnedObjectPath, Vec<u8>, Vec<u8>, String),
        replace: bool,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
        assert!(!replace, "the client must never replace an existing entry");
        assert!(properties.contains_key("org.freedesktop.Secret.Item.Attributes"));
        let mut state = self.0.lock().unwrap();
        let plain = state
            .key
            .as_ref()
            .ok_or_else(|| zbus::fdo::Error::Failed("no session".into()))?
            .decrypt(&secret.1, &secret.2)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        if state.items.len() >= MAX_ITEMS {
            return Err(zbus::fdo::Error::Failed("too many items".into()));
        }
        let created = 1000 + state.items.len() as u64;
        state.items.push(FakeItem {
            created,
            secret: plain.to_vec(),
        });
        let index = state.items.len() - 1;
        Ok((path(&format!("{COLLECTION}/{index}")), path("/")))
    }
}

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl Item {
    fn get_secret(
        &self,
        _session: ObjectPath<'_>,
    ) -> zbus::fdo::Result<(OwnedObjectPath, Vec<u8>, Vec<u8>, String)> {
        let state = self.0.lock().unwrap();
        let item = state
            .items
            .get(self.1)
            .ok_or_else(|| zbus::fdo::Error::Failed("no such item".into()))?;
        let (iv, ct) = state
            .key
            .as_ref()
            .unwrap()
            .encrypt(&item.secret)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok((path(SESSION), iv, ct, "application/octet-stream".into()))
    }

    #[zbus(property)]
    fn created(&self) -> u64 {
        self.0
            .lock()
            .unwrap()
            .items
            .get(self.1)
            .map(|i| i.created)
            .unwrap_or(0)
    }
}

#[zbus::interface(name = "org.freedesktop.Secret.Prompt")]
impl Prompt {
    async fn prompt(
        &self,
        _window_id: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let dismissed = self.0.lock().unwrap().lock == Lock::PromptDismiss;
        if !dismissed {
            self.0.lock().unwrap().lock = Lock::Open;
        }
        Self::completed(&emitter, dismissed, Variant::from(false))
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    #[zbus(signal)]
    async fn completed(
        emitter: &SignalEmitter<'_>,
        dismissed: bool,
        result: Variant<'_>,
    ) -> zbus::Result<()>;
}

/// Private bus daemon, killed on drop.
struct Bus {
    child: Child,
    address: String,
}

impl Bus {
    fn start() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut line)
            .ok()?;
        Some(Self {
            child,
            address: line.trim().to_string(),
        })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts a bus (skipping when `dbus-daemon` is absent unless
/// `GIST_REQUIRE_DBUS=1`) and optionally a fake service on it.
fn setup(state: Option<(Lock, Vec<Vec<u8>>)>) -> Option<(Bus, Shared)> {
    let Some(bus) = Bus::start() else {
        assert_ne!(
            std::env::var("GIST_REQUIRE_DBUS").as_deref(),
            Ok("1"),
            "dbus-daemon required"
        );
        eprintln!("skipping: dbus-daemon not available");
        return None;
    };
    let (lock, seeded) = state.clone().unwrap_or((Lock::Open, vec![]));
    let shared: Shared = Arc::new(Mutex::new(State {
        lock,
        items: seeded
            .into_iter()
            .enumerate()
            .map(|(i, secret)| FakeItem {
                created: 10 + i as u64,
                secret,
            })
            .collect(),
        key: None,
    }));
    if state.is_some() {
        let (address, shared) = (bus.address.clone(), Arc::clone(&shared));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            zbus::block_on(async move {
                let mut builder = zbus::connection::Builder::address(address.as_str())
                    .serve_at("/org/freedesktop/secrets", Service(Arc::clone(&shared)))
                    .serve_at(COLLECTION, Collection(Arc::clone(&shared)))
                    .serve_at(PROMPT, Prompt(Arc::clone(&shared)));
                for i in 0..MAX_ITEMS {
                    builder =
                        builder.serve_at(format!("{COLLECTION}/{i}"), Item(Arc::clone(&shared), i));
                }
                let _conn = builder
                    .name("org.freedesktop.secrets")
                    .build()
                    .await
                    .unwrap();
                tx.send(()).unwrap();
                std::future::pending::<()>().await;
            })
        });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("fake service started");
    }
    Some((bus, shared))
}

fn lock_in(label: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("gist-wire-{label}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    (dir.clone(), lock_path(&dir))
}

#[test]
fn first_open_stores_an_encrypted_key_that_the_service_can_read_back() {
    let Some((bus, shared)) = setup(Some((Lock::Open, vec![]))) else {
        return;
    };
    let (dir, lock) = lock_in("create");
    let store = SecretServiceStore::with_bus_address(&bus.address);

    let first = LinuxKeyProvider::open(&store, &lock).unwrap();
    let key = first.get_or_create_key();
    {
        let state = shared.lock().unwrap();
        assert_eq!(state.items.len(), 1);
        assert_eq!(
            state.items[0].secret,
            key.to_vec(),
            "the service decrypted exactly our key"
        );
    }

    let second = LinuxKeyProvider::open(&store, &lock).unwrap();
    assert_eq!(
        second.get_or_create_key(),
        key,
        "a fresh session reads the same key back"
    );
    assert_eq!(shared.lock().unwrap().items.len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_existing_key_in_the_service_is_used() {
    let Some((bus, shared)) = setup(Some((Lock::Open, vec![vec![0x5A; 32]]))) else {
        return;
    };
    let (dir, lock) = lock_in("existing");
    let provider =
        LinuxKeyProvider::open(&SecretServiceStore::with_bus_address(&bus.address), &lock).unwrap();
    assert_eq!(provider.get_or_create_key(), [0x5A; 32]);
    assert_eq!(shared.lock().unwrap().items.len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_wrong_length_entry_in_the_service_fails_closed() {
    let Some((bus, shared)) = setup(Some((Lock::Open, vec![vec![1; 16]]))) else {
        return;
    };
    let (dir, lock) = lock_in("corrupt");
    let err = LinuxKeyProvider::open(&SecretServiceStore::with_bus_address(&bus.address), &lock)
        .unwrap_err();
    assert!(matches!(err, KeyError::Corrupt(16)), "{err:?}");
    assert_eq!(
        shared.lock().unwrap().items.len(),
        1,
        "nothing added or removed"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_locked_service_that_cannot_prompt_is_reported_as_locked() {
    let Some((bus, shared)) = setup(Some((Lock::LockedNoPrompt, vec![]))) else {
        return;
    };
    let (dir, lock) = lock_in("locked");
    let err = LinuxKeyProvider::open(&SecretServiceStore::with_bus_address(&bus.address), &lock)
        .unwrap_err();
    assert!(matches!(err, KeyError::Locked), "{err:?}");
    assert_eq!(shared.lock().unwrap().items.len(), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_accepted_unlock_prompt_lets_the_key_be_created() {
    let Some((bus, shared)) = setup(Some((Lock::PromptAccept, vec![]))) else {
        return;
    };
    let (dir, lock) = lock_in("prompt-ok");
    let provider =
        LinuxKeyProvider::open(&SecretServiceStore::with_bus_address(&bus.address), &lock).unwrap();
    assert_ne!(provider.get_or_create_key(), [0; 32]);
    assert_eq!(shared.lock().unwrap().items.len(), 1);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_dismissed_unlock_prompt_is_denied_and_creates_nothing() {
    let Some((bus, shared)) = setup(Some((Lock::PromptDismiss, vec![]))) else {
        return;
    };
    let (dir, lock) = lock_in("prompt-no");
    let err = LinuxKeyProvider::open(&SecretServiceStore::with_bus_address(&bus.address), &lock)
        .unwrap_err();
    assert!(matches!(err, KeyError::Denied), "{err:?}");
    assert_eq!(shared.lock().unwrap().items.len(), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_bus_with_no_secret_service_is_unavailable() {
    let Some((bus, _)) = setup(None) else { return };
    let (dir, lock) = lock_in("none");
    let err = LinuxKeyProvider::open(&SecretServiceStore::with_bus_address(&bus.address), &lock)
        .unwrap_err();
    assert!(matches!(err, KeyError::Unavailable(_)), "{err:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_unreachable_bus_address_is_unavailable() {
    let (dir, lock) = lock_in("badbus");
    let store = SecretServiceStore::with_bus_address("unix:path=/nonexistent/gist-test-bus");
    let err = LinuxKeyProvider::open(&store, &lock).unwrap_err();
    assert!(matches!(err, KeyError::Unavailable(_)), "{err:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_service_that_never_answers_times_out_as_unavailable() {
    let Some((bus, _)) = setup(Some((Lock::Hang, vec![]))) else {
        return;
    };
    let (dir, lock) = lock_in("hang");
    let store = SecretServiceStore::with_bus_address(&bus.address)
        .with_method_timeout(std::time::Duration::from_millis(300));
    let started = std::time::Instant::now();
    let err = LinuxKeyProvider::open(&store, &lock).unwrap_err();
    assert!(matches!(err, KeyError::Unavailable(_)), "{err:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let _ = fs::remove_dir_all(dir);
}
