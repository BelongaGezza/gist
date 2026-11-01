using Gist.Core.Rsvp;

namespace Gist.Core.Tests.Rsvp;

public class RsvpWordFitTests
{
    [Fact]
    public void A_word_that_fits_keeps_the_base_size()
    {
        Assert.Equal(48, RsvpWordFit.FittedFontSize(48, naturalWidth: 300, availableWidth: 900));
    }

    [Fact]
    public void A_word_exactly_the_available_width_keeps_the_base_size()
    {
        Assert.Equal(48, RsvpWordFit.FittedFontSize(48, naturalWidth: 900, availableWidth: 900));
    }

    [Fact]
    public void An_over_wide_word_scales_down_in_proportion()
    {
        Assert.Equal(24, RsvpWordFit.FittedFontSize(48, naturalWidth: 1800, availableWidth: 900), precision: 6);
    }

    [Fact]
    public void Scaling_never_goes_below_the_minimum_size()
    {
        Assert.Equal(RsvpWordFit.MinimumFontSize, RsvpWordFit.FittedFontSize(48, naturalWidth: 100_000, availableWidth: 300));
    }

    [Fact]
    public void A_base_size_below_the_minimum_is_never_raised()
    {
        Assert.Equal(6, RsvpWordFit.FittedFontSize(6, naturalWidth: 1000, availableWidth: 100));
    }

    [Theory]
    [InlineData(0, 900)]      // empty token measures nothing
    [InlineData(-5, 900)]
    [InlineData(300, 0)]      // no layout width yet
    [InlineData(300, -10)]
    [InlineData(double.NaN, 900)]
    [InlineData(300, double.NaN)]
    public void Degenerate_input_keeps_the_base_size(double natural, double available)
    {
        Assert.Equal(48, RsvpWordFit.FittedFontSize(48, natural, available));
    }

    [Fact]
    public void A_non_positive_base_size_is_returned_unchanged()
    {
        Assert.Equal(0, RsvpWordFit.FittedFontSize(0, 1000, 100));
    }
}
