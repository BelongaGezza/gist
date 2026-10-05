//! Spike helper: reads a whole PDF from stdin, runs the real
//! `gist_parse_pdf::parse_pdf`, writes a framed reply to stdout.
//!
//! Reply frame: 1 status byte, then payload.
//!   0x00 OK         payload = Document as JSON
//!   0x01 ERR_PDF    payload = "<kind>\t<message>"  (typed PdfError)
//! Anything else (signal, nonzero exit, no frame) is a helper crash and is
//! classified by the host.
//!
//! Fault injection (proves the isolation MECHANISM, not a real pdfium bug):
//! if the input contains the marker `%GIST_SPIKE_FAULT_ABORT` or
//! `%GIST_SPIKE_FAULT_SEGV` the helper aborts / dereferences null *after*
//! loading pdfium, i.e. at the point a native bug would fire.

use gist_model::ParseLimits;
use gist_parse_pdf::PdfError;
use std::io::{Read, Write};

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn kind(e: &PdfError) -> &'static str {
    match e {
        PdfError::Encrypted => "encrypted",
        PdfError::NoTextLayer => "no_text_layer",
        PdfError::LibraryUnavailable(_) => "library_unavailable",
        PdfError::ResourceLimitExceeded { .. } => "resource_limit",
        PdfError::Malformed(_) => "malformed",
    }
}

fn main() {
    let lib = std::env::args().nth(1).expect("usage: spike-helper <libpdfium>");
    gist_parse_pdf::set_library_path(lib);
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).expect("stdin");

    if find(&bytes, b"%GIST_SPIKE_FAULT_ABORT") {
        std::process::abort();
    }
    if find(&bytes, b"%GIST_SPIKE_FAULT_SEGV") {
        unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
    }
    if find(&bytes, b"%GIST_SPIKE_FAULT_HANG") {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    let out = std::io::stdout();
    let mut out = out.lock();
    match gist_parse_pdf::parse_pdf(&bytes, "spike", &ParseLimits::default()) {
        Ok(doc) => {
            let json = serde_json::to_vec(&doc).expect("serialize");
            out.write_all(&[0u8]).unwrap();
            out.write_all(&json).unwrap();
        }
        Err(e) => {
            out.write_all(&[1u8]).unwrap();
            write!(out, "{}\t{}", kind(&e), e).unwrap();
        }
    }
    out.flush().unwrap();
}
