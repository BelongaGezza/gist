#![no_main]

// PDF parsing goes through pdfium (loaded dynamically). The harness needs
// libpdfium: run `tools/fetch-pdfium.sh` first (debug/fuzz builds find it at
// artifacts/pdfium/lib, or via GIST_PDFIUM_LIBRARY). Seed corpus in
// fuzz/corpus/fuzz_parse_pdf/ holds the small synthetic fixtures.

use gist_core::ParseLimits;
use gist_parse_pdf::PdfError;
use libfuzzer_sys::fuzz_target;

fn fuzz_limits() -> ParseLimits {
    ParseLimits {
        max_bytes: 1024 * 1024,
        max_pages: 50,
        max_nesting_depth: 50,
        max_expanded_bytes: 4 * 1024 * 1024,
        ..ParseLimits::default()
    }
}

fuzz_target!(|data: &[u8]| {
    // The parser must NEVER panic on any input; errors are fine.
    let limits = fuzz_limits();
    // A missing pdfium would make this target silently ineffective (every
    // input past the %PDF- check would bounce off), so fail loudly.
    if let Err(PdfError::LibraryUnavailable(why)) =
        gist_parse_pdf::parse_pdf(data, "fuzz", &limits)
    {
        panic!("pdfium not loadable: {why}");
    }
});
