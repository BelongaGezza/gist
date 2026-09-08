#![no_main]

// Fuzz the HTML extraction pathway via the web crate.
// We cannot fuzz fetch_url (needs network), but we can test the HTML parsing
// by constructing a minimal harness. If gist_web exposes extract_content or
// similar, use it directly. Otherwise, attempt a localhost URL (will error on
// network, but exercises the URL validation path) or skip and test HTML via
// the scraper crate directly.
//
// For now: fuzz URL validation + initial parsing — if a URL is syntactically
// valid and the DNS lookup is skipped (because ureq returns Io error quickly),
// the function still exercises the URL-parsing, robots-parsing, and response-
// processing branches with the fuzzer's input treated as a URL string.
//
// A better harness is added once gist_web exposes an extract_html(html_bytes) fn.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // If data is valid UTF-8, use it as an HTML string via the web crate's
    // internal HTML processing — currently this requires the function to be pub.
    // TODO: expose gist_web::extract_html(html: &[u8]) -> (String, Vec<Block>)
    // and call it here once M2 lands.
    if let Ok(s) = std::str::from_utf8(data) {
        // Attempt URL parse (exercises gist_web's URL validation)
        use gist_core::ParseLimits;
        let limits = ParseLimits {
            max_bytes: 1 * 1024 * 1024,
            max_pages: 20,
            max_nesting_depth: 50,
            max_expanded_bytes: 4 * 1024 * 1024,
        };
        // fetch_url will fail fast on non-URL input or non-network input.
        // This exercises the URL-validation and early-exit paths.
        let _ = gist_web::fetch_url(s, &limits);
    }
});
