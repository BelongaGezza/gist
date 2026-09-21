namespace Gist.Core.ViewModels;

/// <summary>
/// The one piece of real time the library view models depend on: the search debounce delay.
/// </summary>
/// <remarks>
/// <para>
/// Apple's <c>LibraryView</c> debounces with <c>Task.sleep(nanoseconds: 300_000_000)</c> inside the
/// view, which makes the behaviour untestable without driving SwiftUI. Windows keeps the same
/// 300 ms cancel-and-restart semantics but routes the wait through this interface, so
/// <see cref="LibraryViewModel"/>'s debounce can be exercised deterministically — no
/// <c>Thread.Sleep</c>, no flaky timing assertions, and a test can prove that the <em>previous</em>
/// wait was cancelled rather than merely that the last query won.
/// </para>
/// <para>
/// The default implementation (<see cref="LibraryScheduler.Default"/>) is a plain
/// <see cref="Task.Delay(TimeSpan, CancellationToken)"/>; the shell never has to supply one.
/// </para>
/// </remarks>
public interface ILibraryScheduler
{
    /// <summary>
    /// Completes after <paramref name="delay"/>, or faults with an
    /// <see cref="OperationCanceledException"/> if <paramref name="cancellationToken"/> is
    /// cancelled first.
    /// </summary>
    Task Delay(TimeSpan delay, CancellationToken cancellationToken);
}

/// <summary>
/// The production <see cref="ILibraryScheduler"/>: a real timer.
/// </summary>
public sealed class LibraryScheduler : ILibraryScheduler
{
    /// <summary>Shared instance; the type is stateless.</summary>
    public static LibraryScheduler Default { get; } = new();

    /// <inheritdoc />
    public Task Delay(TimeSpan delay, CancellationToken cancellationToken) =>
        Task.Delay(delay, cancellationToken);
}
