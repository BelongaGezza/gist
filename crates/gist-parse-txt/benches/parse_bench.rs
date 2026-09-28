//! Performance benchmarks for `gist-parse-txt::parse`, added for M4 role R2
//! (`docs/m4-agent-roles.md` §2) against product-spec §9.1's "20-page
//! document import+normalisation under 5s" target.
//!
//! Informational only — this crate has no PDF backend (`gist-parse-pdf` is a
//! stub) and DOCX/ePub are benchmarked in their own crates' `benches/`, so
//! txt parsing here is a fast-path sanity check, not the primary evidence
//! for the §9.1(a) target.

use criterion::{criterion_group, criterion_main, Criterion};
use gist_model::ParseLimits;

const BASIC_ASCII: &[u8] = include_bytes!("../../../fixtures/txt/basic_ascii.txt");
const SINGLE_LINE: &[u8] = include_bytes!("../../../fixtures/txt/single_line.txt");
/// ~58 KB / ~8,900 words across 172 blank-line-separated paragraphs — a
/// realistic stand-in for a "20-page" document (see `fixtures/README.md`).
const BENCH_20_PAGE: &[u8] = include_bytes!("../../../fixtures/txt/bench_20page.txt");

fn bench_parse_txt(c: &mut Criterion) {
    let limits = ParseLimits::default();

    let mut group = c.benchmark_group("parse_txt");
    group.bench_function("basic_ascii_1kb", |b| {
        b.iter(|| gist_parse_txt::parse(BASIC_ASCII, "basic_ascii", &limits).unwrap())
    });
    group.bench_function("single_line_50kb", |b| {
        b.iter(|| gist_parse_txt::parse(SINGLE_LINE, "single_line", &limits).unwrap())
    });
    group.bench_function("synthetic_20_page_58kb", |b| {
        b.iter(|| gist_parse_txt::parse(BENCH_20_PAGE, "bench_20page", &limits).unwrap())
    });
    group.finish();
}

criterion_group!(benches, bench_parse_txt);
criterion_main!(benches);
