# ADR 019 — IR/schema versioning and forward-compatibility policy

**Date:** 2026-09-29
**Status:** Accepted, implemented and tested

## Context

`CLAUDE.md`'s open-questions table has carried **Q10** ("Schema/IR versioning and forward compatibility policy," due "M4 start") unresolved since M2. `docs/development-plan-v2.md` §7 marks it *"Required before first public beta"* — and M5 (this milestone) is the beta milestone, so it can no longer be deferred.

Confirmed directly from source before this ADR was written (2026-09-29, main-line `integration/m4-2026-09-28`):

- `gist_model::Document` (`crates/gist-model/src/lib.rs`) has no version field of any kind — neither on `Document` itself nor anywhere in its `Section`/`Block`/`Token`/`Metadata` graph.
- `gist-store`'s `Store::insert_item` wrote `<id>.json`/`<id>.tokens.json` (ADR-007) via plain `serde_json::to_string(doc)`/`serde_json::to_string(&doc.token_stream)`, and `Store::get_item`/`Store::get_tokens` read them back via plain `serde_json::from_slice`. No version check of any kind guarded this path.
- This is a genuinely different gap from `SCHEMA_VERSION`/`StoreError::SchemaTooNew` (`crates/gist-store/src/lib.rs`), which only covers the SQLite schema's own shape (tables/columns) — not the JSON *content* of the IR blobs those tables point at. A future GIST binary could bump `SCHEMA_VERSION` correctly and still silently misinterpret an IR blob whose *JSON* shape had changed incompatibly since it was written, because nothing was checking that at all.
- No type in `gist-model` sets `#[serde(deny_unknown_fields)]` (confirmed by grep across `crates/`), and at least one field (`Metadata::source_copy_ref`, added post-hoc by ADR-006) already relies on `#[serde(default)]` for exactly the additive-compatibility behavior this ADR is about to formalize as policy rather than accident.

Nothing has ever shipped a second IR shape, so there is no concrete incompatibility to fix today. This ADR is preventative: it puts a versioning and rejection mechanism in place *before* the first breaking IR change happens, so that change doesn't have to invent this mechanism under time pressure while simultaneously making the change itself, and so existing users' on-disk libraries have a documented guarantee about what happens when they upgrade.

## Decision

### 1. An explicit `ir_version: u32` field — at the storage boundary, not on `Document`

The version field is **not** added to `gist_model::Document` (or any other `gist-model` type). Instead, `gist-store` wraps every IR blob it writes in a thin envelope at write time and unwraps it at read time:

```json
{"ir_version": 1, "payload": { ...the Document or Vec<Token>, exactly as before... }}
```

Implemented as two private, non-generic-boilerplate helpers in `crates/gist-store/src/lib.rs`:

```rust
const CURRENT_IR_VERSION: u32 = 1;

#[derive(Serialize)]
struct IrEnvelopeRef<'a, T> { ir_version: u32, payload: &'a T }

fn serialize_ir_blob<T: Serialize>(payload: &T) -> Result<String, StoreError>;
fn deserialize_ir_blob<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError>;
```

`Store::insert_item` calls `serialize_ir_blob` for both the document and token-stream blobs (replacing the previous bare `serde_json::to_string` calls); `Store::get_item`/`Store::get_tokens` call `deserialize_ir_blob` (replacing the previous bare `serde_json::from_slice` calls).

**Why the storage boundary, not `Document` itself:** this file's own crate conventions (`CLAUDE.md`) require `gist-model` to stay `wasm32-unknown-unknown`-compilable with no I/O dependencies. Versioning a *stored* format is an I/O-boundary concern — it only matters at the moment something is written to or read from disk — not a property of the in-memory IR graph itself, which `gist-model` exists to define. Putting `ir_version` on `Document` would mean every consumer of the type (RSVP pacing, annotation anchoring, the FFI layer, a future wasm build) has to carry a field that's meaningless to all of them except the one crate that persists it. Keeping the envelope entirely inside `gist-store` means `gist_model::Document`/`Token`/`Section`/`Block`/`Metadata` are completely unchanged by this ADR — zero risk to the wasm32 target, and no new field for every existing consumer to thread through or ignore.

A consequence worth naming: this means `ir_version` is invisible to anything above `gist-store` — `gist-core`, `gist-ffi`, and Swift/C# never see it and never need to. That is intentional. If a future feature needs to *expose* "what IR version is this document" to a caller (unclear whether one ever will), that's a new, separate decision to add a read-only accessor — not a reason to revisit where the field lives.

**Human-readability preserved (ADR-007):** the envelope is still a single flat JSON object with two keys — `ir_version` and `payload` — so `<id>.json`/`<id>.tokens.json` remain exactly as inspectable with a plain text editor as ADR-007 requires. Nothing about this decision reintroduces a binary format or requires tooling to read.

### 2. Forward compatibility: reject, don't guess

`deserialize_ir_blob` checks `ir_version` *before* attempting to interpret `payload` as `T` at all:

1. Parse the raw bytes as an untyped `serde_json::Value` first.
2. If a top-level `"ir_version"` key exists and its value exceeds `CURRENT_IR_VERSION`, return the new `StoreError::IrVersionTooNew { found, expected }` immediately. `payload` is never touched, deserialized, or partially interpreted.
3. Otherwise, deserialize `payload` (or, for a legacy blob — see below — the whole value) into `T` as normal.

This deliberately mirrors `StoreError::SchemaTooNew`'s shape and rationale for the SQLite schema: a newer binary might restructure `payload` in a way today's `T` cannot parse at all, or — worse — in a way it can *technically* parse but would silently misinterpret (e.g. a field whose meaning changed, not just a field that was added). Checking the version first and refusing outright, rather than attempting the parse and only reacting if it happens to fail, means GIST never has a chance to render a document it half-understood as if it were fully understood. A malformed/non-numeric `ir_version` value is treated as "at least as new as we can't rule out" and also rejected via `IrVersionTooNew`, rather than falling through to a payload-parse attempt whose resulting error would be far less informative about what actually went wrong.

### 3. Backward compatibility: additive changes need no version bump at all

Confirmed and locked in by tests (see below): a `Document`/`Section`/`Block`/`Token`/`Metadata` gaining a new field with `#[serde(default)]` is silently forward- *and* backward-compatible without touching `CURRENT_IR_VERSION`:

- An **old** blob (missing the new field's key entirely) still deserializes into the **new** type — `#[serde(default)]` fills it in.
- A **new** blob (carrying a field an **old** binary's type doesn't know about) still deserializes into the **old** type — serde's default behavior silently ignores unknown fields unless `#[serde(deny_unknown_fields)]` is set, which is confirmed (by grep) to be the case for every type in `gist-model`'s IR graph, and is now a documented, deliberate policy rather than an accidental absence.

**Policy going forward:** a purely additive change to the IR graph (a new optional field with `#[serde(default)]`, a new enum variant handled additively — see the caveat below on enum variants) does **not** require bumping `CURRENT_IR_VERSION`. Only a change that an older binary would *misinterpret* rather than merely *not see* — a field's meaning changing, a field being removed or renamed, a required field being added with no sensible default, or a restructuring of `Block`/`Section`'s shape — requires a bump. This keeps `ir_version` bumps rare and meaningful, matching `SCHEMA_VERSION`'s own bump discipline (v3→v6 over this project's history, not one per PR).

One caveat this ADR flags but does not resolve, since it's a pre-existing property of every `enum` in `gist-model` (`Block`, `TokenKind`, `AnnotationKind`) rather than something new here: `serde`'s default *externally tagged* enum representation errors on an unrecognized variant name, unlike an unrecognized *struct field*. Adding a new `Block` variant (e.g. a future `Block::Table`) is therefore **not** silently additive the way a new struct field is — an older binary reading a document containing that variant would fail to deserialize the whole blob, not just skip the one block. This is a real, sharp edge for the "first breaking change" scenario below to be aware of; not fixed here because retrofitting every enum to a `#[serde(other)]`-style fallback (which still loses the unrecognized variant's data, just without erroring) is a nontrivial design decision of its own and no such variant addition is on the table today.

### 4. Migration story for the first breaking IR change

Not built now — nothing exists to migrate yet — but specified so it isn't re-litigated from scratch:

When the first genuinely breaking IR change happens, `CURRENT_IR_VERSION` bumps to `2`, and `deserialize_ir_blob`'s dispatch grows a match on the parsed `ir_version` instead of a single "current or reject" check:

```rust
match found_version {
    1 => { /* deserialize payload as DocumentV1, migrate in-memory to current shape */ }
    2 => { /* deserialize payload as the current Document type directly */ }
    v if v > CURRENT_IR_VERSION => Err(StoreError::IrVersionTooNew { found: v, expected: CURRENT_IR_VERSION }),
    _ => unreachable!("no version below 1 is ever written"),
}
```

i.e. a versioned enum of decoders (`IrPayload::V1(DocumentV1)` / `IrPayload::V2(Document)`, or equivalently a match arm per known version, whichever reads more clearly once there's a second real variant to look at) — old-shape blobs read via their own point-in-time type and get migrated in memory to the current `Document` shape on load; new writes always use the current shape and `CURRENT_IR_VERSION`. Whether a migrated-in-memory document gets **rewritten to disk in the new shape** on next save (lazy migration) or left as-is until the item is otherwise touched is a decision for whoever makes that first breaking change — both are consistent with this ADR, which only commits to *detecting* the version and *having a place to dispatch on it*, not to a specific rewrite-eagerness policy. `gist-store`'s existing "no eager backfill" precedent (ADR-011/ADR-013's per-row/per-file migration flags, no bulk rewrite pass) is the natural default to reach for, but that's advisory, not binding.

## What was implemented

- `crates/gist-store/src/lib.rs`: `StoreError::IrVersionTooNew { found: u32, expected: u32 }`; `CURRENT_IR_VERSION`; `IrEnvelopeRef<'a, T>`; `serialize_ir_blob`/`deserialize_ir_blob`. `Store::insert_item` now writes both IR blobs through `serialize_ir_blob`; `Store::get_item`/`Store::get_tokens` (including its full-document fallback branch) now read through `deserialize_ir_blob`.
- No `gist-model` change — by design, per the decision above.
- No `SCHEMA_VERSION` bump — this is a JSON-blob-format concern, not a SQLite-schema concern, exactly the same separation ADR-013 already established for checksum sidecars.
- Two existing `gist-store` tests that inspected on-disk plaintext bytes directly (`migration_old_plaintext_item_readable_after_switching_to_encrypted_store`, `encrypt_item_then_read_through_same_read_capable_store_succeeds`) were updated to parse via `deserialize_ir_blob` instead of bare `serde_json::from_slice::<Document>`, since a freshly-inserted item's on-disk bytes are now the ADR-019 envelope, not a bare `Document` object — their actual assertion ("this is plaintext, not ciphertext") is unchanged and still passes.
- Test coverage — `crates/gist-store/src/lib.rs`, new `tests` under "IR envelope versioning (ADR-019, Q10)":
  - `ir_envelope_current_version_blob_round_trips` — a normally-inserted item round-trips through `get_item`/`get_tokens`, and its on-disk bytes are confirmed to actually be the new `{"ir_version":1,"payload":...}` shape (not just "it still works," but "it works via the mechanism this ADR describes").
  - `ir_envelope_future_version_is_rejected_with_typed_error_not_a_panic` — a hand-written blob with `"ir_version": 9999` and a deliberately-invalid `payload` (proving the version check runs *before* any payload interpretation) is rejected with `Err(StoreError::IrVersionTooNew { found: 9999, expected: CURRENT_IR_VERSION })`, not a panic or a generic JSON error.
  - `ir_envelope_extra_unknown_payload_field_still_deserializes` — a current-version blob with an extra, never-seen `payload` field still deserializes successfully (simulating a hypothetical newer-but-additively-decodable version).
  - **`pre_adr019_unenveloped_blob_is_still_readable`** and **`pre_adr019_unenveloped_doc_blob_readable_via_get_tokens_fallback`** — the most important tests in this change. A blob written in the exact bare, pre-ADR-019 shape (no envelope at all — precisely what every binary before this commit wrote) is confirmed readable through both `get_item` and `get_tokens`'s document-fallback path. **Existing users' on-disk libraries are not broken by this change.**
  - `deserialize_ir_blob_falls_back_correctly_for_both_object_and_array_legacy_shapes` — a unit-level check that the legacy fallback works correctly for both JSON shapes IR blobs actually take on disk (`Document` is an object, `Vec<Token>` is an array), since `serde_json::Value::get("ir_version")` needs to safely return `None` for both, not just one.
  - `metadata_json_missing_a_field_added_after_the_fact_deserializes_via_serde_default` (`gist-store`) and, at the pure-model level in `crates/gist-model/src/lib.rs`: `document_round_trips_through_json`, `document_json_with_an_extra_unknown_field_still_deserializes`, `metadata_missing_source_copy_ref_key_entirely_defaults_to_none` — confirming the backward/forward-compatibility policy in §3 holds both through `gist-store`'s envelope and independently at the `gist-model` layer.

## Verification

`cargo test --workspace` (all 206 tests across the workspace pass, including the existing `crates/gist-store/tests/old_db_compat.rs` real-fixture-database suite — unaffected by this change since those fixtures exercise SQLite schema migration, not IR blob content, and `crates/gist-core`'s existing annotation-reanchoring tests, which already bypass `insert_item` to write bare, unenveloped `Document` JSON directly to `<id>.json` to simulate an out-of-band edit — those continued passing unmodified, which is itself incidental extra evidence for the legacy-fallback path beyond the dedicated tests above), `cargo clippy --workspace -- -D warnings` (clean), `cargo fmt --check` (clean).

## Consequences

**Easier:** the first real breaking IR change has a documented mechanism to slot into (§4) instead of needing to invent versioning under time pressure. A corrupted-version scenario (a bug that writes a garbage `ir_version`, or a user's disk somehow getting a blob from a future GIST version via sync/backup) now fails clearly instead of either silently misreading or panicking.

**Harder / to watch:** `gist-store` is now the *only* place that knows the on-disk IR format differs from `gist_model::Document`'s in-memory shape — any future code that reads `<id>.json` directly (bypassing `Store::get_item`) needs to know to unwrap the envelope, or reuse `deserialize_ir_blob`. This is already true of every existing test/tool that pokes at these files directly (several exist in `gist-core`'s and `gist-store`'s own test suites, all updated or already-legacy-compatible as noted above) — a future contributor adding a new such call site should reuse the helper rather than re-deriving raw `serde_json::from_slice`. `CURRENT_IR_VERSION` also becomes a real piece of shared state to remember to bump — its own doc comment states the bump criterion (§3's "misinterpret vs. merely not see" test) to reduce the chance of either bumping too eagerly (defeating additive compatibility's whole point) or forgetting to bump for a genuinely breaking change.

## Addendum: `Block::Table` and the conditional version bump (2026-10-03, M6 R3)

§3 above flagged — without resolving — that a new `Block` *variant* is not additive the way a new struct field is. `Block::Table { rows, header_row }` is the first such variant, so this is the first real exercise of that caveat. Evidence first, decision second.

### Evidence (tests in `crates/gist-store/src/lib.rs`)

`mod pre_table` in the tests reproduces `Block`/`Section`/`Document` exactly as a pre-table binary knew them (four `Block` variants). Playing that "old binary":

- **Without any version bump** (`without_a_version_bump_an_old_binary_fails_with_an_opaque_json_error`): a document blob containing a table, stamped `ir_version` 1, fails to decode with serde's "unknown variant Table" error, surfaced as `StoreError::Serde`. The *whole document* is unreadable (not just the one block), and the user sees a generic JSON error instead of an "upgrade the app" signal. This confirms the caveat concretely.
- **`<id>.tokens.json` is unaffected** (`tokens_blob_of_a_table_document_is_v1_and_readable_by_old_binaries`): the token stream carries no `Block`, so an old binary can still run RSVP on a table document; only the flow view / annotation re-anchoring / anything reading `<id>.json` breaks.

### Decision: a *conditional* bump — yes, bump, but only for blobs that need it

`CURRENT_IR_VERSION` (the highest version this binary can **read**) becomes **2**. The **writer** stamps each blob with the *lowest* version able to represent it (`required_ir_version`):

| Blob | Written as |
|---|---|
| `<id>.json` of a document with **no** `Block::Table` | `ir_version: 1` — byte-compatible with every earlier binary, exactly as before |
| `<id>.json` of a document containing a `Block::Table` | `ir_version: 2` |
| `<id>.tokens.json` (always) | `ir_version: 1` (tokens contain no `Block`) |
| any legacy bare/unversioned blob | still read as implicitly v1 (unchanged) |

Consequences, each covered by a test: an old binary reading a table document now gets the **typed** `StoreError::IrVersionTooNew { found: 2, expected: 1 }` (`with_the_conditional_bump_an_old_binary_gets_a_typed_version_error`), the designed "upgrade the app" path, instead of the opaque JSON error; documents without tables are unaffected in both directions (`documents_without_tables_stay_v1_and_old_binaries_still_read_them`) so the bump has zero blast radius on existing content; and every blob written by any earlier binary — bare/unversioned or v1 — still loads (`legacy_and_v1_blobs_still_load_with_table_aware_types`, plus the pre-existing `pre_adr019_*` tests). `table_document_round_trips_and_cell_text_is_searchable` confirms the v2 blob round-trips and that FTS finds text that exists only in a table cell.

**Why conditional rather than a flat bump to 2:** a flat bump would lock every older binary out of every newly written document, including the vast majority with no table, for no benefit. The ADR's §3 criterion ("misinterpret vs. merely not see") is, for an enum variant, "an older binary fails outright" — so the version must rise for payloads containing it, but only for those.

**Policy going forward (supersedes §3's one-line caveat):** adding a variant to a serialized enum in the IR graph (`Block`, `TokenKind`, `AnnotationKind`) requires (1) a new `IR_VERSION_*` constant, (2) a matching check in `required_ir_version` so only payloads that actually contain the variant are stamped with it, and (3) a `pre_*`-style old-binary test like the above. Adding a struct field with `#[serde(default)]` still needs none of this. The `#[serde(other)]` fallback alternative (read the document with the unknown block dropped) was again not adopted: silently dropping content a user imported is worse than refusing with a clear message.

**Not changed:** `SCHEMA_VERSION` (no SQLite change), the migration mechanism in §4 (still unbuilt — v1→v2 needs no migration, since v1 is a strict subset of v2), `gist-model`'s `wasm32` posture (the model change is a plain enum variant; note `uuid`'s v7 RNG feature, not this change, is what currently stops `cargo check -p gist-model --target wasm32-unknown-unknown` from compiling).

## Addendum 2: merged-cell tables and the *absence* of a version bump (2026-10-05, M7 R7)

M6 R3 documented a gap: DOCX `w:vMerge` continuation cells became empty cells and `w:gridSpan` produced fewer cells in a row, so later cells shifted left; HTML `colspan`/`rowspan` in the ePub and web parsers behaved the same way. Rendering was misaligned and VoiceOver announced wrong row/column positions.

### Measured before the change (synthetic fixtures `with_merged_cells.docx` / `.epub`, `merged_cells_article.html`; same 4x3 table)

Intended grid: `[Sales(colspan 2), Notes] / [North, 100, Strong(rowspan 2)] / [South, 80, <covered>] / [Grand total(colspan 2), 180]`.

| Format | Rows produced by the M6 code |
|---|---|
| DOCX | `[Sales,Notes]` `[North,100,Strong]` `[South,80,""]` `[Grand total,180]` |
| ePub | `[Sales,Notes]` `[North,100,Strong]` `[South,80]` `[Grand total,180]` |
| web | `[Sales,Notes]` `[North,100,Strong]` `[South,80]` `[Grand total,180]` |

"Notes" and "180" land in column 1 instead of column 2 (gridSpan/colspan), and the rowspan/vMerge is invisible (row 3 of the third column is missing or an unexplained empty cell).

### Model: additive span metadata next to an unchanged grid

Options considered: (a) per-cell `colspan`/`rowspan` in a new cell struct replacing `Vec<Vec<String>>`; (b) repeating the merged text in every covered slot; (c) keep `rows` and add an optional list of spans. **(c) was chosen.** (a) changes the shape every consumer (tab/newline flattening, FTS, Swift/Windows decoders, the cross-language golden) already depends on; (b) duplicates text into tokens, search hits and annotation quotes.

```rust
Block::Table {
    rows: Vec<Vec<String>>,
    header_row: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    spans: Vec<CellSpan>,           // merged cells only
}
pub struct CellSpan { row: u32, col: u32, rowspan: u32, colspan: u32 }
```

Rules (implemented once, in `gist_model::layout_table`, shared by all three parsers):

- The merged cell's text lives in its **top-left slot**; every other slot it covers is `""`. Every row is padded with `""` out to the last slot any cell covers, so **columns are always aligned**.
- `spans` records only cells with `colspan > 1` or `rowspan > 1`. A table with no merges has no `spans` key, so its serialised form is **byte-identical to M6** (the first cross-language golden is unchanged).
- **Flattening** (`plain_text`, tokens, FTS, Rust `section_text`, Swift `plainText`) is unchanged code: covered slots are empty cells, so a merged region contributes its text once and the tab/newline structure keeps columns aligned. Second golden, pinned identically in Rust (`crates/gist-core`) and Swift (`FlowTableTests`): `Intro text.\n\nSales\t\tNotes\nNorth\t100\tStrong\nSouth\t80\t\n\nOutro text after.`
- **Hostile input** (check before allocate): a cell whose `colspan` would pass `max_table_cols` is rejected with `LimitKind::TableTooLarge` (u32::MAX and overflowing digit strings saturate to u32::MAX and are rejected; they never allocate); `rowspan` is clamped to the rows that exist; a `colspan` running into a slot already covered from above is truncated there so spans never overlap, which bounds the slot count to `max_table_rows * max_table_cols`; negative/non-numeric/zero values are 1. DOCX: `w:gridSpan` is the colspan; `w:vMerge` without `w:val="restart"` is a continuation that extends the cell directly above when it starts in the same column with the same width, otherwise (orphan) it is an ordinary empty cell. `rowspan="0"` ("to end of group") is not supported and means 1.

### Forward-compat evidence and the version decision: **no bump**

serde ignores unknown fields by default (`Block` has no `deny_unknown_fields`), so an M6 binary — which knows `Block::Table { rows, header_row }` but not `spans` — should decode a new blob and lose only the spans. Tested rather than assumed, in `crates/gist-store/src/lib.rs` (`mod pre_spans` reproduces the M6 types):

- `an_m6_binary_reads_a_spans_blob_with_the_grid_intact_so_no_bump_is_needed`: a document with merged cells is stamped `ir_version` 2 (it contains a table, as before) and the M6-shaped types decode it with the aligned grid intact.
- `m6_table_blobs_load_with_empty_spans_and_unmerged_output_is_unchanged`: an M6-written table blob (no `spans` key) loads here with empty spans; an unmerged table serialises without the key.

Per this ADR's own policy a bump is required when an older binary would **misinterpret or fail**, and `#[serde(default)]` struct fields are explicitly exempt. An old binary neither fails nor misreads: it shows the correctly aligned grid (covered slots are empty cells) without merge geometry — strictly better than the M6 output. `required_ir_version`, `CURRENT_IR_VERSION` (2) and the `IrVersionTooNew` tests are unchanged; legacy, v1 and v2 blobs all still load. `SCHEMA_VERSION` is untouched (the SQLite schema, v7, is unrelated).

**Cross-binary caveat:** an old binary that opens and *re-saves* a document would drop `spans`. Documents are immutable after import in this app (nothing rewrites `<id>.json`), so this is theoretical; re-import restores them.
