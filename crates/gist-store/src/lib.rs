use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
}

// ── Schema version ────────────────────────────────────────────────────────

const SCHEMA_VERSION: i64 = 3;

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
}

impl Store {
    /// Open (or create) the SQLite database at `db_path`.
    /// Document JSON blobs are stored under `storage_dir`.
    pub fn open(db_path: &Path, storage_dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(storage_dir)?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
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

        Ok(Self {
            conn: Mutex::new(conn),
            storage_dir: storage_dir.to_owned(),
        })
    }

    /// Persist a [`gist_model::Document`] to disk (JSON blob + token file) and
    /// index its metadata and Word tokens in SQLite.
    pub fn insert_item(&self, doc: &gist_model::Document) -> Result<(), StoreError> {
        // Write the full document blob.
        let doc_path = self.storage_dir.join(format!("{}.json", doc.id));
        let doc_json = serde_json::to_string(doc)?;
        std::fs::write(&doc_path, &doc_json)?;

        // Write the token stream as a separate file for RSVP / FTS fast path.
        let token_path = self.storage_dir.join(format!("{}.tokens.json", doc.id));
        let tokens_json = serde_json::to_string(&doc.token_stream)?;
        std::fs::write(&token_path, &tokens_json)?;

        let meta = &doc.metadata;
        // gist_model::Metadata has `author: Option<String>` — normalise to a list.
        let authors: Vec<String> = meta.author.iter().cloned().collect();
        let authors_json = serde_json::to_string(&authors)?;
        let meta_json = serde_json::to_string(meta)?;
        let doc_path_str = doc_path.to_string_lossy().into_owned();
        // source_ref holds the origin path/URL; cover_path is not in M0 Metadata.
        let source_path: Option<&str> = meta.source_ref.as_deref();
        let now_ms = now_millis();

        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT OR REPLACE INTO library_items
             (id, title, authors, source_path, source_url, doc_path, cover_path,
              created_at, updated_at, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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

    /// Remove an item and all its associated files and FTS index entries.
    /// The `tokens` table has `ON DELETE CASCADE` so token rows and FTS5 entries
    /// are cleaned up automatically when the library_items row is deleted.
    pub fn delete_item(&self, id: &str) -> Result<(), StoreError> {
        // Delete JSON and token files first (best-effort; don't fail if missing).
        let doc_path = self.storage_dir.join(format!("{}.json", id));
        let token_path = self.storage_dir.join(format!("{}.tokens.json", id));
        let _ = std::fs::remove_file(&doc_path);
        let _ = std::fs::remove_file(&token_path);

        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        conn.execute("DELETE FROM library_items WHERE id = ?1", params![id])?;
        tracing::debug!("gist-store: deleted item {}", id);
        Ok(())
    }

    /// Full-text search across all imported document tokens.
    /// Returns item IDs (deduplicated) ranked by FTS5 relevance.
    pub fn search_items(&self, query: &str, limit: usize) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "SELECT DISTINCT t.item_id
             FROM tokens t
             WHERE t.rowid IN (
                 SELECT rowid FROM fts_index WHERE token_text MATCH ?1
             )
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Return a paginated list of library items (newest first).
    pub fn list_items(&self, offset: usize, limit: usize) -> Result<Vec<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, title, authors, source_path, cover_path, created_at
             FROM library_items
             ORDER BY created_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            let authors_json: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                authors_json,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut items = Vec::new();
        for row in rows {
            let (id, title, authors_json, source_path, cover_path, created_at) = row?;
            let authors: Vec<String> = serde_json::from_str(&authors_json).unwrap_or_default();
            items.push(LibraryItem {
                id,
                title,
                authors,
                source_path,
                cover_path,
                created_at,
                token_count: None, // TODO M2: populate from tokens table
            });
        }
        Ok(items)
    }

    /// Return the lightweight [`LibraryItem`] row for a single `id`, or
    /// `None` if no such item exists.
    pub fn get_item_by_id(&self, id: &str) -> Result<Option<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let row = conn
            .query_row(
                "SELECT id, title, authors, source_path, cover_path, created_at
                 FROM library_items
                 WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;

        Ok(row.map(
            |(id, title, authors_json, source_path, cover_path, created_at)| {
                let authors: Vec<String> = serde_json::from_str(&authors_json).unwrap_or_default();
                LibraryItem {
                    id,
                    title,
                    authors,
                    source_path,
                    cover_path,
                    created_at,
                    token_count: None, // TODO M2: populate from tokens table
                }
            },
        ))
    }

    /// Load and deserialise the full [`gist_model::Document`] for `id`.
    pub fn get_item(&self, id: &str) -> Result<Option<gist_model::Document>, StoreError> {
        let doc_path: Option<String> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            conn.query_row(
                "SELECT doc_path FROM library_items WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
        };

        match doc_path {
            None => Ok(None),
            Some(path) => {
                let bytes = std::fs::read(&path)?;
                let doc: gist_model::Document = serde_json::from_slice(&bytes)?;
                Ok(Some(doc))
            }
        }
    }

    /// Load only the token stream for an item (for RSVP / TTS).
    /// Loads from `<id>.tokens.json`; falls back to loading the full document.
    pub fn get_tokens(&self, id: &str) -> Result<Option<Vec<gist_model::Token>>, StoreError> {
        let doc_path: Option<String> = {
            let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
            conn.query_row(
                "SELECT doc_path FROM library_items WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
        };

        match doc_path {
            None => Ok(None),
            Some(path) => {
                let token_path = path
                    .strip_suffix(".json")
                    .map(|s| format!("{}.tokens.json", s))
                    .unwrap_or_else(|| format!("{}.tokens.json", path));

                if std::path::Path::new(&token_path).exists() {
                    let bytes = std::fs::read(&token_path)?;
                    let tokens: Vec<gist_model::Token> = serde_json::from_slice(&bytes)?;
                    Ok(Some(tokens))
                } else {
                    // Fallback: load full document and extract token stream.
                    let bytes = std::fs::read(&path)?;
                    let doc: gist_model::Document = serde_json::from_slice(&bytes)?;
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
        let mut stmt = conn.prepare(
            "SELECT li.id, li.title, li.authors, li.source_path, li.cover_path, li.created_at
             FROM library_items li
             JOIN item_collections ic ON ic.item_id = li.id
             WHERE ic.collection_id = ?1
             ORDER BY li.created_at DESC",
        )?;

        let rows = stmt.query_map(params![collection_id], |row| {
            let authors_json: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                authors_json,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut items = Vec::new();
        for row in rows {
            let (id, title, authors_json, source_path, cover_path, created_at) = row?;
            let authors: Vec<String> = serde_json::from_str(&authors_json).unwrap_or_default();
            items.push(LibraryItem {
                id,
                title,
                authors,
                source_path,
                cover_path,
                created_at,
                token_count: None, // TODO M2: populate from tokens table
            });
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
}

// ── helpers ───────────────────────────────────────────────────────────────

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
}
