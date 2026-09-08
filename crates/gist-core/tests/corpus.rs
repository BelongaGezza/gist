//! Parser corpus integration test.
//!
//! Iterates over all fixture files under `<workspace-root>/fixtures/` and
//! verifies that no parser call panics on any of them.  Panic-safety is
//! enforced with `std::panic::catch_unwind`.
//!
//! Rules:
//!   - **No panic** is the hard requirement; the test fails if `catch_unwind`
//!     returns `Err`.
//!   - For normal (non-adversarial) fixtures the result is expected to be
//!     `Ok(doc)` with non-empty sections.  Empty content is logged but does
//!     not fail the build.
//!   - For adversarial fixtures `Ok` or any recognised error is acceptable;
//!     only a panic fails the test.
//!   - Web fetching (`gist_web::fetch_url`) requires a live network and is
//!     skipped here — web extraction is covered by the fuzz/fuzz_web_extract
//!     target.
//!
//! # Running
//! ```
//! cargo test --test corpus --package gist-core -- --nocapture
//! ```

use std::panic;
use std::path::{Path, PathBuf};

use gist_core::ParseLimits;

// ---------------------------------------------------------------------------
// Counters for a per-format summary
// ---------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counts {
    total: usize,
    ok: usize,
    ok_empty: usize,
    err_recognised: usize,
    panicked: usize,
}

impl Counts {
    fn record_ok(&mut self, non_empty: bool) {
        self.total += 1;
        if non_empty {
            self.ok += 1;
        } else {
            self.ok_empty += 1;
        }
    }
    fn record_err(&mut self) {
        self.total += 1;
        self.err_recognised += 1;
    }
    fn record_panic(&mut self) {
        self.total += 1;
        self.panicked += 1;
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Return the `fixtures/` directory at the workspace root.
///
/// `CARGO_MANIFEST_DIR` is `crates/gist-core/`; two `parent()` calls reach
/// the workspace root.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ dir")
        .parent()
        .expect("workspace root")
        .join("fixtures")
}

/// Recursively collect all files under `dir`, sorted for determinism.
fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_files_inner(dir, &mut out);
    out.sort();
    out
}

fn collect_files_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("  [warn] cannot read dir {}: {}", dir.display(), e);
            return;
        }
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_inner(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// True when a file lives inside an `adversarial/` subdirectory.
fn is_adversarial(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "adversarial")
}

/// Extract a display stem (filename without extension, falling back to the
/// full filename) for use as a document title in parser calls.
fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("fixture")
        .to_string()
}

// ---------------------------------------------------------------------------
// Parse wrappers — each wraps the real parser call in catch_unwind.
// Returns Ok(true) = success + non-empty, Ok(false) = error or empty (no panic),
// Err(()) = panic detected.
// ---------------------------------------------------------------------------

/// txt: `gist_parse_txt::parse(bytes, stem)` — no ParseLimits parameter.
fn run_parse_txt(bytes: Vec<u8>, stem: String) -> Result<bool, ()> {
    match panic::catch_unwind(move || gist_parse_txt::parse(&bytes, &stem)) {
        Ok(Ok(doc)) => Ok(!doc.sections.is_empty()),
        Ok(Err(_e))  => Ok(false), // recognised error
        Err(_)       => Err(()),   // panic
    }
}

/// epub: `gist_parse_epub::parse(bytes, stem, limits)`.
fn run_parse_epub(bytes: Vec<u8>, stem: String, limits: ParseLimits) -> Result<bool, ()> {
    match panic::catch_unwind(move || gist_parse_epub::parse(&bytes, &stem, &limits)) {
        Ok(Ok(doc)) => Ok(!doc.sections.is_empty()),
        Ok(Err(_e))  => Ok(false),
        Err(_)       => Err(()),
    }
}

/// docx: `gist_parse_docx::parse(bytes, stem, limits)`.
fn run_parse_docx(bytes: Vec<u8>, stem: String, limits: ParseLimits) -> Result<bool, ()> {
    match panic::catch_unwind(move || gist_parse_docx::parse(&bytes, &stem, &limits)) {
        Ok(Ok(doc)) => Ok(!doc.sections.is_empty()),
        Ok(Err(_e))  => Ok(false),
        Err(_)       => Err(()),
    }
}

// ---------------------------------------------------------------------------
// Helper: process a single file, record result, print outcome.
// ---------------------------------------------------------------------------

fn process_file(
    path: &Path,
    fixtures: &Path,
    counts: &mut Counts,
    run: impl FnOnce(Vec<u8>) -> Result<bool, ()>,
) {
    let rel = path.strip_prefix(fixtures).unwrap_or(path);
    let adversarial = is_adversarial(path);

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  [io-err] {}: {}", rel.display(), e);
            counts.record_err();
            return;
        }
    };

    match run(bytes) {
        Err(()) => {
            eprintln!("  [PANIC ] {}", rel.display());
            counts.record_panic();
        }
        Ok(non_empty) => {
            if !adversarial && !non_empty {
                println!("  [warn  ] {} — Ok but no sections (check fixture)", rel.display());
                counts.record_ok(false); // ok_empty
            } else {
                println!(
                    "  [ok    ] {} — {}",
                    rel.display(),
                    if non_empty { "non-empty" } else { "empty/err (adversarial)" }
                );
                if non_empty {
                    counts.record_ok(true);
                } else {
                    counts.record_err();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

#[test]
fn corpus_no_panics() {
    let fixtures = fixtures_dir();

    println!("\n=== GIST parser corpus ===");
    println!("fixtures dir: {}\n", fixtures.display());

    if !fixtures.exists() {
        panic!(
            "fixtures/ directory not found at {}.\n\
             Generate fixtures with the Python script in tools/ first.",
            fixtures.display()
        );
    }

    let limits = ParseLimits::default();

    let mut txt_counts  = Counts::default();
    let mut epub_counts = Counts::default();
    let mut docx_counts = Counts::default();

    // -----------------------------------------------------------------------
    // txt/ — every file in this subtree is fed to the txt parser, regardless
    // of extension.  Adversarial files such as all_null_bytes.bin are expected
    // to produce a ParseError::Utf8, not a panic.
    // -----------------------------------------------------------------------
    println!("--- txt ---");
    for path in collect_files(&fixtures.join("txt")) {
        let stem = stem_of(&path);
        process_file(&path, &fixtures, &mut txt_counts, |bytes| {
            run_parse_txt(bytes, stem)
        });
    }

    // -----------------------------------------------------------------------
    // epub/
    // -----------------------------------------------------------------------
    println!("\n--- epub ---");
    for path in collect_files(&fixtures.join("epub")) {
        let stem  = stem_of(&path);
        let lim   = limits.clone();
        process_file(&path, &fixtures, &mut epub_counts, |bytes| {
            run_parse_epub(bytes, stem, lim)
        });
    }

    // -----------------------------------------------------------------------
    // docx/
    // -----------------------------------------------------------------------
    println!("\n--- docx ---");
    for path in collect_files(&fixtures.join("docx")) {
        let stem  = stem_of(&path);
        let lim   = limits.clone();
        process_file(&path, &fixtures, &mut docx_counts, |bytes| {
            run_parse_docx(bytes, stem, lim)
        });
    }

    // -----------------------------------------------------------------------
    // web/ — gist_web::fetch_url requires a live HTTPS connection and is not
    // exercised in this test.  The web extraction logic is covered by the
    // nightly `cargo-fuzz` target `fuzz_web_extract`.  Web HTML fixtures are
    // present in fixtures/web/ for manual inspection and future integration
    // tests that can reach the network.
    // -----------------------------------------------------------------------
    println!("\n--- web ---");
    println!("  [skip  ] web corpus requires live network — see fuzz/fuzz_targets/fuzz_web_extract.rs");

    // -----------------------------------------------------------------------
    // Summary
    // -----------------------------------------------------------------------
    println!("\n=== Summary ===");
    println!(
        "txt:  total={} ok={} ok-but-empty={} err={} PANICS={}",
        txt_counts.total, txt_counts.ok, txt_counts.ok_empty,
        txt_counts.err_recognised, txt_counts.panicked
    );
    println!(
        "epub: total={} ok={} ok-but-empty={} err={} PANICS={}",
        epub_counts.total, epub_counts.ok, epub_counts.ok_empty,
        epub_counts.err_recognised, epub_counts.panicked
    );
    println!(
        "docx: total={} ok={} ok-but-empty={} err={} PANICS={}",
        docx_counts.total, docx_counts.ok, docx_counts.ok_empty,
        docx_counts.err_recognised, docx_counts.panicked
    );

    // -----------------------------------------------------------------------
    // Assertion — a panic is the only hard failure
    // -----------------------------------------------------------------------
    let total_panics = txt_counts.panicked + epub_counts.panicked + docx_counts.panicked;

    assert_eq!(
        total_panics,
        0,
        "\n*** CORPUS FAILURE: {} parser panic(s) detected ***\n\
         Every parser must handle arbitrary bytes without panicking.\n\
         See the [PANIC] lines above for which fixtures triggered panics.\n",
        total_panics
    );

    println!(
        "\n[PASS] No panics across {} fixture files.",
        txt_counts.total + epub_counts.total + docx_counts.total
    );

    // Write machine-readable summary for the CI artifact upload step.
    write_corpus_results(&txt_counts, &epub_counts, &docx_counts);
}

// ---------------------------------------------------------------------------
// CI artefact: target/corpus-results/summary.json
// ---------------------------------------------------------------------------

fn write_corpus_results(txt: &Counts, epub: &Counts, docx: &Counts) {
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join("target").join("corpus-results"));

    let Some(out_dir) = target_dir else { return };
    if std::fs::create_dir_all(&out_dir).is_err() { return; }

    let json = format!(
        r#"{{
  "txt":  {{ "total": {}, "ok": {}, "ok_empty": {}, "err": {}, "panics": {} }},
  "epub": {{ "total": {}, "ok": {}, "ok_empty": {}, "err": {}, "panics": {} }},
  "docx": {{ "total": {}, "ok": {}, "ok_empty": {}, "err": {}, "panics": {} }}
}}
"#,
        txt.total,  txt.ok,  txt.ok_empty,  txt.err_recognised,  txt.panicked,
        epub.total, epub.ok, epub.ok_empty, epub.err_recognised, epub.panicked,
        docx.total, docx.ok, docx.ok_empty, docx.err_recognised, docx.panicked,
    );

    let _ = std::fs::write(out_dir.join("summary.json"), json);
}
