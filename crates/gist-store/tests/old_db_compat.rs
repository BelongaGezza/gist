//! Backward-compatibility safety net for the `rusqlite` 0.31 → 0.40 bump
//! (bundled SQLite 3.45.0 → 3.53.2).
//!
//! The risk this guards is not an API break — those the compiler catches — but
//! a silent *data* break: a user who already has a GIST library on disk must
//! still be able to open it, migrate it, read it and search it after the
//! upgrade. Nothing in `cargo test`'s normal path covers that, because every
//! other test creates its database from scratch with the *current* SQLite.
//!
//! So these tests run against two real database files checked in under
//! `tests/fixtures/`, both produced by the OLD stack (rusqlite 0.31 /
//! libsqlite3-sys 0.28 / bundled SQLite 3.45.0) — see that directory's
//! README.md for exactly how they were generated. They are opened read/write
//! from a temp-dir copy, never in place.
//!
//! What is deliberately exercised beyond "it opens":
//!   * the FTS5 **external-content** index (ADR-008) built by SQLite 3.45 is
//!     still queryable by 3.53 — the highest-risk surface here, since FTS5's
//!     shadow tables are an on-disk format, not just an API;
//!   * the `tokens_ad` delete trigger + `ON DELETE CASCADE` still fire
//!     correctly against rows and index entries written by the old version;
//!   * the v3 → v4 → v5 `ALTER TABLE` migrations run successfully on a file
//!     created by the old version (fixture B is a v3-era library, from before
//!     `source_copy_path`/`content_encrypted` existed);
//!   * new writes interleave with old rows in the same index.

use std::path::{Path, PathBuf};

use gist_store::Store;
use rusqlite::Connection;

/// Copy a checked-in fixture database into a fresh temp dir and return
/// `(tempdir, db_path, storage_dir)`. The committed file is never opened
/// directly — opening it would migrate/modify it in place.
fn fixture_copy(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(src.exists(), "missing fixture {name}");

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    std::fs::copy(&src, &db).unwrap();
    let storage = dir.path().join("storage");
    (dir, db, storage)
}

fn user_version(db: &Path) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn titles(store: &Store) -> Vec<String> {
    let mut t: Vec<String> = store
        .list_items(0, 100)
        .unwrap()
        .into_iter()
        .filter_map(|i| i.title)
        .collect();
    t.sort();
    t
}

/// Resolve a title to its id, so assertions never depend on the fixture's
/// (UUIDv7, therefore generation-time-dependent) ids.
fn id_of(store: &Store, title: &str) -> String {
    store
        .list_items(0, 100)
        .unwrap()
        .into_iter()
        .find(|i| i.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no item titled {title}"))
        .id
}

// ── fixture A: an already-current (v5) library written by the old stack ──────

#[test]
fn old_v5_library_opens_reads_and_searches() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    let store = Store::open(&db, &storage).expect("an existing v5 library must still open");

    // Already at the current schema version: nothing to migrate, and in
    // particular the version-ceiling check must not trip.
    assert_eq!(user_version(&db), 5);

    assert_eq!(
        titles(&store),
        vec!["Frankenstein", "Moby Dick", "Treasure Island"]
    );

    // Metadata columns survive intact, including the two added by v4/v5.
    let moby = store
        .list_items(0, 100)
        .unwrap()
        .into_iter()
        .find(|i| i.title.as_deref() == Some("Moby Dick"))
        .unwrap();
    assert_eq!(moby.authors, vec!["Herman Melville".to_string()]);
    assert!(!moby.content_encrypted);

    // Reading progress written by the old version reads back unchanged.
    let treasure = id_of(&store, "Treasure Island");
    assert_eq!(store.get_progress(&treasure).unwrap(), 7);
}

#[test]
fn fts5_index_written_by_sqlite_3_45_is_queryable() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    let store = Store::open(&db, &storage).unwrap();

    // Whole-word match against the old external-content index.
    let hits = store.search_items("Ishmael", 10).unwrap();
    assert_eq!(hits, vec![id_of(&store, "Moby Dick")]);

    // Prefix match (`escape_fts5_query` appends `*`) — the search-as-you-type
    // path that F19's follow-up fix added.
    let hits = store.search_items("Ishma", 10).unwrap();
    assert_eq!(hits, vec![id_of(&store, "Moby Dick")]);

    // The `porter unicode61` tokenizer's stemming still applies across the
    // version change (an index stemmed by 3.45, queried by 3.53).
    let hits = store.search_items("rejoicing", 10).unwrap();
    assert_eq!(hits, vec![id_of(&store, "Frankenstein")]);

    // A term in no document still returns nothing rather than erroring.
    assert!(store.search_items("zzzznotpresent", 10).unwrap().is_empty());
}

#[test]
fn collections_and_tags_written_by_old_version_read_back() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    let store = Store::open(&db, &storage).unwrap();

    let mut names: Vec<String> = store
        .list_collections()
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["Classics", "Nautical"]);

    let classics = store
        .list_collections()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Classics")
        .unwrap();
    let mut in_classics: Vec<String> = store
        .list_items_in_collection(&classics.id)
        .unwrap()
        .into_iter()
        .filter_map(|i| i.title)
        .collect();
    in_classics.sort();
    assert_eq!(in_classics, vec!["Moby Dick", "Treasure Island"]);

    assert_eq!(
        store.list_all_tags().unwrap(),
        vec!["adventure", "gothic", "pirates"]
    );

    let mut tagged: Vec<String> = store
        .list_items_by_tag("adventure")
        .unwrap()
        .into_iter()
        .filter_map(|i| i.title)
        .collect();
    tagged.sort();
    assert_eq!(tagged, vec!["Moby Dick", "Treasure Island"]);

    let mut t = store
        .list_tags_for_item(&id_of(&store, "Treasure Island"))
        .unwrap();
    t.sort();
    assert_eq!(t, vec!["adventure", "pirates"]);
}

#[test]
fn removing_last_use_of_an_old_tag_drops_it_from_list_all_tags() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    let store = Store::open(&db, &storage).unwrap();

    // "pirates" is only on Treasure Island; "gothic" is on some other item.
    let treasure = id_of(&store, "Treasure Island");
    store.remove_tag(&treasure, "pirates").unwrap();
    assert_eq!(store.list_all_tags().unwrap(), vec!["adventure", "gothic"]);

    // Re-adding the orphaned name reuses the old `tags` row and shows again.
    store.add_tag(&treasure, "pirates").unwrap();
    assert_eq!(
        store.list_all_tags().unwrap(),
        vec!["adventure", "gothic", "pirates"]
    );
}

#[test]
fn writes_under_the_new_version_interleave_with_old_rows() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    let store = Store::open(&db, &storage).unwrap();

    let section = gist_model::Section {
        id: "s0".to_string(),
        heading: None,
        blocks: vec![gist_model::Block::Paragraph {
            runs: vec![gist_model::TextRun::plain(
                "a newly imported paragraph mentioning quicksilver".to_string(),
            )],
        }],
    };
    let doc = gist_model::Document::new(gist_model::Metadata::minimal("Newcomer"), vec![section]);
    let new_id = doc.id.clone();
    store.insert_item(&doc).unwrap();

    // New tokens are indexed into the pre-existing (3.45-written) FTS index…
    assert_eq!(store.search_items("quicksilver", 10).unwrap(), vec![new_id]);
    // …and the old entries in that same index still match.
    assert_eq!(
        store.search_items("Ishmael", 10).unwrap(),
        vec![id_of(&store, "Moby Dick")]
    );

    // Tagging/collecting an old row from the new version works too.
    let treasure = id_of(&store, "Treasure Island");
    store.add_tag(&treasure, "reread").unwrap();
    let mut t = store.list_tags_for_item(&treasure).unwrap();
    t.sort();
    assert_eq!(t, vec!["adventure", "pirates", "reread"]);
}

#[test]
fn removing_an_old_row_still_cascades_and_clears_the_old_fts_entries() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    let store = Store::open(&db, &storage).unwrap();

    let moby = id_of(&store, "Moby Dick");
    let removed = store.remove_items(&[moby.clone()]).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].id, moby);

    // The `tokens_ad` trigger must have pushed a 'delete' into the FTS index
    // that SQLite 3.45 built; if the external-content shadow tables were not
    // read/written compatibly this is where it would show up.
    assert!(
        store.search_items("Ishmael", 10).unwrap().is_empty(),
        "FTS entries for a removed item must be gone"
    );
    // Other items' entries are untouched.
    assert_eq!(
        store.search_items("rum", 10).unwrap(),
        vec![id_of(&store, "Treasure Island")]
    );

    // ON DELETE CASCADE (PRAGMA foreign_keys = ON) reached every child table.
    let conn = Connection::open(&db).unwrap();
    for (table, col) in [
        ("tokens", "item_id"),
        ("reading_progress", "item_id"),
        ("item_collections", "item_id"),
        ("item_tags", "item_id"),
    ] {
        let n: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {col} = ?1"),
                [&moby],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "{table} rows should have cascaded away");
    }

    // And the database is structurally sound afterwards, FTS shadow tables
    // included (`integrity_check` walks them).
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
}

// ── fixture B: a v3-era library, so the migrations themselves are exercised ──

#[test]
fn old_v3_library_migrates_forward_to_v5() {
    let (_dir, db, storage) = fixture_copy("library_v3_rusqlite031.db");
    assert_eq!(user_version(&db), 3, "fixture must start at v3");

    let store = Store::open(&db, &storage).expect("a v3 library must migrate, not fail");
    assert_eq!(
        user_version(&db),
        5,
        "must have migrated to the current version"
    );

    // The v4/v5 ALTER TABLE columns exist and carry their intended defaults
    // for a row that predates them.
    let item = store
        .list_items(0, 10)
        .unwrap()
        .into_iter()
        .next()
        .expect("the legacy row survived the migration");
    assert_eq!(item.title.as_deref(), Some("Legacy Volume"));
    assert_eq!(item.authors, vec!["Ada Lovelace".to_string()]);
    assert!(
        !item.content_encrypted,
        "pre-v5 rows must default to unencrypted"
    );

    let removed_paths = {
        let conn = Connection::open(&db).unwrap();
        conn.query_row(
            "SELECT source_copy_path IS NULL FROM library_items WHERE id = ?1",
            [&item.id],
            |r| r.get::<_, bool>(0),
        )
        .unwrap()
    };
    assert!(
        removed_paths,
        "pre-v4 rows must have a NULL source_copy_path"
    );

    // The FTS index and the collection/tag rows written at v3 survive.
    assert_eq!(
        store.search_items("analytical", 10).unwrap(),
        vec![item.id.clone()]
    );
    assert_eq!(store.list_all_tags().unwrap(), vec!["history"]);
    assert_eq!(
        store
            .list_collections()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>(),
        vec!["Legacy Shelf"]
    );

    // Re-opening an already-migrated file is a no-op, not a second migration.
    drop(store);
    let store = Store::open(&db, &storage).unwrap();
    assert_eq!(user_version(&db), 5);
    assert_eq!(store.list_items(0, 10).unwrap().len(), 1);
}

/// The on-disk *file format* must stay in the range old SQLite can also read,
/// so a user who downgrades (or runs an older build alongside) is not locked
/// out. Bytes 18 and 19 of the header are the write/read format versions:
/// 1 = rollback journal, 2 = WAL. SQLite has supported 2 since 3.7.0 (2010),
/// well below the 3.45 floor this project is upgrading from. Anything higher
/// would mean the new bundled SQLite had started writing a format the old one
/// cannot open — the thing that would actually lose a user's library.
#[test]
fn new_version_keeps_writing_a_downgrade_readable_file_format() {
    let (_dir, db, storage) = fixture_copy("library_v5_rusqlite031.db");
    {
        let store = Store::open(&db, &storage).unwrap();
        let doc = gist_model::Document::new(gist_model::Metadata::minimal("Format Probe"), vec![]);
        store.insert_item(&doc).unwrap();
        // Fold the WAL back into the main file so the header we read is the
        // one a downgraded build would see.
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
    }

    let header = std::fs::read(&db).unwrap();
    assert!(header.len() > 20);
    assert_eq!(
        &header[..16],
        b"SQLite format 3\0",
        "still a standard SQLite 3 file"
    );
    assert!(
        header[18] <= 2 && header[19] <= 2,
        "write/read format versions must stay <= 2 (got {} / {})",
        header[18],
        header[19]
    );
}
