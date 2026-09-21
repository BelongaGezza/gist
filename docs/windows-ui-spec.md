# GIST for Windows — UI Specification

**Status:** Draft v1, 2026-09-20. Companion to `docs/windows-development-plan.md`.
**Scope:** WinUI 3 desktop shell for Windows 11 (x64 primary, ARM64 secondary), matching the shipped macOS/SwiftUI app in `apps/apple/` in look, feel and functionality wherever practical.
**Source of truth for Apple behaviour:** the Swift sources under `apps/apple/Shared` and `apps/apple/macOS` as of `main` @ `a46a2e8`. Where this spec says "Apple does X", that was read from code, not from CLAUDE.md prose.

Legend: **[P]** parity with Apple, **[W]** deliberate Windows-idiomatic deviation (with reason), **[+]** improvement over Apple that should later be back-ported, **[–]** not in Apple either (deferred).

---

## 1. Design principles

1. **Same information architecture, native controls.** Users who use both apps should find the same things in the same places: sidebar → library/collections → reader. Controls are WinUI 3 / Fluent, not skinned SwiftUI lookalikes.
2. **The reading surface is ours; the chrome is Windows'.** Reader backgrounds, text colours and the five themes are GIST-defined (identical palettes to Apple). Navigation, dialogs, menus and scrollbars follow Fluent.
3. **No logic in the UI layer that Rust already owns.** Pacing, search, escaping, limits, DRM detection and encryption stay in the core (see plan §4.3 for the one place Apple currently violates this and Windows should not copy it).
4. **Accessible by default.** Keyboard-complete, Narrator-labelled, respects Windows text scaling and high-contrast themes. Where Apple has a gap (§8), Windows closes it.

---

## 2. Window, shell and navigation

| Aspect | Apple (read from code) | Windows spec |
|---|---|---|
| Structure | `NavigationSplitView`: sidebar + detail `NavigationStack` | **[P]** `NavigationView` (left pane, `IsBackButtonVisible`), content `Frame` with back stack |
| Sidebar | "Library" row, then "Collections" section (only if any exist), "Appearance" toolbar button | **[P]** Menu item "Library" (Library icon); header "Collections" + one item per collection (Folder icon), hidden when none; **Appearance** as the pane footer item (Palette icon) which opens the theme dialog |
| Sidebar change | Pops any pushed reader; detail root swaps Library ↔ Collection | **[P]** Navigating the sidebar clears the back stack to the new root |
| Title | "GIST" window title; nav title = "Library"/collection name; readers titled "RSVP" | **[W]** Custom title bar (`ExtendsContentIntoTitleBar`) showing app icon + "GIST"; page header shows Library/collection name; reader shows the item title |
| Window material | Opaque themed backgrounds | **[W]** Mica behind the shell in System/Light/Dark only. Sepia and OLED use solid theme colours everywhere (OLED must be true `#000000`; Mica would tint it). Reader surfaces are always solid |
| Min size | n/a | 800 × 560 epx. Sidebar auto-collapses to icons below 1007 epx (Fluent default) |
| Menu bar | Apple removes File ▸ New only | **[W]** No menu bar. All commands are in the command bar, context menus and keyboard accelerators (§9) |
| Launch state | Library | Library, last sidebar selection is **not** restored (parity) |

Navigation model: `Frame.Navigate` with a `ReadingDestination` parameter (`Rsvp(itemId)` / `Flow(itemId)`), mirroring Apple's `ReadingDestination` enum. Opening the same item in the other reader replaces rather than stacks.

---

## 3. Design tokens

### 3.1 Themes — identical palettes **[P]**

Values taken from `Theme.swift` (RGB fractions converted to hex):

| Theme | Background | Foreground | Accent | Base `RequestedTheme` |
|---|---|---|---|---|
| Light | `#FFFFFF` | `#1A1A1A` | system accent **[W]** (Apple: system blue) | Light |
| Dark | `#1C1C1E` | `#F2F2F7` | system accent **[W]** | Dark |
| Sepia | `#F4ECD8` | `#5B4636` | `#8B5A2B` (fixed brown) | Light |
| OLED | `#000000` | `#F2F2F7` | system accent **[W]** | Dark |
| Follow System (default) | resolves to Light or Dark | | | follows OS |

- **Accent [W]:** Apple hardcodes `.blue`. On Windows the user's system accent colour is a strong native expectation, so Light/Dark/OLED use `SystemAccentColor`. Sepia keeps its fixed brown so the warm palette isn't broken by a blue control.
- Sepia and OLED never participate in OS-follow **[P]**. Selection is persisted (`ApplicationData.LocalSettings["com.gist.themeSelection"]`, same string values as Apple: `system|light|dark|sepia|oled`) and applied instantly, app-wide, including open readers.
- Implementation: one `ResourceDictionary` per theme merged at the root, exposing `GistBackground`, `GistForeground`, `GistAccent`, `GistSecondaryText` (foreground at 60 % opacity, as Apple's RSVP captions) brushes. `RequestedTheme` on the root element forces chrome (buttons, text boxes, scrollbars) to match the base theme, the equivalent of Apple's `.preferredColorScheme`.
- **High contrast [W, required]:** when Windows is in a contrast theme, all GIST palettes are bypassed and system colours are used. The theme dialog notes this.
- OS-follow detection: `UISettings.ColorValuesChanged` + `Application.RequestedTheme`, re-resolved on the UI thread.

### 3.2 Typography

| Role | Apple | Windows |
|---|---|---|
| Library row title | `.headline` | `BodyStrongTextBlockStyle` (Segoe UI Variable, 14 semibold) |
| Row author line | `.caption`, secondary | `CaptionTextBlockStyle`, `TextFillColorSecondaryBrush` |
| Page titles | nav title | `TitleTextBlockStyle` |
| RSVP word | 48 pt serif | 48 epx, **Georgia** **[W]** (New York is Apple-only) |
| RSVP captions | `.caption`, foreground @ 60 % | `CaptionTextBlockStyle`, `GistSecondaryText`, tabular figures (`Typography.NumeralAlignment="Tabular"`) |
| Flow body | user-selectable design + size | §7.2 |

### 3.3 Iconography

Use `SymbolIcon`/`FontIcon` with Segoe Fluent Icons. Mapping from the SF Symbols Apple uses:

| Command | SF Symbol | Windows |
|---|---|---|
| Library | `books.vertical` | `Symbol.Library` |
| Collection | `folder` | `Symbol.Folder` |
| Appearance | `paintpalette` | `Symbol.Font`-family "Color" glyph (`FontIcon` U+E790) |
| Import File | `plus` | `Symbol.Add` |
| Import URL | `link` | `Symbol.Link` |
| Sort | `arrow.up.arrow.down` | `Symbol.Sort` |
| Filter | `line.3.horizontal.decrease…` | `Symbol.Filter` |
| Add to Collection | `folder.badge.plus` | `Symbol.NewFolder` |
| Encrypt / encrypted badge | `lock` / `lock.fill` | `Symbol.Permissions` (lock) / `FontIcon` U+E72E |
| Remove | `trash` | `Symbol.Delete` |
| Open | `book` | `Symbol.OpenFile`/`Symbol.Read` |
| Tags | `tag` | `Symbol.Tag` |
| Contents (TOC) | `list.bullet` | `Symbol.List` |
| Typography | `textformat.size` | `Symbol.FontSize` |
| Play / Pause | `play.fill` / `pause.fill` | `Symbol.Play` / `Symbol.Pause` |

(Glyph availability to be confirmed against the Windows App SDK version chosen in W0; substitute the nearest `FontIcon` code point where a `Symbol` value is missing.)

App icon: per `docs/iconspecification.md` Windows 11 section (256 px `.ico`, flat 2D, brand tokens `#1C1C1E`/`#4361EE`/`#F4F4F0`). Not the macOS tilted-book variant.

---

## 4. Library screen

Screen root for "Library" and, with the differences in §5, for a collection.

### 4.1 Layout

- **Header row:** page title (left); `AutoSuggestBox` "Search library" (right, ~320 epx, `QueryIcon` = Find) **[P]**.
- **Command bar** (`CommandBar`, `DefaultLabelPosition=Right`, inline with header when width allows, otherwise below). Order and enablement match Apple's toolbar:

| # | Command | Enabled when | Notes |
|---|---|---|---|
| 1 | Import File | always | `FileOpenPicker`: `.txt .epub .docx` **[P]** |
| 2 | Import URL | always | dialog, §4.5 |
| 3 | Sort ▾ | always | `RadioMenuFlyoutItem` group: Date Added (Newest) / (Oldest) / Title A–Z / Title Z–A / Author A–Z |
| 4 | Filter ▾ | always | "All Tags" + one item per tag (checked = active); populated from `list_all_tags` |
| 5 | Add to Collection ▾ | ≥1 selected | existing collections + "New Collection…" |
| 6 | Encrypt | ≥1 selected | confirm dialog, §4.5 |
| 7 | Remove | ≥1 selected | confirm dialog |
| 8 | Open | exactly 1 selected | opens RSVP reader |
| 9 | Tags | exactly 1 selected | tag editor, §6 |

  Items 5–9 collapse into the overflow (`…`) at narrow widths; 1–4 stay visible. **[P]** on order; **[W]** on overflow behaviour (Fluent).
- **List:** `ListView`, `SelectionMode=Extended` (Ctrl/Shift multi-select, plus checkbox mode via `IsMultiSelectCheckBoxEnabled`, satisfying the product spec's "keyboard-accessible multi-select"). Virtualised (`ItemsStackPanel`).
- **Row template** **[P]:** title (semibold) over comma-joined authors (secondary caption; omitted if none); a lock glyph, right-aligned with tooltip "Encrypted at rest", when `content_encrypted`. Items with no title show "Untitled" **[P]** (`CoreClient.swift` fallback).
- **Activation [W]:** double-click / Enter on a row opens the RSVP reader. Apple deliberately has no row gesture because on macOS it breaks native selection; WinUI's `ListView` has no such conflict, and double-click/Enter is the Windows expectation. Selection semantics are unaffected.
- **Context menu** (`MenuFlyout`) **[P]:** Open in Reader · Open in Flow View · Manage Tags… · Encrypt… (hidden once encrypted) · separator · Remove… (destructive). Right-click on an unselected row selects it first (WinUI default behaviour).

### 4.2 Search **[P]**

- 300 ms debounce with cancel-and-restart (same as Apple), calling `search_items`. While the query is non-blank the list shows results; blank/whitespace clears without an FFI call.
- The core already escapes input as a prefix-matching FTS5 phrase (F19); the UI must **not** add its own escaping or wildcards.
- **Ctrl+F** focuses the box **[P]** (Apple: ⌘F). `Esc` clears and returns focus to the list.
- Search and tag filter are mutually exclusive **[P]**: choosing either clears the other.

### 4.3 Sort / Filter **[P]**

Sort is client-side over the already-newest-first result (Date Newest is a no-op, Oldest is a reverse), title sort case-insensitive, author sort treats "no author" as empty string. Reproduce `LibraryFiltering.sorted` exactly, with the same test cases (§ plan W2).

### 4.4 Empty states **[P]**

Centered icon + title + secondary text (+ button where applicable):
- No items: "No books yet" / "Import a .txt, .epub, or .docx file to get started" / **Import file…**
- Search with no hits: "No matches" / `No items match "<query>"`
- Tag filter with no hits: "No items" / `No items are tagged "<tag>"`

### 4.5 Dialogs (`ContentDialog`, themed to the active GIST theme)

| Dialog | Content | Buttons |
|---|---|---|
| Import URL | `TextBox`, placeholder `https://…`; body: "GIST fetches the page, extracts the readable content, and adds it to your library." | Import (primary, disabled if blank) · Cancel |
| DRM-protected file | Structured `DrmProtected` error → "This book is DRM-protected and can't be imported." (GIST never circumvents DRM, ADR-004) | OK |
| Generic error | message from `GistError` | OK |
| New Collection | `TextBox` | Create · Cancel |
| Remove | title "Remove N items?" **[P]** plus, **[+]** per product spec §4, the item titles (first 5, "and N more") and an explicit irreversibility line | **Remove** (destructive style) · Cancel (default) |
| Encrypt | explains per-item at-rest encryption (ADR-014), does not touch originals | Encrypt · Cancel |
| Encrypt result | summary: N encrypted, N already encrypted, N failed (+ first errors) | OK |

**Remove is one thing (maintainer decision, 2026-09-21).** Windows briefly offered two buttons — "Remove from Library" (keep GIST's stored copy) and "Also Delete Stored Copy". That choice was misleading rather than useful: the startup orphan sweep reclaims any stored copy no library row references, so "keep" only meant "until the next launch". Removing an item now always deletes **everything GIST holds** for it — the database row and everything cascading from it (tokens, FTS entries, reading progress, collection membership, tag links), the document and tokens blobs with their checksum sidecars, and GIST's own sandboxed stored copy (ADR-006) — and **never the user's own file**. One button, no second choice. See ADR-006's addendum.

**Wording note [+]:** the irreversibility line says what actually happens, in both halves: *"This permanently deletes this item, and GIST's stored copy of it, from this PC. It can't be undone. The original file you imported is not touched."* (plural form for a multi-item selection). Apple still shows the older two-button dialog, whose second button is additionally mislabelled "Also Delete Original File" when it deletes only the sandboxed copy — back-port both the single-button semantics and the wording via `PENDING_APPLE_CHANGES.md`.

**Button roles:** Remove carries the destructive style and **Cancel is the Enter default**, matching Encrypt — the irreversible action must never be what a stray Return key triggers.

**Shared stored copies:** two imports of byte-identical files dedup to one content-addressed file (ADR-006), so the core keeps that file until the *last* item referencing it is removed. Removing one sharer keeps it; removing both together, or the second one later, deletes it. This is neither a failure nor a missing file and raises no warning bar.

Long operations (import, URL fetch) show an indeterminate `ProgressRing` in the command bar and disable Import commands; errors surface through the dialogs above, never silently.

---

## 5. Collection screen **[P]**

Same layout and commands as Library except:
- Remove means **Remove from Collection** (title `Remove N items from "<collection>"?`, buttons Remove / Cancel) (does not delete the item), with its own confirm text. Apple keeps these semantics separate in `CollectionDetailView`; so does Windows (two view models sharing one row template and one sort implementation).
- No tag filter and no Encrypt/Import commands (Apple parity). Sort menu present.
- Context menu: Open in Reader · Open in Flow View · Manage Tags… · Remove from Collection.

---

## 6. Tag editor **[P]**

`ContentDialog` (single item): the item's title; a wrapping list of tags each with an ✕ button; a `TextBox` + Add button (Enter adds). Tags are trimmed, duplicates ignored by the core. Closing the dialog refreshes `allTags` so the Filter menu updates.

---

## 7. Readers

### 7.1 RSVP reader **[P]** (`RsvpView.swift`)

Centered column, solid theme background, all controls in theme colours:

1. **Word** — 48 epx Georgia, centered, single line, no wrapping/truncation animation. (ORP highlighting and precise timing are M3 scope on both platforms.)
2. **Progress** — `n / total`, caption, tabular figures. Persisted via `save_progress` on leave.
3. **WPM control** — label "`{wpm} WPM`", `Slider` 100–1000, step 10, width ~200. **[+]** Adjacent `NumberBox` (spin buttons, same range/step) — the product spec (§5.1.1, §5.5) requires a numeric/stepper alternative to the rotary control, and Apple's slider-only implementation doesn't fully meet it. Range enforced by the core, not just the control.
4. **Play/Pause** — large `Button` with Play/Pause glyph in the accent colour; **Space** toggles **[P]**. Playing holds position on pause (never blanks).
5. Loading state: `ProgressRing` + "Loading…".

Additions beyond Apple, all cheap and in the product spec **[+]**: **←/→** step one word, **Ctrl+←/→** jump ±5 words (spec §5.1.1 "scrub/seek"); a slim scrubber under the progress text. Not required for parity; schedule after core parity (plan W4 stretch).

Playback engine: see plan §4.3 — do not port Apple's `Task.sleep`-per-token loop with its manually mirrored pacing table.

### 7.2 Flow reader **[P]** (`FlowReaderContainer` / `FlowViewSwiftUINative`)

Command bar: **Contents · Typography (Aa) · Find** (Find opens an inline find box).

- **Document model** decoded from `get_document_json` (sections → blocks: Heading(level,text), Paragraph(runs), Image(src,alt,caption), List(ordered,items)); text runs carry bold/italic/code flags.
- **Rendering:** virtualised list of blocks (`ListView`/`ItemsRepeater` with per-block-type templates). Paragraphs use `RichTextBlock` with `Run`s; code spans are always monospaced (Cascadia Mono), regardless of font choice **[P]**. Images sized to column width with alt text as the Narrator name and caption below. Lists render bullet/number prefixes.
- **Text selection/copy [+]:** `IsTextSelectionEnabled=True` gives real selection and Ctrl+C for free. Apple's Q8 notes this was the SwiftUI-native weakness; on Windows it is a strength.
- **Reading column:** max ~70 characters wide, centred, with side margins (spec §5.2 margins/text width — see §7.4 "Margins").
- **Typography menu** (`MenuFlyout`/`Flyout`, one "Aa" button) **[P]**:
  - Size: Smaller / Larger, ±1 pt steps clamped to 13–28, default 17 (`TypographySettings`).
  - Font: Default → Segoe UI Variable Text · Serif → Georgia · Rounded → **[W] open** (no Windows system rounded face; candidates: drop the option, or map to "Trebuchet MS"; decide in W5, keep the persisted enum value `rounded` either way).
  - Line spacing: Compact +2 / Regular +6 / Relaxed +12 (points added to line height, exactly Apple's values).
- **Contents:** flyout listing headings, each row indented `16 epx × (level − 1)` **[P]** (Apple's `TocEntry.indentLevel`), click scrolls to the section. Headless sections are excluded; empty TOC shows a disabled button.
- **Find in document [P]:** inline `TextBox` + Previous/Next buttons; **F3 / Shift+F3** and **Ctrl+G** for next/previous **[W]** (Windows convention; Apple uses ⌘G). Matches highlighted via `TextHighlighter` on the `RichTextBlock`s; case-insensitive, character-based offsets (same semantics as `String.rangesOfSubstring`, including multi-byte characters).
- **Progress:** slim `ProgressBar` + percentage pinned to the bottom, driven by which blocks are realised in the viewport (block-count-based, as Apple). Position persisted per item in `LocalSettings` (`FlowScrollPosition.<itemId>`, 0…1, clamped, non-finite ignored) — deliberately **separate from** the core's `reading_progress` table, which is RSVP-token-indexed **[P]** (see CLAUDE.md M2 item 5 reasoning). Restored before first render, no visible jump.
- **Keyboard:** Home/End first/last block; **PgUp/PgDn use native viewport paging [W]** (Apple pages a fixed 12 blocks because SwiftUI lacks cheap viewport measurement; WinUI has it); ↑/↓ scroll natively. Mouse wheel and precision touchpad are native.
- **Open in the other mode:** toolbar/context switch between RSVP and Flow for the same item is **[–]** in Apple's code (it's two separate context-menu entries); Windows matches (context menu entries) and defers the spec's "exit to flow at same position" to after M3 on both platforms.

### 7.3 Theme dialog **[P]**

`ContentDialog` titled "Appearance": `RadioButtons` — Follow System · Light · Dark · Sepia · OLED (True Black). Applies live on selection; Close only. Note line about high-contrast themes.

### 7.4 Margins / not-in-Apple typography **[–]**

Text width/margins, paragraph spacing, justification and hyphenation (spec §5.2) don't exist in Apple's flow view yet. Windows ships the fixed reading column above and does not add controls until Apple does, to keep behaviour aligned.

---

## 8. Accessibility (Narrator, keyboard, scaling)

- Every interactive element has `AutomationProperties.Name`; icon-only buttons also `ToolTipService.ToolTip`. Row automation name: "`<title>`, `<authors>`, encrypted" so state isn't colour/icon-only.
- **Tab order** follows visual order; `ListView` uses arrow keys, Space toggles selection, Enter opens. No mouse-only paths (spec §4 bulk-removal requirement).
- RSVP: Play/Pause announced with state; word changes are **not** live-announced (it would flood Narrator); a "Read current word" is available via an accessible action. WPM has slider + `NumberBox` **[+]**. Reduced motion respected (no transitions on word swap).
- Respects Windows **text size** (`UISettings.TextScaleFactor` — WinUI scales `TextBlock` automatically; the RSVP word and flow font size scale on top of user choice) and **high contrast** (§3.1).
- Contrast: every theme pair ≥ 4.5:1 for body text (verified in W6 with Accessibility Insights); state is never colour-only.
- Automation is also the test seam: UI tests drive the app through UI Automation (plan W6), which is possible here in a way Apple's environment currently is not.

---

## 9. Keyboard shortcuts

| Action | Apple | Windows |
|---|---|---|
| Import file | (toolbar only) | **Ctrl+O** [W] |
| Focus library search | ⌘F | **Ctrl+F** |
| Find next / prev (flow) | ⌘G | **F3 / Shift+F3**, Ctrl+G |
| Play/Pause (RSVP) | Space | **Space** |
| Open selected | (button/menu) | **Enter** [W] |
| Remove selected | (button/menu) | **Delete** [W] (opens the same confirm dialog) |
| Select all | (system default) | **Ctrl+A** |
| Back | (nav) | **Alt+←** / mouse back button [W] |
| Flow: top/bottom | Home/End | Home/End |

Shortcuts are `KeyboardAccelerator`s with tooltips showing the gesture.

---

## 10. Behaviour that must be identical to Apple

These are functional contracts, not styling, and each has an Apple test to port (plan W2–W5):

1. Removal never deletes the user's original file — only GIST's own stored copy. **Windows diverges deliberately on the choice, not the guarantee (2026-09-21):** Windows always deletes the stored copy (one "Remove" button), Apple still asks. The guarantee that the user's own file is never touched is identical on both, and so is the core rule that a stored copy shared with a surviving item is kept until the last item referencing it goes. See §4.5 and `PENDING_APPLE_CHANGES.md`.
2. Items encrypted via "Encrypt" remain readable in both readers through the same running app (read-capable key provider from launch; ADR-014 `new_with_read_key`). New imports stay plaintext by default.
3. Search input is passed to the core unescaped-by-UI; partial words match.
4. `DrmProtected` is a distinct, structured error path, not string matching.
5. Sidebar selection equality is component-wise (a renamed collection with the same id is a different selection).
6. Theme selection persists across launches; OLED background is exactly `#000000`.
7. Flow scroll position clamps to 0…1 and is stored independently of `reading_progress`.
8. Sort semantics per §4.3.

---

## 11. Out of scope for v1 Windows (same as Apple today)

OCR import (`import_image_with_ocr` is `todo!()`; Windows OCR engine choice, Q4, is decided at W5 — see plan), annotations/highlights/bookmarks, TTS, paginated view (Q3), PDF import, ORP highlighting, export, smart collections, cover thumbnails, per-item size/word-count metadata, cloud sync.
