using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Storage;

// Usage: SeedTool <data-root> <file-to-import>
// Seeds a scratch store (never the real profile unless the caller passes it) and prints the item count.
if (args.Length != 2)
{
    Console.Error.WriteLine("usage: SeedTool <data-root> <file-to-import>");
    return 2;
}

var paths = GistStoragePaths.ForRoot(args[0]);
paths.EnsureCreated();
using var core = new CoreClient(
    new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
if (!await core.InitializeAsync())
{
    Console.Error.WriteLine($"init failed: {core.State}");
    return 1;
}

var id = await core.ImportFileAsync(args[1]);
if (id is null)
{
    Console.Error.WriteLine($"import failed: {core.LastError?.Kind}");
    return 1;
}

await core.RefreshAsync();
foreach (var item in core.Items)
{
    Console.WriteLine($"TITLE={item.Title}");
}

Console.WriteLine($"COUNT={core.Items.Count}");
return 0;
