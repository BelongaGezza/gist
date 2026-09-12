# M2 manual click-through checklist

**Purpose:** every M2 feature below is compiler-verified and unit-tested, but
none of it has been systematically driven through the actual running UI by a
person — see CLAUDE.md's M2 exit-criterion note. This checklist exists so
that pass can happen in a dedicated session (this one or a later one)
instead of being reconstructed from scratch each time. Automated UI-scripted
click-through isn't available in the dev environment this was written in
(`osascript`/System Events has no Accessibility permission there), so this
is written for a human to drive.

**How to use this file:** work through each section in order, checking boxes
as you go. Where a step says "expect X," anything else is a bug — note it
inline (a line right under the checkbox is fine) rather than editing this
file's structure. When you're done, hand the marked-up file back so the
findings can be triaged and fixed.

## 0. Setup

- [ ] Build fresh: `cd apps/apple && xcodegen generate && xcodebuild build -scheme GISTmacOS -destination "platform=macOS" CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO`
- [ ] Launch the built `.app` from DerivedData (or `xcodebuild -scheme GISTmacOS` in Xcode directly) — not `xcodebuild test`, an actual running window.
- [ ] Confirm the window opens and the Library renders without a crash or an empty/blank pane.

## 1. Library basics

- [ ] Existing item(s) show up in the list with correct titles/authors.
- [ ] Import a new file (toolbar "+") — appears in the list after import.
- [ ] Import via URL (toolbar link icon) — appears after fetch completes.
- [ ] Search for a **short partial word** (e.g. the first 3 characters of a
      title/content word) — should now find matches (regression check for
      the FTS5 prefix-matching fix, commit `aaed303`). Confirm it does
      *not* silently show "no results" the way it did before that fix.
- [ ] Search for a full word — still matches.
- [ ] Clear search — full list returns.
- [ ] Select an item, remove it via "Remove from Library" — gone from the
      list; confirm the original source file on disk is untouched.
- [ ] Remove another item via "Also Delete Original File" — confirm GIST's
      sandboxed copy is gone; only touch a throwaway/test file for this one.

## 2. Sort (toolbar ↕ icon)

- [ ] Date Added (Newest) — default order, most recent import first.
- [ ] Date Added (Oldest) — reversed.
- [ ] Title (A–Z) and Title (Z–A) — alphabetical, case-insensitive (a
      lowercase title should sort where its letter says, not after all
      capitalized titles).
- [ ] Author (A–Z) — items with no author sort first (empty string sorts
      before any real name).

## 3. Filter (toolbar circle-with-lines icon)

*(Add a tag to at least one item first — see §4 — if you have none yet.)*

- [ ] Picking a tag narrows the list to only items with that tag.
- [ ] "All Tags" clears the filter back to the full list.
- [ ] Typing in search while a tag filter is active clears the filter
      (they're mutually exclusive by design).
- [ ] Picking a tag while search text is present clears the search field.

## 4. Tags

- [ ] Select an item → toolbar "Tags" button (or right-click → "Manage
      Tags…") opens the tag editor sheet.
- [ ] Add a tag, close the sheet, reopen it on the same item — tag persisted.
- [ ] Remove the tag — gone on reopen.
- [ ] The tag now appears (or disappears) in the Filter menu's tag list.

## 5. Collections (sidebar)

- [ ] "Add to Collection" on a selected item → "New Collection…" → creates
      and adds in one step.
- [ ] New collection appears in the sidebar under "Collections."
- [ ] Clicking a collection in the sidebar shows only its items.
- [ ] Clicking "Library" in the sidebar returns to the full list.
- [ ] Sort menu works the same way inside a collection view.
- [ ] "Remove from Collection" removes the item from that collection's view
      but it's still present under "Library."
- [ ] Rename-sensitivity check (optional, edge case): if you can rename a
      collection, confirm the sidebar selection still tracks the *same*
      collection afterward rather than silently deselecting.

## 6. Theme (sidebar toolbar "Appearance" button)

- [ ] "Follow System" — matches the Mac's current light/dark appearance;
      toggle System Settings' appearance and confirm GIST follows.
- [ ] Light, Dark — each renders distinctly and legibly.
- [ ] Sepia — warm off-white background, dark brown text (not just a light
      gray reskin).
- [ ] OLED — genuinely pure black background, visibly different from Dark's
      background, not just "Dark but darker text."
- [ ] Theme changes apply consistently across: Library list, RSVP reader,
      and Flow View (see §7) — open each while switching themes.

## 7. Flow view (right-click an item → "Open in Flow View")

- [ ] Document text renders (headings, paragraphs, lists all look right).
- [ ] **Typography ("Aa" menu):**
  - [ ] Font size smaller/larger buttons visibly change text size.
  - [ ] Font picker: Default / Serif / Rounded each visibly change the
        typeface.
  - [ ] Line Spacing picker: Compact / Regular / Relaxed visibly change
        spacing between lines.
  - [ ] Code spans (if the document has any) stay monospaced regardless of
        the font-design choice.
- [ ] **Table of contents** (documents with headings): "Contents" menu jumps
      to the right section.
- [ ] **Search:**
  - [ ] Typing a query shows a match count (e.g. "1/5").
  - [ ] Next/previous (chevron buttons, or ⌘G for next) move between
        highlighted matches, current match visually distinct from others.
  - [ ] Clearing the query clears highlights.
- [ ] **Progress bar** (bottom of the window): percentage updates as you
      scroll.
- [ ] **Keyboard navigation** (click into the text area first): Home, End,
      Page Up, Page Down, ↑, ↓ each move the view sensibly.
- [ ] **Scroll-position persistence:** scroll partway through, navigate back
      to Library, reopen the *same* item — resumes near where you left off
      rather than jumping to the top. Open a *different* item — starts at
      the top (confirms persistence is per-item, not global).
- [ ] Trackpad/scroll-wheel scrolling works throughout (should, as a native
      `ScrollView` — flag if it doesn't).

## 8. RSVP reader

- [ ] Opens from the Library ("Open" button or "Open in Reader" context
      item) and starts playback.
- [ ] Play/pause, WPM control all work.
- [ ] Theme colors apply here too (re-check under OLED specifically — this
      is the view where true-black matters most).
- [ ] Closing and reopening resumes roughly where you left off (separate
      persistence mechanism from Flow View's — see CLAUDE.md's note on why
      the two don't share storage).

## Reporting back

For anything that didn't match the expected behavior, note: which checkbox,
what you saw instead, and (if easy to capture) a screenshot. Everything that
passed doesn't need narration — a fully-checked section is enough signal on
its own.
