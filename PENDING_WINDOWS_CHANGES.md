# PENDING_WINDOWS_CHANGES.md

Windows-platform work identified during a non-Windows session that must be applied or
verified in the next Windows session. Typical cause: a macOS session edited Windows-only
C#/XAML under `apps/windows/**` as text (allowed, with a warning) but could not build or
test it. On Windows and Linux, the SessionStart hook (`tools/detect-platform.sh`) flags
this file when it contains entries.

**Delete each entry's block after the change is applied/verified and committed.**

<!-- Template — copy to add an entry (real headings start at column 0 with "## Pending Windows Change"; this example is indented so the detector ignores it):

    ## Pending Windows Change — [YYYY-MM-DD]
**File:** [path]
**Change required:** [what]
**Reason:** [why]
**Related commit/PR:** [hash or PR]
**Action:** [steps to apply / build / test on Windows]

-->

## Pending Windows Change — 2026-09-30 (mechanically verified 2026-10-04; only a Start-tile glance remains)
**File:** `apps/windows/GIST.App/Assets/GIST.ico`, `StoreLogo.png`, `Square44x44Logo.png`, `Square150x150Logo.png`, `Wide310x150Logo.png`
**Change required:** these five files were regenerated from the real designed app icon (`assets/a-windows-11-icon.png` at the repo root, 1024×1024) instead of `generate-assets.ps1`'s programmatic placeholder. `GIST.ico` was rebuilt as a 7-frame (16/24/32/48/64/128/256) PNG-payload ICO via `sips` + a one-off Python packer, since that macOS session had no `pwsh`/`ImageMagick`. `Wide310x150Logo.png` is the square artwork scaled to 150×150 and centred on a 310×150 canvas padded with the icon spec's `#1C1C1E` background token (`docs/iconspecification.md`).
**Reason:** `GIST.App.csproj` references `Assets\GIST.ico` as `<ApplicationIcon>`, and the four PNGs are the app's tile logos (`Package.appxmanifest`) — both were still placeholder artwork despite a real designed icon existing in `assets/`.
**Related commit/PR:** icon rollout alongside the macOS `AppIcon.appiconset` (same session, 2026-09-30).

### 2026-10-04 Windows session — what was actually verified, and what it found

This entry is **not** deleted, because its ask was a *visual* confirmation and part of it is still genuinely unmet (see **Remaining** below). What was mechanically checked, with results:

**A real defect was found and fixed.** The 2026-09-30 `GIST.ico` was rejected outright by Windows' WIC ICO codec — `BitmapDecoder.Create` / `IconBitmapDecoder` failed with *"The image decoder cannot decode the image. The image might be corrupted"* on every create option. Root cause: `sips` emitted PNG colour type 2 (truecolour, **no alpha channel**, inherited from the master, which has none) while each `ICONDIRENTRY` declared `wBitCount=32`; WIC requires an alpha channel on a PNG-payload ICO frame. Isolated by holding everything else constant and re-encoding the identical art as colour type 6 (RGBA), which decodes fine. The test harness was validated against 209 system ICOs (85 with PNG payloads) — all decode; `GIST.ico` was the only failure. The legacy GDI+ loader (`System.Drawing.Icon`), which is what `<ApplicationIcon>` and the shell use, *accepted* the broken file, which is why this survived until a Windows session looked at it. This could not have been caught on macOS.

**Fixes (see the two 2026-10-04 commits):** `generate-assets.ps1` now derives all five assets from `assets/a-windows-11-icon.png` rather than drawing placeholder art, so the committed binaries are reproducible and byte-identical across runs; it self-verifies and fails loudly rather than leaving a malformed asset on disk. All five binaries were regenerated. `MainWindow` now calls `AppWindow.SetIcon` (see below).

**Verified by command on this machine (Windows 11, pwsh 7, .NET SDK 10.0.401):**
- `GIST.ico`: ICONDIR `reserved=0 type=1 count=7`; frames 16/24/32/48/64/128/256 each present exactly once, square, `planes=1 bpp=32`; every payload a real PNG (`\x89PNG` + `IHDR` + `IEND`) whose IHDR dimensions match the declared size; all colour type 6 (RGBA), 8-bit; offsets/lengths all in-bounds and covering the file exactly to EOF with no gaps or overlaps.
- Loads via `System.Drawing.Icon` (all 7 sizes extract independently with real content) **and** via WIC on every create option (7 frames). *Note:* `System.Drawing.Icon(stream, 256, 256)` returns the 128px frame — a legacy GDI+ API limitation with PNG-payload 256 frames, not a file defect; WIC and direct extraction both see a valid 256×256 frame.
- Tile PNGs are exactly 50×50 / 44×44 / 150×150 / 310×150, matching what `Package.appxmanifest` and `GIST.App.csproj` reference; every referenced asset exists at its referenced path. All five assets are **fully opaque** (every pixel alpha 255, checked exhaustively, not sampled).
- `Wide310x150Logo.png`: the full 160×150 padding region is **exactly** `#1C1C1E` — 0 of 24,000 pixels deviate. The art band is exactly x=80..229 (centred: (310−150)/2 = 80), and its centre band is **pixel-identical** to `Square150x150Logo.png` (0 of 22,500 pixels differ). Not clipped: a perceptual diff against a 150×150 downscale of the master minimises sharply at offset 80 (mean 0.06/255) versus neighbouring offsets (8.6 at 79 and 81).
- `GIST.exe` embeds all 7 frames as `RT_ICON` #1–#7 under `RT_GROUP_ICON` #32512, byte lengths matching the ICO exactly.
- Build/test: `dotnet build GIST.sln -warnaserror` → 0 warnings; `dotnet test GIST.Core.Tests` → 224/224; the app launches, renders, and exits cleanly.
- **Visually inspected** (rendered to contact sheets and looked at, since this environment cannot screenshot a live window — captures come out black): all 7 ICO frames and all four tiles show the correct open-book-and-magnifier artwork, consistent across sizes, with no clipping, distortion, letterboxing, or edge halo against white, black, or magenta backdrops.

**A second real defect, found by launching the app:** the window had **no icon at all** — `WM_GETICON` (SMALL/BIG/SMALL2) and `GetClassLongPtr` (`GCLP_HICON`/`GCLP_HICONSM`) all returned NULL. `<ApplicationIcon>` only sets the *executable's* Explorer icon; WinUI 3 never gives the window an `HICON`, so the taskbar and Alt+Tab were falling back to the process image. `MainWindow.SetWindowIcon()` now calls `AppWindow.SetIcon`; re-querying a running build returns a non-null 56×56 `HICON` (DPI-scaled) with 30 distinct sampled colours, i.e. real artwork.

### Remaining — a glance by a person, and one design judgement

1. **Taskbar / Alt+Tab / title-bar icon** (~10 seconds): run the app and confirm it looks right. The window icon is now programmatically confirmed non-null and correct, and the custom title bar renders `StoreLogo.png` at 16×16 (see `MainWindow.xaml`), so this is a sanity glance rather than an open question.
2. **Start tile logos — blocked, not merely unobserved.** `Square150x150Logo`/`Wide310x150Logo`/`Square44x44Logo`/`StoreLogo` only render as Start tiles for an **MSIX-packaged install**, and `Package.appxmanifest` is explicitly "prepared but NOT installed or run (Developer Mode is off on the authoring machine)." Nobody can see these as real tiles until MSIX packaging is exercised, so fold this into whatever milestone first installs a packaged build rather than treating it as a pending glance. The assets themselves are verified correct at the pixel level above.
3. **One design judgement for a human, not a defect.** The wide tile's padding uses the spec's `#1C1C1E` token, exactly as this entry describes — but the designed master's own background is `#201520` (a slightly purple-tinted charcoal), up to 7/255 per channel different. The 150px art square is therefore faintly distinguishable from its padding on the wide tile. Options: pad with `#201520` to match the art, or keep the spec token. Left as-is deliberately, since padding with the spec token was this entry's stated intent.

**Delete this entry** once 1 is glanced at and 2 is either done or moved to the packaging milestone.

## Pending Windows Change — 2026-10-04 (M7 R1, ADR-021 reading-state model)
**File:** `apps/windows/GIST.Core/Client/CoreClient.cs` (`Map(FfiLibraryItem[])`), `apps/windows/GIST.Core/Models/LibraryItemVM.cs`, the Windows Library sort code, the RSVP and flow reader open paths.
**Change required:** (1) Regenerate the C# bindings; `FfiLibraryItem` gained `source_type` (string, normalised: txt/epub/docx/web/pdf/ocr or ""), `last_opened_at` (nullable Unix ms) and `progress_fraction` (double 0..1). `Map` uses an object initialiser so existing code should still compile — confirm. (2) Add `GistCore.MarkItemOpened(itemId)` (new export, returns Unix ms, idempotent, unknown id is a no-op) calls when the RSVP reader and the flow reader open an item (once per open, fire-and-forget, failures logged without paths, never shown to the user). (3) Optional parity: add sort keys Type / Last read (newest, oldest) / Progress (highest, lowest) with the same rules as Apple's `LibraryFiltering.sorted` (never-opened items always last for last-read; ties keep date-added order; unknown type last), and a progress bar + "Last read" line on rows. (4) If progress is shown, state the accepted limitation: progress counts RSVP reading only; flow-view-only items show a last-read date but 0%.
**Reason:** v1.0 spec promises sort by type / date last read / progress. The SQLite schema is now v7 (an older Windows build opening a v7 database gets `SchemaTooNew`, so Windows and macOS builds sharing one data dir must be updated together).
**Related commit/PR:** M7 R1 (branch `worktree-agent-a4e1f0fab1d2327a3`), `docs/adr/021-reading-state-model.md`.
**Action:** Windows session: regenerate bindings, `dotnet build GIST.sln -warnaserror`, `dotnet test GIST.Core.Tests`; check the generated `FfiLibraryItem` and any test that constructs one positionally (none known); run the Windows `old_db`/migration paths against a v6 database to confirm it migrates to v7; then wire (2) and optionally (3)/(4).



## Pending Windows Change — 2026-10-05 (M7 R3, typed resource-limit errors)
**File:** `apps/windows/GIST.Core/Client/CoreClient.cs` (`MapGistException`), `CoreError.cs` (`CoreErrorKind`), and whatever dialog code presents import failures (`LibraryViewModel*`, `GIST.App/Dialogs/LibraryDialogHost.cs`)
**Change required:** `GistError` (gist-ffi) gained seven new flat variants for resource-limit rejections: `ResourceLimitTooLarge`, `ResourceLimitTooManyPages`, `ResourceLimitTooManyEntries`, `ResourceLimitTooDeeplyNested`, `ResourceLimitContentTooLarge`, `ResourceLimitTableTooLarge`, `ResourceLimitOther`. Nothing existing was renamed or removed. After regenerating the C# bindings (`tools/gen-bindings-cs.sh`) each appears as a `GistException.ResourceLimit*` subclass. Today they fall through `MapGistException`'s `_ => new CoreError(CoreErrorKind.Core, e.Message)` arm, which already shows a fixed, path-free, honest sentence (the Rust `#[error]` text, e.g. "this document has too many pages for GIST's import limits"), so **no change is required for correctness**. Optional: add a `CoreErrorKind.ResourceLimit` and match the seven exception types (never `e.Message`) to give the import-failure dialog a dedicated title, mirroring macOS's "Can't Import This File" alert.
**Reason:** macOS now matches these cases by type to show a specific alert instead of the generic error; Windows should not regress to a message-text match if it adds the same.
**Related commit/PR:** M7 role R3 (typed resource-limit error).
**Action:** On a Windows session: regenerate bindings (`tools/gen-bindings-cs.sh`), build, run `GIST.Core.Tests`; confirm an over-limit import (e.g. a `.txt` larger than 256 MiB, or a zip with more than 10 000 entries renamed `.docx`) shows the new text rather than a raw Rust message; delete this entry once verified.

## Pending Windows Change — 2026-10-05 (M7 R7: merged-cell table spans)
**File:** `apps/windows/GIST.Core/Flow/FlowDocumentDecoder.cs` (`DecodeTable`), `FlowDocument.cs` (`TableBlock`), `FlowBlockChunker.cs` (`SplitTable`), `GIST.App/Views/FlowBlockRenderer.cs` (`BuildTable`). **No Windows code was edited.**
**Change required:** none is needed for correctness. `Block::Table` gained an optional, additive `"spans": [{"row","col","rowspan","colspan"}]` field (omitted when the table has no merges, so unmerged tables are byte-identical to before). The existing decoder reads only `rows`/`header_row` and ignores unknown keys, and `rows` is now the aligned grid (the merged cell's text in its top-left slot, `""` in every covered slot), so merged tables already render with correct column alignment (just without a visual merge) and the annotation/search text is unchanged. Optional follow-up: decode `spans` into `TableBlock` and render merged cells (colspan via `Grid.ColumnSpan`, rowspan via `Grid.RowSpan`), and announce a merged cell once in UIA with its covered extent. **Invariants that must keep holding:** `TableBlock` flattening stays tab between cells and newline between rows (covered slots are empty cells, so a merged region contributes its text once); `FlowBlockChunker.SplitTable` must not split a row-spanned region across chunks without re-basing or dropping its span (it can ignore spans entirely if `spans` is not decoded). Second cross-language golden (also pinned in Rust `gist-core` and Swift `FlowTableTests`): block JSON `{"Table":{"rows":[["Sales","","Notes"],["North","100","Strong"],["South","80",""]],"header_row":true,"spans":[{"row":0,"col":0,"rowspan":1,"colspan":2},{"row":1,"col":2,"rowspan":2,"colspan":1}]}}` inside `[paragraph "Intro text.", this table, paragraph "Outro text after."]` flattens (blocks joined by `\n\n`) to `Intro text.\n\nSales\t\tNotes\nNorth\t100\tStrong\nSouth\t80\t\n\nOutro text after.`.
**Reason:** keep the Windows flow reader's annotation anchoring aligned with Rust/Swift for merged tables.
**Related commit/PR:** M7 role R7 (branch worktree-agent-a86e55a250667a5b0), ADR-019 addendum 2.
**Action:** On a Windows session: add a `FlowDocumentDecoderTests` case decoding the golden block above (assert `Rows` and that unknown `spans` does not throw) and a flattening assertion equal to the golden text; import `fixtures/docx/with_merged_cells.docx` and confirm the table renders with 3 columns aligned. Delete this entry once verified.

## Pending Windows Change — 2026-10-09 (Rust: `Config::pause_on_punctuation` + `FfiRsvpSession.set_pause_on_punctuation`/`pause_on_punctuation`)
**File:** `apps/windows/GIST.Core/Generated/` (gitignored) and, optionally, the RSVP settings UI.
**Change required:** regenerate the C# bindings (`tools/gen-bindings-cs` equivalent) so the two new `FfiRsvpSession` methods exist; build and run the Windows tests. Additive only — nothing existing changed. Optional follow-up: surface a "Pause longer at punctuation" toggle in the W4 RSVP reader like Apple's, using the same mutate-while-playing sequence (`pause(elapsed)` → `setPauseOnPunctuation(enabled, 0)` → `resume()` → re-anchor).
**Reason:** added from a macOS session so Apple could finish adopting the FFI pacing session without keeping a Swift copy of the pacing table. `Config` gained a `#[serde(default)]` field (old JSON still decodes; `start_rsvp`'s JSON gains one key, which C#/Swift decoders ignore).
**Related commit/PR:** branch `feat/apple-rsvp-ffi-adoption`.
**Action:** regenerate bindings, `dotnet build`/test on Windows; confirm the `windows-build` CI leg is green on the PR.
