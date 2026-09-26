using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using Gist.Core.Theming;

namespace Gist.App.UITests;

/// <summary>
/// FlaUI coverage for W3 (sidebar/collections navigation and the Appearance dialog,
/// <c>docs/windows-ui-spec.md</c> §2, §5, §7.3) — same idioms as
/// <see cref="LibraryClickThroughTests"/>: launch the real GIST.exe against a seeded scratch root,
/// drive it with UIA, and verify anything with a disk effect independently rather than trusting the
/// UI text.
/// </summary>
[Trait("Category", "UI")]
public class CollectionAndThemeTests
{
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

    // ── Sidebar (spec §2) ────────────────────────────────────────────────────

    [UiFact]
    public async Task Sidebar_hides_the_Collections_header_when_there_are_none()
    {
        using var s = await GistAppSession.StartAsync(async root =>
        {
            var paths = Gist.Core.Storage.GistStoragePaths.ForRoot(root);
            paths.EnsureCreated();
            using var core = new Gist.Core.Client.CoreClient(new Gist.Core.Client.CoreClientOptions(
                paths.DbPath, paths.StorageDir, new Gist.Core.Keys.DpapiKeyProvider(paths.KeyDir)));
            await core.InitializeAsync();
        });
        var d = new LibraryDriver(s);
        // The library itself is empty too, so the Library page may still be showing its own
        // loading/empty state; the sidebar pane loads independently of ContentFrame's page.
        d.Need("LibraryPage_Root");

        Assert.Null(d.ByName("Collections"));
    }

    [UiFact]
    public async Task Sidebar_shows_the_seeded_collection_and_navigates_without_disturbing_the_back_stack()
    {
        using var c = await LaunchAsync();
        var d = c.D;

        var header = LibraryDriver.Poll(() => d.ByName("Collections"), "Collections header");
        Assert.True(header.Properties.IsOffscreen.ValueOrDefault == false);

        var entry = LibraryDriver.Poll(() => d.ByName(LibrarySeed.CollectionName), "sidebar collection entry");
        LibraryDriver.Activate(entry);

        d.Need("CollectionPage_Root");
        Assert.Equal(LibrarySeed.CollectionName, d.Need("CollectionPage_Title").Name);

        // Navigating the sidebar clears the back stack (spec §2): there is nothing to go "back" to.
        Assert.True(c.S.Window.FindFirstDescendant(cf => cf.ByAutomationId("LibraryPage_Root")) is null,
            "the Library page must not still be present underneath");
    }

    [UiFact]
    public async Task Collection_page_shows_only_its_own_item_with_the_narrower_command_set()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var zebraTitle = c.Lib.ByFile("zebra-notes.txt").Title;

        LibraryDriver.Activate(LibraryDriver.Poll(() => d.ByName(LibrarySeed.CollectionName), "sidebar collection entry"));
        d.Need("CollectionPage_Root");
        LibraryDriver.PollUntil(() => d.Titles().SequenceEqual(new[] { zebraTitle }), "collection shows only its own item");

        // §5: exactly Sort/Remove/Open/Tags — no Search/Filter/Import/Add-to-Collection/Encrypt.
        Assert.Null(d.ById("LibraryPage_SearchBox"));
        var buttonIds = c.S.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.Button))
            .Select(e => e.Properties.AutomationId.ValueOrDefault ?? "")
            .Where(id => id.StartsWith("CollectionPage_", StringComparison.Ordinal) && id.EndsWith("Button", StringComparison.Ordinal))
            .ToHashSet();
        Assert.Equal(
            new HashSet<string> { "CollectionPage_SortButton", "CollectionPage_RemoveButton", "CollectionPage_OpenButton", "CollectionPage_TagsButton" },
            buttonIds);

        Assert.True(d.Command("CollectionPage_SortButton").Properties.IsEnabled.Value);
        Assert.False(d.CommandEnabled("CollectionPage_RemoveButton"), "nothing selected yet");
        d.Select(zebraTitle);
        Assert.True(d.CommandEnabled("CollectionPage_RemoveButton"));
        Assert.True(d.CommandEnabled("CollectionPage_OpenButton"));
        Assert.True(d.CommandEnabled("CollectionPage_TagsButton"));
    }

    [UiFact]
    public async Task Removing_from_a_collection_detaches_without_deleting_the_item()
    {
        string? root = null;
        try
        {
            string itemId;
            using (var c = await LaunchAsync())
            {
                var d = c.D;
                root = c.S.Root;
                var zebra = c.Lib.ByFile("zebra-notes.txt");
                itemId = zebra.Id;

                LibraryDriver.Activate(LibraryDriver.Poll(() => d.ByName(LibrarySeed.CollectionName), "sidebar collection entry"));
                d.Need("CollectionPage_Root");
                LibraryDriver.PollUntil(() => d.Titles().SequenceEqual(new[] { zebra.Title }), "collection shows its item");

                d.Select(zebra.Title);
                var expectedTitle = $"Remove 1 item from “{LibrarySeed.CollectionName}”?";
                var dlg = d.OpenDialog("CollectionPage_RemoveButton", expectedTitle);
                var primary = LibraryDriver.DialogPart(dlg, "PrimaryButton");
                Assert.Equal("Remove", primary.Name);
                var texts = LibraryDriver.DialogTexts(dlg);
                Assert.Contains(texts, t => t.Contains("stay in your library", StringComparison.OrdinalIgnoreCase));

                d.ClickPart(dlg, "PrimaryButton");
                d.WaitDialogGone(expectedTitle);

                LibraryDriver.PollUntil(() => d.HasText("This collection is empty."), "collection empty state shown");

                // Verify inside this block: the scratch root is torn down at the end of it, and the
                // in-process CoreClient below needs it to still be there.
                Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
                using var core = await LibrarySeed.OpenAsync(root);
                Assert.Contains(core.Items, i => i.Id == itemId); // still in the library
                await core.ListCollectionsAsync();
                var favourites = core.Collections.Single(x => x.Name == LibrarySeed.CollectionName);
                Assert.Empty(await core.ListItemsInCollectionAsync(favourites.Id)); // but no longer in the collection
            }
        }
        finally
        {
            if (root is not null) GistAppSession.DeleteRoot(root);
        }
    }

    // ── Appearance dialog (spec §7.3) ────────────────────────────────────────

    [UiFact]
    public async Task Appearance_dialog_applies_live_and_persists_across_a_restart()
    {
        string? root = null;
        try
        {
            using (var c = await LaunchAsync())
            {
                c.S.KeepRoot = true; // the restart below needs the same data root
                var d = c.D;
                root = c.S.Root;

                var appearance = LibraryDriver.Poll(() => d.ByName("Appearance"), "Appearance footer item");
                LibraryDriver.Activate(appearance);
                var dlg = d.Dialog("Appearance");

                var dark = LibraryDriver.DialogPart(dlg, "AppearanceDialog_" + ThemeSelection.Dark);
                LibraryDriver.Activate(dark);
                LibraryDriver.PollUntil(() => dark.Patterns.SelectionItem.Pattern.IsSelected.Value, "Dark selected");

                d.ClickPart(dlg, "CloseButton");
                d.WaitDialogGone("Appearance");

                // Close-only: the sidebar must land back on whatever screen was showing before, not stay
                // parked on "Appearance" with the Library page underneath.
                d.Need("LibraryPage_Root");
                Assert.True(c.S.Responding);

                Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
                var persisted = await File.ReadAllTextAsync(Path.Combine(root, "theme-selection.txt"));
                Assert.Equal(ThemeSelection.Dark.ToStorageString(), persisted.Trim());
            }

            // A fresh launch against the same root must come back up already Dark, not System.
            using var s2 = await GistAppSession.StartAsync(existingRoot: root);
            var d2 = new LibraryDriver(s2);
            d2.WaitForLibrary();
            var appearance2 = LibraryDriver.Poll(() => d2.ByName("Appearance"), "Appearance footer item");
            LibraryDriver.Activate(appearance2);
            var dlg2 = d2.Dialog("Appearance");
            var darkRadio = LibraryDriver.DialogPart(dlg2, "AppearanceDialog_" + ThemeSelection.Dark);
            Assert.True(darkRadio.Patterns.SelectionItem.Pattern.IsSelected.Value);
            d2.ClickPart(dlg2, "CloseButton");
        }
        finally
        {
            if (root is not null) GistAppSession.DeleteRoot(root);
        }
    }
}
