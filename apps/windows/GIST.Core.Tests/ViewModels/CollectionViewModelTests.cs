using Gist.Core.Client;
using Gist.Core.Filtering;
using Gist.Core.Models;
using Gist.Core.Tests.TestSupport;
using Gist.Core.ViewModels;

namespace Gist.Core.Tests.ViewModels;

/// <summary>
/// Tests for <see cref="CollectionViewModel"/> (<c>docs/windows-ui-spec.md</c> §5), against a real
/// <c>GistCore</c> on a temp directory. The point of most of them is the difference from the
/// Library screen: removal here detaches an item from a collection and must never delete anything.
/// </summary>
public sealed class CollectionViewModelTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keyProvider = new();
    private readonly List<IDisposable> _disposables = new();

    private CoreClient _client = null!;

    public void Dispose()
    {
        for (var i = _disposables.Count - 1; i >= 0; i--)
        {
            _disposables[i].Dispose();
        }

        _workspace.Dispose();
    }

    private async Task<LibraryViewModel> NewLibraryAsync()
    {
        _client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        _disposables.Add(_client);
        Assert.True(await _client.InitializeAsync());

        var vm = new LibraryViewModel(_client, new FakeLibraryScheduler());
        _disposables.Add(vm);
        await vm.LoadAsync();
        return vm;
    }

    private CollectionViewModel NewCollection(CollectionVM collection)
    {
        var vm = new CollectionViewModel(_client, collection);
        _disposables.Add(vm);
        return vm;
    }

    private string WriteTextFile(string name, string body)
    {
        var path = Path.Combine(_workspace.Root, name);
        File.WriteAllText(path, body);
        return path;
    }

    /// <summary>Imports two files and puts both in a new collection.</summary>
    private async Task<(CollectionVM Collection, string First, string Second)> SeedAsync()
    {
        var library = await NewLibraryAsync();
        var alpha = await library.ImportFileAsync(WriteTextFile("alpha.txt", "First."));
        var zulu = await library.ImportFileAsync(WriteTextFile("zulu.txt", "Second."));
        library.SetSelection(new[] { alpha!, zulu! });

        var collection = await library.CreateCollectionAndAddAsync("Reading List");
        Assert.NotNull(collection);
        return (collection!, alpha!, zulu!);
    }

    [Fact]
    public async Task Load_lists_the_collections_contents_newest_first()
    {
        var (collection, _, _) = await SeedAsync();
        var vm = NewCollection(collection);

        await vm.LoadAsync();

        Assert.Equal("Reading List", vm.Title);
        Assert.Equal(new[] { "zulu", "alpha" }, vm.DisplayedItems.Select(i => i.Title));
        Assert.Equal(LibraryEmptyState.None, vm.EmptyState);
    }

    [Fact]
    public async Task The_collection_screen_has_its_own_sort()
    {
        var (collection, _, _) = await SeedAsync();
        var vm = NewCollection(collection);
        await vm.LoadAsync();

        vm.SortOrder = LibrarySortOrder.TitleAZ;

        Assert.Equal(new[] { "alpha", "zulu" }, vm.DisplayedItems.Select(i => i.Title));
    }

    /// <summary>§5: no Encrypt and no Add-to-Collection here; Open/Tags still need exactly one.</summary>
    [Fact]
    public async Task Encrypt_and_add_to_collection_are_never_available()
    {
        var (collection, first, second) = await SeedAsync();
        var vm = NewCollection(collection);
        await vm.LoadAsync();

        vm.SetSelection(new[] { first });
        Assert.False(vm.CanEncrypt);
        Assert.False(vm.CanAddToCollection);
        Assert.True(vm.CanRemove);
        Assert.True(vm.CanOpen);
        Assert.True(vm.CanEditTags);

        vm.SetSelection(new[] { first, second });
        Assert.False(vm.CanEncrypt);
        Assert.False(vm.CanAddToCollection);
        Assert.True(vm.CanRemove);
        Assert.False(vm.CanOpen);
        Assert.False(vm.CanEditTags);
    }

    [Fact]
    public async Task Remove_preview_names_the_collection_and_offers_no_destructive_button()
    {
        var (collection, first, _) = await SeedAsync();
        var vm = NewCollection(collection);
        await vm.LoadAsync();
        vm.SetSelection(new[] { first });

        var preview = vm.RemovePreview();

        Assert.Equal("Remove 1 item from “Reading List”?", preview.Title);
        Assert.False(preview.AllowsDeletingStoredCopy);
        Assert.Equal(new[] { "alpha" }, preview.Titles);
    }

    /// <summary>
    /// The semantic difference that matters: the item leaves the collection, stays in the library,
    /// and nothing on disk is deleted — neither GIST's stored copy nor the user's file.
    /// </summary>
    [Fact]
    public async Task Remove_detaches_the_item_but_leaves_the_library_and_the_files_alone()
    {
        var library = await NewLibraryAsync();
        var original = WriteTextFile("alpha.txt", "First.");
        var alpha = await library.ImportFileAsync(original);
        var zulu = await library.ImportFileAsync(WriteTextFile("zulu.txt", "Second."));
        library.SetSelection(new[] { alpha!, zulu! });
        var collection = await library.CreateCollectionAndAddAsync("Reading List");

        var storedCopy = _workspace.PredictSandboxedCopyPath(original);
        Assert.True(File.Exists(storedCopy));

        var vm = NewCollection(collection!);
        await vm.LoadAsync();
        vm.SetSelection(new[] { alpha! });

        await vm.RemoveAsync();

        Assert.Equal(new[] { "zulu" }, vm.DisplayedItems.Select(i => i.Title));
        Assert.Empty(vm.SelectedIds);
        Assert.Null(vm.LastError);

        // Still in the library, and both files untouched.
        await library.LoadAsync();
        Assert.Equal(2, library.DisplayedItems.Count);
        Assert.True(File.Exists(original));
        Assert.True(File.Exists(storedCopy));
    }

    [Fact]
    public async Task Removing_everything_shows_the_empty_collection_state()
    {
        var (collection, first, second) = await SeedAsync();
        var vm = NewCollection(collection);
        await vm.LoadAsync();
        vm.SetSelection(new[] { first, second });

        await vm.RemoveAsync();

        Assert.Empty(vm.DisplayedItems);
        Assert.Equal(LibraryEmptyState.NoItems, vm.EmptyState);
        Assert.Equal("No items", vm.EmptyTitle);
        Assert.Equal("This collection is empty.", vm.EmptyMessage);
    }

    [Fact]
    public async Task Remove_with_nothing_selected_does_nothing()
    {
        var (collection, _, _) = await SeedAsync();
        var vm = NewCollection(collection);
        await vm.LoadAsync();

        await vm.RemoveAsync();

        Assert.Equal(2, vm.DisplayedItems.Count);
        Assert.Null(vm.LastError);
    }

    /// <summary>Tag editing is available from this screen too (§5's context menu).</summary>
    [Fact]
    public async Task Tags_can_be_edited_from_the_collection_screen()
    {
        var (collection, first, _) = await SeedAsync();
        var vm = NewCollection(collection);
        await vm.LoadAsync();

        Assert.True(await vm.AddTagAsync(first, "  history "));
        Assert.Equal(new[] { "history" }, await vm.TagsForAsync(first));

        Assert.True(await vm.RemoveTagAsync(first, "history"));
        Assert.Empty(await vm.TagsForAsync(first));
    }
}
