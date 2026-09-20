namespace Gist.App;

/// <summary>
/// Composition root. The W1 shell has no services yet.
/// TODO(W1 integration): construct the CoreClient here (DpapiKeyProvider first, then NewWithReadKey) and
/// expose it as a property, e.g. `public static CoreClient Core { get; private set; }`. Pages must obtain
/// it from here (or via constructor injection into view models), never construct it themselves.
/// </summary>
public static class AppServices
{
    public static bool IsInitialized { get; private set; }

    public static void Initialize()
    {
        if (IsInitialized) return;
        // TODO(W1 integration): create CoreClient and wire failure states (key store corrupt etc.).
        IsInitialized = true;
    }
}
