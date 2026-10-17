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

## Pending Windows Change — 2026-10-04 (M7 R1, ADR-021 reading-state model) — mostly applied 2026-10-12
**Applied and verified on Windows 2026-10-12:** bindings regenerated (`FfiLibraryItem.SourceType`/`LastOpenedAt`/`ProgressFraction`, `MarkItemOpened`); `LibraryItemVM` carries the three fields; `CoreClient.MarkItemOpenedAsync` (quiet: never sets `LastError`, swallows failures) is called once per open from `RsvpPage` and `FlowPage`; five new sort keys (Type, Last Read newest/oldest, Progress highest/lowest) in `LibrarySortOrder`/`LibraryFiltering.Sorted` with Apple's rules (unknown type last, never-opened last in both directions, ties keep incoming order) and in both Sort menus. `dotnet build GIST.sln -warnaserror` 0 warnings; `GIST.Core.Tests` 406 pass / 2 skipped (9 new); `cargo test --workspace` green; FlaUI suite 46/50 pass, 3 skipped, 1 fail (see PLATFORM_VERIFICATION.md).
**Row progress UI applied 2026-10-12:** Library and Collection rows now show a "Last read" line (`ReadingStateText`, unit-tested) and a thin progress bar when progress > 0 (screen readers get the text and percent via the row name; the bar itself is hidden from UIA). **Also applied 2026-10-12:** SC-15 "N words · ≈ M min" line on rows (reading time uses the saved RSVP WPM), SC-01 encoding prompt for `.txt` imports, SC-03 tracked-changes notice for DOCX, dedicated error kinds for unsupported encodings and resource-limit rejections. **Remaining:** a human look at the extended Sort menu, the new row layout and the encoding dialog (dark/light/high contrast), and running the Windows old-database migration test against a real v6 library. **Delete this entry once those are done or declined.**

<!-- Restored 2026-10-12: the six entries below were deleted by commit 663e319 (PR #104) together with the ADR-021 entry it applied, but none of them had been applied (checked against apps/windows on main @ 473ff86: no ResourceLimit kind, no table-spans decoding, no pause-on-punctuation toggle, no word count, metadata notice or encoding prompt). Bindings were regenerated on Windows on 2026-10-12, so the "regenerate bindings" steps are done; the UI/test steps are not. -->

## Pending Windows Change — 2026-10-11 (SC-06 web byline/dates UI) — applied 2026-10-13, human look remaining
**Applied and verified 2026-10-13:** `ItemMetadata` now carries title, author, import/published/retrieved dates, language and canonical URL (`CoreClient.GetItemMetadataAsync`); `ItemInfoPresentation` (`SafeLink` http/https only, `DisplayDate`, `HasWebDetails`) mirrors Apple's; a "Get Info…" row-menu item on Library and Collection opens an item-info dialog (`LibraryDialog.ItemInfo`, `LibraryDialogHost.ShowItemInfoAsync`) showing the fields that exist, the tracked-changes notice, and the canonical URL as a link only when it is http(s) (the user's own click follows it; never auto-opened). Build 0 warnings; `GIST.Core.Tests` 461 pass / 2 skipped (18 new incl. a real-core file-import info test). **Remaining:** a human look at the dialog (dark/light/high contrast) and with a real web item (needs network); no FlaUI test yet. **Delete this entry once that is done or declined.**

## Pending Windows Change — 2026-10-18 (verification todo for issue #123: removing the open item)
**File:** none yet — check first. Likely places to read: `apps/windows/GIST.Core/Rsvp/RsvpReaderViewModel.cs`, `apps/windows/GIST.App/Views/RsvpPage.xaml.cs` (progress saves while playing), `apps/windows/GIST.Core/Client/CoreClient.cs` (remove, save-progress calls).
**Change required:** none known. Linux found a bug where removing the library item that is open in the RSVP reader left the reader saving progress for the deleted item once a second; every failure opened a modal error and closing it just triggered the next, so the app had to be killed. Confirm whether the Windows app has the same problem.
**Reason:** GitHub issue #123. The Linux fix is in the PR that closes it; Windows has not been checked.
**Related commit/PR:** issue #123.
**Action:** on Windows: (1) import a document, open it in the RSVP reader and press Play; (2) remove that same document from the library; (3) see whether errors repeat or the reader keeps running on a removed item. Also try removing the item while it is open in the Flow reader. If it reproduces: stop/clear the reader view model when its item is removed and make a failed progress save report once, not per tick; add a test. Tick the Windows box in #123, record the result in `PLATFORM_VERIFICATION.md`, then delete this entry.
