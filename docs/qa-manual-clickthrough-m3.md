# M3 manual click-through checklist

**Purpose:** every M3 feature below is compiler-verified and unit-tested
(183 `xcodebuild test` tests as of 2026-10-03), but none of it has been
driven through the running UI by a person, and several items — speech,
VoiceOver, Dynamic Type, contrast — can't be verified by a compiler at all.
See CLAUDE.md's M3 milestone-register row and security-register item `F28`.
This is the M3 sibling of `qa-manual-clickthrough-m2.md`; run that one for the
library/sort/filter/tags/collections/theme/flow-view basics. This file only
covers what M3 added.

The dev environment this was written in has no Accessibility permission for
`osascript`/System Events, so this is written for a human to drive.

**How to use this file:** work through each section in order, checking boxes
as you go. Where a step says "expect X," anything else is a bug — note it
inline under the checkbox rather than editing the file's structure. When done,
hand the marked-up file back for triage. Control names below come from the
current source (button/label strings); if one differs on screen, note the
actual name — don't treat it as a failure by itself.

## 0. Setup

- [ ] Build fresh: `cd apps/apple && xcodegen generate && xcodebuild build -scheme GISTmacOS -destination "platform=macOS" CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO`
- [ ] Launch the built `.app` itself (not `xcodebuild test`). Library renders, no crash.
- [ ] Have ready: 2–3 text/epub/docx files (one long, ≥ several thousand
      words), and 2–4 page images of printed text (photo or screenshot; at
      least one deliberately blurry/low-contrast for §5).
- [ ] Use a **throwaway library** or accept that test imports/annotations will
      land in your real one.

## 1. RSVP reading view

Open a long item with "Open" (or the context menu's "Open in Reader").

- [ ] Words appear one at a time, centered; the optimal-recognition-point
      letter of each word is visually emphasized and stays at a fixed screen
      position as words change (no horizontal jitter).
- [ ] Space plays/pauses. Pausing and resuming doesn't skip or repeat a word.
- [ ] Reading speed dial: dragging changes the WPM readout; actual pace
      visibly changes. Pace feels right over ~1 minute (use a stopwatch
      against a known word count: e.g. 300 WPM ≈ 5 words/sec) — this is the
      wall-clock drift fix, check it holds on a real display.
- [ ] Reading speed stepper (increase/decrease buttons) changes WPM in steps
      and agrees with the dial.
- [ ] Change speed **mid-playback**: no jump forward/back in position.
- [ ] Seek slider: dragging moves position; releasing resumes from there.
- [ ] "Back 5 words" moves back five words, exactly.
- [ ] "Pause longer on punctuation" toggle: with it on, pauses at `.`/`,`/`;`
      are noticeably longer; off, pacing is uniform.
- [ ] Session stats show plausible values after reading for a while.
- [ ] "Continue in Flow View" opens the same document in flow view near the
      position you left RSVP.
- [ ] Close and reopen the item: it resumes where you stopped.
- [ ] Try each theme (Light/Dark/Sepia/OLED): RSVP background and text follow
      it; OLED background is true black.

## 2. Annotations

Open an item in **Flow View** (context menu → "Open in Flow View").

- [ ] Open the Annotations sidebar (toolbar "Annotations"). Empty state reads
      "No annotations yet".
- [ ] **Highlight:** select text, create a highlight; the composer shows
      "Selected: “…”" and colour options. Saved highlight appears under
      "Highlights" in the sidebar with its colour.
- [ ] **Note:** add a note to a highlight ("Add a note to this highlight");
      appears under "Notes". "Edit note" changes it; Save persists.
- [ ] **Bookmark:** create a bookmark; appears under "Bookmarks".
- [ ] "Jump to this annotation" scrolls the flow view to the right spot —
      check one from the *middle of a long section*, not just the first
      paragraph (regression check for the block-join separator bug fixed in
      R4a/R1).
- [ ] "Delete annotation" removes it from the sidebar and the document.
- [ ] Quit and relaunch: annotations are still there and still jump correctly.
- [ ] "Export" produces output containing your highlights/notes; open it and
      check the text matches what you annotated.
- [ ] **Re-anchoring / orphans:** this needs an out-of-band edit, so it's
      optional — if you can modify an item's stored text, then reopen it,
      annotations whose text moved should still resolve; ones whose text was
      removed should show "Anchor not found" / "Needs review", not crash or
      point somewhere wrong.

## 3. Read aloud (needs speakers/headphones)

In flow view:

- [ ] Start read-aloud from the toolbar. **You hear speech** reading the
      document text, starting from the right place.
- [ ] Pause/resume works; resume continues, doesn't restart the block.
- [ ] "Stop Read Aloud" stops speech immediately.
- [ ] "Read-Aloud Speed" picker changes rate (slow/normal/fast); the label
      shows the current speed.
- [ ] Speech does not read out markup artefacts, stray symbols, or skip whole
      headings/paragraphs.
- [ ] Closing the reader or switching documents while speaking stops speech.

## 4. Settings (⌘, or app menu → Settings)

- [ ] Tabs present: Reading, Typography, RSVP, Import, Storage, About.
- [ ] **Reading / Typography / RSVP:** default WPM, default font size,
      font/line-spacing and the punctuation-pause default change what newly
      opened readers use; values persist across relaunch.
- [ ] **Import:** "Automatically encrypt newly imported items" — turn on,
      import a file, confirm the row shows the lock indicator and the item
      still opens in RSVP and Flow View. Turn off again afterwards if desired.
- [ ] **Library removal setting:** "Delete GIST's stored copy when removing
      items" — with it on, removing an item deletes GIST's sandboxed copy but
      **never the original file on disk** (check the original is still there).
- [ ] **Storage:** "Storage Usage" shows non-zero, plausible sizes after
      "Calculating…". "Verify Library Integrity" on a healthy library reports
      a calm result (pass/unverified for older items), not an error.
- [ ] **About:** shows app name/tagline; "Third-Party Notices…" opens the
      licenses screen and it scrolls/renders.

## 5. OCR import ("Scan or Import Page Images")

- [ ] Open the OCR import from the Library import controls; "Choose Page
      Images…" lets you pick several images at once.
- [ ] Review screen lists "Page 1", "Page 2"…; selecting a page shows its
      recognized text and (if available) the page image.
- [ ] Recognised text is mostly accurate for the clean image.
- [ ] The blurry/low-contrast page shows a "Low confidence — please check this
      page" indicator and a confidence percentage.
- [ ] Edit text on one page; edits persist when switching pages and back.
- [ ] "Import N Pages" creates **one** library item containing all pages in
      order; "Added to Your Library" confirms. Open it: your *edited* text is
      what appears, and it's readable in RSVP and Flow View.
- [ ] "Cancel" mid-review imports nothing.
- [ ] Choosing a non-image or an unreadable file fails with a clear message,
      not a crash.
- [ ] Network check: do this with Wi-Fi off — OCR is on-device and must work
      offline.

## 6. VoiceOver (⌘F5 to toggle)

Walk Library → RSVP → Flow View → Annotations → Settings → OCR review with
VoiceOver on and the screen visually ignored where practical.

- [ ] Every control is announced with a meaningful name (no "button",
      "image", or unlabeled elements). Spot-check: reading speed dial,
      stepper, seek slider ("Seek position in document"), Play/Pause,
      "Back 5 words", sidebar rows, colour swatches.
- [ ] The RSVP current word is announced (or its absence is acceptable and
      noted) — record how VoiceOver behaves while words change; it should
      not spam or go silent confusingly.
- [ ] The speed dial is operable with VoiceOver gestures (adjustable
      increment/decrement), not mouse-only.
- [ ] Annotation rows announce type, colour and text; orphaned ones announce
      "Anchor not found"/"Needs review".
- [ ] OCR low-confidence pages are announced as such, not conveyed by colour
      alone.
- [ ] Focus order in each sheet/popover is sensible and sheets can be
      dismissed from the keyboard.

## 7. Dynamic Type / text size

macOS has no system Dynamic Type slider for all apps, so use Settings →
Typography font size plus System Settings → Accessibility → Display → Text
size where supported.

- [ ] Largest font size in flow view: no clipped text, no overlapping
      controls, TOC popover still usable.
- [ ] Annotation composer, sidebar and Settings tabs at large sizes: text
      wraps/scrolls rather than truncating critical content.
- [ ] Smallest size remains legible.

## 8. Contrast and themes

The WCAG helper computed ≥ 4.5:1 for text in all four themes (4.96–18.82:1);
confirm it looks right to a human.

- [ ] Light, Dark, Sepia, OLED each: body text, secondary text, accent/links,
      annotation highlight colours and the low-confidence marker are all
      comfortably readable.
- [ ] Highlight colours remain distinguishable from each other and from the
      background in every theme (including OLED).
- [ ] Enable System Settings → Accessibility → Display → "Increase contrast"
      and "Reduce transparency": nothing becomes unreadable.

## 9. Localisation sanity

- [ ] Launch with a pseudo-language or any non-English language (System
      Settings → Language, or Xcode scheme → Application Language → a
      pseudolanguage). Menus, settings, OCR and annotation strings show
      translations/pseudo text; **no raw keys**, and layouts survive longer
      strings. Note any hard-coded English that remains.

## 10. Regression spot-check

- [ ] Import txt, epub, docx, and a URL; each still opens.
- [ ] Remove with "Remove from Library" leaves the original file on disk.
- [ ] Quit via ⌘Q mid-RSVP and mid-read-aloud: no hang, no crash log in
      Console.app for GIST.
- [ ] Console.app filtered to "GIST" during this whole pass: record any
      errors/faults that appear, even if the UI looked fine.

## 11. Paginated reading view (M7 R8, ADR-023) - NOT YET RUN

Written by an agent without display access; none of this has been verified visually.

- [ ] Default unchanged: a fresh install opens the flow view in Scroll layout.
- [ ] Reader toolbar shows a Scroll / Pages control; Pages shows one page at a time with "Page n of m" and prev/next buttons.
- [ ] No text is cut mid-line at the bottom of any page (long paragraph, bold/italic runs, serif and rounded fonts, all three line spacings, sizes 13 and 28). Note any clipped line.
- [ ] Resize the window (narrow, wide, tall, very small): pages reflow, the page still contains the text you were reading, no blank pages, no hang.
- [ ] Change font size/design/line spacing in Aa: same place after reflow.
- [ ] Keys: left/right arrows, up/down arrows, Space, PageUp/PageDown, Home/End turn pages and stop at the first/last page.
- [ ] Switch Scroll -> Pages -> Scroll mid-book: lands at the same block each time; closing and reopening in Pages restores the page.
- [ ] A table or list taller than the window sits alone on a page and scrolls inside it; a heading is not stranded at the bottom of a page.
- [ ] Find: next/previous jump to the page containing the match with the highlight; TOC entries jump to the right page; Annotations "Jump" works.
- [ ] Highlight / note / bookmark via the block context menu in Pages; they also show in Scroll layout and vice versa.
- [ ] Read Aloud starts from the current page and the page follows the spoken block.
- [ ] VoiceOver: page announced as "Page n of m" on turn; page text readable; Next/Previous Page actions available.
- [ ] Reduce Motion on: no fade on page turn.
- [ ] All four themes: page text and backgrounds use the theme, nothing hard-coded.
- [ ] Very large book (over 1 MB of text): repagination after a resize does not freeze the UI for long; note timings.
- [ ] Library "last read" sort updates after opening a book in Pages mode.

## Sign-off

- Tester / date / macOS version / hardware:
- Build (commit SHA):
- Failures found (list, with section numbers):
- M3 exit gate met? (yes only if §§1–6 pass and §§7–9 have no blockers)
