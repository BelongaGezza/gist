using Gist.Core.Flow;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Flow;

public sealed class TypographySettingsTests
{
    [Fact]
    public void Defaults_are_17pt_default_font_regular_spacing()
    {
        var t = TypographySettings.Default;

        Assert.Equal(17, t.FontSize);
        Assert.Equal(ReadingFontDesign.Default, t.FontDesign);
        Assert.Equal(LineSpacingOption.Regular, t.LineSpacing);
    }

    [Theory]
    [InlineData(5, 13)]
    [InlineData(13, 13)]
    [InlineData(20.5, 20.5)]
    [InlineData(28, 28)]
    [InlineData(99, 28)]
    [InlineData(double.NaN, 17)]
    [InlineData(double.PositiveInfinity, 17)]
    public void Font_size_is_clamped_into_13_to_28_and_non_finite_becomes_default(double input, double expected) =>
        Assert.Equal(expected, new TypographySettings { FontSize = input }.FontSize);

    [Fact]
    public void With_expressions_re_clamp()
    {
        Assert.Equal(28, (TypographySettings.Default with { FontSize = 500 }).FontSize);
        Assert.Equal(13, TypographySettings.Default.WithFontSize(-1).FontSize);
    }

    [Theory]
    [InlineData(LineSpacingOption.Compact, 2)]
    [InlineData(LineSpacingOption.Regular, 6)]
    [InlineData(LineSpacingOption.Relaxed, 12)]
    public void Line_spacing_adds_fixed_points(LineSpacingOption option, double points) =>
        Assert.Equal(points, option.ExtraPoints());

    [Fact]
    public void Serialise_then_parse_round_trips_with_enum_names_as_persisted_strings()
    {
        var t = new TypographySettings { FontSize = 21.5, FontDesign = ReadingFontDesign.Rounded, LineSpacing = LineSpacingOption.Relaxed };

        var text = t.Serialize();

        Assert.Equal("size=21.5;font=Rounded;spacing=Relaxed", text);
        Assert.Equal(t, TypographySettings.Parse(text));
    }

    [Fact]
    public void Serialisation_is_culture_invariant()
    {
        var saved = System.Globalization.CultureInfo.CurrentCulture;
        try
        {
            System.Globalization.CultureInfo.CurrentCulture = new System.Globalization.CultureInfo("de-DE");
            Assert.Equal("size=18.5;font=Default;spacing=Regular", new TypographySettings { FontSize = 18.5 }.Serialize());
            Assert.Equal(18.5, TypographySettings.Parse("size=18.5").FontSize);
        }
        finally
        {
            System.Globalization.CultureInfo.CurrentCulture = saved;
        }
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("garbage")]
    [InlineData("size=;font=;spacing=")]
    [InlineData("size=abc;font=Comic;spacing=Huge")]
    [InlineData("font=7;spacing=99")] // numeric enum strings must not be accepted as undefined values
    public void Parse_falls_back_to_defaults_for_unknown_or_malformed_parts(string? text) =>
        Assert.Equal(TypographySettings.Default, TypographySettings.Parse(text));

    [Fact]
    public void Parse_keeps_the_good_parts_when_others_are_bad()
    {
        var t = TypographySettings.Parse("size=99;font=Serif;spacing=nope");

        Assert.Equal(28, t.FontSize);
        Assert.Equal(ReadingFontDesign.Serif, t.FontDesign);
        Assert.Equal(LineSpacingOption.Regular, t.LineSpacing);
    }

    [Fact]
    public void Parse_ignores_over_long_input()
    {
        Assert.Equal(TypographySettings.Default, TypographySettings.Parse("size=20;" + new string('x', 1000)));
    }

    [Fact]
    public void State_raises_changed_only_for_real_changes()
    {
        var state = new TypographyState();
        var raised = 0;
        state.Changed += (_, _) => raised++;

        state.Settings = TypographySettings.Default;
        state.Settings = TypographySettings.Default with { FontSize = 20 };
        state.Settings = TypographySettings.Default with { FontSize = 20 };

        Assert.Equal(1, raised);
        Assert.Equal(20, state.Settings.FontSize);
    }

    [Fact]
    public void File_store_round_trips_and_is_best_effort()
    {
        using var ws = new TempWorkspace();
        var path = Path.Combine(ws.Root, "typography.txt");
        var store = new FileTypographySettingsStore(path);

        Assert.Equal(TypographySettings.Default, store.Load()); // absent file

        var t = new TypographySettings { FontSize = 24, FontDesign = ReadingFontDesign.Serif, LineSpacing = LineSpacingOption.Compact };
        store.Save(t);
        Assert.Equal(t, store.Load());

        File.WriteAllBytes(path, new byte[100_000]); // hostile: huge binary file
        Assert.Equal(TypographySettings.Default, store.Load());

        // Unwritable location: Save must not throw.
        new FileTypographySettingsStore(Path.Combine(ws.Root, "missing-dir", "t.txt")).Save(t);
    }
}

public sealed class ReadingProgressTests
{
    [Theory]
    [InlineData(-3, 0)]
    [InlineData(0.25, 0.25)]
    [InlineData(7, 1)]
    [InlineData(double.NaN, 0)]
    [InlineData(double.PositiveInfinity, 0)]
    public void Initial_fraction_is_clamped_and_non_finite_becomes_zero(double input, double expected)
    {
        var p = new ReadingProgress(input);

        Assert.Equal(expected, p.InitialFraction);
        Assert.Equal(expected, p.Fraction);
    }

    [Fact]
    public void Update_clamps_ignores_non_finite_and_raises_changed_only_on_change()
    {
        var p = new ReadingProgress(0.5);
        var raised = 0;
        p.Changed += (_, _) => raised++;

        p.Update(double.NaN);
        p.Update(double.NegativeInfinity);
        p.Update(0.5);
        Assert.Equal(0, raised);
        Assert.Equal(0.5, p.Fraction);

        p.Update(2);
        Assert.Equal(1, p.Fraction);
        p.Update(-1);
        Assert.Equal(0, p.Fraction);
        Assert.Equal(2, raised);
        Assert.Equal(0.5, p.InitialFraction); // restore target never moves
    }

    [Fact]
    public void Percent_text_is_a_whole_percent()
    {
        Assert.Equal("0%", new ReadingProgress(0).PercentText);
        Assert.Equal("42%", new ReadingProgress(0.42).PercentText);
        Assert.Equal("100%", new ReadingProgress(1).PercentText);
    }

    [Fact]
    public void Block_and_fraction_mapping_round_trips_and_handles_tiny_lists()
    {
        Assert.Equal(0, ReadingProgress.FractionForBlock(0, 0));
        Assert.Equal(0, ReadingProgress.FractionForBlock(0, 1));
        Assert.Equal(1, ReadingProgress.FractionForBlock(9, 10));
        Assert.Equal(1, ReadingProgress.FractionForBlock(99, 10)); // clamped
        Assert.Equal(-1, ReadingProgress.BlockForFraction(0.5, 0));
        Assert.Equal(0, ReadingProgress.BlockForFraction(double.NaN, 10));
        Assert.Equal(9, ReadingProgress.BlockForFraction(5, 10));
        for (var i = 0; i < 200; i++)
        {
            Assert.Equal(i, ReadingProgress.BlockForFraction(ReadingProgress.FractionForBlock(i, 200), 200));
        }
    }
}

public sealed class FileFlowScrollPositionStoreTests : IDisposable
{
    private readonly TempWorkspace _ws = new();

    public void Dispose() => _ws.Dispose();

    private string StoreDir => Path.Combine(_ws.Root, "flow");

    [Fact]
    public void Missing_position_loads_as_zero()
    {
        Assert.Equal(0, new FileFlowScrollPositionStore(StoreDir).Load("01a10779-ed15-7372-8d1f-82349032bf86"));
    }

    [Fact]
    public void Positions_round_trip_per_item_and_the_directory_is_created_on_save()
    {
        var store = new FileFlowScrollPositionStore(StoreDir);
        const string a = "01a10779-ed15-7372-8d1f-82349032bf86";
        const string b = "01a10779-ed05-7189-a189-5f425f16905f";

        store.Save(a, 0.3125);
        store.Save(b, 0.9);

        Assert.Equal(0.3125, store.Load(a));
        Assert.Equal(0.9, store.Load(b));
        Assert.True(File.Exists(Path.Combine(StoreDir, "FlowScrollPosition." + a)));
    }

    [Fact]
    public void Values_are_clamped_on_save_and_non_finite_is_not_written()
    {
        var store = new FileFlowScrollPositionStore(StoreDir);

        store.Save("x1", 5);
        Assert.Equal(1, store.Load("x1"));
        store.Save("x1", double.NaN);
        Assert.Equal(1, store.Load("x1")); // unchanged
        store.Save("x2", -2);
        Assert.Equal(0, store.Load("x2"));
    }

    [Fact]
    public void Corrupt_or_hostile_file_contents_load_as_zero()
    {
        Directory.CreateDirectory(StoreDir);
        var store = new FileFlowScrollPositionStore(StoreDir);
        File.WriteAllText(Path.Combine(StoreDir, "FlowScrollPosition.bad1"), "not a number");
        File.WriteAllText(Path.Combine(StoreDir, "FlowScrollPosition.bad2"), "NaN");
        File.WriteAllText(Path.Combine(StoreDir, "FlowScrollPosition.bad3"), "9e99");
        File.WriteAllBytes(Path.Combine(StoreDir, "FlowScrollPosition.bad4"), new byte[500_000]);

        Assert.Equal(0, store.Load("bad1"));
        Assert.Equal(0, store.Load("bad2"));
        Assert.Equal(1, store.Load("bad3")); // finite but huge: clamped
        Assert.Equal(0, store.Load("bad4"));
    }

    [Theory]
    [InlineData("")]
    [InlineData("..")]
    [InlineData("../evil")]
    [InlineData("..\\evil")]
    [InlineData("a/b")]
    [InlineData("a\\b")]
    [InlineData("C:\\Windows\\win")]
    [InlineData("C:evil")]
    [InlineData("file.txt")]
    [InlineData("name:stream")]
    [InlineData("con ")]
    [InlineData("tab\tid")]
    [InlineData("nul\0byte")]
    [InlineData("é")]
    public void Unsafe_item_ids_are_refused_and_never_touch_the_filesystem(string itemId)
    {
        var store = new FileFlowScrollPositionStore(StoreDir);

        Assert.False(FileFlowScrollPositionStore.IsValidItemId(itemId));
        store.Save(itemId, 0.5);
        Assert.Equal(0, store.Load(itemId));
        Assert.False(Directory.Exists(StoreDir)); // nothing was created
        Assert.DoesNotContain(
            Directory.GetFiles(_ws.Root, "*", SearchOption.AllDirectories),
            f => f.Contains("FlowScrollPosition", StringComparison.Ordinal));
    }

    [Fact]
    public void A_traversal_id_cannot_overwrite_a_file_outside_the_store_directory()
    {
        var victim = Path.Combine(_ws.Root, "FlowScrollPosition.victim");
        File.WriteAllText(victim, "keep");
        var store = new FileFlowScrollPositionStore(StoreDir);

        store.Save("..\\FlowScrollPosition.victim", 0.7);
        store.Save("../victim", 0.7);

        Assert.Equal("keep", File.ReadAllText(victim));
    }

    [Fact]
    public void Over_long_ids_are_refused_and_uuid_shaped_ids_are_accepted()
    {
        Assert.False(FileFlowScrollPositionStore.IsValidItemId(new string('a', 65)));
        Assert.True(FileFlowScrollPositionStore.IsValidItemId(new string('a', 64)));
        Assert.True(FileFlowScrollPositionStore.IsValidItemId("01a10779-ed15-7372-8d1f-82349032bf86"));
        Assert.False(FileFlowScrollPositionStore.IsValidItemId(null));
    }

    [Fact]
    public void An_unwritable_store_location_is_swallowed()
    {
        var blocker = Path.Combine(_ws.Root, "afile");
        File.WriteAllText(blocker, "x");
        var store = new FileFlowScrollPositionStore(Path.Combine(blocker, "sub")); // parent is a file

        store.Save("ok", 0.5); // must not throw
        Assert.Equal(0, store.Load("ok"));
    }
}

public sealed class SectionNavigatorTests
{
    private static FlowDocument TwoSections() => new("d", "T", null,
    [
        new FlowSection("s0", null, []),
        new FlowSection("s1", null, [new ParagraphBlock([new FlowTextRun("x", false, false, false)])]),
    ]);

    [Fact]
    public void A_request_is_raised_once_and_consumed_once()
    {
        var nav = new SectionNavigator(TwoSections());
        var raised = 0;
        nav.Requested += (_, _) => raised++;

        nav.Request("s1");

        Assert.Equal(1, raised);
        Assert.True(nav.TryConsume(out var id));
        Assert.Equal("s1", id);
        Assert.False(nav.TryConsume(out _));
    }

    [Fact]
    public void Requesting_the_same_section_again_is_a_new_request()
    {
        var nav = new SectionNavigator(TwoSections());
        var raised = 0;
        nav.Requested += (_, _) => raised++;

        nav.Request("s0");
        nav.TryConsume(out _);
        nav.Request("s0");

        Assert.Equal(2, raised);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("nope")]
    public void Unknown_or_empty_ids_are_ignored(string? id)
    {
        var nav = new SectionNavigator(TwoSections());
        var raised = 0;
        nav.Requested += (_, _) => raised++;

        nav.Request(id);

        Assert.Equal(0, raised);
        Assert.Null(nav.PendingSectionId);
    }

    [Fact]
    public void Document_section_lookup_and_first_entry_mapping()
    {
        var doc = TwoSections();

        Assert.Equal(1, doc.FindSectionIndex("s1"));
        Assert.Equal(-1, doc.FindSectionIndex("zz"));
        Assert.Equal(-1, doc.FindSectionIndex(null));
        Assert.Equal(-1, doc.FirstEntryIndexOfSection(0)); // empty section has no block to scroll to
        Assert.Equal(0, doc.FirstEntryIndexOfSection(1));
    }
}

public sealed class ReadingLayoutSeamTests
{
    private sealed class RecordingLayout : IReadingLayout
    {
        public ReadingLayoutContext? Bound { get; private set; }

        public void Bind(ReadingLayoutContext context) => Bound = context;
    }

    [Fact]
    public void A_layout_receives_the_shared_state_objects_through_the_seam()
    {
        var doc = new FlowDocument("d", "T", null, []);
        var typography = new TypographyState();
        var search = new FlowSearchState();
        var nav = new SectionNavigator(doc);
        var progress = new ReadingProgress(0.4);
        IReadingLayout layout = new RecordingLayout();

        layout.Bind(new ReadingLayoutContext(doc, typography, search, nav, progress));

        var bound = ((RecordingLayout)layout).Bound!;
        Assert.Same(doc, bound.Document);
        Assert.Same(typography, bound.Typography);
        Assert.Same(search, bound.Search);
        Assert.Same(nav, bound.Navigation);
        Assert.Same(progress, bound.Progress);
    }
}
