# ADR-003: Annotation anchoring

**Status:** Accepted
**Date:** 2026-09

## Decision

Anchor annotations as `(block_id, start, len, prefix_hash, quote_hash)` tuples
with re-anchoring on hash mismatch.

## Problem

Parser improvements, re-imports, and document updates change the text under
existing highlights. Without content-fingerprinting, annotations silently point
to wrong text or nowhere.

## Approach

- `block_id`: stable UUID v7 assigned at block creation
- `start`, `len`: byte offsets within the block's plain text at anchoring time
- `prefix_hash`: FNV-1a of the 30 characters before `start` — detects shifted
  context
- `quote_hash`: FNV-1a of the highlighted text itself
- On load: verify both hashes. If `prefix_hash` mismatches, search for
  `quote_hash` match within the same block (text shifted). If `quote_hash` not
  found, mark annotation orphaned — surface in UI.

## Consequences

- ~2 days implementation cost at M0
- Prevents silent data-loss bug class that is unrecoverable once annotations
  accumulate
- Property tests should perturb documents and verify re-anchoring finds the
  correct position

## Addendum: `block_id` is a Section id, not a per-block UUID (2026-09-26)

Implemented in `gist_core::anchoring` (re-anchoring logic) and the M3
annotations backend scaffold (`39c0942`, CRUD). Two details below deviate from
this ADR's original wording, discovered during implementation because
`gist_model::Block` turned out to have no identity of its own — only
`Section` does:

- **`block_id` addresses a `Section`, not an individual block.** A section can
  hold multiple blocks, so `start`/`len` are byte offsets into the whole
  section's concatenated plain text (`gist_core::anchoring::section_text`,
  every block's `plain_text()` joined with `"\n\n"`), not one block's text in
  isolation. This lets a highlight's `prefix_hash` context correctly span a
  block boundary (e.g. a highlight starting at the first word of a paragraph,
  whose preceding context is the end of the previous block).
- **Re-anchoring searches the whole section's text for a `quote_hash` match,
  not just "the same block".** Same reasoning — "block" in this codebase
  means "section" for anchoring purposes.
- **The `"\n\n"` join separator is load-bearing, not cosmetic.** The Swift
  reading view (`FlowSectionVM.concatenatedPlainText`,
  `apps/apple/Shared/FlowDocumentModel.swift`) computes `start`/`len`/hashes
  against this exact same concatenation when creating an annotation, and must
  keep matching `section_text`'s separator byte-for-byte — a mismatch here
  doesn't fail loudly, it just makes every annotation past the first block in
  its section silently misanchor. This was caught as a real integration bug
  during M3 (the two sides were built concurrently in isolated worktrees and
  independently chose `"\n\n"` vs. `"\n"`) and fixed by aligning Swift to
  Rust's convention — see the doc comments on both functions for the
  cross-reference.

Otherwise the ADR's decision stands as designed: verify `prefix_hash` first,
fall back to a `quote_hash` search, mark orphaned (never error) if neither is
found. `Core::reanchor_annotations`/`GistCore::reanchor_annotations`
(`ffi_catch!`-wrapped) expose `AnchorStatus`/`FfiAnchorStatus`
(`Valid`/`Reanchored`/`Orphaned`) for the UI, which surfaces `.orphaned` as a
badge in the annotations sidebar (`apps/apple/Shared/AnnotationsSidebarView.swift`).
Property tests perturbing documents (shift, delete-around, delete-through,
missing-block) exist in `crates/gist-core/src/lib.rs`, per this ADR's own
closing note.
