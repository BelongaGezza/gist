using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.Input;
using FlaUI.Core.WindowsAPI;
using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Storage;
using Gist.Core.Tests.TestSupport;
using Xunit.Abstractions;
using static Gist.App.UITests.FlowPerfTests;

namespace Gist.App.UITests;

/// <summary>
/// F60: how the flow reader copes when ONE block is enormous (a 2 MB .txt with no blank lines is a single paragraph;
/// a 20,000-item list; a 15,000-cell table). Same stall probe as <see cref="FlowPerfTests"/>. Opt-in:
/// GIST_RUN_UI_TESTS=1 and GIST_RUN_PERF=1; GIST_PERF_REPORT=&lt;file&gt; saves the report.
/// </summary>
[Trait("Category", "UI")]
[Trait("Category", "Perf")]
public sealed class FlowPathologicalPerfTests
{
    private readonly ITestOutputHelper _output;

    public FlowPathologicalPerfTests(ITestOutputHelper output) => _output = output;

    [PerfUiFact]
    public async Task Pathological_single_block_documents_are_measured()
    {
        var report = new StringBuilder();
        void Line(string s)
        {
            report.AppendLine(s);
            _output.WriteLine(s);
        }

        var root = Path.Combine(Path.GetTempPath(), "gist-perf-path-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        GistAppSession? session = null;
        try
        {
            var files = Path.Combine(root, "user-files");
            var txt = PerfDocumentGenerator.GeneratePathologicalTxt(files);
            var epub = PerfDocumentGenerator.GeneratePathologicalEpub(files, out var listItems, out var tableCells);
            {
                var paths = GistStoragePaths.ForRoot(root);
                paths.EnsureCreated();
                using var core = new CoreClient(new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
                Assert.True(await core.InitializeAsync());
                foreach (var f in new[] { txt, epub })
                {
                    var sw = Stopwatch.StartNew();
                    Assert.NotNull(await core.ImportFileAsync(f));
                    Line($"import {Path.GetFileName(f)} ({new FileInfo(f).Length / 1024} KiB): {sw.Elapsed.TotalMilliseconds.ToString("F0", CultureInfo.InvariantCulture)} ms");
                }
            }
            Line($"epub contents: one ~150k-char paragraph, one {listItems}-item list, one {tableCells}-cell table (1900 x 8)");
            Line($"machine: {Environment.MachineName}, {Environment.ProcessorCount} logical CPUs, {RuntimeInformation.OSDescription}");

            session = await GistAppSession.StartAsync(existingRoot: root);
            var d = new LibraryDriver(session);
            d.WaitForLibrary();
            d.WaitForTitles(t => t.Length == 2, "the two pathological items");
            var hwnd = session.Window.Properties.NativeWindowHandle.Value;
            using var mon = new FlowPerfTests.Monitor(hwnd, session.ProcessId);

            foreach (var title in new[] { PerfDocumentGenerator.PathologicalTextTitle, PerfDocumentGenerator.PathologicalStructureTitle })
            {
                var tag = title.Contains("Text", StringComparison.Ordinal) ? "T" : "S";
                Line("");
                Line($"== {title} ==");
                mon.Phase = $"{tag}0 library idle (no UIA)";
                Thread.Sleep(1500);

                mon.Phase = $"{tag}1 open menu (UIA)";
                AutomationElement? item = null;
                for (var attempt = 0; attempt < 4 && item is null; attempt++)
                {
                    d.ClickRow(title);
                    d.Press(VirtualKeyShort.APPS);
                    try
                    {
                        item = LibraryDriver.Poll(
                            () => d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem).And(cf.ByName("Open in Flow View"))).FirstOrDefault(),
                            "Open in Flow View", TimeSpan.FromSeconds(4));
                    }
                    catch (TimeoutException) when (attempt < 3)
                    {
                        d.Press(VirtualKeyShort.ESCAPE);
                    }
                }

                mon.Phase = $"{tag}2 open flow page (UIA polling)";
                var open = Stopwatch.StartNew();
                LibraryDriver.Activate(item!);
                var ready = Poll(() => d.ById("FlowPage_ProgressText") is { } p && System.Text.RegularExpressions.Regex.IsMatch(p.Name, @"^\d+%$"), TimeSpan.FromSeconds(240));
                Line($"  open -> progress text shown: {F(open.Elapsed.TotalMilliseconds, "F0")} ms (ready={ready})");
                var first = Poll(() => FlowReaderTests.VisibleBlocks(d).Length > 0, TimeSpan.FromSeconds(120));
                Line($"  open -> a block visible: {F(open.Elapsed.TotalMilliseconds, "F0")} ms (visible={first})");
                Line($"  realised block elements at the top: {FlowReaderTests.RealizedBlocks(d).Length}; app responding: {session.Responding}");
                var viewport = d.Need("FlowPage_List").BoundingRectangle;

                mon.Phase = $"{tag}3 top idle (no UIA)";
                Thread.Sleep(2500);

                mon.Phase = $"{tag}4 Page Down x30 at ~6/s (no UIA)";
                for (var i = 0; i < 30; i++)
                {
                    Keyboard.Type(VirtualKeyShort.NEXT);
                    Thread.Sleep(160);
                }
                Thread.Sleep(800);

                mon.Phase = $"{tag}5 mouse wheel 80 notches (no UIA)";
                Mouse.MoveTo(new System.Drawing.Point((int)(viewport.Left + viewport.Width / 2), (int)(viewport.Top + viewport.Height / 2)));
                for (var i = 0; i < 80; i++)
                {
                    Mouse.Scroll(-1);
                    Thread.Sleep(30);
                }
                Thread.Sleep(800);

                mon.Phase = $"{tag}5b progress read (UIA)";
                Line($"  progress after paging + wheel: {FlowReaderTests.ProgressPercent(d)}%");

                mon.Phase = $"{tag}6 End then Home x3 (no UIA)";
                for (var i = 0; i < 3; i++)
                {
                    Keyboard.Type(VirtualKeyShort.END);
                    Thread.Sleep(1500);
                    Keyboard.Type(VirtualKeyShort.HOME);
                    Thread.Sleep(1500);
                }

                mon.Phase = $"{tag}7 open find (UIA)";
                FlowReaderTests.ShowFind(d);
                mon.Phase = $"{tag}8 find 'a' typed (UIA set only)";
                var sw = Stopwatch.StartNew();
                FlowReaderTests.SetFind(d, "a");
                mon.Phase = $"{tag}8 find 'a' settle (no UIA)";
                Thread.Sleep(4000);
                mon.Phase = $"{tag}8b find status read (UIA)";
                Line($"  find 'a' settled status after 4 s: '{FlowReaderTests.FindStatus(d)}'");
                mon.Phase = $"{tag}9 F3 x20 at ~6/s (no UIA)";
                for (var i = 0; i < 20; i++)
                {
                    Keyboard.Type(VirtualKeyShort.F3);
                    Thread.Sleep(160);
                }
                Thread.Sleep(800);
                mon.Phase = $"{tag}9b status read (UIA)";
                Line($"  status after 20 F3 steps: '{FlowReaderTests.FindStatus(d)}'; responding: {session.Responding}");

                mon.Phase = $"{tag}10 leave (UIA)";
                d.InvokeCommand("FlowPage_FindClose");
                Thread.Sleep(500);
                d.InvokeCommand("FlowPage_BackButton");
                d.WaitForLibrary();
                mon.Phase = $"{tag}11 library idle after leave (no UIA)";
                Thread.Sleep(2000);
            }

            var (probes, mems) = mon.Snapshot();
            Line("");
            Line($"UI-thread message-pump latency (WM_NULL round trip, {probes.Length} probes):");
            Line("  phase                                                     n     p50     p99     max  >16ms >50ms >100ms >500ms");
            foreach (var g in probes.GroupBy(p => p.Phase).OrderBy(g => g.Key, StringComparer.Ordinal))
            {
                var v = g.Select(p => p.LatencyMs).OrderBy(x => x).ToArray();
                Line($"  {g.Key,-52} {v.Length,6} {F(Pct(v, 50)),7} {F(Pct(v, 99)),7} {F(v[^1]),7} {v.Count(x => x > 16),6} {v.Count(x => x > 50),5} {v.Count(x => x > 100),6} {v.Count(x => x > 500),6}");
            }
            Line("  worst 8 stalls (at ms, latency, phase):");
            foreach (var p in probes.OrderByDescending(p => p.LatencyMs).Take(8))
            {
                Line($"    {F(p.AtMs, "F0"),8}  {F(p.LatencyMs),8} ms  {p.Phase}");
            }
            Line("");
            Line("app process memory sampled every 100 ms (MiB):");
            Line("  phase                                                     peakWS  peakPrivate  lastWS lastPrivate");
            foreach (var g in mems.GroupBy(m => m.Phase).OrderBy(g => g.Key, StringComparer.Ordinal))
            {
                Line($"  {g.Key,-52} {F(g.Max(m => m.WorkingSetMiB), "F0"),8} {F(g.Max(m => m.PrivateMiB), "F0"),12} {F(g.Last().WorkingSetMiB, "F0"),7} {F(g.Last().PrivateMiB, "F0"),11}");
            }
            Line($"  responding at the end: {session.Responding}");
        }
        finally
        {
            session?.Dispose();
            if (Environment.GetEnvironmentVariable("GIST_PERF_REPORT") is { Length: > 0 } path)
            {
                try { File.WriteAllText(path, report.ToString()); } catch (Exception) { }
            }
            try { Directory.Delete(root, recursive: true); } catch (Exception) { }
        }
    }
}
