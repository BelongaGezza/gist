namespace Gist.Core.Models;

/// <summary>
/// One row in the library list. Windows counterpart of Apple's
/// <c>LibraryItemVM</c> (<c>apps/apple/Shared/CoreClient.swift</c>), mapped from the
/// generated <c>FfiLibraryItem</c> record by <see cref="Gist.Core.Client.CoreClient"/>.
/// </summary>
/// <remarks>
/// <para>
/// <see cref="Title"/> is never null: an item with no parsed title is mapped to
/// <c>"Untitled"</c> at the mapping boundary, matching Apple and
/// <c>docs/windows-ui-spec.md</c> §4.2.
/// </para>
/// <para>
/// Equality is component-wise <em>including</em> <see cref="Authors"/>, which is compared
/// element-by-element rather than by reference. The compiler-generated record equality would
/// have compared the author collection by reference, so two otherwise-identical items mapped
/// from two separate FFI calls would never compare equal — a trap for any future caller that
/// diffs list snapshots.
/// </para>
/// </remarks>
public sealed record LibraryItemVM
{
    public LibraryItemVM(
        string id,
        string title,
        IReadOnlyList<string> authors,
        string? sourcePath = null,
        bool contentEncrypted = false)
    {
        Id = id;
        Title = title;
        Authors = authors;
        SourcePath = sourcePath;
        ContentEncrypted = contentEncrypted;
    }

    /// <summary>Stable document id (UUIDv7) as produced by the Rust core.</summary>
    public string Id { get; init; }

    /// <summary>Display title; <c>"Untitled"</c> when the core reported none.</summary>
    public string Title { get; init; }

    public IReadOnlyList<string> Authors { get; init; }

    /// <summary>
    /// The user's original file path as recorded at import (informational only — GIST never
    /// deletes it, see ADR-006). Null for URL imports.
    /// </summary>
    public string? SourcePath { get; init; }

    /// <summary>
    /// Mirrors <c>FfiLibraryItem.content_encrypted</c> (ADR-011/ADR-014): whether this item's
    /// <c>.json</c>/<c>.tokens.json</c> blobs are encrypted at rest.
    /// </summary>
    public bool ContentEncrypted { get; init; }

    /// <summary>First author, or the empty string when there is none (see sort semantics).</summary>
    public string PrimaryAuthor => Authors.Count > 0 ? Authors[0] : string.Empty;

    public bool Equals(LibraryItemVM? other) =>
        other is not null
        && Id == other.Id
        && Title == other.Title
        && SourcePath == other.SourcePath
        && ContentEncrypted == other.ContentEncrypted
        && Authors.SequenceEqual(other.Authors);

    public override int GetHashCode()
    {
        var hash = new HashCode();
        hash.Add(Id);
        hash.Add(Title);
        hash.Add(SourcePath);
        hash.Add(ContentEncrypted);
        foreach (var author in Authors)
        {
            hash.Add(author);
        }

        return hash.ToHashCode();
    }
}
