#![no_main]

// ePub files are ZIP archives. libFuzzer will discover valid ZIP structure over time.
// Seed corpus in fuzz/corpus/fuzz_parse_epub/ should include small valid ePub files.

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
    let _ = gist_parse_epub::parse(data, &limits);
});
