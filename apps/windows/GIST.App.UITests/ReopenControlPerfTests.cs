using System.Diagnostics;
using System.Globalization;
using System.Text;
using FlaUI.Core.Definitions;
using FlaUI.Core.Input;
using FlaUI.Core.WindowsAPI;
using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Storage;
using Xunit.Abstractions;
using static Gist.App.UITests.FlowPerfTests;

namespace Gist.App.UITests;

/// <summary>
/// F64 control runs: does private-memory growth across reopen cycles belong to the flow reader, or is it generic
/// (any page navigation, or even just the harness driving the library)? Each arm runs in its own app session on a
/// tiny document, and process counters are sampled only while the harness is idle (leave + idle, no UIA calls
/// in flight). Opt-in: GIST_RUN_UI_TESTS=1 and GIST_RUN_PERF=1; GIST_PERF_REPORT=&lt;file&gt;; GIST_PERF_CYCLES (default 16).
/// </summary>
[Trait("Category", "UI")]
[Trait("Category", "Perf")]
public sealed class ReopenControlPerfTests
{
    private const string Title = "Tiny Control";
    private readonly ITestOutputHelper _output;

    public ReopenControlPerfTests(ITestOutputHelper output) => _output = output;

    private enum Arm { FlowTiny, RsvpTiny, LibraryOnly, RsvpViaMenu, MenuEscapeThenEnter }

    [PerfUiFact]
    public async Task Reopen_growth_control_runs_are_measured()
    {
        var report = new StringBuilder();
        void Line(string s)
        {
            report.AppendLine(s);
            _output.WriteLine(s);
        }

        var cycles = int.TryParse(Environment.GetEnvironmentVariable("GIST_PERF_CYCLES"), out var cyc) ? cyc : 16;
        var root = Path.Combine(Path.GetTempPath(), "gist-perf-ctl-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            var file = Path.Combine(root, "user-files", Title + ".txt");
            Directory.CreateDirectory(Path.GetDirectoryName(file)!);
            File.WriteAllText(file, "Alpha beta gamma delta.\n\nEpsilon zeta eta theta. Iota kappa lambda mu.\n", new UTF8Encoding(false));
            {
                var paths = GistStoragePaths.ForRoot(root);
                paths.EnsureCreated();
                using var core = new CoreClient(new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
                Assert.True(await core.InitializeAsync());
                Assert.NotNull(await core.ImportFileAsync(file));
            }

            Line($"F64 control runs: {cycles} cycles per arm on a tiny 3-sentence document; private bytes read from the process counter after leave + 4 s idle, with no UIA call in flight.");
            Line("(growth/cycle = least-squares slope over cycles 4..N, so warm-up allocations are excluded)");
            var only = Environment.GetEnvironmentVariable("GIST_PERF_ARM");
            foreach (var arm in new[] { Arm.LibraryOnly, Arm.FlowTiny, Arm.RsvpTiny, Arm.RsvpViaMenu, Arm.MenuEscapeThenEnter }.Where(a => only is not { Length: > 0 } || a.ToString() == only))
            {
                using var session = await GistAppSession.StartAsync(existingRoot: root);
                session.KeepRoot = true; // Dispose would otherwise delete the shared root before the next arm
                var d = new LibraryDriver(session);
                d.WaitForLibrary();
                d.WaitForTitles(t => t.Length == 1, "the control item");
                Thread.Sleep(3000);
                var samples = new List<double>();
                double Priv()
                {
                    using var p = Process.GetProcessById(session.ProcessId);
                    return p.PrivateMemorySize64 / 1048576.0;
                }
                Line("");
                Line($"== arm {arm} == start: private {F(Priv(), "F0")} MiB");
                for (var cycle = 1; cycle <= cycles; cycle++)
                {
                    switch (arm)
                    {
                        case Arm.FlowTiny:
                            FlowReaderTests.OpenFlowFromRow(d, Title);
                            Thread.Sleep(1000);
                            d.InvokeCommand("FlowPage_BackButton");
                            d.WaitForLibrary();
                            break;
                        case Arm.RsvpTiny:
                            d.ClickRow(Title);
                            d.Press(VirtualKeyShort.RETURN);
                            d.Need("RsvpPage_PlayPauseButton");
                            Thread.Sleep(1000);
                            d.InvokeCommand("RsvpPage_BackButton");
                            d.WaitForLibrary();
                            break;
                        case Arm.RsvpViaMenu:
                            // Same UIA menu path as FlowTiny, but opening the RSVP page.
                            d.ClickRow(Title);
                            d.Press(VirtualKeyShort.APPS);
                            LibraryDriver.Activate(LibraryDriver.Poll(
                                () => d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem).And(cf.ByName("Open in Reader"))).FirstOrDefault(),
                                "Open in Reader", TimeSpan.FromSeconds(4)));
                            d.Need("RsvpPage_PlayPauseButton");
                            Thread.Sleep(1000);
                            d.InvokeCommand("RsvpPage_BackButton");
                            d.WaitForLibrary();
                            break;
                        case Arm.MenuEscapeThenEnter:
                            // Opens and dismisses the row context menu (no item chosen), then opens RSVP with Enter:
                            // isolates "the menu was shown" from "a menu item was activated".
                            d.ClickRow(Title);
                            d.Press(VirtualKeyShort.APPS);
                            Thread.Sleep(700);
                            d.Press(VirtualKeyShort.ESCAPE);
                            Thread.Sleep(500);
                            d.ClickRow(Title);
                            d.Press(VirtualKeyShort.RETURN);
                            d.Need("RsvpPage_PlayPauseButton");
                            Thread.Sleep(1000);
                            d.InvokeCommand("RsvpPage_BackButton");
                            d.WaitForLibrary();
                            break;
                        default:
                            // Harness traffic only: select the row and open + dismiss its context menu.
                            d.ClickRow(Title);
                            d.Press(VirtualKeyShort.APPS);
                            Thread.Sleep(1000);
                            d.Press(VirtualKeyShort.ESCAPE);
                            Thread.Sleep(500);
                            break;
                    }
                    if (Environment.GetEnvironmentVariable("GIST_PERF_HARNESS_GC") == "1")
                    {
                        // Release this test process's UIA/COM references (finalizers) before sampling the app.
                        for (var g = 0; g < 3; g++)
                        {
                            GC.Collect();
                            GC.WaitForPendingFinalizers();
                        }
                    }
                    Thread.Sleep(4000);
                    samples.Add(Priv());
                    Line($"  cycle {cycle}: private {F(samples[^1], "F1")} MiB");
                }
                var xs = Enumerable.Range(4, Math.Max(cycles - 3, 2)).Select(i => (double)i).ToArray();
                var ys = samples.Skip(3).ToArray();
                var n = Math.Min(xs.Length, ys.Length);
                if (n < 2) continue;
                var mx = xs.Take(n).Average();
                var my = ys.Take(n).Average();
                var slope = Enumerable.Range(0, n).Sum(i => (xs[i] - mx) * (ys[i] - my)) / Enumerable.Range(0, n).Sum(i => (xs[i] - mx) * (xs[i] - mx));
                Line($"  arm {arm}: growth/cycle (cycles 4..{cycles}) = {F(slope, "F2")} MiB; total first->last = {F(samples[^1] - samples[0], "F1")} MiB");
            }
        }
        finally
        {
            if (Environment.GetEnvironmentVariable("GIST_PERF_REPORT") is { Length: > 0 } path)
            {
                try { File.WriteAllText(path, report.ToString()); } catch (Exception) { }
            }
            try { Directory.Delete(root, recursive: true); } catch (Exception) { }
        }
    }
}
