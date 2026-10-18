using Gist.Core.Models;

namespace Gist.Core.Tests.Models;

public sealed class ReadingStateTextTests
{
    private static readonly DateTimeOffset Now = new(2026, 10, 12, 15, 0, 0, TimeSpan.Zero);

    private static long Ms(DateTimeOffset t) => t.ToUnixTimeMilliseconds();

    [Fact]
    public void Never_opened_reads_as_not_read_yet() =>
        Assert.Equal("Not read yet", ReadingStateText.LastRead(null, Now));

    [Fact]
    public void Same_local_day_is_today_and_previous_day_is_yesterday()
    {
        Assert.Equal("Last read today", ReadingStateText.LastRead(Ms(Now), Now));
        Assert.Equal("Last read yesterday", ReadingStateText.LastRead(Ms(Now.AddDays(-1)), Now));
    }

    [Fact]
    public void Older_dates_show_the_date()
    {
        var text = ReadingStateText.LastRead(Ms(Now.AddDays(-30)), Now);
        Assert.StartsWith("Last read ", text);
        Assert.Contains("2026", text);
    }

    [Fact]
    public void Out_of_range_timestamp_does_not_throw() =>
        Assert.Equal("Not read yet", ReadingStateText.LastRead(long.MaxValue, Now));

    [Theory]
    [InlineData(0, 0)]
    [InlineData(0.456, 46)]
    [InlineData(1, 100)]
    [InlineData(7, 100)]
    [InlineData(-1, 0)]
    [InlineData(double.NaN, 0)]
    [InlineData(double.PositiveInfinity, 0)]
    public void Progress_percent_is_clamped_and_finite(double fraction, int expected) =>
        Assert.Equal(expected, ReadingStateText.ProgressPercent(fraction));
}
