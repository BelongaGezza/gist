#![no_main]

// DOCX files are ZIP archives (Office Open XML).
// Seed corpus in fuzz/corpus/fuzz_parse_docx/ should include minimal valid DOCX files.

use libfuzzer_sys::fuzz_target;
use gist_core::ParseLimits;

fn fuzz_limits() -> ParseLimits {
    ParseLimits {
        max_bytes: 1 * 1024 * 1024,
        max_pages: 20,
        max_nesting_depth: 50,
        max_expanded_bytes: 4 * 1024 * 1024,
    }
}

fuzz_target!(|data: &[u8]| {
    // The parser should NEVER panic on any input.
    // Errors are acceptable — panics are not.
    let limits = fuzz_limits();
    let _ = gist_parse_docx::parse(data, &limits);
});
