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
