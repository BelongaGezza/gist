namespace Gist.Core.Client;

/// <summary>
/// How <see cref="CoreClient"/> gets back onto the UI thread after an FFI call.
/// </summary>
/// <remarks>
/// <para>
/// <c>GIST.Core</c> is UI-free by design (docs/windows-development-plan.md §2) and must not
/// reference WinUI, so it cannot depend on <c>DispatcherQueue</c> directly. The WinUI shell
/// injects one of these instead:
/// <c>new DelegateUiDispatcher(a =&gt; dispatcherQueue.TryEnqueue(() =&gt; a()))</c>.
/// </para>
/// <para>
/// Headless callers (tests, CI) use <see cref="ImmediateUiDispatcher"/>, which runs the action
/// inline. Whichever is used, <see cref="CoreClient"/> awaits completion of the posted action, so
/// an awaited <c>…Async</c> call has always finished publishing its results by the time it
/// returns — with an asynchronous dispatcher too.
/// </para>
/// </remarks>
public interface IUiDispatcher
{
    /// <summary>
    /// Schedules <paramref name="action"/> on the UI thread. May run it inline.
    /// Implementations must not throw for a well-formed action; exceptions raised by
    /// <paramref name="action"/> itself are the caller's to observe.
    /// </summary>
    void Post(Action action);
}

/// <summary>
/// Runs actions inline on the calling thread. The default for
/// <see cref="CoreClient"/> and what the headless tests use; also correct for any caller
/// with no UI thread to marshal to.
/// </summary>
public sealed class ImmediateUiDispatcher : IUiDispatcher
{
    public static ImmediateUiDispatcher Instance { get; } = new();

    public void Post(Action action)
    {
        ArgumentNullException.ThrowIfNull(action);
        action();
    }
}

/// <summary>
/// Adapts any <c>Action&lt;Action&gt;</c> post function — e.g. WinUI's
/// <c>DispatcherQueue.TryEnqueue</c> — to <see cref="IUiDispatcher"/>.
/// </summary>
public sealed class DelegateUiDispatcher : IUiDispatcher
{
    private readonly Action<Action> _post;

    public DelegateUiDispatcher(Action<Action> post)
    {
        ArgumentNullException.ThrowIfNull(post);
        _post = post;
    }

    public void Post(Action action)
    {
        ArgumentNullException.ThrowIfNull(action);
        _post(action);
    }
}

/// <summary>
/// Posts to a captured <see cref="SynchronizationContext"/>, falling back to running inline when
/// the captured context is null (no UI thread — e.g. a console host or a test).
/// </summary>
public sealed class SynchronizationContextUiDispatcher : IUiDispatcher
{
    private readonly SynchronizationContext? _context;

    /// <summary>Captures <see cref="SynchronizationContext.Current"/> at construction time.</summary>
    public SynchronizationContextUiDispatcher()
        : this(SynchronizationContext.Current)
    {
    }

    public SynchronizationContextUiDispatcher(SynchronizationContext? context)
    {
        _context = context;
    }

    public void Post(Action action)
    {
        ArgumentNullException.ThrowIfNull(action);
        if (_context is null)
        {
            action();
            return;
        }

        _context.Post(static state => ((Action)state!)(), action);
    }
}
