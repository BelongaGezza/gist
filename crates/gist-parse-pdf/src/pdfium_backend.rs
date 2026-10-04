//! pdfium implementation of [`PdfParser`] (ADR-002).
//!
//! pdfium is bound **dynamically** by absolute path. The macOS app embeds
//! `libpdfium.dylib` in `Contents/Frameworks/` (signed with the app's own
//! identity - security register N8) and this module resolves it relative to
//! the running executable. A missing or unloadable library is a typed
//! [`PdfError::LibraryUnavailable`], never a panic.
//!
//! `pdfium-render`'s bindings live in a process-global `OnceCell` and block
//! forever if a `Pdfium` handle is created before they are initialised, so
//! this module only ever creates a handle after a successful first bind.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gist_model::ParseLimits;
use pdfium_render::prelude::*;

use crate::layout::{Glyph, RawPage};
use crate::{PdfError, PdfParser, RawMeta, MAX_GLYPHS_PER_PAGE};

/// Explicit library path (file, or directory containing the library),
/// set by the host app at startup. Takes priority over the default search.
static CONFIGURED_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Serialises binding and parsing: pdfium is not safe to use concurrently
/// for independent documents beyond what `thread_safe` guards per call.
static PARSE_LOCK: Mutex<bool> = Mutex::new(false);

/// Override where pdfium is loaded from (file path, or a directory that
/// contains the platform library). Must be called before the first PDF parse
/// to have an effect on binding; later calls are ignored once bound.
pub fn set_library_path(path: impl Into<PathBuf>) {
    let mut g = CONFIGURED_PATH.lock().unwrap_or_else(|p| p.into_inner());
    *g = Some(path.into());
}

fn lib_file_in(dir: &Path) -> PathBuf {
    dir.join(libloading_name())
}

fn libloading_name() -> std::ffi::OsString {
    if cfg!(target_os = "macos") {
        "libpdfium.dylib".into()
    } else if cfg!(target_os = "windows") {
        "pdfium.dll".into()
    } else {
        "libpdfium.so".into()
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = CONFIGURED_PATH
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
    {
        out.push(if p.is_dir() { lib_file_in(&p) } else { p });
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // <App>.app/Contents/MacOS/<exe> -> Contents/Frameworks/
            if let Some(contents) = dir.parent() {
                out.push(lib_file_in(&contents.join("Frameworks")));
            }
            out.push(lib_file_in(dir));
        }
    }
    // Developer / test conveniences. Compiled out of release builds so a
    // shipped app can never be steered to an attacker-chosen library through
    // its environment or a baked-in build path.
    #[cfg(debug_assertions)]
    {
        if let Some(p) = std::env::var_os("GIST_PDFIUM_LIBRARY") {
            out.push(PathBuf::from(p));
        }
        out.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../artifacts/pdfium/lib")
                .join(libloading_name()),
        );
    }
    out
}

/// Make sure the global bindings exist; returns a handle usable for parsing.
/// `Pdfium::default()` panics if nothing is loadable, so it is only reached
/// once `bound` records a successful first bind (flag guarded by `PARSE_LOCK`,
/// held by the caller).
fn bind(bound: &mut bool) -> Result<Pdfium, PdfError> {
    if *bound {
        // Bindings are set; `Default` returns a handle without panicking
        // (it matches `PdfiumLibraryBindingsAlreadyInitialized`).
        return Ok(Pdfium::default());
    }
    let mut last = String::from("no candidate library path");
    for path in candidate_paths() {
        if !path.is_file() {
            last = "libpdfium not found in the app bundle".to_string();
            tracing::debug!("gist-parse-pdf: no pdfium at {}", path.display());
            continue;
        }
        match Pdfium::bind_to_library(&path) {
            Ok(b) => {
                let p = Pdfium::new(b);
                *bound = true;
                return Ok(p);
            }
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                *bound = true;
                return Ok(Pdfium::default());
            }
            Err(e) => {
                tracing::debug!("gist-parse-pdf: failed to load {}: {e}", path.display());
                last = "libpdfium could not be loaded".to_string();
            }
        }
    }
    Err(PdfError::LibraryUnavailable(last))
}

/// The pdfium-backed [`PdfParser`].
#[derive(Debug, Default, Clone, Copy)]
pub struct PdfiumParser;

impl PdfiumParser {
    pub fn new() -> Self {
        PdfiumParser
    }
}

fn map_load_error(e: PdfiumError) -> PdfError {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)
        | PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::SecurityError) => {
            PdfError::Encrypted
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            PdfError::Malformed("invalid or truncated PDF structure".into())
        }
        other => PdfError::Malformed(format!("could not open PDF ({other:?})")),
    }
}

impl PdfParser for PdfiumParser {
    fn extract(
        &self,
        bytes: &[u8],
        limits: &ParseLimits,
        sink: &mut dyn FnMut(RawPage) -> Result<(), PdfError>,
    ) -> Result<RawMeta, PdfError> {
        let mut bound = PARSE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let pdfium = bind(&mut bound)?;

        let doc = pdfium
            .load_pdf_from_byte_slice(bytes, None)
            .map_err(map_load_error)?;

        // Encrypted documents: pdfium opens an empty-user-password file
        // transparently. We never try a password, and we additionally honour
        // the document's own permission bits: a document that forbids text
        // extraction is treated as protected (ADR-004 posture).
        let perms = doc.permissions();
        if let Ok(rev) = perms.security_handler_revision() {
            if rev != PdfSecurityHandlerRevision::Unprotected
                && !perms.can_extract_text_and_graphics().unwrap_or(false)
            {
                return Err(PdfError::Encrypted);
            }
        }

        // Page-count gate before any page is loaded (F16/N9 ordering).
        let page_count = doc.pages().len() as usize;
        if page_count > limits.max_pages {
            return Err(PdfError::ResourceLimitExceeded {
                limit: format!("max_pages={}", limits.max_pages),
                attempted: page_count,
            });
        }

        let mut remaining_chars = limits.max_expanded_bytes;
        for index in 0..page_count {
            let page = match doc.pages().get(index as PdfPageIndex) {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("gist-parse-pdf: page {index} failed to load: {e:?}");
                    sink(RawPage {
                        width: 0.0,
                        height: 0.0,
                        glyphs: Vec::new(),
                    })?;
                    continue;
                }
            };
            let width = page.width().value;
            let height = page.height().value;
            let mut glyphs = Vec::new();

            if let Ok(text) = page.text() {
                let chars = text.chars();
                // Check-before-allocate: pdfium reports the char count up front.
                let n = chars.len();
                if n > MAX_GLYPHS_PER_PAGE || n > remaining_chars {
                    return Err(PdfError::ResourceLimitExceeded {
                        limit: format!("glyphs_per_page={MAX_GLYPHS_PER_PAGE}"),
                        attempted: n,
                    });
                }
                remaining_chars = remaining_chars.saturating_sub(n);
                glyphs.reserve(n);
                for c in chars.iter() {
                    let Some(ch) = c.unicode_char() else { continue };
                    let Ok(b) = c.loose_bounds() else { continue };
                    glyphs.push(Glyph {
                        ch,
                        x0: b.left().value,
                        x1: b.right().value,
                        y0: b.bottom().value,
                        y1: b.top().value,
                        size: c.scaled_font_size().value,
                    });
                }
            }
            sink(RawPage {
                width,
                height,
                glyphs,
            })?;
        }

        let tag =
            |t: PdfDocumentMetadataTagType| doc.metadata().get(t).map(|v| v.value().to_string());
        Ok(RawMeta {
            title: tag(PdfDocumentMetadataTagType::Title),
            author: tag(PdfDocumentMetadataTagType::Author),
        })
    }
}
