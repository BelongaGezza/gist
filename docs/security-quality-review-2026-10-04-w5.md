# W5 independent security and quality review (F30) — 2026-10-04

Reviewer: role R5, a fresh agent that wrote none of W5. Branch reviewed: `integration/w4-2026-10-04` at `4690412`
(fast-forwarded into this worktree). Scope: everything after `4a7fab3^`, excluding `docs/w5-measurement/` raw files
(which were read for the numbers cross-check): R1 `apps/windows/GIST.Core/Flow/*` and tests, R2 `FlowPage*`,
`FlowBlockRenderer.cs`, Library/Collection wiring, `AppServices`, FlaUI tests and the test-parallelism change, R3 docs
(ADR-020, spec/plan/QA edits), R4 perf generator/tests/harness and measurement docs. No Rust crate changed in W5
(`git diff --stat` shows none), so no Apple CI impact.

Labels: **[confirmed]** = reproduced or directly shown by code/output I ran; **[reading]** = from reading source only,
not executed; **[hypothesis]** = a candidate cause, not proven.

## 1. Method and commands actually run

Environment: Windows 11, Git Bash. Native DLL and bindings are gitignored, so first:

| Command | Result |
|---|---|
| `bash tools/build-core-windows.sh x64 debug` | OK, staged `gist_ffi.dll` |
| `bash tools/gen-bindings-cs.sh` | OK (pinned generator rev) |
| `cargo test --workspace` | exit 0, all suites passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --check` | exit 0 |
| `dotnet build apps/windows/GIST.sln -warnaserror` | 0 warnings, 0 errors |
| `dotnet test apps/windows/GIST.Core.Tests` (first run) | **host crashed after 297 passed** (see F67); rerun: 387 passed, 2 skipped (opt-in perf/soak), 0 failed |
| `dotnet test ... --filter DpapiKeyProviderTests` x3 | 32/32 each time (flake did not recur) |
| Throwaway probe test (deleted afterwards, `git status` clean) | decoder memory/duplicate-key/two-variant behaviour, see F58 and section 4 |
| `dotnet test ... --filter FlowDocumentDecoderTests` after adding 3 tests | 38/38 |

Not run, per instructions: the FlaUI suite and every perf run. UI behaviour findings are therefore from reading
`FlowPage.xaml(.cs)` / `FlowBlockRenderer.cs` and the committed raw reports.

One fix commit made (tests only, `af452c2`): `FlowDocumentDecoderTests` gained three tests pinning the
`MaxSections` and `MaxTotalBlocks` typed rejections and the exact-cap boundary. Before this, **no test exercised
`FlowDecodeError.TooManyItems` at all** (grep of `GIST.Core.Tests/Flow` found no use of it or the caps). No production
code was changed.

## 2. Findings

IDs continue from F57. Severity is for the Windows shell as it stands (local single-user app; the JSON comes from the
user's own integrity-checked store).

### F58 — Low (confirmed): decoder limits are enforced after the parse, and real memory cost is ~13x the char cap's text size
`FlowDocumentDecoder.Decode` (`GIST.Core/Flow/FlowDocumentDecoder.cs:49-58`) checks `json.Length <= 128M chars` and
then `JsonDocument.Parse` builds the whole tree, including the ignored `token_stream`. `MaxSections` /
`MaxTotalBlocks` / `MaxItemsPerBlock` are checked only on the already-parsed tree (lines 108-126), so they bound
decoder allocations but not the parse's. Depth is correctly bounded by `JsonDocumentOptions.MaxDepth = 32`, enforced
inside the parser before the tree is built.
Measured with a throwaway probe on `{"sections":[],"token_stream":[0,0,...]}` (the worst-case token-dense shape):

| chars | time | allocated | peak working set |
|---|---|---|---|
| 16,000,004 | 334 ms | 256 MiB | 344 MiB |
| 64,000,004 | 694 ms | 833 MiB | 1,281 MiB |

Linear extrapolation to the 128M-char cap is ~1.7 GiB allocated and ~2.5 GiB working set (extrapolated, not run).
A real 149k-word book is 12.7M chars, so the cap is ~10x the largest measured real document; it is sane as a
sanity bound but not as a memory bound. `MaxTotalBlocks = 2,000,000` and `MaxItemsPerBlock = 1,000,000` are also
far above anything the UI can render (see F60). Fixed-size rejections do work: 3M `{}` sections returned
`TooManyItems` in 258 ms. Fix: lower `MaxJsonChars` to about 32-64M, or (better) see F59 so the huge array is
never parsed.

### F59 — Low (confirmed, design): `get_document_json` ships and the decoder parses the entire RSVP token stream that flow view discards
R4's own numbers: a 295 KiB epub becomes a 12.4 MiB JSON string (12.7M UTF-16 chars, a 25 MB managed string), taking
~325 ms in FFI plus ~130 ms decode, and the decoder "never walks" `token_stream` (`FlowDocumentDecoder.cs:32`) while
`JsonDocument` still tokenises it. About 98% of the payload is wasted work and the main driver of F58 and the load
spike (peak private 378-395 MiB when opening, settling at ~235). Suggested fix (needs an Apple CI flag because it is
shared Rust): a document-only variant of the export, or stream-parse with `Utf8JsonReader` and skip the
`token_stream` property.

### F60 — Medium (reading-only + confirmed parser behaviour): virtualisation granularity is the block, so one huge block defeats it
`FlowPage` virtualises a `ListView`/`ItemsStackPanel` over blocks, and R4 measured 3-21 realised blocks of 1,330
(sound). But each realised block is one `RichTextBlock` with one `Run` per text run (`FlowBlockRenderer.cs:176-192`),
a list builds one `Grid` row per item (`:196-232`) and a table one `TextBlock`+`Border` per cell (`:236-299`), all in
a single container. Nothing caps a block's size on the UI side. **Confirmed** in `crates/gist-parse-txt/src/lib.rs:39-55`:
a `.txt` with no blank lines (hard-wrapped or single-line text) becomes **one section with one paragraph block**
(single-newline lines are rejoined into one run). Such a 100k-word file would be laid out as a single
multi-hundred-KB `RichTextBlock` on the UI thread, and `AutomationProperties.SetName(container, block.PlainText)`
(`FlowPage.xaml.cs:258`) copies the whole text into the UIA name. Likewise a 100k-cell DOCX table (decoder allows
1,000,000 cells, `FlowDocumentDecoder.cs:47`) builds 100k+ elements synchronously. R4's measurement used a
1,330-block epub, so it does not cover this shape and its "UI thread never blocked > 50 ms" conclusion should not be
read as covering txt. I did not run it (no perf runs allowed), so the stall length is unmeasured. Suggested fix:
split oversize paragraphs at the view layer (e.g. on line breaks/sentence boundaries into virtual sub-blocks, keeping
Rust `section_text` offsets via `BlockUtf8Offset`), and truncate the UIA name for long blocks.

### F61 — Low (confirmed by reading; cause of R4's find-next stall): per-step work is O(matches) plus a full re-render
R4 measured ~65 ms/step (max 82 ms, 30 of 298 probes > 50 ms) stepping F3 through a 20,000-match query. Reading the
path: `StepFind` -> `AfterSearchChanged` (`FlowPage.xaml.cs:472-478`) calls `RebuildContext` (`:275-328`), which
allocates a new `AccessibilitySettings`, five new brushes and a fresh `Dictionary<int, List<BlockHighlight>>`
over **all** 20,000 matches, then `RefreshRealized` (`:262-273`) tears down and rebuilds every realised block, each
with all its `TextHighlighter`/`TextRange` WinRT objects (~130 per visible paragraph per R4). Only the previous and new
"current" match change. R4's suggested fix is correct: re-render only those two entries and reuse the per-query
dictionary. Related: `FlowSearch.FindAll` runs synchronously on the UI thread in `ApplyQuery` (`:464-470`) and calls
`ParagraphBlock.PlainText`, which re-concatenates the runs on every call (`FlowDocument.cs:178`); fine at 1,330
blocks (12-20 ms), but linear in document size, so a 10x larger document is a visible stall per keystroke.
Also, `BuildHighlighters` silently drops a match that spans two list items/table cells (`:127`) while the status
still counts it ("3 of 27"), so F3 can land on an entry with no visible highlight (**reading**).

### F62 — Low (dormant; confirmed no producer, reading for the rest): inline image path
`TryDecodeDataImage` (`FlowBlockRenderer.cs:409-433`) is careful: only `data:image/{png,jpeg,gif,bmp};base64`, comma
within 64 chars, payload length gated (`payloadLength/4*3 > 4 MiB+3`) **before** `Convert.FromBase64String`, format
errors caught, decoded size re-checked, no network or file access from `Src`, no raw exception text, fallback is a
placeholder. Issues: (a) a 4 MiB PNG can declare enormous pixel dimensions, and `BitmapImage` is created with no
`DecodePixelWidth/Height` (`:373`), so a pixel bomb is not bounded by the byte cap (**reading**; not run);
(b) the base64 is re-decoded on every `RenderContainer`, i.e. on every find step/typography change for each visible
image; (c) the function and the whole renderer have **zero tests** (`internal`, no `InternalsVisibleTo`; grep of all
test projects finds no reference). Reachability today: **confirmed dormant**, since no parser constructs
`Block::Image` (grep of `crates/`: only the model definition and an unrelated match in epub), so only a crafted store
blob could reach it. Fix before any parser starts emitting images: set `DecodePixelWidth` to the column width, cache
decoded bytes per entry, and move `TryDecodeDataImage` into `GIST.Core` with unit tests (it is pure).

### F63 — Low (reading-only): `RestoreAsync` runs its `finally` after `Leave()`, so it can steal focus and re-hook handlers
`Leave()` sets `_left` (`FlowPage.xaml.cs:130-150`) but `RestoreAsync`'s `finally` (`:202-211`) is unconditional: it
adds `_scroll.ViewChanged += OnViewChanged` (never removed), sets opacity, and calls `FocusDocument()` on a page
whose frame is already navigating away. If the user presses Back/Alt+Left during the restore (up to ~6 x 30 ms
plus layout), focus may be pulled from the Library page. The handler re-hook is harmless (same visual tree), the
focus call is the only user-visible risk. Fix: `if (_left) return;` at the top of the `finally` body. UI-only, so not
changed here.

### F64 — Informational (cause not determinable from source): reopen memory growth
R4 reports ~27 MiB private growth per reopen cycle for cycles 1-8, ~9 MiB for 8-16 (summary file: 381 -> 617 MiB over
16 cycles; my arithmetic from the summary gives ~23-26 and ~9, consistent). I read `FlowPage`, `FlowBlockRenderer`
and `AppServices` for a managed retention path and found **none**:
- `AppServices.Theme.PropertyChanged` is unhooked in `Leave`; typography `Changed` unhooked; `_progress.Changed` and
  `_search` are page-owned; timers are stopped; `Frame` uses no `NavigationCacheMode`, and the back/forward stack
  holds only the `FlowNavigationArgs` record, not the page.
- `FlowRenderContext` statics are three `FontFamily` objects; brushes come from shared app resources or are per-context.
- Recycled containers null their `Border.Child` (`:245`).
- R4's core loop is flat (private ~240 MiB over 12 `get_document_json`+decode iterations with full GCs) and
  `DOTNET_GCHeapHardLimit=256 MiB` did not change growth, which argues against managed retention and against the
  FFI path.
Candidate causes I cannot confirm or rule out [hypothesis]: native XAML/text-layout caches for `RichTextBlock` with
`IsTextSelectionEnabled = true`; UI Automation provider objects for the realised blocks (each block gets a name and
automation id, `:257-258`, and the harness walks the tree with `FindAllDescendants` in every UIA phase); allocator
retention after the transient ~230 MiB spike per open (raw report: peak private 648-717 MiB at reopen vs ~490 after).
A coincidence worth noting but not evidence: 27 MiB is close to the 25 MB JSON string. The measurement has **no
control** (the same cycle test against the RSVP page or a library-only UIA walk), so growth is not attributed to the
flow page at all. Recommended next steps: run the cycle test with (1) no UIA walks in the reader, (2) a tiny
document, (3) RSVP instead of flow, then `umdh`/`dotnet-gcdump` + VMMap to split native from managed.

### F65 — Low (confirmed doc/test claim is wrong): "no parser populates `Section.heading`"
`FlowDocument.cs:83-85`, `FlowDocumentDecoderTests.Toc_falls_back_to_heading_blocks_because_real_parsers_never_set_section_headings`
(and R1's report) say no parser sets `Section.heading`. `crates/gist-parse-pdf/src/layout.rs:690-700` sets
`heading: Some((lvl, text))` for h1/h2 sections (and also emits the same heading as a `Heading` block). txt, docx, epub
and web all pass `None`. Consequence is dormant today (the Windows app has no PDF import: no `.pdf` reference in
`GIST.App`/`GIST.Core`), but a PDF document opened in the flow reader would get a TOC containing only h1/h2 (the
fallback to `Heading` blocks only runs when the section-heading TOC is empty, `FlowDocument.cs:81`) and the test name
would be false. Fix: correct the comment/test name; consider merging both sources.

### F66 — Informational: dead seam and a tautological test
`FlowPage.OnLoaded` builds `new SectionNavigator(result.Document)` (`:117`) but nothing ever calls `Request` or
`TryConsume`; Contents clicks call `ScrollToEntryAsync` directly. `SectionNavigatorTests` therefore test code with no
production consumer. `ReadingLayoutSeamTests` (`FlowStateTests.cs:380-407`) binds a recording test double and asserts it
holds the same objects it was given; it cannot fail and proves nothing about `FlowPage`. Either route Contents through
the navigator or drop the types until the paginated layout exists.

### F67 — Low (confirmed, pre-existing, not W5 code): `DpapiKeyProviderTests` can take down the whole test host
First `dotnet test GIST.Core.Tests` of this session aborted ("Test host process crashed") after 297 tests with an
unhandled `KeyStoreIoException: Cannot persist key file` (inner `IOException` sharing violation from `File.Move` at
`DpapiKeyProvider.cs:87`) thrown on a raw `Thread` in `Concurrent_SameInstanceAndManyInstances_ConvergeOnOneKeyAndOneFile`
(`DpapiKeyProviderTests.cs:63`). It happened while cargo/dotnet builds were saturating the machine; four later runs
(1 full, 3 filtered) passed. Two defects: the provider's lost-race handler only recovers when
`File.Exists(KeyFilePath)` is already true (`:90`), so a transient sharing violation on the move (e.g. scanner holding
the new temp file) surfaces as a hard error; and the test lets an exception escape a bare `Thread`, converting a flake
into a crashed run. Fix: retry the move briefly, and capture thread exceptions into the result array so the assertion
fails normally.

### F68 — Medium (confirmed documentation error): ADR-020 describes an `OcrEngine` contract that is not the shipped one
ADR-020 states the contract is "`OcrEngine { recognise(image_data, page_index) -> Result<OcrPageResult, OcrError>;
is_cancelled() -> bool }`", returns blocks with bounding boxes, and designs items 3-4 around `OcrBlock` boxes and
`OcrError.RecognitionFailed`. The code differs: `crates/gist-ffi/src/lib.rs:132-139` and
`crates/gist-core/src/lib.rs:389-393` define `fn recognize_page(&self, page_index: u32, image_bytes: Vec<u8>) ->
Option<OcrPageResult>` (`None` = cancellation), and `OcrPageResult` is `{ page_index, text, confidence }` (a
single per-page text and confidence; no blocks, no boxes, no `OcrError`, no `is_cancelled` on the engine). ADR-009's
text has the same older shape (`recognise`, `OcrError`, `is_cancelled`), so R3 inherited the mismatch. Consequences for
the design: per-line blocks/boxes do not exist; failures cannot be reported as a typed error, only as `None`
(which the pipeline treats as cancellation) or empty text, so "plain message" error reporting needs a new contract;
the review screen's low-confidence behaviour is per page. ADR-020 is Proposed, so this is a correction, not a
regression. Fix: rewrite "How it plugs in" against `recognize_page`, and fix ADR-009 in the same pass.

### F69 — Low: other documentation claims contradicted by source or raw data
1. `docs/windows-development-plan.md` W5 measurement: "no unpolled probe over 22 ms" for normal use and "idle
   library/reader p99 about 2.4 ms". Raw `ui-report.txt`: `20 cycle 4 idle in reader (no UIA)` max **49.2 ms**
   (p99 49.2) and `20 cycle 2 idle in reader` max 18.6 ms. Also "ALL phases p99 50.9 ms, >50 ms: 56" in the raw
   report. The headline conclusion survives (exceptions are at the edge) but the stated bound is wrong.
2. The 'native' PgUp/PgDn claims: `docs/windows-ui-spec.md:211` ("PgUp/PgDn use native viewport paging; up/down scroll
   natively"), plan line 200 ("native paging") and `docs/w5-agent-roles.md:53` ("native PgUp/PgDn") describe native
   behaviour, but `FlowPage.OnPreviewKeyDown` (`:654-686`) intercepts Home/End/PgUp/PgDn/Up/Down in the tunnelling pass
   and implements them by hand (`ChangeView` by 0.9 x viewport and 48 epx). It does page by viewport, as intended; the
   docs should say "custom viewport paging".
3. `docs/qa-manual-clickthrough-windows.md` flow section: "Tags are provisional; role R2 retags with real test names"
   is stale (the tags are real); "including nested" lists (the model has flat `Items: string[]`, no nesting);
   "command bar collapses to overflow" contradicts `OverflowButtonVisibility="Collapsed"` in `FlowPage.xaml:46`.
4. `FlowDocument.cs` doc comment claims the TOC "headless sections excluded" yet silently switches source (F65).
5. The two perf test classes have no assertions on the 50 ms or bounded-memory targets (only sanity asserts), so the
   W5 exit criteria are recorded, not enforced. Acceptable for a measurement, but "met" language in the plan should
   stay conditional.

### F70 — Informational: per-item position files
`FileFlowScrollPositionStore` (`GIST.Core/Flow/ReadingProgress.cs:63-122`) is sound for path safety (see section 3) but:
files are never removed when an item is deleted (tiny orphan files keyed by item id accumulate and outlive the item);
ids differing only in case map to one file on NTFS (irrelevant for lower-case UUIDs); writes are
truncate-then-write, so a crash can leave an empty file, which loads as 0 (benign). `FileTypographySettingsStore.Save`
requires the parent directory to exist and silently does nothing otherwise (works today because `Paths.Root` exists).
`docs/PRIVACY.md` lists local state; flow positions (item ids, no content) should be added.

## 3. Checked and sound

- **Decoder robustness [confirmed by tests + probe]:** never throws on content; wrong-typed fields fall back; unknown
  block kinds skipped and counted; depth bomb returns `TooDeep` (100,000-deep and 5,000-deep inside an unknown block
  both pass); not-JSON/truncated/NUL input is `Malformed`; empty/oversize/wrong-root are typed; `MaxDepth = 32` is above
  real nesting (~8; tables ~9). Duplicate keys: last-wins for object properties (`sections`, `id`), first-wins when a
  block object has two variant keys (probe: `dup: sections=1 id=2`, `twovariants: first=HeadingBlock`), both
  harmless. `TooManyItems` now has tests (my commit).
- **Position store path safety [confirmed by tests + reading]:** ids restricted to `[A-Za-z0-9_-]{1,64}`, validated
  before any path is built; file name has the fixed prefix `FlowScrollPosition.`, so reserved device names (CON, NUL),
  trailing dots/spaces, stream colons and separators cannot occur or are rejected; reads capped at 64 bytes (typography
  1 KiB); values clamped and non-finite rejected; failures swallowed without exception text. The traversal and
  unsafe-id tests assert nothing is created on disk.
- **FlowPage lifecycle [reading]:** FFI call and decode are inside one try/catch that only shows fixed text (F50);
  decode runs off-thread; `_left` is re-checked after each await; theme/typography/scroll handlers and timers are
  unhooked in `Leave`; scroll/progress updates are coalesced on the dispatcher (F49); no `async void` other than
  `OnLoaded`, which is fully guarded; ScrollToEntry catches and is token-guarded.
- **No document-controlled fetch [confirmed by reading]:** `Src` only ever becomes a size-capped `data:` raster
  `BitmapImage`; http(s)/file/relative produce a placeholder. No raw exception text reaches the UI anywhere in the
  new files.
- **Highlight offsets [reading]:** `BlockHighlight` uses UTF-16 offsets from `FlowSearch`, which snaps matches to
  `StringInfo` text-element boundaries; list items (`offset += item.Length + 1`) and table cells (+1 for tab/newline)
  match the `PlainText` joins in `FlowDocument.cs`. Emoji/ZWJ/combining behaviour of the finder has real tests.
  Whether `TextHighlighter` places highlights at exactly those offsets on screen is **not verified by any automated
  test** (UIA cannot see highlights); it stays a manual QA item as the checklist says.
- **Virtualisation [R4 data + reading]:** 3-21 of 1,330 block elements realised; `ItemsStackPanel`, items built in
  `ContainerContentChanging`, recycled children released. Subject to F60.
- **R4 numbers vs raw:** core report and UI report agree with the plan section except F69 item 1 (find-next stall,
  F3 probe counts, memory peaks 433/378 and 434/379 all match raw).
- **Rust and .NET gates:** all green on the integrated tree (section 1). Focus: disabling test parallelism does not mask
  app focus bugs; it removes cross-class focus theft between separately launched `GIST.exe` processes on one desktop,
  which is a harness artefact. It does mean one slow UI test class delays the rest.

## 4. Test quality notes

- Strong: decoder tests run against real engine output from the fixture corpus (txt/epub/docx/table), the
  Rust/C# `section_text` golden is pinned as a literal, find tests cover surrogates/combining/ZWJ, store tests assert
  the filesystem is untouched on refused ids.
- Weak: `ReadingLayoutSeamTests` (tautology) and `SectionNavigatorTests` (unused production code), F66; no tests for
  `FlowBlockRenderer`/`TryDecodeDataImage` (F62); `TooManyItems` was untested (fixed); UI test
  `Only_blocks_near_the_viewport_are_realised` asserts only that one far-away heading is absent, which would also pass
  if the list were empty after load, though R4's realised counts cover the positive case.
- Opt-in UI/perf tests report Skipped without the env var, which is the established convention.

## 5. Inaccurate documentation or claims (summary)

1. R1: "no current parser populates `Section.heading`" (code comment, test name, report) is false for `gist-parse-pdf` (F65).
2. ADR-020 (and ADR-009): the `OcrEngine` callback signature, `OcrError`, `is_cancelled` and per-line block/box output
   do not match `recognize_page(...) -> Option<OcrPageResult>` and the `{page_index, text, confidence}` record (F68).
3. Plan W5 measurement: "no unpolled probe over 22 ms" vs raw 49.2 ms (F69.1).
4. Spec §7.2 / plan / role plan: "native" PgUp/PgDn and arrow scrolling; the implementation is custom (F69.2).
5. QA checklist flow section: stale "provisional tags", nested lists, command-bar overflow claims (F69.3).
6. R4's attribution of memory growth to the flow page is unestablished (no control run) (F64).
7. CLAUDE.md was not edited (per instructions); when the W5 closeout is recorded it should not repeat items 1-3 above.

## 6. Recommended order

F60 (largest real-world UX risk: hard-wrapped txt), F68 (correct before ADR-020 is Accepted), F61 (small, R4 already
proposed the fix), F58/F59 together (stop parsing the token stream), then F63, F65, F66, F67, F62 (before any parser
emits images), F69, F70, and the F64 control experiments before W6 sign-off.
