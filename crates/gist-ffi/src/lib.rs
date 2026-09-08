use std::path::PathBuf;
use std::sync::Arc;

uniffi::setup_scaffolding!();

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum GistError {
    #[error("{0}")]
    Core(String),
    #[error("internal panic: {0}")]
    InternalPanic(String),
}

impl From<gist_core::CoreError> for GistError {
    fn from(e: gist_core::CoreError) -> Self {
        GistError::Core(e.to_string())
    }
}

// ── panic guard ──────────────────────────────────────────────────────────────

macro_rules! ffi_catch {
    ($body:expr) => {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body))
            .map_err(|_| GistError::InternalPanic("unexpected internal error".to_string()))?
    };
}

// ── Record types ─────────────────────────────────────────────────────────────

#[derive(uniffi::Record)]
pub struct FfiLibraryItem {
    pub id: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub source_path: Option<String>,
    pub cover_path: Option<String>,
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
        ffi_catch! {{
            let core = gist_core::Core::init(&PathBuf::from(&db_path), &PathBuf::from(&storage_dir))
                .map_err(GistError::from)?;
            Ok(Arc::new(Self { inner: core }))
        }}
    }

    pub fn health(&self) -> Result<(), GistError> {
        ffi_catch! {
            self.inner.health().map_err(GistError::from)
        }
    }

    pub fn import_txt(&self, path: String) -> Result<String, GistError> {
        ffi_catch! {
            self.inner
                .import_txt(&PathBuf::from(path))
                .map_err(GistError::from)
        }
    }

    pub fn list_items(&self, offset: u64, limit: u64) -> Result<Vec<FfiLibraryItem>, GistError> {
        ffi_catch! {{
            let items = self
                .inner
                .list_items(offset as usize, limit as usize)
                .map_err(GistError::from)?;
            Ok(items
                .into_iter()
                .map(|i| FfiLibraryItem {
                    id: i.id,
                    title: i.title,
                    authors: i.authors,
                    source_path: i.source_path,
                    cover_path: i.cover_path,
                })
                .collect())
        }}
    }

    pub fn start_rsvp(&self, item_id: String, wpm: u32) -> Result<String, GistError> {
        ffi_catch! {{
            let config = gist_rsvp::Config {
                wpm,
                ..Default::default()
            };
            self.inner
                .start_rsvp(&item_id, config)
                .map_err(GistError::from)
        }}
    }

    pub fn save_progress(&self, item_id: String, token_index: u64) -> Result<(), GistError> {
        ffi_catch! {
            self.inner
                .save_progress(&item_id, token_index as usize)
                .map_err(GistError::from)
        }
    }
}
