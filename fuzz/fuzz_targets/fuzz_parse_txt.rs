#![no_main]

use libfuzzer_sys::fuzz_target;
use gist_core::ParseLimits;
use gist_parse_txt::EncodingChoice;

fn fuzz_limits() -> ParseLimits {
    ParseLimits {
        max_bytes: 1 * 1024 * 1024,
        max_pages: 20,
        max_nesting_depth: 50,
        max_expanded_bytes: 4 * 1024 * 1024,
        ..ParseLimits::default()
    }
}

fuzz_target!(|data: &[u8]| {
    // The parser should NEVER panic on any input.
    // Errors are acceptable — panics are not.
    let limits = fuzz_limits();
    let _ = gist_parse_txt::parse(data, "fuzz", &limits);

    // SC-01: the first byte picks a (possibly hostile) encoding label, the
    // rest is the file body. Exercises Named decoding for many WHATWG labels,
    // the label length cap, detect_encoding and the expansion budget.
    if let Some((&sel, body)) = data.split_first() {
        const LABELS: &[&str] = &[
            "utf-8", "utf-16le", "utf-16be", "windows-1252", "windows-1251", "koi8-r",
            "shift_jis", "gbk", "big5", "euc-kr", "iso-2022-jp", "x-user-defined",
            "replacement", "gb18030", "", "bogus",
        ];
        let label = LABELS[(sel as usize) % LABELS.len()];
        let _ = gist_parse_txt::parse_with_encoding(
            body,
            "fuzz",
            &limits,
            EncodingChoice::Named(label),
        );
        let _ = gist_parse_txt::detect_encoding(body, EncodingChoice::Auto, &limits);
        // A label taken from the input itself (arbitrary bytes, length-capped).
        let n = (sel as usize).min(body.len());
        if let Ok(l) = std::str::from_utf8(&body[..n]) {
            let _ = gist_parse_txt::parse_with_encoding(
                &body[n..],
                "fuzz",
                &limits,
                EncodingChoice::Named(l),
            );
        }
    }
});
