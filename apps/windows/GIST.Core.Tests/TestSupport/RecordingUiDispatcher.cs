using System.Collections.Concurrent;
using Gist.Core.Client;

namespace Gist.Core.Tests.TestSupport;

/// <summary>
/// An <see cref="IUiDispatcher"/> that runs actions on one dedicated thread and records what it
/// saw, so a test can prove two things at once: that observable state is only ever published
/// through the dispatcher, and that the FFI itself ran somewhere else.
/// </summary>
/// <remarks>
/// A single long-lived worker thread stands in for the UI thread. Actions are queued and executed
/// in order, so this also exercises the genuinely <i>asynchronous</i> dispatcher case that
/// <see cref="ImmediateUiDispatcher"/> cannot — the case where an <c>…Async</c> method could return
/// before its own publish ran, if the client were not awaiting it.
/// </remarks>
public sealed class RecordingUiDispatcher : IUiDispatcher, IDisposable
{
    private readonly BlockingCollection<Action> _queue = new();
    private readonly Thread _thread;

    public RecordingUiDispatcher()
    {
        _thread = new Thread(Pump) { IsBackground = true, Name = "test-ui-thread" };
        _thread.Start();
        ThreadId = _thread.ManagedThreadId;
    }

    /// <summary>The managed id of the stand-in UI thread.</summary>
    public int ThreadId { get; }

    public int PostCount => _postCount;

    private int _postCount;

    public void Post(Action action)
    {
        Interlocked.Increment(ref _postCount);
        _queue.Add(action);
    }

    private void Pump()
    {
        foreach (var action in _queue.GetConsumingEnumerable())
        {
            action();
        }
    }

    public void Dispose()
    {
        _queue.CompleteAdding();
        _thread.Join(TimeSpan.FromSeconds(5));
        _queue.Dispose();
    }
}
