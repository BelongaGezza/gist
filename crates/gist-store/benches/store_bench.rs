//! Performance benchmarks for `gist-store` at a synthetic 1,000+-row scale,
//! added for M4 role R2 (`docs/m4-agent-roles.md` §2) against product-spec
//! §9.1(b): "library with 1,000+ items must remain responsive (virtualised
//! lists, paged library API)".
//!
//! `list_items`/`search_items`/`insert_item` are benchmarked against a store
//! pre-seeded with 1,000 synthetic items, matching the scale §9.1(b) names.

use criterion::{criterion_group, criterion_main, Criterion};
use gist_model::{Block, Document, Metadata, Section, TextRun};
use gist_store::Store;
use tempfile::TempDir;

const SEED_ROWS: usize = 1_000;

fn make_document(i: usize) -> Document {
    let metadata = Metadata {
        title: format!("Bench Book {i}"),
        author: Some(format!("Author {}", i % 50)),
        source_type: "txt".to_string(),
        source_ref: None,
        source_copy_ref: None,
        import_date: None,
        language: None,
        word_count: 0,
    };
    let section = Section {
        id: "s0".to_string(),
        heading: None,
        blocks: vec![Block::Paragraph {
            runs: vec![TextRun::plain(format!(
                "This is synthetic benchmark content for item number {i}, covering words \
                 like reading pacing performance library search example text so that \
                 full-text search has something realistic to match against."
            ))],
        }],
    };
    Document::new(metadata, vec![section])
}

/// Opens a fresh temp-directory `Store` and inserts `n` synthetic items.
/// The `TempDir` must be kept alive by the caller for as long as `Store` is
/// used (dropping it deletes the backing directory).
fn seeded_store(n: usize) -> (TempDir, Store) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("bench.sqlite3");
    let storage_dir = dir.path().join("storage");
    std::fs::create_dir_all(&storage_dir).expect("mkdir storage");
    let store = Store::open(&db_path, &storage_dir).expect("open store");
    for i in 0..n {
        store
            .insert_item(&make_document(i))
            .expect("insert seed item");
    }
    (dir, store)
}

/// Steady-state insert cost into an already-populated (1,000-row) library —
/// the realistic case, not an empty-database best case. Each iteration
/// inserts one more new item, so the store grows past 1,000 rows over the
/// course of the benchmark; that's expected and still representative of
/// §9.1(b)'s "1,000+ items" scale.
fn bench_insert_item(c: &mut Criterion) {
    let (_dir, store) = seeded_store(SEED_ROWS);
    let mut next_id = SEED_ROWS;
    c.bench_function("gist_store/insert_item_1000row_store", |b| {
        b.iter(|| {
            let doc = make_document(next_id);
            next_id += 1;
            store.insert_item(&doc).unwrap();
        })
    });
}

/// `list_items` (the paged library API §9.1(b) calls out) at three offsets
/// across a 1,000-row store: the first page, a middle page, and the last
/// page — `OFFSET` cost in SQLite can grow with offset size, so all three
/// matter, not just the cheap first-page case.
fn bench_list_items(c: &mut Criterion) {
    let (_dir, store) = seeded_store(SEED_ROWS);
    let mut group = c.benchmark_group("gist_store/list_items_1000row_store");
    for &offset in &[0usize, 500, 950] {
        group.bench_function(format!("offset_{offset}_limit_50"), |b| {
            b.iter(|| store.list_items(offset, 50).unwrap())
        });
    }
    group.finish();
}

/// FTS5 `search_items` against a 1,000-row store — every seeded document's
/// body contains the word "performance", so this measures real FTS5 match
/// cost, not just an empty-result fast path.
fn bench_search_items(c: &mut Criterion) {
    let (_dir, store) = seeded_store(SEED_ROWS);
    c.bench_function("gist_store/search_items_1000row_store", |b| {
        b.iter(|| store.search_items("performance", 50).unwrap())
    });
}

criterion_group!(
    benches,
    bench_insert_item,
    bench_list_items,
    bench_search_items
);
criterion_main!(benches);
