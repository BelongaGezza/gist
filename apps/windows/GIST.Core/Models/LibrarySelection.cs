namespace Gist.Core.Models;

/// <summary>
/// What the sidebar has selected, driving which root the detail pane shows: the whole library,
/// or one collection's contents. Windows counterpart of Apple's
/// <c>LibrarySelection</c> enum (<c>apps/apple/Shared/LibrarySelection.swift</c>).
/// </summary>
/// <remarks>
/// Modelled as a closed record hierarchy (C#'s nearest equivalent to a Swift enum with an
/// associated value). Equality is component-wise through <see cref="CollectionVM"/>'s own
/// record equality, and records compare their runtime type first, so
/// <see cref="All"/> is never equal to a <see cref="Collection"/>.
/// </remarks>
public abstract record LibrarySelection
{
    // Private ctor: only the two nested cases below can derive from this, making the
    // hierarchy closed in practice the way a Swift enum is by construction.
    private LibrarySelection()
    {
    }

    /// <summary>Title for the detail pane's header.</summary>
    public abstract string Title { get; }

    /// <summary>The whole library. A singleton, since the case carries no data.</summary>
    public static LibrarySelection AllItems { get; } = new All();

    /// <summary>Convenience factory mirroring <c>.collection(_:)</c> on the Swift side.</summary>
    public static LibrarySelection ForCollection(CollectionVM collection) => new Collection(collection);

    /// <summary>The whole library ("Library" in the sidebar).</summary>
    public sealed record All : LibrarySelection
    {
        public override string Title => "Library";
    }

    /// <summary>One collection's contents.</summary>
    /// <param name="Value">The selected collection; compared component-wise.</param>
    public sealed record Collection(CollectionVM Value) : LibrarySelection
    {
        public override string Title => Value.Name;
    }
}
