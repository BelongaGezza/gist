# ADR-006: Copy-on-import vs reference-in-place

**Status:** Accepted, implemented 2026-09-12
**Date:** 2026-09

## Decision

When a user imports a file, GIST **copies** the source file into its sandboxed
storage directory at import time:

```
<storage_dir>/originals/<content-hash>.<ext>
```

`<content-hash>` is the SHA-256 of the file bytes, hex-encoded. This resolves
open question Q6 from the development plan.

`Metadata.source_ref` stores the original import path for provenance display
only. `Metadata.source_copy_ref` references the copy inside `<storage_dir>`.

**Naming note (2026-09-12):** this ADR originally named the copy-path field
`Metadata.doc_path`. By the time this was implemented, `doc_path` already had
an established, different meaning in the live schema — the path to the
serialised IR JSON blob (`<storage_dir>/<id>.json`). Rather than overload
that name, the implementation uses `Metadata.source_copy_ref` (document
model) / `library_items.source_copy_path` (SQLite schema) for the sandboxed
copy described here. This is a naming clarification only — the decision and
directory layout are otherwise unchanged from the original ADR.

**Scope:** applies to file-based imports (`Core::import_txt`,
`Core::import_file` — txt/epub/docx today, pdf/OCR-image images in later
milestones). `Core::import_url` has no local file to copy — the fetched,
extracted content is the only artifact that exists — so `source_copy_ref`
stays `None` for URL-imported items, and `source_ref` (the URL) remains their
only provenance record. `delete_source_files` on removal has nothing to do
for such items.

## Reasoning

- **iOS sandboxing**: iOS grants access to a user-selected file via a
  security-scoped bookmark that may be revoked at any time (app update, file
  moved, bookmark expired). A permanent copy requires no ongoing permission.
- **Portability**: the library database and its `originals/` directory can be
  transferred to another device and opened without any reference to the source
  filesystem.
- **Dangling-reference elimination**: if the user moves or deletes the original
  file, the imported copy is unaffected. Reference-in-place would produce a
  silent read failure at the worst possible moment (mid-session).
- **Simplicity**: the reader layer always opens `doc_path` (the serialised IR);
  there is no runtime branch for "is the original file still reachable?" and
  no bookmark-renewal logic.
- **Content-addressed storage**: using the content hash as the filename
  deduplicates re-imports of the same file automatically.

## Consequences

- `source_ref` in `Metadata` is informational only and is never used for
  reading or deletion. The UI may display it as "Imported from
  ~/Downloads/book.epub".
- `source_copy_ref`/`source_copy_path` is the only file-path field that
  removal (`Core::remove_items` with `delete_source_files: true`) is allowed
  to delete. It must never fall back to deleting `source_ref`'s raw path —
  that is the user's real file, at its real location, and GIST must never
  touch it.
- `doc_path` (the serialised IR blob) always resolves within `<storage_dir>`
  and is stable for the lifetime of the library; it is a separate concept
  from the original-file copy this ADR governs.
- The storage management UI (spec §4) must expose a "discard original copy"
  action so users can recover space; deletion removes both the copy and the
  `library_items` row (cascading to tokens and FTS index) — implemented at
  the `gist-store`/`gist-core` level as of 2026-09-12; no UI yet (tracked in
  the development plan's M2 section).
- Import cost increases by one file copy; for typical ePubs and PDFs
  (< 50 MB) this is imperceptible. Large files may be noticeable on slow
  storage.
- Reference-in-place is not supported. If a future use-case requires it (e.g.
  a network-mounted library), a new ADR must supersede this one.
- **Known limitation — dedup vs. deletion:** two library items imported from
  byte-identical content share one file under `originals/` (that's the point
  of content-addressing). Nothing currently reads document content from that
  path — reads always go through the separate serialised IR at `doc_path` —
  so sharing it is safe today. But removing one such item with
  `delete_source_files: true` deletes the shared file while the other item's
  row still references it. Harmless under current behaviour (nothing depends
  on the file surviving), but would need reference counting — e.g. skip
  deletion while another `library_items` row still has the same
  `source_copy_path` — before any future feature relies on that file
  persisting for a surviving item.
