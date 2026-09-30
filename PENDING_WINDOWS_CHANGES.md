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

## Pending Windows Change — 2026-09-30
**File:** `apps/windows/GIST.App/Assets/GIST.ico`, `StoreLogo.png`, `Square44x44Logo.png`, `Square150x150Logo.png`, `Wide310x150Logo.png`
**Change required:** these five files were regenerated from the real designed app icon (`assets/a-windows-11-icon.png` at the repo root, 1024×1024) instead of `generate-assets.ps1`'s programmatic placeholder (open book + magnifier drawn via `System.Drawing`). `GIST.ico` was rebuilt as a 7-frame (16/24/32/48/64/128/256) PNG-payload ICO via `sips` + a one-off Python packer (matching the frame-size set and ICONDIR layout the old PowerShell script used), since this macOS session has no `pwsh`/`ImageMagick`. `Wide310x150Logo.png` is the square artwork scaled to 150×150 and centered on a 310×150 canvas padded with the icon spec's `#1C1C1E` background token (`docs/iconspecification.md`), since the source art is square and Windows' wide tile isn't.
**Reason:** `apps/windows/GIST.App/GIST.App.csproj` references `Assets\GIST.ico` as `<ApplicationIcon>`, and the four PNGs are the app's UWP/WinUI tile logos (`Package.appxmanifest`) — both were still the placeholder artwork `generate-assets.ps1`'s own header comment flags, despite a real designed icon now existing in `assets/`.
**Related commit/PR:** icon rollout alongside the macOS `AppIcon.appiconset` (same session, 2026-09-30).
**Action:** on a Windows session, build/run the app and visually confirm: the taskbar/title-bar icon (from `GIST.ico`), and the Start tile logos (`Square44x44Logo.png`/`Square150x150Logo.png`/`Wide310x150Logo.png`/`StoreLogo.png`) all render correctly and aren't clipped or distorted — the ICO packing and the wide-tile padding were done without pwsh's `System.Drawing` available to preview them. Delete this entry once confirmed.

