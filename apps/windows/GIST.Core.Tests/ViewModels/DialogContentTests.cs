using Gist.Core.Client;
using Gist.Core.Models;
using Gist.Core.ViewModels;

namespace Gist.Core.Tests.ViewModels;

public class DialogContentTests
{
    [Theory]
    [InlineData(null, false)]
    [InlineData("", false)]
    [InlineData("   ", false)]
    [InlineData("https://example.com", true)]
    public void Import_and_create_are_disabled_while_blank(string? text, bool enabled)
    {
        Assert.Equal(enabled, DialogContent.CanImportUrl(text));
        Assert.Equal(enabled, DialogContent.CanCreateCollection(text));
        Assert.Equal(enabled, DialogContent.CanAddTag(text));
    }

    [Fact]
    public void Wording_matches_the_spec()
    {
        Assert.Equal("This book is DRM-protected and can't be imported.", DialogContent.DrmMessage);
        // One destructive removal button, not the old "Remove from Library" / "Also Delete Stored
        // Copy" pair (maintainer decision, 2026-09-21 — see ADR-006's addendum).
        Assert.Equal("Remove", DialogContent.RemoveButton);
        Assert.StartsWith("GIST fetches the page", DialogContent.ImportUrlBody);
    }

    [Fact]
    public void Every_error_kind_gets_fixed_text_and_drm_is_the_drm_sentence()
    {
        foreach (var kind in Enum.GetValues<CoreErrorKind>())
        {
            var text = DialogContent.ErrorMessage(kind);
            Assert.False(string.IsNullOrWhiteSpace(text));
            Assert.DoesNotContain("\\", text);
            Assert.DoesNotContain("panicked", text, StringComparison.OrdinalIgnoreCase);
        }

        Assert.Equal(DialogContent.DrmMessage, DialogContent.ErrorMessage(CoreErrorKind.DrmProtected));
        Assert.False(string.IsNullOrWhiteSpace(DialogContent.ErrorMessage(null)));
    }

    [Fact]
    public void Encrypt_result_lines_include_first_errors_only_for_failures()
    {
        var ok = DialogContent.EncryptResultLines(new EncryptItemsSummary(2, 1, 0));
        Assert.Equal(new[] { "2 items encrypted, 1 was already encrypted." }, ok);

        var failed = DialogContent.EncryptResultLines(
            new EncryptItemsSummary(1, 0, 1) { FirstErrors = new[] { "An item could not be encrypted." } });
        Assert.Equal(3, failed.Count);
        Assert.Equal("• An item could not be encrypted.", failed[2]);

        Assert.Single(DialogContent.EncryptResultLines(null));
    }

    [Fact]
    public void Encrypt_preview_carries_the_required_recovery_warning()
    {
        var p = EncryptPreview.For(2);
        Assert.Contains("no way to recover", p.RecoveryWarning);
        Assert.Equal("Encrypt 2 items?", p.Title);
    }
}
