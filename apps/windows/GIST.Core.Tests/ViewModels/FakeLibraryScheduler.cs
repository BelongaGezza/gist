using Gist.Core.ViewModels;

namespace Gist.Core.Tests.ViewModels;

/// <summary>
/// An <see cref="ILibraryScheduler"/> whose delays only complete when a test says so.
/// </summary>
/// <remarks>
/// <para>
/// The search debounce is the one piece of real time in the library view model, and timing-based
/// tests for it would be exactly the kind of slow, flaky test this project has avoided everywhere
/// else. Instead the delay is a promise this class holds: <see cref="Advance"/> completes whatever
/// is outstanding, and <see cref="CanceledCount"/> records how many waits were <em>cancelled</em>
/// rather than completed — which is the direct witness that the debounce really is
/// cancel-and-restart and not "start a second timer and let both fire".
/// </para>
/// <para>
/// Thread-safe because cancellation callbacks run on whichever thread cancelled.
/// </para>
/// </remarks>
public sealed class FakeLibraryScheduler : ILibraryScheduler
{
    private readonly object _gate = new();
    private readonly List<TaskCompletionSource> _pending = new();

    private int _requested;
    private int _completed;
    private int _canceled;

    /// <summary>How many delays have ever been requested.</summary>
    public int RequestedCount => Volatile.Read(ref _requested);

    /// <summary>How many delays ran to completion (i.e. a search actually fired).</summary>
    public int CompletedCount => Volatile.Read(ref _completed);

    /// <summary>How many delays were cancelled before completing.</summary>
    public int CanceledCount => Volatile.Read(ref _canceled);

    /// <summary>The delays requested so far, in order, so a test can assert the debounce length.</summary>
    public IReadOnlyList<TimeSpan> RequestedDelays
    {
        get
        {
            lock (_gate)
            {
                return _requestedDelays.ToArray();
            }
        }
    }

    private readonly List<TimeSpan> _requestedDelays = new();

    /// <inheritdoc />
    public Task Delay(TimeSpan delay, CancellationToken cancellationToken)
    {
        var source = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);

        lock (_gate)
        {
            Interlocked.Increment(ref _requested);
            _requestedDelays.Add(delay);
            _pending.Add(source);
        }

        // Registering after the fact is fine: an already-cancelled token runs the callback inline.
        cancellationToken.Register(() =>
        {
            if (source.TrySetCanceled(cancellationToken))
            {
                Interlocked.Increment(ref _canceled);
                lock (_gate)
                {
                    _pending.Remove(source);
                }
            }
        });

        return source.Task;
    }

    /// <summary>Completes every delay outstanding right now, as if the debounce had elapsed.</summary>
    public void Advance()
    {
        TaskCompletionSource[] snapshot;
        lock (_gate)
        {
            snapshot = _pending.ToArray();
            _pending.Clear();
        }

        foreach (var source in snapshot)
        {
            if (source.TrySetResult())
            {
                Interlocked.Increment(ref _completed);
            }
        }
    }
}
