using System.Diagnostics;
using FlaUI.Core;
using FlaUI.Core.AutomationElements;
using FlaUI.UIA3;
using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Storage;

namespace Gist.App.UITests;

/// <summary>
/// One launched GIST.exe against a private scratch data root. Dispose always kills the process and
/// deletes the scratch root, even if the test failed.
/// </summary>
public sealed class GistAppSession : IDisposable
{
    private const string AppEnvVar = "GIST_DATA_ROOT";

    private readonly UIA3Automation _automation;
    private readonly Application _app;
    private readonly Process _proc;

    public string Root { get; }
    public string KeyFile => Path.Combine(Root, "keys", "content-key.dpapi");
    public Window Window { get; }
    public int ProcessId { get; }

    private GistAppSession(string root, Application app, Process proc, UIA3Automation automation, Window window)
    {
        Root = root;
        _app = app;
        _automation = automation;
        Window = window;
        ProcessId = app.ProcessId;
        _proc = proc;
    }

    /// <summary>Creates a scratch root, lets <paramref name="prepare"/> populate it, then launches the app.</summary>
    public static async Task<GistAppSession> StartAsync(Func<string, Task>? prepare = null, TimeSpan? windowTimeout = null)
    {
        var root = Path.Combine(Path.GetTempPath(), "gist-uitest-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        Application? app = null;
        Process? proc = null;
        UIA3Automation? automation = null;
        try
        {
            if (prepare is not null) await prepare(root);

            var psi = new ProcessStartInfo(LocateExe()) { UseShellExecute = false };
            psi.Environment[AppEnvVar] = root;
            proc = Process.Start(psi) ?? throw new InvalidOperationException("GIST.exe did not start.");
            app = Application.Attach(proc.Id);
            automation = new UIA3Automation();
            var win = app.GetMainWindow(automation, windowTimeout ?? TimeSpan.FromSeconds(30))
                ?? throw new TimeoutException("GIST main window did not appear.");
            return new GistAppSession(root, app, proc, automation, win);
        }
        catch
        {
            KillAndClean(app, root);
            try { if (proc is { HasExited: false }) proc.Kill(true); } catch (Exception) { }
            proc?.Dispose();
            automation?.Dispose();
            throw;
        }
    }

    /// <summary>Imports one generated text file through GIST.Core (real DPAPI key, real store); returns its title.</summary>
    public static async Task<string> SeedOneItemAsync(string root)
    {
        var paths = GistStoragePaths.ForRoot(root);
        paths.EnsureCreated();
        using var core = new CoreClient(new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
        Assert.True(await core.InitializeAsync(), $"seed init failed: {core.State}");
        var file = Path.Combine(root, "uitest-seed.txt");
        await File.WriteAllTextAsync(file, "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\nSed do eiusmod tempor incididunt ut labore.\n");
        Assert.NotNull(await core.ImportFileAsync(file));
        await core.RefreshAsync();
        Assert.Single(core.Items);
        var title = core.Items[0].Title;
        File.Delete(file);
        return title;
    }

    /// <summary>All descendant elements of the main window as (Name, ControlType).</summary>
    public (string Name, string Type)[] Elements() =>
        Window.FindAllDescendants().Select(e => (Safe(() => e.Name) ?? "", Safe(() => e.ControlType.ToString()) ?? "")).ToArray();

    public bool WaitFor(Func<(string Name, string Type)[], bool> predicate, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        do
        {
            try { if (predicate(Elements())) return true; } catch (Exception) { /* tree still building */ }
            Thread.Sleep(300);
        } while (DateTime.UtcNow < deadline && !_app.HasExited);
        return false;
    }

    public bool Responding
    {
        get { using var p = Process.GetProcessById(ProcessId); p.Refresh(); return p.Responding; }
    }

    /// <summary>Requests a normal window close; returns the exit code, or null if it did not exit in time.</summary>
    public int? CloseCleanly(TimeSpan timeout)
    {
        Window.Close();
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            if (_proc.HasExited) return _proc.ExitCode;
            Thread.Sleep(100);
        }
        return null;
    }

    /// <summary>Saves a PNG of the main window; returns false (never throws) if capture fails.</summary>
    public bool TrySaveScreenshot(string path)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            try { Window.SetForeground(); } catch (Exception) { }
            Thread.Sleep(500);
            using var img = FlaUI.Core.Capturing.Capture.Element(Window);
            img.ToFile(path);
            return true;
        }
        catch (Exception) { return false; }
    }

    public void Dispose()
    {
        KillAndClean(_app, Root);
        _proc.Dispose();
        _automation.Dispose();
    }

    private static void KillAndClean(Application? app, string root)
    {
        try { if (app is not null && !app.HasExited) app.Kill(); } catch (Exception) { }
        try { app?.Dispose(); } catch (Exception) { }
        for (var i = 0; i < 15; i++)
        {
            try { if (Directory.Exists(root)) Directory.Delete(root, true); return; }
            catch (IOException) { Thread.Sleep(200); }
            catch (UnauthorizedAccessException) { Thread.Sleep(200); }
        }
    }

    private static T? Safe<T>(Func<T> f) { try { return f(); } catch (Exception) { return default; } }

    private static string LocateExe()
    {
        var env = Environment.GetEnvironmentVariable("GIST_APP_EXE");
        if (!string.IsNullOrEmpty(env)) return env;
        var cfg = new DirectoryInfo(AppContext.BaseDirectory.TrimEnd('\\', '/')).Parent!.Name; // .../bin/<Config>/<tfm>
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, "GIST.App", "bin", cfg, "net10.0-windows10.0.19041.0", "win-x64", "GIST.exe");
            if (File.Exists(candidate)) return candidate;
        }
        throw new FileNotFoundException("GIST.exe not found; build GIST.sln first or set GIST_APP_EXE.");
    }
}
