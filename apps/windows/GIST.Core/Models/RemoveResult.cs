namespace Gist.Core.Models;

/// <summary>Why one stored file could not be deleted, coarsely. Carries no path or OS text.</summary>
public enum FileDeleteFailureKind
{
    /// <summary>Another open handle (antivirus, indexer, backup agent, ...) prevents deletion. Usually transient.</summary>
    Locked,

    /// <summary>Access was denied.</summary>
    Permission,

    /// <summary>Anything else.</summary>
    Other,
}

/// <summary>
/// What a removal actually did (mirrors the core's <c>RemoveOutcome</c>). Counts and coarse kinds
/// only — never a path or a title.
/// </summary>
/// <remarks>
/// <b>The database row being gone is the success.</b> A file that could not be deleted is a
/// warning (<see cref="HasFileFailures"/>), never a failed removal. <see cref="FilesMissing"/> is
/// benign (items imported before ADR-013 have no checksum sidecars) and must never be surfaced.
/// </remarks>
public sealed record RemoveResult(
    IReadOnlyList<string> RemovedIds,
    int FilesDeleted,
    int FilesMissing,
    int FilesFailed,
    IReadOnlyList<FileDeleteFailureKind> FailureKinds)
{
    /// <summary>A removal that did nothing (no ids, or the core was unavailable).</summary>
    public static RemoveResult Empty { get; } = new(
        Array.Empty<string>(), 0, 0, 0, Array.Empty<FileDeleteFailureKind>());

    /// <summary>True only when at least one file genuinely failed to delete. Missing files never count.</summary>
    public bool HasFileFailures => FilesFailed > 0;
}

/// <summary>What the orphan sweep did (mirrors the core's <c>SweepOutcome</c>). Counts only.</summary>
public sealed record SweepResult(
    int FilesScanned,
    int FilesDeleted,
    int FilesMissing,
    int FilesFailed,
    IReadOnlyList<FileDeleteFailureKind> FailureKinds);
