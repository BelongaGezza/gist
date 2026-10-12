using Gist.Core.Client;
using Gist.Core.Models;
using Gist.Core.Tests.TestSupport;
using Gist.Core.ViewModels;

namespace Gist.Core.Tests.ViewModels;

/// <summary>SC-01 / SC-03 / SC-15 pure logic, then the same flows against a real core.</summary>
public sealed class ImportMetadataTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keys = new();
    private readonly FakeLibraryScheduler _scheduler = new();
    private readonly List<IDisposable> _disposables = new();

    public void Dispose()
    {
        foreach (var d in Enumerable.Reverse(_disposables)) d.Dispose();
        _workspace.Dispose();
    }

    private async Task<(CoreClient Client, LibraryViewModel Vm)> NewAsync()
    {
        var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keys);
        _disposables.Add(client);
        Assert.True(await client.InitializeAsync());
        var vm = new LibraryViewModel(client, _scheduler);
        _disposables.Add(vm);
        return (client, vm);
    }

    // ── Pure logic ─────────────────────────────────────────────────────────

    [Fact]
    public void Options_prepend_an_unlisted_detected_encoding_and_keep_the_curated_list_otherwise()
    {
        Assert.Same(TextEncodingChoices.Curated, TextEncodingChoices.Options("UTF-8"));
        var withExtra = TextEncodingChoices.Options("windows-874");
        Assert.Equal("windows-874", withExtra[0].Label);
        Assert.Equal(TextEncodingChoices.Curated.Count + 1, withExtra.Count);
        Assert.Equal("windows-1251", TextEncodingChoices.Selection("Windows-1251"));
        Assert.Equal("windows-874", TextEncodingChoices.Selection("windows-874"));
    }

    [Theory]
    [InlineData("a.txt", true)]
    [InlineData("A.MD", true)]
    [InlineData("a.markdown", true)]
    [InlineData("a.docx", false)]
    [InlineData("a", false)]
    public void Plain_text_detection_is_by_extension(string path, bool expected) =>
        Assert.Equal(expected, TextEncodingChoices.IsPlainTextFile(path));

    [Theory]
    [InlineData(false, false, false)]
    [InlineData(true, false, true)]
    [InlineData(false, true, true)]
    public void Probe_prompts_when_uncertain_or_lossy(bool uncertain, bool lossy, bool expected) =>
        Assert.Equal(expected, new TextEncodingProbe("x", uncertain, lossy).NeedsPrompt);

    [Fact]
    public void Reading_time_caption_rounds_up_and_hides_empty_documents()
    {
        Assert.Null(ReadingTimeFormat.Caption(0, 0));
        Assert.Null(ReadingTimeFormat.AccessibilityLabel(0, 0));
        Assert.Equal(1u, ReadingTimeFormat.Minutes(0));
        Assert.Equal(1u, ReadingTimeFormat.Minutes(60));
        Assert.Equal(2u, ReadingTimeFormat.Minutes(61));
        Assert.Equal(71582789u, ReadingTimeFormat.Minutes(uint.MaxValue));
        Assert.Contains("≈ 5 min", ReadingTimeFormat.Caption(1234, 300));
        Assert.Contains("about 5 minutes", ReadingTimeFormat.AccessibilityLabel(1234, 300));
    }

    // ── Against a real core ────────────────────────────────────────────────

    [Fact]
    public async Task Imported_text_item_reports_a_word_count_and_reading_time()
    {
        var (client, vm) = await NewAsync();
        var path = Path.Combine(_workspace.Root, "five.txt");
        await File.WriteAllTextAsync(path, "one two three four five");

        await vm.ImportFileAsync(path);

        var item = Assert.Single(client.Items);
        Assert.Equal(5u, item.WordCount);
        Assert.True(item.ReadingSeconds >= 1);
    }

    [Fact]
    public async Task Unambiguous_text_imports_without_prompting()
    {
        var (client, vm) = await NewAsync();
        var path = Path.Combine(_workspace.Root, "plain.txt");
        await File.WriteAllTextAsync(path, "Plain ASCII text that is perfectly clear.");

        var id = await vm.ImportFileAsync(path);

        Assert.NotNull(id);
        Assert.Null(vm.PendingEncoding);
        Assert.Equal(LibraryDialog.None, vm.PendingDialog);
        Assert.Single(client.Items);
    }

    [Fact]
    public async Task Ambiguous_text_pauses_for_an_encoding_choice_and_cancel_imports_nothing()
    {
        var (client, vm) = await NewAsync();
        var path = _workspace.CopyFixture("invalid_utf8_mixed.txt");

        var id = await vm.ImportFileAsync(path);

        Assert.NotNull(vm.PendingEncoding);
        Assert.Null(id);
        Assert.Equal(LibraryDialog.EncodingPrompt, vm.PendingDialog);
        Assert.Equal(path, vm.PendingEncoding!.Path);
        Assert.Empty(client.Items);

        vm.CancelEncodingPrompt();
        Assert.Null(vm.PendingEncoding);
        Assert.Empty(client.Items);
    }

    [Fact]
    public async Task Choosing_an_encoding_imports_with_it_and_a_bad_label_is_a_typed_error()
    {
        var (client, vm) = await NewAsync();
        var path = _workspace.CopyFixture("windows1251_russian.txt");

        Assert.Null(await vm.ImportFileWithEncodingAsync(path, "not-a-real-encoding"));
        Assert.Equal(CoreErrorKind.UnsupportedTextEncoding, vm.LastError?.Kind);
        Assert.Empty(client.Items);

        var id = await vm.ImportFileWithEncodingAsync(path, "windows-1251");
        Assert.NotNull(id);
        Assert.Single(client.Items);
    }

    [Fact]
    public async Task Tracked_changes_docx_shows_a_notice_and_a_plain_docx_does_not()
    {
        var (_, vm) = await NewAsync();

        await vm.ImportFileAsync(_workspace.CopyFixture("with_tracked_changes.docx"));
        Assert.Equal(DialogContent.TrackedChangesNotice, vm.ImportNotice);

        vm.DismissImportNotice();
        Assert.Null(vm.ImportNotice);

        await vm.ImportFileAsync(_workspace.CopyFixture("headings_and_paragraphs.docx"));
        Assert.Null(vm.ImportNotice);
    }
}
