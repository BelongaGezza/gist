using System.Reflection;
using System.Runtime.InteropServices;

namespace Gist.Core.Tests.Native;

public class NativeLibraryConfigTests
{
    [Fact]
    public void CoreAssemblyRestrictsDllSearchToAssemblyDirectory()
    {
        var core = Assembly.Load("GIST.Core");
        var attr = core.GetCustomAttribute<DefaultDllImportSearchPathsAttribute>();
        Assert.NotNull(attr);
        Assert.Equal(DllImportSearchPath.AssemblyDirectory, attr!.Paths);
    }
}

public class NativeLibraryResolverTests
{
    [Fact]
    public void Candidates_are_absolute_app_local_and_arch_specific()
    {
        var baseDir = Path.Combine(Path.GetTempPath(), "app");
        var x64 = Gist.Core.Native.NativeLibraryResolver.Candidates(baseDir, System.Runtime.InteropServices.Architecture.X64).ToArray();
        var arm = Gist.Core.Native.NativeLibraryResolver.Candidates(baseDir, System.Runtime.InteropServices.Architecture.Arm64).ToArray();
        Assert.All(x64.Concat(arm), p => Assert.StartsWith(baseDir, p));
        Assert.EndsWith(Path.Combine("runtimes", "win-x64", "native", "gist_ffi.dll"), x64[1]);
        Assert.EndsWith(Path.Combine("runtimes", "win-arm64", "native", "gist_ffi.dll"), arm[1]);
        Assert.DoesNotContain(x64, p => p.Contains("win-arm64"));
    }
}
