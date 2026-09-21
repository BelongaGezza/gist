using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Storage;
using Microsoft.UI.Dispatching;

namespace Gist.App;

/// <summary>
/// Composition root. Pages obtain the <see cref="CoreClient"/> from here, never construct it themselves.
/// </summary>
public static class AppServices
{
    /// <summary>
    /// Dev/test only (SETUP_NOTES.md, "Windows developer inner loop"): when set, the store lives under this
    /// directory instead of the real profile. Never set by the product.
    /// </summary>
    public const string DataRootEnvVar = "GIST_DATA_ROOT";

    public static bool IsInitialized { get; private set; }

    /// <summary>The main window (set by App); owner for pickers.</summary>
    public static Microsoft.UI.Xaml.Window? MainWindow { get; set; }

    /// <summary>TODO(W2): swap for Gist.App.Dialogs.LibraryDialogHost once merged.</summary>
    internal static Views.ILibraryDialogHost DialogHost { get; set; } = new Views.StubLibraryDialogHost();

    public static CoreClient Core { get; private set; } = null!;

    public static GistStoragePaths Paths { get; private set; } = null!;

    /// <summary>Completes when the startup initialise + first list load has finished (never faults).</summary>
    public static Task StartupTask { get; private set; } = Task.CompletedTask;

    /// <summary>Must be called on the UI thread (captures its DispatcherQueue). Does no FFI and no blocking I/O.</summary>
    public static void Initialize()
    {
        if (IsInitialized) return;

        var overrideRoot = Environment.GetEnvironmentVariable(DataRootEnvVar);
        Paths = string.IsNullOrWhiteSpace(overrideRoot)
            ? GistStoragePaths.Resolve()
            : GistStoragePaths.ForRoot(overrideRoot);

        var queue = DispatcherQueue.GetForCurrentThread();
        // If the queue is shutting down the publish is dropped (the awaiting background task just never completes).
        var dispatcher = new DelegateUiDispatcher(action => queue.TryEnqueue(() => action()));

        Core = new CoreClient(
            new CoreClientOptions(Paths.DbPath, Paths.StorageDir, new DpapiKeyProvider(Paths.KeyDir)),
            dispatcher);
        IsInitialized = true;

        // Directory creation, DPAPI, FFI and the first list all happen off the UI thread.
        StartupTask = Task.Run(StartAsync);
    }

    /// <summary>Re-runs initialisation (used by the Retry buttons) and reloads the list.</summary>
    public static Task RetryAsync() => Task.Run(async () =>
    {
        await Core.RetryAsync().ConfigureAwait(false);
        await AfterInitializeAsync().ConfigureAwait(false);
    });

    private static async Task StartAsync()
    {
        try
        {
            Paths.EnsureCreated();
        }
        catch (Exception)
        {
            // No exception text is surfaced; InitializeAsync below will report the store as unavailable.
        }

        await Core.InitializeAsync().ConfigureAwait(false);
        await AfterInitializeAsync().ConfigureAwait(false);
    }

    private static async Task AfterInitializeAsync()
    {
        if (Core.IsReady) await Core.RefreshAsync().ConfigureAwait(false);
        WriteDiagnostic();
    }

    /// <summary>
    /// Only when GIST_DATA_ROOT is set (dev/test): writes counts and state names, never titles or paths, to
    /// diag.log under that root so scripted verification has evidence independent of UI Automation.
    /// </summary>
    private static void WriteDiagnostic()
    {
        if (string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable(DataRootEnvVar))) return;
        try
        {
            File.AppendAllText(
                Path.Combine(Paths.Root, "diag.log"),
                $"{DateTime.UtcNow:O} state={Core.State} items={Core.Items.Count}{Environment.NewLine}");
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
