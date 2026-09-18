#![no_main]

// Fuzzes the HTML readability-extraction path directly (`gist_web::extract_content`),
// not `fetch_url` — fetch_url requires a well-formed `https://` URL and live
// network access before it ever reaches HTML parsing, so fuzzer bytes
// reinterpreted as a URL string almost never got past URL validation (N2).
// This harness feeds fuzzer bytes straight in as HTML, which is what
// actually exercises `collect_text`'s recursion-depth cap (F17) and the
// rest of the DOM-walking code.

use libfuzzer_sys::fuzz_target;

// Matches the depth used by gist-core's default ParseLimits::max_nesting_depth
// (200) — see crates/gist-model/src/lib.rs.
const MAX_DEPTH: usize = 200;

fuzz_target!(|data: &[u8]| {
    // extract_content takes &str; skip non-UTF-8 input rather than lossily
    // converting it, since scraper's own HTML parser is the thing under test,
    // not our encoding-recovery behavior.
    if let Ok(html) = std::str::from_utf8(data) {
        // The parser should NEVER panic on any input, and unbounded-depth
        // input must be rejected (Err), not exhaust the stack.
        let _ = gist_web::extract_content(html, MAX_DEPTH);
    }
});
