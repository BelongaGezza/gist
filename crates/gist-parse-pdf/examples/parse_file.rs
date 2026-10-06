//! Parse one PDF with default limits and print a one-line summary.
//!
//! Exists so peak memory can be measured from outside the process:
//!
//! ```text
//! cargo build --release -p gist-parse-pdf --example parse_file
//! GIST_PDFIUM_LIBRARY=artifacts/pdfium/lib/libpdfium.dylib \
//!   /usr/bin/time -l target/release/examples/parse_file some.pdf
//! ```
//!
//! (`GIST_PDFIUM_LIBRARY` is honoured by debug builds only; for a release
//! build pass the library path as the second argument.)

use gist_model::ParseLimits;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: parse_file <file.pdf> [libpdfium path]");
        std::process::exit(2);
    };
    if let Some(lib) = args.next() {
        gist_parse_pdf::set_library_path(lib);
    }
    let bytes = std::fs::read(&path).expect("read input");
    let start = std::time::Instant::now();
    // Diagnostic: `PARSE_FILE_MODE=extract` runs only pdfium extraction and
    // drops each page immediately (no layout, nothing retained), and
    // `PARSE_FILE_MODE=lines` additionally lays each page out into lines and
    // retains them -- to attribute peak memory to pdfium vs. our buffers.
    let mode = std::env::var("PARSE_FILE_MODE").unwrap_or_default();
    if mode == "extract" || mode == "lines" {
        use gist_parse_pdf::PdfParser;
        let mut glyphs = 0usize;
        let mut keep = Vec::new();
        let r = gist_parse_pdf::PdfiumParser::new().extract(
            &bytes,
            &ParseLimits::default(),
            &mut |page| {
                glyphs += page.glyphs.len();
                if mode == "lines" {
                    keep.push(gist_parse_pdf::layout::page_to_lines(&page));
                }
                Ok(())
            },
        );
        println!(
            "mode={mode} glyphs={glyphs} pages_kept={} ok={} elapsed={:?}",
            keep.len(),
            r.is_ok(),
            start.elapsed()
        );
        return;
    }
    // `PARSE_FILE_MODE=json` prints the parsed document's content (title,
    // word count, sections, token stream -- not the random id or the
    // timestamps) as JSON, for byte-for-byte before/after comparisons.
    if mode == "json" {
        match gist_parse_pdf::parse_pdf(&bytes, "measure", &ParseLimits::default()) {
            Ok(doc) => println!(
                "{}",
                serde_json::json!({
                    "title": doc.metadata.title,
                    "author": doc.metadata.author,
                    "word_count": doc.metadata.word_count,
                    "sections": doc.sections,
                    "token_stream": doc.token_stream,
                })
            ),
            Err(e) => println!("err {e}"),
        }
        return;
    }
    match gist_parse_pdf::parse_pdf(&bytes, "measure", &ParseLimits::default()) {
        Ok(doc) => {
            // Count text bytes without formatting/cloning anything (the
            // measurement itself must not add to peak memory).
            let chars: usize = doc
                .sections
                .iter()
                .flat_map(|s| &s.blocks)
                .map(|b| match b {
                    gist_model::Block::Heading { text, .. } => text.len(),
                    gist_model::Block::Paragraph { runs } => {
                        runs.iter().map(|r| r.text.len()).sum()
                    }
                    _ => 0,
                })
                .sum();
            println!(
                "ok sections={} approx_chars={chars} elapsed={:?}",
                doc.sections.len(),
                start.elapsed()
            );
        }
        Err(e) => println!("err {e} elapsed={:?}", start.elapsed()),
    }
}
