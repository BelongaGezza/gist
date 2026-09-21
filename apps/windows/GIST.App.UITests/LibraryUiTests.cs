using System.Security.Cryptography;
using System.Text.RegularExpressions;

namespace Gist.App.UITests;

[Trait("Category", "UI")]
public class LibraryUiTests
{
    private static readonly TimeSpan Wait = TimeSpan.FromSeconds(30);
    private static readonly Regex Locked = new("^Encrypted items can.t be unlocked$");

    [UiFact]
    public async Task Seeded_item_is_visible_and_app_closes_cleanly()
    {
        string title = "";
        using var s = await GistAppSession.StartAsync(async root => title = await GistAppSession.SeedOneItemAsync(root));
        Assert.True(s.WaitFor(e => e.Any(x => x.Name == title), Wait), "seeded title not visible");
        Assert.DoesNotContain(s.Elements(), x => x.Name == "No books yet");
        Assert.True(s.Responding);
        Shot(s, "library");
        Assert.Equal(0, s.CloseCleanly(TimeSpan.FromSeconds(10)));
    }

    [UiFact]
    public async Task Corrupt_key_shows_blocking_page_without_touching_key()
    {
        string hashBefore = "";
        using var s = await GistAppSession.StartAsync(async root =>
        {
            await GistAppSession.SeedOneItemAsync(root);
            var key = Path.Combine(root, "keys", "content-key.dpapi");
            File.WriteAllBytes(key, Enumerable.Range(1, 97).Select(i => (byte)(i * 37 % 251)).ToArray());
            hashBefore = Hash(key);
        });
        Assert.True(s.WaitFor(e => e.Any(x => Locked.IsMatch(x.Name)), Wait), "blocking page not shown");
        var buttons = s.Elements()
            .Where(x => x.Type == "Button" && x.Name != "" && x.Name is not ("Minimize" or "Maximize" or "Restore" or "Close" or "Back"))
            .Select(x => x.Name).ToArray();
        Assert.Contains("Retry", buttons);
        Assert.DoesNotContain(buttons, n => Regex.IsMatch(n, "delete|reset|regenerate|erase|clear", RegexOptions.IgnoreCase));
        Assert.DoesNotContain(s.Elements(), x => Regex.IsMatch(x.Name, @"Exception|panic|[A-Za-z]:\\"));
        Shot(s, "corrupt-key");
        var keyFile = s.KeyFile;
        Assert.Equal(0, s.CloseCleanly(TimeSpan.FromSeconds(10)));
        Assert.Equal(hashBefore, Hash(keyFile));
        Assert.Single(Directory.GetFiles(Path.GetDirectoryName(keyFile)!));
    }

    [UiFact]
    public async Task Empty_profile_shows_empty_state_and_exits_cleanly()
    {
        using var s = await GistAppSession.StartAsync();
        Assert.True(s.WaitFor(e => e.Any(x => x.Name == "No books yet"), Wait), "empty state not shown");
        Assert.True(s.Responding);
        Shot(s, "empty");
        Assert.Equal(0, s.CloseCleanly(TimeSpan.FromSeconds(10)));
    }

    private static string Hash(string path) => Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path)));

    /// <summary>Best-effort PNG for the human visual check; only when GIST_UI_SCREENSHOT_DIR is set.</summary>
    private static void Shot(GistAppSession s, string name)
    {
        var dir = Environment.GetEnvironmentVariable("GIST_UI_SCREENSHOT_DIR");
        if (!string.IsNullOrEmpty(dir)) s.TrySaveScreenshot(Path.Combine(dir, name + ".png"));
    }
}
