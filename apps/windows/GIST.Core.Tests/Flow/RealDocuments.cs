using Gist.Core.Client;
using Gist.Core.Flow;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Flow;

/// <summary>
/// Imports corpus fixtures through a <b>real</b> <c>GistCore</c> and returns the exact
/// <c>get_document_json</c> text, so decoder tests run against what the engine really emits.
/// </summary>
internal sealed class RealDocuments : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keys = new();
    private CoreClient? _client;

    public void Dispose()
    {
        _client?.Dispose();
        _workspace.Dispose();
    }

    public async Task<string> JsonAsync(string fixture)
    {
        if (_client is null)
        {
            _client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keys);
            Assert.True(await _client.InitializeAsync());
        }

        var id = await _client.ImportFileAsync(_workspace.CopyFixture(fixture));
        Assert.NotNull(id);
        var json = await _client.GetDocumentJsonAsync(id!);
        Assert.False(string.IsNullOrEmpty(json));
        return json!;
    }

    public async Task<FlowDocument> DocumentAsync(string fixture)
    {
        var result = FlowDocumentDecoder.Decode(await JsonAsync(fixture));
        Assert.True(result.Succeeded, $"decode failed: {result.Error}");
        return result.Document!;
    }
}
