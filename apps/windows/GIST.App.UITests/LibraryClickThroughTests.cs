using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.WindowsAPI;
using Gist.Core.Client;

namespace Gist.App.UITests;

/// <summary>
/// The automated equivalent of the Library part of the manual click-through (docs/qa-manual-clickthrough-windows.md).
/// Every test launches the real GIST.exe against its own scratch root seeded with five items, drives it with
/// FlaUI (UIA3) and, where an action has a disk/DB effect, closes the app and verifies that effect independently
/// through GIST.Core / the filesystem instead of trusting the UI text.
/// </summary>
[Trait("Category", "UI")]
public class LibraryClickThroughTests
{
    // Titles are derived by the core: txt items from the file name, epubs from their metadata.
    private const string Zebra = "zebra-notes", Apple = "apple-orchard", Mango = "mango-diary";
    private const string Minimal = "Minimal Valid", Multi = "Multi Chapter";

    private static readonly string[] NewestFirst = { Multi, Minimal, Mango, Apple, Zebra };

    private sealed record Ctx(GistAppSession S, LibraryDriver D, SeededLibrary Lib) : IDisposable
    {
        public void Dispose() => S.Dispose();
    }

    private static async Task<Ctx> LaunchAsync()
    {
        SeededLibrary? lib = null;
        var s = await GistAppSession.StartAsync(async root => lib = await LibrarySeed.SeedAsync(root));
        try
        {
            var d = new LibraryDriver(s);
            d.WaitForLibrary();
            d.WaitForTitles(t => t.Length == 5, "five seeded rows");
            return new Ctx(s, d, lib!);
        }
        catch
        {
            s.Dispose();
            throw;
        }
    }

    private static string Sha(string path) => Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path)));

    // ── 1. Command enablement ──────────────────────────────────────────────

    [UiFact]
    public async Task Commands_enable_with_selection_count()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        string[] selectionCommands =
        {
            "LibraryPage_EncryptButton", "LibraryPage_RemoveButton", "LibraryPage_OpenButton", "LibraryPage_TagsButton",
        };

        // Nothing selected: all four disabled; import stays enabled.
        Assert.Empty(d.SelectedTitles());
        foreach (var id in selectionCommands) Assert.False(d.CommandEnabled(id), id + " should be disabled with no selection");
        Assert.True(d.CommandEnabled("LibraryPage_ImportFileButton"));
        Assert.True(d.CommandEnabled("LibraryPage_ImportUrlButton"));

        // One selected: all four enabled.
        d.Select(Apple);
        LibraryDriver.PollUntil(() => selectionCommands.All(d.CommandEnabled), "all four enabled for one selected row");

        // Two selected: Encrypt/Remove stay enabled, Open/Tags (single-item commands) disable.
        d.Select(Apple, Mango);
        LibraryDriver.PollUntil(() => !d.CommandEnabled("LibraryPage_OpenButton"), "Open disabled for two rows");
        Assert.True(d.CommandEnabled("LibraryPage_EncryptButton"));
        Assert.True(d.CommandEnabled("LibraryPage_RemoveButton"));
        Assert.False(d.CommandEnabled("LibraryPage_TagsButton"));
        Assert.True(d.CommandEnabled("LibraryPage_AddToCollectionButton"));
    }

    // ── 2. Keyboard ────────────────────────────────────────────────────────

    [UiFact]
    public async Task CtrlF_focuses_the_search_box()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.ClickRow(Apple); // put keyboard focus inside the page, like a user who has clicked the list
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_F);
        d.WaitFocus("LibraryPage_SearchBox");
    }

    [UiFact]
    public async Task Typing_a_partial_word_filters_by_prefix_and_Esc_clears_and_refocuses_list()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.ClickRow(Zebra);
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_F);
        d.WaitFocus("LibraryPage_SearchBox");

        // "pomegr" is a prefix of a word that occurs in exactly one document's body (not its title).
        d.Type("pomegr");
        d.WaitForTitles(t => t.SequenceEqual(new[] { Apple }), "prefix search narrows to the one matching item (after debounce)");

        d.Press(VirtualKeyShort.ESCAPE);
        d.WaitForTitles(t => t.Length == 5, "Esc restores the full list");
        LibraryDriver.PollUntil(() => d.SearchText() == "", "search box emptied");
        d.WaitFocus("LibraryPage_List");
    }

    [UiFact]
    public async Task Search_matches_partial_words_by_prefix_after_the_debounce_and_clearing_restores()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.SetSearch("pomegr"); // prefix of a word that occurs only in apple-orchard's body, not in any title
        d.WaitForTitles(t => t.SequenceEqual(new[] { Apple }), "prefix search narrows to the one matching item");
        d.SetSearch("zzzz-no-such-word");
        d.WaitForTitles(t => t.Length == 0, "no hits -> empty list");
        LibraryDriver.PollUntil(() => d.HasText("No matches"), "no-results empty state shown");
        d.SetSearch("");
        d.WaitForTitles(t => t.Length == 5, "clearing the query restores the full list");
    }

    [UiFact]
    public async Task Delete_key_opens_remove_dialog_and_Cancel_changes_nothing()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.ClickRow(Mango);
        d.Press(VirtualKeyShort.DELETE);
        var dlg = d.Dialog("Remove 1 item?");
        Assert.Contains(LibraryDriver.DialogTexts(dlg), t => t == Mango);
        d.ClickPart(dlg, "CloseButton");
        d.WaitDialogGone("Remove 1 item?");
        Assert.Equal(NewestFirst, d.Titles());
        Assert.True(File.Exists(c.Lib.ByFile("mango-diary.txt").SourcePath));
    }

    [UiFact]
    public async Task CtrlA_selects_every_row()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.ClickRow(Multi);
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_A);
        LibraryDriver.PollUntil(() => d.SelectedTitles().Length == 5, "all five rows selected");
        Assert.False(d.CommandEnabled("LibraryPage_OpenButton")); // multi-selection consequences reach the commands
    }

    [UiFact]
    public async Task Enter_on_a_row_triggers_open_which_is_the_coming_later_notice()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.ClickRow(Apple);
        d.Press(VirtualKeyShort.RETURN);
        var dlg = d.Dialog("Coming in a later update");
        Assert.Contains(LibraryDriver.DialogTexts(dlg), t => t == "The reader isn't available yet.");
        d.ClickPart(dlg, "CloseButton");
        d.WaitDialogGone("Coming in a later update");
    }

    // ── 3. Sort ────────────────────────────────────────────────────────────

    [UiFact]
    public async Task Sort_menu_reorders_the_list_for_all_five_orders()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        Assert.Equal(NewestFirst, d.Titles()); // default: date added, newest first

        var titlesAz = NewestFirst.OrderBy(t => t, StringComparer.CurrentCultureIgnoreCase).ToArray();

        d.ChooseFromFlyout("LibraryPage_SortButton", "Date Added (Oldest)");
        d.WaitForTitles(t => t.SequenceEqual(NewestFirst.Reverse()), "oldest first");

        d.ChooseFromFlyout("LibraryPage_SortButton", "Title A–Z");
        d.WaitForTitles(t => t.SequenceEqual(titlesAz), "title A-Z");

        d.ChooseFromFlyout("LibraryPage_SortButton", "Title Z–A");
        d.WaitForTitles(t => t.SequenceEqual(titlesAz.Reverse()), "title Z-A");

        // Author A-Z: items with no author sort as "" (first, keeping their newest-first order), then the two
        // epubs (both authored "Public Domain"), also in their stable order.
        d.ChooseFromFlyout("LibraryPage_SortButton", "Author A–Z");
        d.WaitForTitles(t => t.SequenceEqual(new[] { Mango, Apple, Zebra, Multi, Minimal }), "author A-Z");

        d.ChooseFromFlyout("LibraryPage_SortButton", "Date Added (Newest)");
        d.WaitForTitles(t => t.SequenceEqual(NewestFirst), "newest first again");
    }

    // ── 4. Filter by tag + tag editor ──────────────────────────────────────

    [UiFact]
    public async Task Tag_added_in_editor_can_be_filtered_by_and_All_Tags_restores()
    {
        string root;
        string mangoId;
        using (var c = await LaunchAsync())
        {
            var d = c.D;
            root = c.S.Root;
            mangoId = c.Lib.ByFile("mango-diary.txt").Id;

            // Tag editor: add "novel" to a single item.
            d.Select(Mango);
            d.InvokeCommand("LibraryPage_TagsButton");
            var dlg = d.Dialog(Mango);
            Assert.Contains(LibraryDriver.DialogTexts(dlg), t => t == "reference"); // its seeded tag is listed
            var box = LibraryDriver.Poll(() => dlg.FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit)), "tag text box").AsTextBox();
            box.Text = "novel";
            var add = LibraryDriver.Poll(() => dlg.FindFirstDescendant(cf => cf.ByControlType(ControlType.Button).And(cf.ByName("Add"))), "Add button");
            LibraryDriver.PollUntil(() => add.Properties.IsEnabled.Value, "Add enabled after typing");
            LibraryDriver.Activate(add);
            LibraryDriver.PollUntil(() => LibraryDriver.DialogTexts(dlg).Contains("novel"), "new tag chip shown");
            d.ClickPart(dlg, "CloseButton");
            d.WaitDialogGone(Mango);

            // The Filter menu offers every tag now in use, plus All Tags.
            var items = d.FlyoutItems("LibraryPage_FilterButton");
            Assert.Contains("All Tags", items);
            foreach (var tag in new[] { "fiction", "garden", "novel", "reference" }) Assert.Contains(tag, items);

            d.ChooseFromFlyout("LibraryPage_FilterButton", "novel");
            d.WaitForTitles(t => t.SequenceEqual(new[] { Mango }), "filter by novel narrows to the tagged item");

            d.ChooseFromFlyout("LibraryPage_FilterButton", "fiction");
            d.WaitForTitles(t => t.SequenceEqual(new[] { Apple, Zebra }), "filter by fiction");

            d.ChooseFromFlyout("LibraryPage_FilterButton", "All Tags");
            d.WaitForTitles(t => t.SequenceEqual(NewestFirst), "All Tags restores the full list");
            c.S.CloseCleanly(TimeSpan.FromSeconds(10));
            // Verify the tag reached the database. (Dispose runs after; the app is already closed.)
            using var core = await LibrarySeed.OpenAsync(root);
            Assert.Contains("novel", await core.ListTagsForItemAsync(mangoId));
            Assert.Contains("reference", await core.ListTagsForItemAsync(mangoId));
        }
    }

    [UiFact]
    public async Task Search_and_tag_filter_clear_each_other()
    {
        using var c = await LaunchAsync();
        var d = c.D;

        // Filter first, then search: the tag filter is dropped (otherwise "persim" AND tag fiction would be empty).
        d.ChooseFromFlyout("LibraryPage_FilterButton", "fiction");
        d.WaitForTitles(t => t.SequenceEqual(new[] { Apple, Zebra }), "fiction filter applied");
        d.SetSearch("persim");
        d.WaitForTitles(t => t.SequenceEqual(new[] { Mango }), "searching cleared the tag filter and found the untagged-as-fiction item");

        // Search active, then filter: the search box is cleared and only the tag filter applies.
        d.ChooseFromFlyout("LibraryPage_FilterButton", "fiction");
        d.WaitForTitles(t => t.SequenceEqual(new[] { Apple, Zebra }), "tag filter replaced the search");
        LibraryDriver.PollUntil(() => d.SearchText() == "", "search box cleared by the tag filter");
    }

    // ── 5. Remove ──────────────────────────────────────────────────────────

    [UiFact]
    public async Task Remove_dialog_lists_titles_states_irreversibility_and_Cancel_changes_nothing()
    {
        string root;
        SeededItem apple, mango;
        using (var c = await LaunchAsync())
        {
            var d = c.D;
            root = c.S.Root;
            apple = c.Lib.ByFile("apple-orchard.txt");
            mango = c.Lib.ByFile("mango-diary.txt");
            var appleCopy = LibrarySeed.PredictSandboxedCopyPath(root, apple.SourcePath);
            Assert.True(File.Exists(appleCopy), "seed should have produced the sandboxed copy");

            d.Select(Apple, Mango);
            d.InvokeCommand("LibraryPage_RemoveButton");
            var dlg = d.Dialog("Remove 2 items?");
            var texts = LibraryDriver.DialogTexts(dlg);
            Assert.Contains(Apple, texts);
            Assert.Contains(Mango, texts);
            Assert.Contains(texts, t => t.Contains("This can't be undone", StringComparison.Ordinal));
            Assert.Contains(texts, t => t.Contains("Your original files are never touched", StringComparison.Ordinal));
            Assert.Equal("Remove from Library", LibraryDriver.DialogPart(dlg, "PrimaryButton").Name);
            Assert.Equal("Also Delete Stored Copy", LibraryDriver.DialogPart(dlg, "SecondaryButton").Name);

            Assert.Equal("Cancel", d.ClickPart(dlg, "CloseButton"));
            d.WaitDialogGone("Remove 2 items?");
            LibraryDriver.StaysTrue(() => d.Titles().Length == 5, "Cancel must not remove anything", TimeSpan.FromSeconds(1));
            Assert.True(File.Exists(appleCopy));
            Assert.True(File.Exists(apple.SourcePath));
            c.S.CloseCleanly(TimeSpan.FromSeconds(10));
        }
    }

    [UiFact]
    public async Task Remove_from_Library_keeps_the_stored_copy_and_the_original()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var apple = c.Lib.ByFile("apple-orchard.txt");
        var copy = LibrarySeed.PredictSandboxedCopyPath(c.S.Root, apple.SourcePath);
        var originalHash = Sha(apple.SourcePath);
        Assert.True(File.Exists(copy));

        d.Select(Apple);
        d.InvokeCommand("LibraryPage_RemoveButton");
        var dlg = d.Dialog("Remove 1 item?");
        Assert.Equal("Remove from Library", d.ClickPart(dlg, "PrimaryButton"));
        d.WaitDialogGone("Remove 1 item?");
        d.WaitForTitles(t => t.Length == 4 && !t.Contains(Apple), "row removed");

        Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
        // Check the disk BEFORE opening a CoreClient: opening one runs the startup orphan sweep, which (by design,
        // sweep_orphaned_files) reclaims a stored copy no row references any more - i.e. this one.
        Assert.True(File.Exists(copy), "'Remove from Library' must keep GIST's stored copy until the next start");
        Assert.True(File.Exists(apple.SourcePath), "the user's original must never be touched");
        Assert.Equal(originalHash, Sha(apple.SourcePath));
        using var core = await LibrarySeed.OpenAsync(c.S.Root);
        Assert.DoesNotContain(core.Items, i => i.Id == apple.Id);
        Assert.Equal(4, core.Items.Count);
        await core.WaitForSweepAsync();
        Assert.True(File.Exists(apple.SourcePath), "even the sweep never touches the user's original");
    }

    [UiFact]
    public async Task Also_Delete_Stored_Copy_removes_the_copy_but_never_the_original()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var apple = c.Lib.ByFile("apple-orchard.txt");
        var other = c.Lib.ByFile("zebra-notes.txt");
        var copy = LibrarySeed.PredictSandboxedCopyPath(c.S.Root, apple.SourcePath);
        var otherCopy = LibrarySeed.PredictSandboxedCopyPath(c.S.Root, other.SourcePath);
        var originalHash = Sha(apple.SourcePath);
        Assert.True(File.Exists(copy));

        d.Select(Apple);
        d.InvokeCommand("LibraryPage_RemoveButton");
        var dlg = d.Dialog("Remove 1 item?");
        Assert.Equal("Also Delete Stored Copy", d.ClickPart(dlg, "SecondaryButton"));
        d.WaitDialogGone("Remove 1 item?");
        d.WaitForTitles(t => t.Length == 4 && !t.Contains(Apple), "row removed");

        Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
        using var core = await LibrarySeed.OpenAsync(c.S.Root);
        Assert.DoesNotContain(core.Items, i => i.Id == apple.Id);
        Assert.False(File.Exists(copy), "'Also Delete Stored Copy' removes GIST's stored copy");
        Assert.True(File.Exists(otherCopy), "an unrelated item's stored copy is untouched");
        Assert.True(File.Exists(apple.SourcePath), "the user's original must never be touched");
        Assert.Equal(originalHash, Sha(apple.SourcePath));
    }

    // ── 5b. Remove warning (stored file could not be deleted) + orphan sweep ──

    [UiFact]
    public async Task Removal_with_a_locked_stored_file_warns_dismissibly_and_the_next_start_sweeps_the_orphan()
    {
        string? root = null;
        FileStream? hold = null;
        try
        {
            SeededItem apple;
            string blob;
            using (var c = await LaunchAsync())
            {
                c.S.KeepRoot = true; // the restart below needs the same data root
                root = c.S.Root;
                var d = c.D;
                apple = c.Lib.ByFile("apple-orchard.txt");
                blob = Path.Combine(Gist.Core.Storage.GistStoragePaths.ForRoot(root).StorageDir, apple.Id + ".json");
                Assert.True(File.Exists(blob), blob);

                // Windows refuses to delete a file another handle holds open with FileShare.None.
                hold = new FileStream(blob, FileMode.Open, FileAccess.Read, FileShare.None);

                d.Select(Apple);
                d.InvokeCommand("LibraryPage_RemoveButton");
                d.ClickPart(d.Dialog("Remove 1 item?"), "PrimaryButton"); // Remove from Library
                d.WaitDialogGone("Remove 1 item?");

                // (a) the row goes even though the file could not be deleted.
                d.WaitForTitles(t => t.Length == 4 && !t.Contains(Apple), "row removed despite the locked file");

                // (b) the warning bar appears with the fixed 'in use by another program' wording and nothing identifying.
                var bar = d.Need("LibraryPage_RemoveWarning");
                var texts = LibraryDriver.Poll(() =>
                {
                    var t = bar.FindAllDescendants().Select(e => e.Properties.Name.ValueOrDefault ?? "").Concat(new[] { bar.Properties.Name.ValueOrDefault ?? "" })
                        .Where(n => n.Length > 0).ToArray();
                    return t.Any(x => x.Contains("in use by another program", StringComparison.Ordinal)) ? t : null;
                }, "warning text in the InfoBar; UIA exposes: bar.Name='" + bar.Properties.Name.ValueOrDefault + "' children="
                    + string.Join(" / ", bar.FindAllDescendants().Select(e => e.ControlType + ":" + e.Properties.AutomationId.ValueOrDefault + ":" + e.Properties.Name.ValueOrDefault)));
                Assert.Contains(texts, t => t.Contains("could not be deleted", StringComparison.Ordinal));
                Assert.DoesNotContain(texts, t => t.Contains(Apple) || t.Contains(apple.Id) || t.Contains(":\\")
                    || t.Contains(root, StringComparison.OrdinalIgnoreCase) || t.Contains(".json"));

                // (c) the close button is reachable by the documented AutomationId and dismisses the bar.
                var close = d.Need("LibraryPage_RemoveWarningClose");
                LibraryDriver.Activate(close);
                LibraryDriver.PollUntil(() => d.ById("LibraryPage_RemoveWarning") is not { } b
                    || b.Properties.IsOffscreen.ValueOrDefault, "warning bar dismissed");

                // The orphan is still on disk while it is locked.
                Assert.True(File.Exists(blob));
                Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
            }

            // (d) release the lock, restart on the same root: the startup sweep deletes the orphan.
            hold!.Dispose();
            hold = null;
            using var again = await GistAppSession.StartAsync(existingRoot: root);
            var d2 = new LibraryDriver(again);
            d2.WaitForLibrary();
            d2.WaitForTitles(t => t.Length == 4, "four items after restart");
            LibraryDriver.PollUntil(() => !File.Exists(blob), "orphaned stored blob swept after restart", TimeSpan.FromSeconds(30));
            Assert.True(File.Exists(apple.SourcePath), "the user's original is never touched");
            Assert.True(again.Responding);
        }
        finally
        {
            hold?.Dispose();
            if (root is not null) GistAppSession.DeleteRoot(root);
        }
    }

    // ── 6. Encrypt ─────────────────────────────────────────────────────────

    [UiFact]
    public async Task Encrypt_warns_then_encrypts_marks_the_row_and_reports_already_encrypted()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var zebra = c.Lib.ByFile("zebra-notes.txt");
        var apple = c.Lib.ByFile("apple-orchard.txt");

        d.Select(Zebra);
        d.InvokeCommand("LibraryPage_EncryptButton");
        var dlg = d.Dialog("Encrypt 1 item?");
        var texts = LibraryDriver.DialogTexts(dlg);
        Assert.Contains(texts, t => t.StartsWith("There is no way to recover encrypted items", StringComparison.Ordinal));
        Assert.Contains("Warning: encrypted items cannot be recovered", texts);
        Assert.Equal("Encrypt", d.ClickPart(dlg, "PrimaryButton"));

        var result = d.Dialog("Encryption finished");
        Assert.Contains(LibraryDriver.DialogTexts(result), t => t.StartsWith("1 item encrypted", StringComparison.Ordinal));
        d.ClickPart(result, "CloseButton");
        d.WaitDialogGone("Encryption finished");

        // The row now carries the lock indicator (its accessible name ends ", encrypted"; the glyph is named too).
        LibraryDriver.PollUntil(() => d.Row(Zebra).Name.EndsWith(", encrypted", StringComparison.Ordinal), "encrypted row name");
        Assert.NotNull(d.Row(Zebra).FindFirstDescendant(cf => cf.ByName("Encrypted at rest")));
        Assert.DoesNotContain(", encrypted", d.Row(Apple).Name);

        // Encrypt the already-encrypted item together with a plain one: the summary must say so.
        d.Select(Zebra, Apple);
        d.InvokeCommand("LibraryPage_EncryptButton");
        var dlg2 = d.Dialog("Encrypt 2 items?");
        d.ClickPart(dlg2, "PrimaryButton");
        var result2 = d.Dialog("Encryption finished");
        var t2 = string.Join(" | ", LibraryDriver.DialogTexts(result2));
        Assert.Contains("1 item encrypted", t2);
        Assert.Contains("1 was already encrypted", t2);
        d.ClickPart(result2, "CloseButton");
        d.WaitDialogGone("Encryption finished");

        // And a lone already-encrypted item reports no new encryption.
        d.Select(Zebra);
        d.InvokeCommand("LibraryPage_EncryptButton");
        d.ClickPart(d.Dialog("Encrypt 1 item?"), "PrimaryButton");
        var result3 = d.Dialog("Encryption finished");
        var t3 = string.Join(" | ", LibraryDriver.DialogTexts(result3));
        Assert.Contains("already encrypted", t3);
        Assert.DoesNotContain("1 item encrypted", t3);
        d.ClickPart(result3, "CloseButton");
        d.WaitDialogGone("Encryption finished");

        // On disk: blobs are ciphertext (unique body word absent) yet the item still reads through the core.
        Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
        var paths = Gist.Core.Storage.GistStoragePaths.ForRoot(c.S.Root);
        var blob = Path.Combine(paths.StorageDir, zebra.Id + ".json");
        Assert.True(File.Exists(blob), blob);
        Assert.DoesNotContain(zebra.UniqueWord, File.ReadAllText(blob));
        Assert.DoesNotContain(zebra.UniqueWord, File.ReadAllText(Path.Combine(paths.StorageDir, zebra.Id + ".tokens.json")));

        using var core = await LibrarySeed.OpenAsync(c.S.Root);
        Assert.True(core.Items.Single(i => i.Id == zebra.Id).ContentEncrypted);
        Assert.True(core.Items.Single(i => i.Id == apple.Id).ContentEncrypted);
        Assert.False(core.Items.Single(i => i.Id == c.Lib.ByFile("mango-diary.txt").Id).ContentEncrypted);
        Assert.NotNull(await core.StartRsvpAsync(zebra.Id, 300));
        var doc = await core.GetDocumentJsonAsync(zebra.Id);
        Assert.NotNull(doc);
        Assert.Contains(zebra.UniqueWord, doc);
        Assert.Contains(c.Lib.ByFile("mango-diary.txt").UniqueWord, File.ReadAllText(Path.Combine(paths.StorageDir, c.Lib.ByFile("mango-diary.txt").Id + ".json"))); // unencrypted control item stays plaintext
        // Neither the stored copy nor the user's original were encrypted or changed.
        Assert.Contains(zebra.UniqueWord, File.ReadAllText(zebra.SourcePath));
        Assert.Contains(zebra.UniqueWord, File.ReadAllText(LibrarySeed.PredictSandboxedCopyPath(c.S.Root, zebra.SourcePath)));
    }

    // ── 7. Import URL / New Collection ─────────────────────────────────────

    [UiFact]
    public async Task Import_URL_is_disabled_while_blank_and_rejects_non_https_without_touching_the_network()
    {
        // A loopback listener stands in for "the network": if the app dials it, the test fails.
        using var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var port = ((IPEndPoint)listener.LocalEndpoint).Port;

        using var c = await LaunchAsync();
        var d = c.D;
        d.InvokeCommand("LibraryPage_ImportUrlButton");
        var dlg = d.Dialog("Import URL");
        var import = LibraryDriver.DialogPart(dlg, "PrimaryButton");
        Assert.Equal("Import", import.Name);
        Assert.False(import.Properties.IsEnabled.Value, "Import must be disabled while the field is blank");

        var box = LibraryDriver.Poll(() => dlg.FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit)), "URL text box").AsTextBox();
        box.Text = "   ";
        LibraryDriver.StaysTrue(() => !LibraryDriver.DialogPart(dlg, "PrimaryButton").Properties.IsEnabled.Value,
            "whitespace is still blank", TimeSpan.FromMilliseconds(500));

        var url = $"http://127.0.0.1:{port}/secret-article";
        box.Text = url;
        LibraryDriver.PollUntil(() => LibraryDriver.DialogPart(dlg, "PrimaryButton").Properties.IsEnabled.Value, "Import enabled with text");
        d.ClickPart(dlg, "PrimaryButton");

        var err = d.Dialog("Something went wrong");
        var texts = LibraryDriver.DialogTexts(err);
        Assert.Contains(texts, t => t == "That action couldn't be completed.");
        Assert.DoesNotContain(texts, t => t.Contains("127.0.0.1") || t.Contains("http", StringComparison.OrdinalIgnoreCase)
            || t.Contains("Exception") || t.Contains("https"));
        d.ClickPart(err, "CloseButton");
        d.WaitDialogGone("Something went wrong");

        Assert.False(listener.Pending(), "a non-https URL must be rejected before any connection is attempted");
        Assert.Equal(NewestFirst, d.Titles());
        Assert.True(c.S.Responding);
    }

    [UiFact]
    public async Task New_Collection_dialog_creates_and_adds_selection_and_existing_collection_works()
    {
        string root, appleId, mangoId;
        using (var c = await LaunchAsync())
        {
            var d = c.D;
            root = c.S.Root;
            appleId = c.Lib.ByFile("apple-orchard.txt").Id;
            mangoId = c.Lib.ByFile("mango-diary.txt").Id;


            d.Select(Apple);
            d.ChooseFromFlyout("LibraryPage_AddToCollectionButton", "New Collection…");
            var dlg = d.Dialog("New Collection");
            var create = LibraryDriver.DialogPart(dlg, "PrimaryButton");
            Assert.Equal("Create", create.Name);
            Assert.False(create.Properties.IsEnabled.Value, "Create disabled while the name is blank");
            var box = LibraryDriver.Poll(() => dlg.FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit)), "name box").AsTextBox();
            box.Text = "Reading list";
            LibraryDriver.PollUntil(() => LibraryDriver.DialogPart(dlg, "PrimaryButton").Properties.IsEnabled.Value, "Create enabled");
            d.ClickPart(dlg, "PrimaryButton");
            d.WaitDialogGone("New Collection");
            // The create-and-add runs after the dialog closes and reads the live selection; wait for it to finish
            // (the new collection shows up in the menu) before changing the selection, or the test races the app.
            LibraryDriver.PollUntil(() => d.FlyoutItems("LibraryPage_AddToCollectionButton").Contains("Reading list"),
                "new collection offered in the Add to Collection menu");

            // Add a different item to the pre-existing collection through the flyout.
            d.Select(Mango);
            d.ChooseFromFlyout("LibraryPage_AddToCollectionButton", LibrarySeed.CollectionName);
            LibraryDriver.StaysTrue(() => d.Titles().Length == 5, "collections don't change the library list", TimeSpan.FromMilliseconds(500));
            Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
            using var core = await LibrarySeed.OpenAsync(root);
            await core.ListCollectionsAsync();
            var reading = core.Collections.Single(x => x.Name == "Reading list");
            Assert.Equal(new[] { appleId }, (await core.ListItemsInCollectionAsync(reading.Id)).Select(i => i.Id).ToArray());
            var fav = core.Collections.Single(x => x.Name == LibrarySeed.CollectionName);
            var favIds = (await core.ListItemsInCollectionAsync(fav.Id)).Select(i => i.Id).ToHashSet();
            Assert.Contains(mangoId, favIds);
            Assert.Contains(c.Lib.ByFile("zebra-notes.txt").Id, favIds); // the seeded member is still there
        }
    }

    // ── 8. Import File ─────────────────────────────────────────────────────

    [UiFact]
    public async Task Import_File_opens_the_native_picker_which_can_be_cancelled()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.InvokeCommand("LibraryPage_ImportFileButton");
        var picker = NativePicker.WaitForPicker(c.S);
        Assert.NotNull(picker);
        NativePicker.Cancel(picker);
        NativePicker.WaitGone(c.S);

        Assert.True(c.S.Responding);
        d.WaitForTitles(t => t.SequenceEqual(NewestFirst), "library unchanged after cancelling the picker");
        Assert.True(d.CommandEnabled("LibraryPage_ImportFileButton"), "import is usable again after a cancelled pick");
    }

    [UiFact]
    public async Task CtrlO_opens_the_native_picker_which_can_be_cancelled()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.ClickRow(Zebra); // keyboard focus inside the page
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_O);
        var picker = NativePicker.WaitForPicker(c.S);
        NativePicker.Cancel(picker);
        NativePicker.WaitGone(c.S);
        Assert.True(c.S.Responding);
        d.WaitForTitles(t => t.SequenceEqual(NewestFirst), "library unchanged after cancelling the picker");
    }

    [UiFact]
    public async Task Import_File_can_pick_a_file_by_typing_its_path_into_the_native_picker()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var newFile = Path.Combine(c.Lib.UserFilesDir, "kiwi-journal.txt");
        File.WriteAllText(newFile, "The rambutan sentence is unique to this imported document.\n");
        var hashBefore = Sha(newFile);

        d.InvokeCommand("LibraryPage_ImportFileButton");
        var picker = NativePicker.WaitForPicker(c.S);
        NativePicker.PickFile(picker, newFile);
        d.WaitForTitles(t => t.Length == 6 && t.Contains("kiwi-journal"), "the picked file is imported and listed");

        Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
        Assert.True(File.Exists(LibrarySeed.PredictSandboxedCopyPath(c.S.Root, newFile)), "import made GIST's stored copy");
        Assert.Equal(hashBefore, Sha(newFile));
    }
}
