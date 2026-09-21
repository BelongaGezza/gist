namespace Gist.App.UITests;

/// <summary>A Fact that reports Skipped (not failed) unless GIST_RUN_UI_TESTS=1 (needs an interactive desktop).</summary>
public sealed class UiFactAttribute : FactAttribute
{
    public const string EnvVar = "GIST_RUN_UI_TESTS";

    public UiFactAttribute()
    {
        if (Environment.GetEnvironmentVariable(EnvVar) != "1")
        {
            Skip = $"UI tests are opt-in: set {EnvVar}=1 (requires an interactive desktop session).";
        }
    }
}
