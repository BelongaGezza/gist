using System.Runtime.InteropServices;

namespace Gist.Core.Storage;

/// <summary>
/// The single source of truth for where GIST keeps its data on Windows. Every other component —
/// <c>CoreClient</c>'s <c>NewWithReadKey(dbPath, storageDir)</c> call, the DPAPI key provider, tests,
/// diagnostics — takes its paths from here rather than composing its own.
/// <para>
/// <b>Invariant:</b> <see cref="DbPath"/>, <see cref="StorageDir"/> and <see cref="KeyDir"/> are always
/// derived from one <see cref="Root"/>, so the key and the content it unlocks move, back up and get
/// deleted together (review finding Q9). A debug build pointed at a scratch root can never read the
/// production store with a different key, because a root chooses all three at once.
/// </para>
/// </summary>
public sealed partial class GistStoragePaths
{
    /// <summary>SQLite database file name (matches the Apple shell's).</summary>
    public const string DbFileName = "gist.sqlite3";

    /// <summary>IR blobs and ADR-006 <c>originals/</c> copies live here.</summary>
    public const string StorageDirName = "storage";

    /// <summary>ADR-016 DPAPI key file lives here, not loose in the root.</summary>
    public const string KeyDirName = "keys";

    /// <summary>Folder under <c>%LOCALAPPDATA%</c> used when the process has no package identity.</summary>
    public const string UnpackagedFolderName = "GIST";

    private const int ErrorSuccess = 0;
    private const int ErrorInsufficientBuffer = 122;
    private const int AppModelErrorNoPackage = 15700;

    private static readonly Lazy<string?> PackageFamily = new(TryGetCurrentPackageFamilyName, isThreadSafe: true);

    private GistStoragePaths(string root)
    {
        Root = root;
        DbPath = Path.Combine(root, DbFileName);
        StorageDir = Path.Combine(root, StorageDirName);
        KeyDir = Path.Combine(root, KeyDirName);
    }

    /// <summary>Absolute, normalised root directory. Everything else hangs off it.</summary>
    public string Root { get; }

    /// <summary>Absolute path of the SQLite database (may not exist yet; the core creates it).</summary>
    public string DbPath { get; }

    /// <summary>Absolute path of the storage directory passed to the Rust core.</summary>
    public string StorageDir { get; }

    /// <summary>Absolute path of the directory holding the DPAPI key file.</summary>
    public string KeyDir { get; }

    /// <summary><c>true</c> when this process runs with MSIX package identity.</summary>
    public static bool IsPackaged => PackageFamily.Value is not null;

    /// <summary>The package family name, or <c>null</c> when the process has no package identity.</summary>
    public static string? CurrentPackageFamilyName => PackageFamily.Value;

    /// <summary>Explicit root — used by tests, tooling and any host that already knows its data folder.</summary>
    public static GistStoragePaths ForRoot(string root)
    {
        if (string.IsNullOrWhiteSpace(root)) throw new ArgumentException("root required", nameof(root));
        return new GistStoragePaths(Path.TrimEndingDirectorySeparator(Path.GetFullPath(root)));
    }

    /// <summary>
    /// Unpackaged layout: <c>%LOCALAPPDATA%\GIST</c>. Local, never Roaming — a DPAPI key that followed a
    /// roaming profile to another machine would be useless and the content would not be there (ADR-016).
    /// </summary>
    public static GistStoragePaths ForUnpackaged()
    {
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (string.IsNullOrEmpty(localAppData))
            throw new InvalidOperationException("LOCALAPPDATA is not available for this process.");
        return ForRoot(Path.Combine(localAppData, UnpackagedFolderName));
    }

    /// <summary>
    /// Packaged layout: the package's LocalState folder,
    /// <c>%LOCALAPPDATA%\Packages\&lt;PackageFamilyName&gt;\LocalState</c> — the same directory
    /// <c>Windows.Storage.ApplicationData.Current.LocalFolder</c> returns, resolved without a WinRT
    /// reference so that <c>GIST.Core</c> stays UI-free and headless-testable.
    /// </summary>
    /// <exception cref="InvalidOperationException">This process has no package identity.</exception>
    /// <remarks>
    /// Unverified: no MSIX package exists yet, so this path has never been observed from inside one
    /// (see the ADR-016 addendum). If it ever disagrees with <c>ApplicationData.Current.LocalFolder</c>,
    /// the escape hatch needs no redesign — the packaged host passes that folder to <see cref="ForRoot"/>.
    /// </remarks>
    public static GistStoragePaths ForPackaged()
    {
        var family = PackageFamily.Value
            ?? throw new InvalidOperationException(
                "This process has no package identity; use ForUnpackaged() or ForRoot(localFolderPath).");
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (string.IsNullOrEmpty(localAppData))
            throw new InvalidOperationException("LOCALAPPDATA is not available for this process.");
        return ForRoot(Path.Combine(localAppData, "Packages", family, "LocalState"));
    }

    /// <summary>Picks the packaged or unpackaged layout for the current process.</summary>
    public static GistStoragePaths Resolve() => IsPackaged ? ForPackaged() : ForUnpackaged();

    /// <summary>Creates <see cref="Root"/>, <see cref="StorageDir"/> and <see cref="KeyDir"/> if absent. Idempotent.</summary>
    public void EnsureCreated()
    {
        Directory.CreateDirectory(Root);
        Directory.CreateDirectory(StorageDir);
        Directory.CreateDirectory(KeyDir);
    }

    public override string ToString() => $"GistStoragePaths(Root={Root})";

    private static unsafe string? TryGetCurrentPackageFamilyName()
    {
        try
        {
            uint length = 0;
            int rc = GetCurrentPackageFamilyName(&length, null);
            if (rc == AppModelErrorNoPackage) return null;
            if (rc != ErrorInsufficientBuffer && rc != ErrorSuccess) return null;
            if (length == 0) return null;

            var buffer = new char[length];
            fixed (char* p = buffer)
            {
                rc = GetCurrentPackageFamilyName(&length, p);
            }
            if (rc != ErrorSuccess || length == 0) return null;
            return new string(buffer, 0, (int)length - 1); // length includes the terminating NUL
        }
        catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException)
        {
            return null; // pre-Windows-8 / trimmed host: treat as unpackaged
        }
    }

    [LibraryImport("kernel32.dll", EntryPoint = "GetCurrentPackageFamilyName")]
    private static unsafe partial int GetCurrentPackageFamilyName(uint* packageFamilyNameLength, char* packageFamilyName);
}
