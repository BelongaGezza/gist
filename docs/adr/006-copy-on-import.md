# ADR-006: Copy-on-import vs reference-in-place

**Status:** Accepted
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
only. `Metadata.doc_path` references the copy inside `<storage_dir>`.

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
- **Simplicity**: the reader layer always opens `doc_path`; there is no runtime
  branch for "file still reachable?" and no bookmark-renewal logic.
- **Content-addressed storage**: using the content hash as the filename
  deduplicates re-imports of the same file automatically.

## Consequences

- `source_ref` in `Metadata` is informational only and is not used for reading.
  The UI may display it as "Imported from ~/Downloads/book.epub".
- `doc_path` always resolves within `<storage_dir>` and is stable for the
  lifetime of the library.
- The storage management UI (spec §4) must expose a "discard original copy"
  action so users can recover space; deletion removes both `doc_path` and the
  `library_items` row (cascading to tokens and FTS index).
- Import cost increases by one file copy; for typical ePubs and PDFs
  (< 50 MB) this is imperceptible. Large files may be noticeable on slow
  storage.
- Reference-in-place is not supported. If a future use-case requires it (e.g.
  a network-mounted library), a new ADR must supersede this one.
