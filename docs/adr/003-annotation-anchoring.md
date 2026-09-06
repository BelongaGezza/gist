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
