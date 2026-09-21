using Gist.Core.Models;

namespace Gist.Core.ViewModels;

/// <summary>
/// Fixed warning text for a removal that left stored files behind. Deliberately a lookup of
/// constants: nothing from the core (paths, titles, OS messages) is ever interpolated.
/// </summary>
public static class RemoveWarning
{
    public const string Locked =
        "Some stored files are in use by another program and could not be deleted. "
        + "GIST will retry them the next time it starts.";

    public const string Permission =
        "Some stored files couldn't be deleted because access was denied. "
        + "GIST will retry them the next time it starts.";

    public const string Other =
        "Some stored files could not be deleted. GIST will retry them the next time it starts.";

    /// <summary>Warning for <paramref name="result"/>, or null when no file genuinely failed.</summary>
    public static string? For(RemoveResult result)
    {
        ArgumentNullException.ThrowIfNull(result);
        if (!result.HasFileFailures)
        {
            return null;
        }

        // Mixed kinds: the most actionable (and most transient) wording wins.
        if (result.FailureKinds.Contains(FileDeleteFailureKind.Locked))
        {
            return Locked;
        }

        return result.FailureKinds.Contains(FileDeleteFailureKind.Permission) ? Permission : Other;
    }
}
