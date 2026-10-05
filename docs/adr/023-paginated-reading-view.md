# ADR-023: Paginated reading view (spec Q3, v1.1)

Status: Accepted (M7 R8, 2026-10-05). Implementation unverified visually (see "Not verified").

## Context

The spec (section 5.1) lists an optional page-turn reading mode, deferred to v1.1, to be built as a second `ReadingLayout` conformer. Q8 (SwiftUI-native vs TextKit 2) is decided: SwiftUI-native; this ADR does not revisit it. The scrolling flow view (`FlowViewSwiftUINative`) stays the default and is not changed.

Constraints: minimum macOS 14; one `ReadingLayout` protocol shared with the flow view (typography, search, section navigation, progress, annotation state); annotation anchors are `(section id, UTF-8 byte offset into the section's "\n\n"-joined text)` (ADR-003) and must never become page-relative.

## Decision 1: pagination strategy - measured line fragments into fixed-size pages

Options considered:

1. **Column/offset layout**: lay the whole document out as one tall column and show it through a window that moves by one viewport height. Rejected: SwiftUI gives no reliable cheap way to cut a `Text` at an exact line boundary, so the last line of each page would be sliced mid-glyph, and it would require laying out the entire document at once (no virtualisation).
2. **Measure, then slice (chosen)**: measure each block at the current content width and typography using AppKit text layout (`NSLayoutManager`, used only as a measuring instrument), record line-break character offsets for flowing text, then assemble pages greedily by line. A page is rendered from the exact character sub-range it owns, so a page never contains a partial line by construction. Pagination re-runs when the page size or typography changes.
3. **Render-and-probe** (render each block in SwiftUI and read back `GeometryReader` sizes): rejected, non-deterministic, asynchronous, cannot be unit tested.

The paginator itself is a **pure value-type function** (`Paginator.paginate`) over `PageBlockLayout` values (per block: either `lines(lineStarts, lineHeight)` or `atomic(height)`, plus its character count), a page height and a block spacing. It does no text measurement. The measurement step (`PageBlockMeasurer`, AppKit) is a separate thin layer producing `PageBlockLayout`. This makes the page-boundary algorithm deterministic and testable without a display.

Known risk, stated plainly: SwiftUI `Text` and `NSLayoutManager` may disagree by a fraction of a line for some fonts. Mitigations: the measurer uses the same font size/design/traits and the same width; a safety slack (one `lineSpacing`) is subtracted from the usable page height; each page's content is clipped to its frame so any residual disagreement clips at most a sliver rather than spilling. **This was not visually verified** (no display access); the manual checklist in `docs/qa-manual-clickthrough-m3.md` covers it.

### Oversized and special blocks

- **Paragraph / list / heading** (flowing text): split at line boundaries across pages. A heading is kept with the next block (if a heading would be the last thing on a page and a following block exists, it moves to the next page, unless it is already first on its page).
- **Table taller than a page**: not split. It occupies its own page, rendered in a vertical `ScrollView` inside the page frame. Splitting at row boundaries would need per-row heights from the grid layout (which depends on column sizing in `FlowTableView`); deferred. A table that fits is paginated like any atomic block.
- **Image**: atomic; fixed placeholder height (the flow view renders a symbol plus alt/caption text, not the bitmap), clamped to page height.
- **Degenerate sizes**: page height or width below a minimum is raised to a floor; every loop iteration consumes at least one line or one atomic block, so pagination terminates and never yields a zero-size page. An empty document yields zero pages (the view shows an empty state).

## Decision 2: position is an anchor, not a page number (the position-meaning rule)

A page index is a **third meaning** of "position": RSVP is a token index in `reading_progress` (Rust store); flow is a block fraction in `FlowScrollPositionStore`. Page numbers change with window size and typography, so they are never persisted.

The paged view persists a `PagedAnchor { blockIndex, characterOffset }` in its own `UserDefaults` store, `PagedPositionStore` (key `pagedPosition.<itemId>`), clamped on read (negative or non-finite values become 0; the layout clamps the block index to the document and the offset to the block). `blockIndex` is the flat block index (the same ordering as `FlowViewSwiftUINative.flatBlocks` and `TtsTextExtraction.speakableBlocks`); `characterOffset` is a character (grapheme) offset into that block's `plainText`, which is the first character on the current page. After a reflow the current page is the one containing the anchor. The anchor converts to an ADR-003 annotation coordinate by `(section id, section.blockByteOffset(at:) + utf8 offset)`; annotation anchors themselves are untouched by pagination.

`reading_progress` and `FlowScrollPositionStore` are never written by the paged view's own persistence.

### Carry-over between scroll and paged modes

Both modes can express "the current block": flow publishes `ReadingProgress.fraction = currentBlock / lastBlock`; paged publishes the same quantity for the block at the top of the current page. Switching modes goes through `ReadingPositionMapping`: the outgoing layout's fraction becomes the incoming container's `initialFraction` (paged: block = round(fraction * last), offset 0; scroll: the flow view's own restore path). The mapping adds a quarter-block bias so the flow view's truncating restore (`Int(fraction * last)`) cannot land one block early through float error. A mode switch does **not** write the other mode's store; each container persists only to its own store (`FlowScrollPositionStore` in scroll mode, `PagedPositionStore` in paged mode), so neither store silently changes meaning.

## Decision 3: mapping of existing features onto pages

- **Search**: match list is computed over `plainText` of every block (same as the flow view). Navigating to a match finds the page containing `(block, matchStartOffset)`; matches are highlighted on the page using the same colours.
- **TOC / section jump**: `SectionNavigator.pendingSectionId` jumps to the page containing the section's first block.
- **Annotations**: highlights are painted from `(blockId, start, len)` clipped to the page's piece of a block; creating highlights, notes and bookmarks uses the same composer sheets as the flow view through the block context menu (the highlight composer operates on the whole block's text, so a paragraph split across pages is still highlighted from its full text); anchors remain `(section, byte offset)`. Annotation sidebar "Jump" goes to the page containing the annotation start. Real text selection stays out of scope (Q8).
- **TTS**: "Read Aloud" starts from the block at the top of the current page (via `ReadingProgress.fraction`, as in the flow view). Follow-along: while speaking, a new optional `SectionNavigator.pendingBlockIndex` moves the page to the block being spoken; the flow view ignores it.
- **Progress**: the shared bottom bar shows the block fraction; the paged view additionally shows "Page n of m".
- **Keyboard**: Left/Up/PageUp = previous page, Right/Down/PageDown/Space = next page, Home/End = first/last page. Trackpad: horizontal swipe via a two-finger scroll gesture is not implemented (SwiftUI on macOS 14 has no clean API for discrete swipe on a non-scrolling view); previous/next page buttons are always visible. Documented as a gap.
- **VoiceOver**: each page is one accessibility container labelled "Page n of m"; page changes post an accessibility announcement; each block's text is individually readable; previous/next are labelled buttons and also exposed as accessibility actions.
- **Reduce Motion**: `accessibilityReduceMotion` disables the page-turn transition.
- **Theme/typography**: theme colours from the environment only; all `TypographySettings` apply and trigger repagination.

## Decision 4: entry point

A Scroll / Pages choice lives in the reader toolbar and is persisted by `ReadingLayoutSettings` (`UserDefaults`, default Scroll). A host view (`FlowReaderHost`) selects `FlowReaderContainer<FlowViewSwiftUINative>` or `FlowReaderContainer<PagedReadingLayout>`. Opening goes through `openFlowDocument` in both cases, so `markItemOpened` (ADR-021) is stamped identically. The Settings Reading tab exposes the same default.

## Consequences

- Repagination cost is O(total lines) per typography or size change; measurement is done lazily per block and results are cached by (block, width, typography). Very large documents (hundreds of thousands of lines) are not performance-tested here.
- Tables taller than a page are scrolled within their page rather than split.
- No swipe gesture; buttons and keys only.

## Not verified at write time

Visual layout, page-turn feel, VoiceOver on hardware, measurement/render agreement, performance on very large documents.
