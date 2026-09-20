namespace Gist.Core.Models;

/// <summary>
/// A user collection, mapped from the generated <c>FfiCollection</c> record.
/// </summary>
/// <remarks>
/// Equality is deliberately <b>component-wise</b> (the compiler-generated record equality over
/// all three members), not id-only. The sidebar selection is matched against row values by
/// equality, so if this ever degraded to comparing <see cref="Id"/> alone, a stale selection
/// captured before a rename would compare equal to the renamed collection and the detail pane
/// could show the wrong contents. Apple's <c>CollectionVM</c> has the same property, and
/// <c>GISTTests.testLibrarySelectionEqualityIsComponentWise</c> is the test this mirrors.
/// </remarks>
/// <param name="Id">Stable collection id from the core.</param>
/// <param name="Name">Display name.</param>
/// <param name="CreatedAt">Creation timestamp as stored by the core (Unix seconds).</param>
public sealed record CollectionVM(string Id, string Name, long CreatedAt);
