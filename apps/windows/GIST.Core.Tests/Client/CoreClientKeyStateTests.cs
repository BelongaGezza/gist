using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Client;

/// <summary>
/// The key-custody behaviour ADR-016 and W1 requirement (1) pin down: the key is obtained
/// <b>eagerly, in managed code</b>, before the store is opened, because the uniffi callback has no
/// error channel; a corrupt key store is a blocking state that never mints a replacement; a
/// temporarily unavailable one is retryable.
/// </summary>
public sealed class CoreClientKeyStateTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();

    public void Dispose() => _workspace.Dispose();

    /// <summary>
    /// The central safety property: a corrupt key store must never cause a new key to be generated.
    /// Doing so would silently orphan every item already encrypted under the old key — the data
    /// loss ADR-016 exists to prevent. The store is not opened at all, so the blocking state cannot
    /// be mistaken for a working library.
    /// </summary>
    [Fact]
    public async Task A_corrupt_key_store_blocks_the_client_and_never_generates_a_replacement_key()
    {
        var provider = new FailingKeyProvider(() => new KeyStoreCorruptException("tampered"));
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, provider);

        var ok = await client.InitializeAsync();

        Assert.False(ok);
        Assert.Equal(CoreClientState.KeyStoreCorrupt, client.State);
        Assert.False(client.IsReady);
        Assert.NotNull(client.LastError);
        Assert.Equal(CoreErrorKind.KeyStoreCorrupt, client.LastError!.Kind);
        Assert.True(client.LastError.IsBlocking);
        Assert.False(client.LastError.IsRetryable);

        Assert.Equal(0, provider.KeysGenerated);
        Assert.False(File.Exists(_workspace.DbPath), "no GistCore may be created in the corrupt state");
    }

    [Fact]
    public async Task In_the_corrupt_state_every_operation_fails_fast_without_touching_the_key_store()
    {
        var provider = new FailingKeyProvider(() => new KeyStoreCorruptException("tampered"));
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, provider);
        Assert.False(await client.InitializeAsync());
        var callsAfterInit = provider.CallCount;

        await client.RefreshAsync();
        await client.ListCollectionsAsync();
        await client.ListAllTagsAsync();
        var id = await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        var summary = await client.EncryptItemsAsync(new[] { "some-id" });
        var tags = await client.ListTagsForItemAsync("some-id");

        Assert.Null(id);
        Assert.Empty(client.Items);
        Assert.Empty(client.Collections);
        Assert.Empty(client.AllTags);
        Assert.Empty(tags);
        Assert.Equal(1, summary.FailedCount);

        // The blocking explanation survives: it is not replaced by a vaguer per-operation message.
        Assert.Equal(CoreErrorKind.KeyStoreCorrupt, client.LastError!.Kind);
        Assert.Equal(CoreClientState.KeyStoreCorrupt, client.State);

        // No operation re-entered the key store, and still nothing was generated.
        Assert.Equal(callsAfterInit, provider.CallCount);
        Assert.Equal(0, provider.KeysGenerated);
        Assert.False(File.Exists(_workspace.DbPath));
    }

    /// <summary>
    /// Review item Q9: Retry is offered even from the corrupt state, and it only re-reads — it
    /// deletes, moves, rewrites and regenerates nothing.
    /// </summary>
    [Fact]
    public async Task Retry_from_the_corrupt_state_re_reads_and_stays_blocked_while_the_store_is_still_corrupt()
    {
        var provider = new FailingKeyProvider(() => new KeyStoreCorruptException("tampered"));
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, provider);
        Assert.False(await client.InitializeAsync());

        Assert.False(await client.RetryAsync());

        Assert.Equal(2, provider.CallCount);
        Assert.Equal(0, provider.KeysGenerated);
        Assert.Equal(CoreClientState.KeyStoreCorrupt, client.State);
    }

    /// <summary>
    /// A wrong-length key must be rejected <b>in managed code</b>. Rust treats it as a fatal
    /// misconfiguration and panics, which reaches the shell as an opaque <c>InternalPanic</c> —
    /// indistinguishable from a genuine core bug. Catching it here keeps it a clear, typed,
    /// blocking key error instead.
    /// </summary>
    [Theory]
    [InlineData(0)]
    [InlineData(16)]
    [InlineData(31)]
    [InlineData(33)]
    [InlineData(64)]
    public async Task A_key_of_the_wrong_length_is_rejected_before_the_store_is_opened(int length)
    {
        var provider = new WrongLengthKeyProvider(length);
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, provider);

        var ok = await client.InitializeAsync();

        Assert.False(ok);
        Assert.Equal(CoreClientState.KeyStoreCorrupt, client.State);
        Assert.Equal(CoreErrorKind.KeyStoreCorrupt, client.LastError!.Kind);
        Assert.Equal(1, provider.CallCount);
        Assert.False(File.Exists(_workspace.DbPath), "the store must not be opened with an invalid key");

        // And specifically NOT an InternalPanic, which is what would surface if the bad key were
        // handed across the FFI instead.
        Assert.NotEqual(CoreErrorKind.InternalPanic, client.LastError.Kind);
    }

    [Fact]
    public async Task An_unavailable_key_store_is_a_retryable_state_and_Retry_recovers()
    {
        var provider = new FlakyKeyProvider(
            failuresBeforeSuccess: 1,
            () => new KeyStoreUnavailableException("locked"));
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, provider);

        Assert.False(await client.InitializeAsync());
        Assert.Equal(CoreClientState.KeyStoreUnavailable, client.State);
        Assert.Equal(CoreErrorKind.KeyStoreUnavailable, client.LastError!.Kind);
        Assert.True(client.LastError.IsRetryable);
        Assert.Equal(0, provider.KeysGenerated);
        Assert.False(File.Exists(_workspace.DbPath));

        Assert.True(await client.RetryAsync());

        Assert.Equal(CoreClientState.Ready, client.State);
        Assert.True(client.IsReady);
        Assert.Null(client.LastError);
        Assert.Equal(1, provider.KeysGenerated);

        // And the recovered client genuinely works.
        var id = await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        Assert.NotNull(id);
        Assert.Single(client.Items);
    }

    /// <summary>
    /// An I/O failure is not evidence of corruption (ADR-016), so it lands in the retryable state
    /// rather than the blocking one — and still generates nothing.
    /// </summary>
    [Fact]
    public async Task An_io_failure_is_retryable_rather_than_blocking()
    {
        var provider = new FlakyKeyProvider(
            failuresBeforeSuccess: 2,
            () => new KeyStoreIoException("disk"));
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, provider);

        Assert.False(await client.InitializeAsync());
        Assert.Equal(CoreClientState.KeyStoreUnavailable, client.State);
        Assert.Equal(0, provider.KeysGenerated);

        Assert.False(await client.RetryAsync());
        Assert.True(await client.RetryAsync());
        Assert.Equal(CoreClientState.Ready, client.State);
        Assert.Equal(1, provider.KeysGenerated);
    }

    /// <summary>
    /// A store that cannot be opened is reported as its own retryable state, with no key material
    /// or path in the message.
    /// </summary>
    [Fact]
    public async Task A_store_that_cannot_be_opened_reports_StoreUnavailable()
    {
        var provider = new FakeKeyProvider();
        // A path whose parent is an existing *file*, so the store cannot be created there.
        var blocker = Path.Combine(_workspace.Root, "blocker");
        await File.WriteAllTextAsync(blocker, "not a directory");
        using var client = new CoreClient(
            Path.Combine(blocker, "gist.sqlite3"), Path.Combine(blocker, "storage"), provider);

        var ok = await client.InitializeAsync();

        Assert.False(ok);
        Assert.Equal(CoreClientState.StoreUnavailable, client.State);
        Assert.Equal(CoreErrorKind.StoreUnavailable, client.LastError!.Kind);
        Assert.True(client.LastError.IsRetryable);
        Assert.DoesNotContain(blocker, client.LastError.Message, StringComparison.OrdinalIgnoreCase);
    }
}
