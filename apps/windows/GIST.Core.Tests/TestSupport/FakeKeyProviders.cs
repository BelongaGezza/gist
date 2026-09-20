using System.Security.Cryptography;
using Gist.Core.Keys;

namespace Gist.Core.Tests.TestSupport;

/// <summary>
/// A well-behaved provider: one 32-byte key, generated on first use, identical on every later call.
/// Records how many keys it has ever generated so a test can prove a failure path did <b>not</b>
/// quietly mint a replacement (ADR-016's central safety property).
/// </summary>
public sealed class FakeKeyProvider : IKeyProvider
{
    private byte[]? _key;

    public int CallCount { get; private set; }

    /// <summary>How many distinct keys this provider has ever created. Must stay at most 1.</summary>
    public int KeysGenerated { get; private set; }

    /// <summary>
    /// The managed thread the last call ran on. The client fetches the key inside
    /// <see cref="Task.Run(Action)"/>, the same way it runs every FFI call, so this is a usable
    /// witness that the work did not happen on the UI thread.
    /// </summary>
    public int LastCallThreadId { get; private set; }

    public byte[] GetOrCreateKey()
    {
        CallCount++;
        LastCallThreadId = Environment.CurrentManagedThreadId;
        if (_key is null)
        {
            _key = RandomNumberGenerator.GetBytes(32);
            KeysGenerated++;
        }

        return _key;
    }
}

/// <summary>
/// Fails every call with the exception the constructor was given, and never generates a key. Models
/// a key store that is corrupt, or temporarily unreachable, depending on the exception supplied.
/// </summary>
public sealed class FailingKeyProvider : IKeyProvider
{
    private readonly Func<Exception> _failure;

    public FailingKeyProvider(Func<Exception> failure) => _failure = failure;

    public int CallCount { get; private set; }

    /// <summary>Always 0: this provider never mints anything, which is the point of the tests using it.</summary>
    public int KeysGenerated => 0;

    public byte[] GetOrCreateKey()
    {
        CallCount++;
        throw _failure();
    }
}

/// <summary>
/// Fails the first <c>failuresBeforeSuccess</c> calls, then behaves like
/// <see cref="FakeKeyProvider"/>. Models a transient outage that <c>RetryAsync</c> should clear —
/// and proves the retry re-reads the <em>same</em> key rather than making a new one.
/// </summary>
public sealed class FlakyKeyProvider : IKeyProvider
{
    private readonly int _failuresBeforeSuccess;
    private readonly Func<Exception> _failure;
    private byte[]? _key;

    public FlakyKeyProvider(int failuresBeforeSuccess, Func<Exception> failure)
    {
        _failuresBeforeSuccess = failuresBeforeSuccess;
        _failure = failure;
    }

    public int CallCount { get; private set; }

    public int KeysGenerated { get; private set; }

    public byte[] GetOrCreateKey()
    {
        CallCount++;
        if (CallCount <= _failuresBeforeSuccess)
        {
            throw _failure();
        }

        if (_key is null)
        {
            _key = RandomNumberGenerator.GetBytes(32);
            KeysGenerated++;
        }

        return _key;
    }
}

/// <summary>
/// Returns a key of the wrong length. Rust treats a non-32-byte key as a fatal misconfiguration and
/// panics (surfacing as an opaque <c>InternalPanic</c>), so the client must reject this in managed
/// code before the store is ever opened.
/// </summary>
public sealed class WrongLengthKeyProvider : IKeyProvider
{
    private readonly int _length;

    public WrongLengthKeyProvider(int length) => _length = length;

    public int CallCount { get; private set; }

    public byte[] GetOrCreateKey()
    {
        CallCount++;
        return new byte[_length];
    }
}
