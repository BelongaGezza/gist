//! Secret Service (`org.freedesktop.secrets`) client on zbus 6.
//!
//! Only what key custody needs: open a DH-encrypted session, resolve the
//! default collection, unlock (handling the interactive prompt), search items
//! by attribute, read a secret, create an item. There is no delete or replace.
//!
//! Each call opens its own connection and session: the key is resolved once
//! per launch, so reuse buys nothing. Call from a worker thread, not a GUI
//! main loop; an unlock prompt blocks until the user answers it.

use std::collections::HashMap;
use std::future::poll_fn;
use std::pin::Pin;

use zbus::export::futures_core::Stream;
use zbus::message::Type as MessageType;
use zbus::{Connection, MatchRule, MessageStream};
use zbus::{ObjectPath, OwnedObjectPath, OwnedVariant as OwnedValue, Variant as Value};

use crate::dh::{self, KeyPair, SessionKey};
use crate::{Entry, KeyError, SecretStore};

const SERVICE: &str = "org.freedesktop.secrets";
const ROOT: &str = "/org/freedesktop/secrets";
const IFACE_SERVICE: &str = "org.freedesktop.Secret.Service";
const IFACE_COLLECTION: &str = "org.freedesktop.Secret.Collection";
const IFACE_ITEM: &str = "org.freedesktop.Secret.Item";
const IFACE_PROMPT: &str = "org.freedesktop.Secret.Prompt";
const IFACE_PROPS: &str = "org.freedesktop.DBus.Properties";
const NO_PROMPT: &str = "/";
const CONTENT_TYPE: &str = "application/octet-stream";

const ATTRIBUTES: [(&str, &str); 3] = [
    ("xdg:schema", "org.gist.ContentKey"),
    ("application", "gist"),
    ("purpose", "content-key-v1"),
];

/// How long any single D-Bus method call may take. A wedged key service must
/// become a typed error, not a hung launch. Waiting for the user to answer an
/// unlock prompt is a signal wait, not a method call, so it is not bounded.
const METHOD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Secret Service backend over the user's default collection.
pub struct SecretServiceStore {
    /// Test seam: connect to this bus address instead of the session bus.
    bus_address: Option<String>,
    method_timeout: std::time::Duration,
}

impl Default for SecretServiceStore {
    fn default() -> Self {
        Self {
            bus_address: None,
            method_timeout: METHOD_TIMEOUT,
        }
    }
}

impl SecretServiceStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Use a specific D-Bus address (an isolated test bus).
    pub fn with_bus_address(address: impl Into<String>) -> Self {
        Self {
            bus_address: Some(address.into()),
            ..Self::default()
        }
    }

    /// Override the per-call timeout (tests).
    pub fn with_method_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.method_timeout = timeout;
        self
    }

    async fn connect(&self) -> Result<Session, KeyError> {
        let builder = match &self.bus_address {
            Some(address) => zbus::connection::Builder::address(address.as_str()),
            None => zbus::connection::Builder::session(),
        };
        let conn = builder
            .method_timeout(self.method_timeout)
            .build()
            .await
            .map_err(unavailable)?;

        let pair = KeyPair::generate()?;
        let reply = conn
            .call_method(
                Some(SERVICE),
                ROOT,
                Some(IFACE_SERVICE),
                "OpenSession",
                &(dh::ALGORITHM, Value::from(pair.public_bytes().to_vec())),
            )
            .await
            .map_err(unavailable)?;
        let (server_public, path): (OwnedValue, OwnedObjectPath) =
            reply.body().deserialize().map_err(unavailable)?;
        let server_public = Vec::<u8>::try_from(server_public).map_err(unavailable)?;
        let key = pair.derive(&server_public)?;
        Ok(Session { conn, path, key })
    }

    async fn collection(&self, session: &Session) -> Result<OwnedObjectPath, KeyError> {
        let reply = session
            .call(ROOT, IFACE_SERVICE, "ReadAlias", &("default",))
            .await?;
        let (path,): (OwnedObjectPath,) = reply.body().deserialize().map_err(store)?;
        if path.as_str() == NO_PROMPT {
            return Err(KeyError::Unavailable(
                "no default Secret Service collection".into(),
            ));
        }
        Ok(path)
    }
}

struct Session {
    conn: Connection,
    path: OwnedObjectPath,
    key: SessionKey,
}

impl Session {
    async fn call<B>(
        &self,
        path: &str,
        interface: &str,
        member: &str,
        body: &B,
    ) -> Result<zbus::Message, KeyError>
    where
        B: serde_like::Body,
    {
        self.conn
            .call_method(Some(SERVICE), path, Some(interface), member, body)
            .await
            .map_err(store)
    }

    /// Unlock `objects`, driving the user prompt if the service needs one.
    async fn unlock(&self, objects: &[OwnedObjectPath]) -> Result<(), KeyError> {
        let paths: Vec<ObjectPath<'_>> = objects.iter().map(|p| p.as_ref()).collect();
        let reply = self.call(ROOT, IFACE_SERVICE, "Unlock", &(paths,)).await?;
        let (unlocked, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
            reply.body().deserialize().map_err(store)?;
        if prompt.as_str() != NO_PROMPT {
            self.run_prompt(&prompt).await?;
            return Ok(());
        }
        if objects.iter().all(|o| unlocked.contains(o)) {
            Ok(())
        } else {
            Err(KeyError::Locked)
        }
    }

    /// Run an interactive prompt to completion. Dismissal is `Denied`.
    async fn run_prompt(&self, prompt: &OwnedObjectPath) -> Result<(), KeyError> {
        // Subscribe before prompting so `Completed` cannot be missed.
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(SERVICE)
            .interface(IFACE_PROMPT)
            .member("Completed")
            .path(prompt.as_str())
            .build()
            .map_err(store)?;
        let mut stream =
            MessageStream::for_match_rule(rule, &self.conn, std::num::NonZeroUsize::new(8))
                .await
                .map_err(store)?;
        self.call(prompt.as_str(), IFACE_PROMPT, "Prompt", &("",))
            .await?;
        let signal = poll_fn(|cx| Pin::new(&mut stream).poll_next(cx))
            .await
            .ok_or_else(|| KeyError::Store("connection closed during the unlock prompt".into()))?
            .map_err(store)?;
        let (dismissed, _result): (bool, OwnedValue) =
            signal.body().deserialize().map_err(store)?;
        if dismissed {
            Err(KeyError::Denied)
        } else {
            Ok(())
        }
    }
}

impl SecretStore for SecretServiceStore {
    fn list(&self) -> Result<Vec<Entry>, KeyError> {
        zbus::block_on(async {
            let session = self.connect().await?;
            let collection = self.collection(&session).await?;
            session.unlock(std::slice::from_ref(&collection)).await?;

            let attributes: HashMap<&str, &str> = ATTRIBUTES.into_iter().collect();
            let reply = session
                .call(
                    collection.as_str(),
                    IFACE_COLLECTION,
                    "SearchItems",
                    &(attributes,),
                )
                .await?;
            let (items,): (Vec<OwnedObjectPath>,) = reply.body().deserialize().map_err(store)?;
            if !items.is_empty() {
                session.unlock(&items).await?;
            }

            let mut entries = Vec::with_capacity(items.len());
            for item in &items {
                let reply = session
                    .call(item.as_str(), IFACE_PROPS, "Get", &(IFACE_ITEM, "Created"))
                    .await?;
                let created: OwnedValue = reply.body().deserialize().map_err(store)?;
                let created = u64::try_from(created).map_err(store)?;

                let reply = session
                    .call(
                        item.as_str(),
                        IFACE_ITEM,
                        "GetSecret",
                        &(session.path.as_ref(),),
                    )
                    .await?;
                let (_, iv, ciphertext, _content_type): (
                    OwnedObjectPath,
                    Vec<u8>,
                    Vec<u8>,
                    String,
                ) = reply.body().deserialize().map_err(store)?;
                let secret = session.key.decrypt(&iv, &ciphertext)?;
                entries.push(Entry {
                    created,
                    id: item.to_string(),
                    secret: secret.to_vec(),
                });
            }
            Ok(entries)
        })
    }

    fn add(&self, secret: &[u8]) -> Result<(), KeyError> {
        zbus::block_on(async {
            let session = self.connect().await?;
            let collection = self.collection(&session).await?;
            session.unlock(std::slice::from_ref(&collection)).await?;

            let attributes: HashMap<&str, &str> = ATTRIBUTES.into_iter().collect();
            let mut properties: HashMap<&str, Value<'_>> = HashMap::new();
            properties.insert(
                "org.freedesktop.Secret.Item.Label",
                Value::from("GIST content key"),
            );
            properties.insert(
                "org.freedesktop.Secret.Item.Attributes",
                Value::from(attributes),
            );
            let (iv, ciphertext) = session.key.encrypt(secret)?;
            let reply = session
                .call(
                    collection.as_str(),
                    IFACE_COLLECTION,
                    "CreateItem",
                    &(
                        properties,
                        (session.path.as_ref(), iv, ciphertext, CONTENT_TYPE),
                        false, // never replace an existing entry
                    ),
                )
                .await?;
            let (_item, prompt): (OwnedObjectPath, OwnedObjectPath) =
                reply.body().deserialize().map_err(store)?;
            if prompt.as_str() != NO_PROMPT {
                session.run_prompt(&prompt).await?;
            }
            Ok(())
        })
    }
}

/// A failure to reach or negotiate with the service at all.
fn unavailable(error: impl std::fmt::Display) -> KeyError {
    KeyError::Unavailable(error.to_string())
}

/// A failure after the session was established.
fn store(error: impl std::fmt::Display) -> KeyError {
    let text = error.to_string();
    if text.contains("IsLocked") {
        KeyError::Locked
    } else if text.contains("timed out") {
        KeyError::Unavailable("the key store did not respond in time".into())
    } else {
        KeyError::Store(text)
    }
}

mod serde_like {
    /// Bound for D-Bus message bodies.
    pub trait Body: zbus::export::serde::Serialize + zbus::DynamicType {}
    impl<T: zbus::export::serde::Serialize + zbus::DynamicType> Body for T {}
}
