# Windows manual click-through checklist (Library)

Windows counterpart of `docs/qa-manual-clickthrough-m2.md`. Every Library behaviour is listed once and marked either
`[automated: <TestName>]` (a FlaUI test in `apps/windows/GIST.App.UITests/LibraryClickThroughTests.cs` drives the real
`GIST.exe` and asserts it; a person need not repeat it) or `[manual only]` (needs human eyes or hardware). The human
visual pass therefore has two parts: spot-check the automated items once if you distrust the suite, and do every
`[manual only]` item.

Run the automated part (needs an **unlocked, interactive desktop**: real key events are sent with SendInput, which
Windows rejects on a locked screen):

```
export PATH="$TEMP/bindgen-mirror-check/bin:$PATH"
tools/build-core-windows.sh x64 debug && tools/gen-bindings-cs.sh
cd apps/windows && dotnet build GIST.sln
GIST_RUN_UI_TESTS=1 dotnet test GIST.App.UITests --no-build
```

Tests use a private scratch data root (`GIST_DATA_ROOT`), never your real library. Set `GIST_UI_SCREENSHOT_DIR` to
also get PNGs of the empty / seeded / corrupt-key states for the visual pass.

## 0. Setup (manual)

- [ ] [manual only] Launch `apps/windows/GIST.App/bin/Debug/net10.0-windows10.0.19041.0/win-x64/GIST.exe` against a
      throwaway root (`GIST_DATA_ROOT=<empty dir>`) and against a copy of a real library. Window opens, no crash.
- [ ] [automated: Seeded_item_is_visible_and_app_closes_cleanly] Seeded item visible; window closes with exit code 0.
- [ ] [automated: Empty_profile_shows_empty_state_and_exits_cleanly] "No books yet" empty state on a fresh profile.
- [ ] [automated: Corrupt_key_shows_blocking_page_without_touching_key] Corrupt key: blocking page, Retry only, key file untouched.

## 1. Command bar enablement

- [ ] [automated: Commands_enable_with_selection_count] Nothing selected: Encrypt, Remove, Open, Tags disabled; Import File/URL enabled.
- [ ] [automated: Commands_enable_with_selection_count] One row selected: all four enabled.
- [ ] [automated: Commands_enable_with_selection_count] Two rows selected: Open and Tags disabled; Encrypt, Remove, Add to Collection enabled.
- [ ] [manual only] Narrow the window until commands collapse into the "..." overflow; the same enablement holds there and
      the last items overflow first (Import File/URL stay visible longest).
- [ ] [manual only] Command icons and labels look right (Import URL, Encrypt, Tags glyphs), tooltips show the shortcut text.

## 2. Keyboard

- [ ] [automated: CtrlF_focuses_the_search_box] Ctrl+F focuses the search box.
- [ ] [automated: Typing_a_partial_word_filters_by_prefix_and_Esc_clears_and_refocuses_list] Typing a partial word filters (prefix) after the debounce; Esc clears and returns focus to the list.
- [ ] [automated: Delete_key_opens_remove_dialog_and_Cancel_changes_nothing] Delete on a selection opens the Remove dialog.
- [ ] [automated: CtrlA_selects_every_row] Ctrl+A selects all rows.
- [ ] [automated: Enter_on_a_row_triggers_open_which_is_the_coming_later_notice] Enter on a row triggers Open (currently the "Coming in a later update" notice; replace when the reader lands, W4).
- [ ] [automated: CtrlO_opens_the_native_picker_which_can_be_cancelled] Ctrl+O opens the file picker.
- [ ] [manual only] Tab order: search box -> command bar -> list; arrow keys move through rows; Space toggles the row checkbox; focus rings visible in every theme.
- [ ] [manual only] Right-click a row: context menu (Open in Reader, Open in Flow View, Manage Tags..., Encrypt..., Remove...); right-click on an unselected row selects it first; "Encrypt..." is hidden when every selected item is already encrypted.
- [ ] [manual only] Double-click a row opens it (notice for now).

## 3. Search

- [ ] [automated: Search_matches_partial_words_by_prefix_after_the_debounce_and_clearing_restores] Partial word finds body-text matches; no hits shows "No matches"; clearing restores the list.
- [ ] [automated: Search_and_tag_filter_clear_each_other] Searching drops an active tag filter and vice versa.
- [ ] [manual only] Search box look (placeholder "Search library", find glyph), and that results update smoothly while typing.

## 4. Sort

- [ ] [automated: Sort_menu_reorders_the_list_for_all_five_orders] Date Added (Newest/Oldest), Title A-Z / Z-A, Author A-Z each reorder correctly.
- [ ] [manual only] The checked radio item in the Sort menu matches the current order; menu opens in the right place and follows the theme.

## 5. Tags and filter

- [ ] [automated: Tag_added_in_editor_can_be_filtered_by_and_All_Tags_restores] Add a tag in the editor (persisted to the DB), filter by it (list narrows), "All Tags" restores.
- [ ] [manual only] Tag editor dialog layout: chips wrap in a grid, remove (x) button per chip, Enter in the field adds, focus lands in the field.
- [ ] [manual only] Remove a tag via its chip's x; the filter menu drops a tag no item uses any more.

## 6. Collections

- [ ] [automated: New_Collection_dialog_creates_and_adds_selection_and_existing_collection_works] New Collection: Create disabled while blank; creates and adds the selection; an existing collection is offered and adding works (verified in the DB).
- [ ] [manual only] Collection screen (sidebar entry, browsing, "Remove" from collection wording) - not covered by the Library tests.

## 7. Remove

Removal is a **complete delete** (maintainer decision, 2026-09-21): one "Remove" button, no second choice. See §4.5 of
the UI spec and ADR-006's addendum.

- [ ] [automated: Remove_dialog_lists_titles_states_irreversibility_and_Cancel_changes_nothing] Dialog lists the selected titles, says it permanently deletes the items *and* GIST's stored copies from this PC, that it can't be undone, and that the original files you imported are not touched; the only buttons are Remove and Cancel; Cancel changes nothing.
- [ ] [automated: Remove_deletes_everything_GIST_holds_and_never_the_users_original] "Remove": row gone, stored copy gone *immediately* (not deferred to the next start), no file named after the item left anywhere under the storage directory, an unrelated item's stored copy untouched, and the user's original byte-for-byte unchanged (SHA-256) both before and after the next start's sweep.
- [ ] [automated: Removal_with_a_locked_stored_file_warns_dismissibly_and_the_next_start_sweeps_the_orphan] A file another program holds open: row removed, warning bar with generic wording (no path/title), its close button dismisses it, and the next start sweeps the orphan.
- [ ] [automated: Two_dialogs_in_quick_succession_both_open] Dialogs opened back to back with no pause all appear, and the last one's action really happens (no silently dropped request).
- [ ] [manual only] Remove dialog appearance: destructive styling on "Remove", **Cancel is the Enter default**, "and N more" for >5 titles, warning bar look and placement above the header.
- [ ] [manual only] Shared stored copy: import the same file twice under two names, remove one item — the other still opens and reads normally, and no warning bar appears. (Covered by unit tests at three levels; this is the eyes-on confirmation.)

## 8. Encrypt

- [ ] [automated: Encrypt_warns_then_encrypts_marks_the_row_and_reports_already_encrypted] Irrecoverability warning shown; confirm shows result counts; row gets the lock glyph and ", encrypted" name; re-encrypting reports "already encrypted"; ciphertext on disk, item still readable through the core, stored copy and original untouched.
- [ ] [manual only] Encrypt dialog look: warning InfoBar prominence, Cancel is the Enter default, lock glyph alignment in the row in every theme.

## 9. Import

- [ ] [automated: Import_URL_is_disabled_while_blank_and_rejects_non_https_without_touching_the_network] Import URL: Import disabled while blank; a non-https URL is rejected with the generic error dialog and no connection is made.
- [ ] [automated: Import_File_opens_the_native_picker_which_can_be_cancelled] Import File button opens the native picker; Cancel leaves the app healthy.
- [ ] [automated: Import_File_can_pick_a_file_by_typing_its_path_into_the_native_picker] Picking a file imports it and creates the stored copy; the original is unchanged.
- [ ] [manual only] Real https import of a public article (needs network); DRM-protected epub shows "Can't import this book"; a corrupt/empty file shows the generic error, never a path or exception text.
- [ ] [manual only] Native picker look, starting folder, and file-type filter (.txt/.epub/.docx only).

## 10. Whole-window visual pass (manual only)

- [ ] Themes: light, dark, sepia, OLED (and OS-follow): backgrounds/text/accent correct on the Library, every dialog, the InfoBars and the flyouts.
- [ ] Mica backdrop renders and the title bar matches the theme.
- [ ] Layout at 800, 1100 and 1600 px wide and at 125/150/200 % display scaling: no clipping, header search stays right-aligned.
- [ ] Narrator: reads the page title, the list ("Library items"), each row ("Title, Author, encrypted"), the commands and dialogs' titles/bodies in a sensible order.
- [ ] High contrast (Aquatic, Desert, Night sky): all controls and glyphs remain visible.
- [ ] Nothing shows a raw exception message, GUID or file path anywhere.

## Known behaviour to be aware of

- **Remove is a complete delete and always has been, in effect.** Windows briefly offered "Remove from Library" and
  "Also Delete Stored Copy"; the first only kept GIST's stored copy until the next launch, because the startup orphan
  sweep deletes any stored copy no library row references. The two buttons therefore differed only in *when* the copy
  went, which made the choice misleading. Since 2026-09-21 there is one "Remove" button that deletes everything GIST
  holds straight away. The file you imported is never touched, and never was.
- **A stored copy shared by two items is kept until the last of them is removed.** Two imports of byte-identical files
  share one content-addressed file (ADR-006), so removing one of them deliberately leaves that file on disk for the
  other. This is not a failure and shows no warning bar.
- **Apple still has the two-button dialog** ("Remove from Library" / "Also Delete Original File", the second of which is
  additionally mislabelled — it deletes only the sandboxed copy). Adopting the Windows semantics there is logged in
  `PENDING_APPLE_CHANGES.md`.
