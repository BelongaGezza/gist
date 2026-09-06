use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
}

// ── LibraryItem (lightweight row, not the full Document) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub source_path: Option<String>,
    pub cover_path: Option<String>,
    pub created_at: i64,
    /// M0: not stored separately; always None.
    pub token_count: Option<usize>,
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

        if version == 0 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS library_items (
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
                PRAGMA user_version = 1;",
            )?;
            tracing::info!("gist-store: migrated schema to version 1");
        }

        Ok(Self {
            conn: Mutex::new(conn),
            storage_dir: storage_dir.to_owned(),
        })
    }

    /// Persist a [`gist_model::Document`] to disk (JSON blob) and index its
    /// metadata in SQLite.
    pub fn insert_item(&self, doc: &gist_model::Document) -> Result<(), StoreError> {
        let doc_path = self.storage_dir.join(format!("{}.json", doc.id));
        let doc_json = serde_json::to_string(doc)?;
        std::fs::write(&doc_path, &doc_json)?;

        let meta = &doc.metadata;
        // gist_model::Metadata has `author: Option<String>` — normalise to a list.
        let authors: Vec<String> = meta.author.iter().cloned().collect();
        let authors_json = serde_json::to_string(&authors)?;
        let meta_json = serde_json::to_string(meta)?;
        let doc_path_str = doc_path.to_string_lossy().into_owned();
        // source_ref holds the origin path/URL; cover_path is not in M0 Metadata.
        let source_path: Option<&str> = meta.source_ref.as_deref();
        let now_ms = now_millis();

        let conn = self.conn.lock().unwrap();
        conn.execute(
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
        tracing::debug!("gist-store: inserted item {}", doc.id);
        Ok(())
    }

    /// Return a paginated list of library items (newest first).
    pub fn list_items(&self, offset: usize, limit: usize) -> Result<Vec<LibraryItem>, StoreError> {
        let conn = self.conn.lock().unwrap();
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
                token_count: None,
            });
        }
        Ok(items)
    }

    /// Load and deserialise the full [`gist_model::Document`] for `id`.
    pub fn get_item(&self, id: &str) -> Result<Option<gist_model::Document>, StoreError> {
        let doc_path: Option<String> = {
            let conn = self.conn.lock().unwrap();
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

    /// Upsert the reading position (token index) for an item.
    pub fn save_progress(&self, item_id: &str, token_index: usize) -> Result<(), StoreError> {
        let now_ms = now_millis();
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let idx: Option<i64> = conn
            .query_row(
                "SELECT token_index FROM reading_progress WHERE item_id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(idx.unwrap_or(0) as usize)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
