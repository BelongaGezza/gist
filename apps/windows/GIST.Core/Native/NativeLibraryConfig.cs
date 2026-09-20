using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

// gist_ffi.dll may only be resolved from the application's own directory tree: never PATH, the current
// working directory or System32 (DLL-planting hardening, docs/windows-development-plan.md W1 item 6).
// The generated uniffi P/Invokes declare no DefaultDllImportSearchPaths of their own, so this
// assembly-level default governs them. AssemblyDirectory implies no PATH/CWD lookup.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]
[assembly: InternalsVisibleTo("GIST.Core.Tests")]

namespace Gist.Core.Native;

/// <summary>
/// Resolves <c>gist_ffi</c> by ABSOLUTE path, architecture-aware: first next to the assembly, then under
/// <c>runtimes/win-{x64|arm64}/native/</c> (where the GIST.Core build stages it and where it flows into
/// consuming apps). Anything else returns zero and falls back to the restricted default search above,
/// so a missing DLL is a plain DllNotFoundException, never a load from an untrusted directory.
/// </summary>
internal static class NativeLibraryResolver
{
    internal const string LibraryName = "gist_ffi";

    // CA2255 suppressed deliberately: the resolver must exist before the first generated P/Invoke runs,
    // and a class library has no other entry point to register it from.
#pragma warning disable CA2255
    [ModuleInitializer]
    internal static void Register() =>
        NativeLibrary.SetDllImportResolver(typeof(NativeLibraryResolver).Assembly, Resolve);
#pragma warning restore CA2255

    internal static IEnumerable<string> Candidates(string baseDirectory, Architecture arch)
    {
        var dll = LibraryName + ".dll";
        yield return Path.Combine(baseDirectory, dll);
        var rid = arch switch
        {
            Architecture.X64 => "win-x64",
            Architecture.Arm64 => "win-arm64",
            _ => null,
        };
        if (rid is not null)
            yield return Path.Combine(baseDirectory, "runtimes", rid, "native", dll);
    }

    private static IntPtr Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (!string.Equals(libraryName, LibraryName, StringComparison.OrdinalIgnoreCase))
            return IntPtr.Zero;

        var baseDir = Path.GetDirectoryName(assembly.Location);
        if (string.IsNullOrEmpty(baseDir))
            baseDir = AppContext.BaseDirectory;

        foreach (var path in Candidates(baseDir, RuntimeInformation.ProcessArchitecture))
            if (File.Exists(path) && NativeLibrary.TryLoad(path, out var handle))
                return handle;
        return IntPtr.Zero;
    }
}
