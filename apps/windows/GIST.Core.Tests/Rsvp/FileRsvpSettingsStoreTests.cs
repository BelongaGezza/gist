using Gist.Core.Rsvp;

namespace Gist.Core.Tests.Rsvp;

public sealed class FileRsvpSettingsStoreTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "GistRsvpSettings-" + Guid.NewGuid().ToString("N"));

    public FileRsvpSettingsStoreTests() => Directory.CreateDirectory(_dir);

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    private string FilePath => Path.Combine(_dir, "rsvp-settings.txt");

    [Fact]
    public void Missing_file_loads_null()
    {
        Assert.Null(new FileRsvpSettingsStore(FilePath).LoadWpm());
    }

    [Fact]
    public void Saved_wpm_round_trips_through_a_fresh_instance()
    {
        new FileRsvpSettingsStore(FilePath).SaveWpm(425);

        Assert.Equal(425U, new FileRsvpSettingsStore(FilePath).LoadWpm());
    }

    [Theory]
    [InlineData("")]
    [InlineData("fast")]
    [InlineData("-5")]
    [InlineData("1e3")]
    public void Garbage_loads_null(string text)
    {
        File.WriteAllText(FilePath, text);

        Assert.Null(new FileRsvpSettingsStore(FilePath).LoadWpm());
    }

    [Fact]
    public void An_out_of_range_value_is_clamped_into_the_engine_range()
    {
        File.WriteAllText(FilePath, "99999");

        Assert.Equal(RsvpWpm.Max, new FileRsvpSettingsStore(FilePath).LoadWpm());
    }

    [Fact]
    public void A_write_to_a_missing_directory_is_swallowed()
    {
        new FileRsvpSettingsStore(Path.Combine(_dir, "nope", "x.txt")).SaveWpm(300);
    }
}
