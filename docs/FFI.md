# GIST — FFI boundary

`crates/gist-ffi` is the only crate the native shells link. It wraps `gist-core` with
[uniffi](https://mozilla.github.io/uniffi-rs/) in proc-macro mode (ADR-001) and is built as a
`staticlib` (Apple), a `cdylib` (Windows DLL, host tooling) and an `rlib`.

This page is the short map. The authoritative detail is the source (`crates/gist-ffi/src/lib.rs`),
the ADRs and `CLAUDE.md`'s "Security policies". It was an empty file until 2026-10-09.

## Shape

- **One main object, `GistCore`.** Constructors: `new(db_path, storage_dir)` (plaintext),
  `new_encrypted(...)` (write-auto-encrypt) and `new_with_read_key(...)` (reads encrypted items, new
  imports stay plaintext; this is what the production macOS and Windows apps use, ADR-014).
- **A second object, `FfiRsvpSession`,** wraps the pure `gist-rsvp` engine. Shells call
  `frame_at_elapsed`, `pause`, `resume`, `seek`, `set_wpm`, `set_pause_on_punctuation`, `back_words`
  and `stats_*` with their own monotonic clock; there is no timer in Rust and no second copy of the
  pacing maths in any shell (Apple since 2026-10-09, Windows since W4).
- **Operation groups on `GistCore`:** import (`import_txt`, `import_file`, `import_url`,
  `import_image_with_ocr`), library (`list_items` paged, `search_items`, `remove_items`,
  `remove_items_detailed`, `sweep_orphaned_files`), collections and tags, reading state
  (`start_rsvp`, `open_rsvp_session`, `save_progress`, `mark_item_opened`, `get_document_json`),
  annotations (CRUD plus `reanchor_annotations`), and security (`encrypt_items`,
  `verify_item_integrity`, `verify_library_integrity`).
- **Callback interfaces implemented by the shell:**
  - `OcrEngine` (ADR-009): `recognize_page(page_index, image_bytes) -> Option<OcrPageResult>`;
    `None` means cancelled. Vision on Apple, `Windows.Media.Ocr` planned for Windows (ADR-020).
  - `KeyProvider` (ADR-011/016): returns the 32-byte at-rest key from the Keychain (Apple) or DPAPI
    (Windows). The callback has no error channel, so shells must acquire the key eagerly before
    constructing the core (ADR-016).
- **Errors:** one flat `GistError` (`flat_error`). Variants the UI switches on instead of parsing
  text: `DrmProtected`, `ChecksumMismatch`, `PdfEncrypted`, `PdfNoTextLayer`, `PdfUnavailable`, seven
  `ResourceLimit*` cases (one per `LimitKind`), and `InternalPanic`. Messages never carry user file
  paths.

## Safety rules (must not be relaxed)

1. Every `#[uniffi::export]` body runs inside `ffi_catch!`, which wraps `catch_unwind` and maps a
   panic to `GistError::InternalPanic`. A panic must never cross the C ABI.
2. The release profile keeps `panic = "unwind"`. `panic = "abort"` would defeat rule 1 (the original
   spec required abort; that was a bug, corrected 2026-09-20). CI proves containment in release mode
   with `cargo run --release -p gist-ffi --features test-panic --example panic_containment`.
3. A default panic hook is replaced by one that logs at `debug!`, so payloads and paths do not reach
   stderr (`F22`).
4. Source paths appear only in `debug!` logs.
5. The shipped Apple link path selects `libgist_ffi.a` by explicit path in
   `tools/build-core-xcframework.sh`. Do not refactor it to `-l`/search-path linking: the `cdylib`
   emitted alongside would be preferred and rejected by Hardened Runtime on a signed build (`N8`).

## Generating bindings

| Target | Script | Output (never committed) |
|---|---|---|
| Swift | `tools/gen-bindings.sh`, `tools/build-core-xcframework.sh` | `apps/apple/Generated/`, `GistCore.xcframework` |
| C# | `tools/gen-bindings-cs.sh` (pinned, reviewed `uniffi-bindgen-cs`; ADR-015) | `apps/windows/GIST.Core/Generated/` |
| Kotlin | `tools/gen-bindings-kt.sh` (Android plan, ADR-025; spike only) | caller-chosen directory |

Regenerate after any change to an exported signature, then rebuild the shell. A stale
`Generated/` directory is the usual cause of "method not found" errors in a shell.
