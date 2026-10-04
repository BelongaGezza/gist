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

namespace Gist.App.UITests;

/// <summary>Skipped unless both GIST_RUN_UI_TESTS=1 (interactive desktop) and GIST_RUN_PERF=1.</summary>
public sealed class PerfUiFactAttribute : FactAttribute
{
    public PerfUiFactAttribute()
    {
        if (Environment.GetEnvironmentVariable(UiFactAttribute.EnvVar) != "1" || Environment.GetEnvironmentVariable("GIST_RUN_PERF") != "1")
        {
            Skip = "UI perf measurement is opt-in: set GIST_RUN_UI_TESTS=1 and GIST_RUN_PERF=1 (needs an unlocked interactive desktop; "
                 + "GIST_PERF_REPORT=<file> saves the report; nothing else may use the desktop while it runs).";
        }
    }
}

/// <summary>
/// W5 exit criterion, UI half: opens a generated ≥100k-word document in the real flow page and measures, from outside
/// the process, (a) UI-thread message-pump latency while scrolling/jumping/finding and (b) the app's working set and
/// private bytes. See docs/windows-development-plan.md "W5 measurement" for what this can and cannot show.
/// </summary>
/// <remarks>
/// The stall probe is a dedicated thread that sends WM_NULL to the main window with SendMessageTimeout and times the
/// round trip. A cross-thread sent message is only answered when the window's thread pumps messages, so the latency is
/// "how long the UI thread was away from its message loop" at ~1 ms resolution; it does NOT see compositor/GPU frame
/// time (WinUI composes on another thread) nor a slow frame that still pumps messages. Phases are labelled
/// "(no UIA)" when the harness issued no UI Automation calls during them, because UIA tree walks are themselves served
/// by the app's UI thread and inflate the probe; "(UIA)" phases contain harness-induced stalls.
/// </remarks>
[Trait("Category", "UI")]
[Trait("Category", "Perf")]
public sealed class FlowPerfTests
{
    private const uint WmNull = 0;
    private const uint SmtoBlock = 0x1, SmtoAbortIfHung = 0x2;

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SendMessageTimeoutW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);

    [DllImport("winmm.dll")]
    private static extern uint timeBeginPeriod(uint ms);

    [DllImport("winmm.dll")]
    private static extern uint timeEndPeriod(uint ms);

    private readonly ITestOutputHelper _output;

    public FlowPerfTests(ITestOutputHelper output) => _output = output;

    internal sealed record Probe(double AtMs, double LatencyMs, string Phase);

    internal sealed record Mem(double AtMs, double WorkingSetMiB, double PrivateMiB, string Phase);

    internal sealed class Monitor : IDisposable
    {
        private readonly IntPtr _hwnd;
        private readonly int _pid;
        private readonly Stopwatch _clock = Stopwatch.StartNew();
        private readonly CancellationTokenSource _cts = new();
        private readonly Thread _probeThread;
        private readonly Thread _memThread;
        public readonly List<Probe> Probes = new(1 << 16);
        public readonly List<Mem> Mems = new(4096);
        public volatile string Phase = "init";
        private readonly object _gate = new();

        public Monitor(IntPtr hwnd, int pid)
        {
            _hwnd = hwnd;
            _pid = pid;
            timeBeginPeriod(1);
            _probeThread = new Thread(ProbeLoop) { IsBackground = true, Name = "ui-stall-probe" };
            _memThread = new Thread(MemLoop) { IsBackground = true, Name = "mem-sampler" };
            _probeThread.Start();
            _memThread.Start();
        }

        public double NowMs => _clock.Elapsed.TotalMilliseconds;

        private void ProbeLoop()
        {
            while (!_cts.IsCancellationRequested)
            {
                var t0 = _clock.Elapsed.TotalMilliseconds;
                var ok = SendMessageTimeoutW(_hwnd, WmNull, IntPtr.Zero, IntPtr.Zero, SmtoBlock | SmtoAbortIfHung, 10_000, out _);
                var t1 = _clock.Elapsed.TotalMilliseconds;
                if (ok != IntPtr.Zero)
                {
                    lock (_gate) Probes.Add(new Probe(t0, t1 - t0, Phase));
                }
                Thread.Sleep(2);
            }
        }

        private void MemLoop()
        {
            while (!_cts.IsCancellationRequested)
            {
                try
                {
                    using var p = Process.GetProcessById(_pid);
                    lock (_gate) Mems.Add(new Mem(_clock.Elapsed.TotalMilliseconds, p.WorkingSet64 / 1048576.0, p.PrivateMemorySize64 / 1048576.0, Phase));
                }
                catch (Exception)
                {
                    // process gone
                }
                Thread.Sleep(100);
            }
        }

        public (Probe[] Probes, Mem[] Mems) Snapshot()
        {
            lock (_gate) return (Probes.ToArray(), Mems.ToArray());
        }

        public void Dispose()
        {
            _cts.Cancel();
            _probeThread.Join(2000);
            _memThread.Join(2000);
            timeEndPeriod(1);
            _cts.Dispose();
        }
    }

    internal static double Pct(double[] sorted, double p) => sorted.Length == 0 ? 0 : sorted[Math.Min(sorted.Length - 1, (int)Math.Ceiling(p / 100.0 * sorted.Length) - 1)];

    internal static string F(double v, string fmt = "F1") => v.ToString(fmt, CultureInfo.InvariantCulture);

    [PerfUiFact]
    public async Task Large_document_flow_reader_is_measured()
    {
        var report = new StringBuilder();
        void Line(string s)
        {
            report.AppendLine(s);
            _output.WriteLine(s);
        }

        var root = Path.Combine(Path.GetTempPath(), "gist-perf-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        GistAppSession? session = null;
        try
        {
            // ── Seed: real core import of the generated document ───────────────
            var gen = PerfDocumentGenerator.GenerateEpub(Path.Combine(root, "user-files"));
            string title;
            double importMs;
            {
                var paths = GistStoragePaths.ForRoot(root);
                paths.EnsureCreated();
                using var core = new CoreClient(new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
                Assert.True(await core.InitializeAsync());
                var sw = Stopwatch.StartNew();
                var id = await core.ImportFileAsync(gen.EpubPath);
                sw.Stop();
                Assert.NotNull(id);
                importMs = sw.Elapsed.TotalMilliseconds;
                await core.RefreshAsync();
                title = core.Items.Single().Title;
            }

            Line($"W5 UI measurement: document '{title}', {gen.WordCount} words, {gen.ChapterCount} chapters, {gen.HeadingCount} headings, {gen.BlockCount} blocks");
            Line($"machine: {Environment.MachineName}, {Environment.ProcessorCount} logical CPUs, {RuntimeInformation.OSDescription}; app build under test: whichever GIST.exe the UITests project output resolves to (see LocateExe)");
            Line($"import through the real core with the real DPAPI key provider: {F(importMs, "F0")} ms");

            session = await GistAppSession.StartAsync(existingRoot: root);
            var d = new LibraryDriver(session);
            d.WaitForLibrary();
            d.WaitForTitles(t => t.Length == 1, "the one large item");

            var hwnd = session.Window.Properties.NativeWindowHandle.Value;
            using var mon = new Monitor(hwnd, session.ProcessId);
            var tl = new List<(string Name, double Ms)>();

            void Timed(string name, double ms)
            {
                tl.Add((name, ms));
                Line($"  {name}: {F(ms, "F0")} ms");
            }

            // ── Baseline ───────────────────────────────────────────────────────
            mon.Phase = "01 library idle (no UIA)";
            Thread.Sleep(2500);

            // ── Open the flow page ─────────────────────────────────────────────
            mon.Phase = "02 open menu (UIA)";
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

            Line("timings (driver polling granularity applies; see notes):");
            mon.Phase = "03 open flow page (UIA)";
            var open = Stopwatch.StartNew();
            LibraryDriver.Activate(item!);
            var ready = Poll(() => d.ById("FlowPage_ProgressText") is { } p && System.Text.RegularExpressions.Regex.IsMatch(p.Name, @"^\d+%$"), TimeSpan.FromSeconds(120));
            var tReady = open.Elapsed.TotalMilliseconds;
            Timed("open -> flow page ready (progress text shown)", tReady);
            Assert.True(ready, "flow page did not become ready within 120 s");
            var first = Poll(() => FlowReaderTests.VisibleBlocks(d).Length > 0, TimeSpan.FromSeconds(60));
            Timed("open -> first block visible in the viewport", open.Elapsed.TotalMilliseconds);
            Assert.True(first);

            // How expensive is one harness poll (the resolution of the two numbers above)?
            var pollWatch = Stopwatch.StartNew();
            for (var i = 0; i < 5; i++) FlowReaderTests.VisibleBlocks(d);
            var pollCostMs = pollWatch.Elapsed.TotalMilliseconds / 5;
            Line($"  (one VisibleBlocks UIA poll costs ~{F(pollCostMs, "F0")} ms: resolution of the readiness times)");
            var viewport = d.Need("FlowPage_List").BoundingRectangle;
            Line($"  realised block elements at the top of the document: {FlowReaderTests.RealizedBlocks(d).Length} of {gen.BlockCount} blocks");

            mon.Phase = "04 top idle (no UIA)";
            Thread.Sleep(2500);

            // ── Keyboard paging, no UIA while measuring ────────────────────────
            mon.Phase = "05 Page Down x80 at ~12/s (no UIA)";
            for (var i = 0; i < 80; i++)
            {
                Keyboard.Type(VirtualKeyShort.NEXT);
                Thread.Sleep(80);
            }
            Thread.Sleep(800);
            mon.Phase = "05b after paging (UIA)";
            var afterPgDn = FlowReaderTests.ProgressPercent(d);
            Line($"  progress after 80 Page Downs: {afterPgDn}%");

            // ── Mouse wheel, no UIA while measuring ────────────────────────────
            mon.Phase = "06 mouse wheel 120 notches (no UIA)";
            Mouse.MoveTo(new System.Drawing.Point((int)(viewport.Left + viewport.Width / 2), (int)(viewport.Top + viewport.Height / 2)));
            for (var i = 0; i < 120; i++)
            {
                Mouse.Scroll(-1);
                Thread.Sleep(16);
            }
            Thread.Sleep(800);
            mon.Phase = "06b after wheel (UIA)";
            Line($"  progress after wheel scrolling: {FlowReaderTests.ProgressPercent(d)}%");

            // ── Long jumps (time to settle at 100% / 0%) ──────────────────────
            mon.Phase = "07 End (UIA polling)";
            var sw2 = Stopwatch.StartNew();
            Keyboard.Type(VirtualKeyShort.END);
            Poll(() => FlowReaderTests.ProgressPercent(d) == 100, TimeSpan.FromSeconds(60));
            Timed("End key -> progress 100%", sw2.Elapsed.TotalMilliseconds);
            Poll(() => FlowReaderTests.VisibleBlocks(d).Length > 0, TimeSpan.FromSeconds(30));
            Timed("End key -> a block visible at the end", sw2.Elapsed.TotalMilliseconds);
            Line($"  realised block elements at the end: {FlowReaderTests.RealizedBlocks(d).Length}");

            mon.Phase = "08 Home (UIA polling)";
            sw2.Restart();
            Keyboard.Type(VirtualKeyShort.HOME);
            Poll(() => FlowReaderTests.ProgressPercent(d) == 0 && FlowReaderTests.VisibleBlocks(d).Any(b => b.Name.StartsWith("Chapter 1:", StringComparison.Ordinal)), TimeSpan.FromSeconds(60));
            Timed("Home key -> progress 0% and first heading visible", sw2.Elapsed.TotalMilliseconds);

            mon.Phase = "09 End then Home, quick, no UIA";
            for (var i = 0; i < 5; i++)
            {
                Keyboard.Type(VirtualKeyShort.END);
                Thread.Sleep(900);
                Keyboard.Type(VirtualKeyShort.HOME);
                Thread.Sleep(900);
            }

            // ── Contents jump ──────────────────────────────────────────────────
            mon.Phase = "10 Contents flyout open (UIA)";
            sw2.Restart();
            FlowReaderTests.OpenFlyout(d, "FlowPage_ContentsButton", "FlowPage_TocList");
            Timed("Contents click -> flyout list present (350 entries)", sw2.Elapsed.TotalMilliseconds);
            AutomationElement? row = null;
            // The Contents list virtualises (350 rows), so the target row only exists once scrolled near: chapter 40's
            // h1 is the 196th of 350 entries (5 headings per chapter), ~56% down.
            var toc = d.Need("FlowPage_TocList");
            Poll(() =>
            {
                row = d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.ListItem).And(cf.ByName(PerfDocumentGenerator.RareChapterHeading))).FirstOrDefault();
                if (row is null && toc.Patterns.Scroll.IsSupported)
                {
                    toc.Patterns.Scroll.Pattern.SetScrollPercent(FlaUI.Core.Patterns.ScrollPatternConstants.NoScroll, 55.7);
                }
                return row is not null;
            }, TimeSpan.FromSeconds(20));
            Line($"  Contents row for chapter 40 {(row is null ? "NOT found" : "found")} after scrolling the virtualised list via UIA ScrollPattern");
            Assert.NotNull(row);
            mon.Phase = "11 Contents jump to chapter 40 (UIA polling)";
            sw2.Restart();
            FlowReaderTests.ActivateRow(d, row!);
            Poll(() => FlowReaderTests.BlockVisible(d, PerfDocumentGenerator.RareChapterHeading), TimeSpan.FromSeconds(60));
            Timed("Contents row activated -> chapter 40 heading visible", sw2.Elapsed.TotalMilliseconds);
            Line($"  progress after the jump: {FlowReaderTests.ProgressPercent(d)}%");
            Thread.Sleep(500);

            // ── Find ───────────────────────────────────────────────────────────
            mon.Phase = "12 open find (UIA)";
            FlowReaderTests.ShowFind(d);

            string status = "";
            var statusRx = new System.Text.RegularExpressions.Regex(@"^1 of (\d+)\+?$");

            // Typed -> status, polling (timing; the UIA polling contaminates the stall probe in these phases).
            void FindTimed(string label, string q)
            {
                var prev = FlowReaderTests.FindStatus(d);
                mon.Phase = $"13 find '{label}' typed, UIA polling";
                var s = Stopwatch.StartNew();
                FlowReaderTests.SetFind(d, q);
                Poll(() => (status = FlowReaderTests.FindStatus(d)) != prev && (statusRx.IsMatch(status) || status == "No matches"), TimeSpan.FromSeconds(60));
                Timed($"find '{label}' typed -> status '{status}'", s.Elapsed.TotalMilliseconds);
            }

            // Typed, then 2.5 s with NO UIA traffic so the probe sees only the app's own work; status read afterwards.
            void FindQuiet(string label, string q)
            {
                mon.Phase = $"14 find '{label}' typed (UIA set only)";
                FlowReaderTests.SetFind(d, q);
                mon.Phase = $"14 find '{label}' settle (no UIA)";
                Thread.Sleep(2500);
                mon.Phase = $"14b read status '{label}' (UIA)";
                status = FlowReaderTests.FindStatus(d);
                Line($"  find '{label}' settled status: '{status}'");
            }

            FindTimed("rare word", PerfDocumentGenerator.RareWord);
            FindTimed("water", "water");
            FindQuiet("the", "the");
            FindQuiet("a (hits the 20000-match cap)", "a");

            mon.Phase = "15 F3 x40 at ~7/s (no UIA)";
            for (var i = 0; i < 40; i++)
            {
                Keyboard.Type(VirtualKeyShort.F3);
                Thread.Sleep(150);
            }
            Thread.Sleep(500);
            mon.Phase = "15b read status after F3 (UIA)";
            Line($"  status after 40 F3 steps from 1: '{FlowReaderTests.FindStatus(d)}'");

            mon.Phase = "16 F3 -> status changed, polling (UIA)";
            var nextTimes = new List<double>();
            for (var i = 0; i < 25; i++)
            {
                var before = FlowReaderTests.FindStatus(d);
                var s = Stopwatch.StartNew();
                Keyboard.Type(VirtualKeyShort.F3);
                Poll(() => FlowReaderTests.FindStatus(d) != before, TimeSpan.FromSeconds(30));
                nextTimes.Add(s.Elapsed.TotalMilliseconds);
            }
            Line($"  F3 -> status changed, 25 steps: median {F(nextTimes.OrderBy(x => x).ElementAt(12), "F0")} ms, max {F(nextTimes.Max(), "F0")} ms (includes UIA poll granularity of ~{F(pollCostMs, "F0")} ms)");
            FindTimed("no match", "qqqqnotpresent");

            mon.Phase = "17 find closed, idle (no UIA)";
            d.InvokeCommand("FlowPage_FindClose");
            Thread.Sleep(2500);

            // ── Leave, then repeated reopen/leave cycles to tell "retained until GC" from "leaked" ─────────
            mon.Phase = "18 back to library (UIA)";
            d.InvokeCommand("FlowPage_BackButton");
            d.WaitForLibrary();
            mon.Phase = "19 library idle after first leave (no UIA)";
            Thread.Sleep(5000);
            (double Ws, double Priv) Current()
            {
                using var p = Process.GetProcessById(session.ProcessId);
                return (p.WorkingSet64 / 1048576.0, p.PrivateMemorySize64 / 1048576.0);
            }
            var c0 = Current();
            Line($"  after the first leave + 5 s idle: WS {F(c0.Ws, "F0")} MiB, private {F(c0.Priv, "F0")} MiB");
            var cycles = int.TryParse(Environment.GetEnvironmentVariable("GIST_PERF_CYCLES"), out var cyc) ? cyc : 5;
            for (var cycle = 1; cycle <= cycles; cycle++)
            {
                mon.Phase = $"20 cycle {cycle} reopen (UIA)";
                AutomationElement? it2 = null;
                for (var attempt = 0; attempt < 4 && it2 is null; attempt++)
                {
                    d.ClickRow(title);
                    d.Press(VirtualKeyShort.APPS);
                    try
                    {
                        it2 = LibraryDriver.Poll(
                            () => d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem).And(cf.ByName("Open in Flow View"))).FirstOrDefault(),
                            "Open in Flow View", TimeSpan.FromSeconds(4));
                    }
                    catch (TimeoutException) when (attempt < 3)
                    {
                        d.Press(VirtualKeyShort.ESCAPE);
                    }
                }
                var sw3 = Stopwatch.StartNew();
                LibraryDriver.Activate(it2!);
                Poll(() => d.ById("FlowPage_ProgressText") is { } p && System.Text.RegularExpressions.Regex.IsMatch(p.Name, @"^\d+%$"), TimeSpan.FromSeconds(120));
                var reopenMs = sw3.Elapsed.TotalMilliseconds;
                mon.Phase = $"20 cycle {cycle} idle in reader (no UIA)";
                Thread.Sleep(1500);
                mon.Phase = $"20 cycle {cycle} leave (UIA)";
                d.InvokeCommand("FlowPage_BackButton");
                d.WaitForLibrary();
                mon.Phase = $"20 cycle {cycle} idle in library (no UIA)";
                Thread.Sleep(4000);
                var cur = Current();
                Line($"  cycle {cycle}: reopen -> ready {F(reopenMs, "F0")} ms; after leaving + 4 s idle: WS {F(cur.Ws, "F0")} MiB, private {F(cur.Priv, "F0")} MiB");
            }

            // ── Analyse ────────────────────────────────────────────────────────
            var (probes, mems) = mon.Snapshot();
            Line("");
            Line($"UI-thread message-pump latency (WM_NULL round trip, {probes.Length} probes at ~2 ms spacing):");
            Line("  phase                                                     n     p50     p99     max  >16ms >50ms >100ms");
            foreach (var g in probes.GroupBy(p => p.Phase).OrderBy(g => g.Key, StringComparer.Ordinal))
            {
                var v = g.Select(p => p.LatencyMs).OrderBy(x => x).ToArray();
                Line($"  {g.Key,-52} {v.Length,6} {F(Pct(v, 50)),7} {F(Pct(v, 99)),7} {F(v[^1]),7} {v.Count(x => x > 16),6} {v.Count(x => x > 50),5} {v.Count(x => x > 100),6}");
            }
            var all = probes.Select(p => p.LatencyMs).OrderBy(x => x).ToArray();
            Line($"  ALL phases: p50 {F(Pct(all, 50))} ms, p99 {F(Pct(all, 99))} ms, max {F(all[^1])} ms; probes >50 ms: {all.Count(x => x > 50)}");
            var noUia = probes.Where(p => p.Phase.Contains("(no UIA)", StringComparison.Ordinal)).Select(p => p.LatencyMs).OrderBy(x => x).ToArray();
            Line($"  '(no UIA)' phases only (no harness UIA traffic): {noUia.Length} probes, p50 {F(Pct(noUia, 50))} ms, p99 {F(Pct(noUia, 99))} ms, max {F(noUia[^1])} ms, >50 ms: {noUia.Count(x => x > 50)}");
            Line("  worst 12 stalls (at ms since monitor start, latency, phase):");
            foreach (var p in probes.OrderByDescending(p => p.LatencyMs).Take(12))
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
            Line($"  OVERALL peak working set {F(mems.Max(m => m.WorkingSetMiB), "F0")} MiB, peak private bytes {F(mems.Max(m => m.PrivateMiB), "F0")} MiB; baseline (library idle) WS {F(mems.Where(m => m.Phase.StartsWith("01", StringComparison.Ordinal)).Average(m => m.WorkingSetMiB), "F0")} MiB, private {F(mems.Where(m => m.Phase.StartsWith("01", StringComparison.Ordinal)).Average(m => m.PrivateMiB), "F0")} MiB");
                        Line($"  responding at the end: {session.Responding}");

            if (Environment.GetEnvironmentVariable("GIST_PERF_REPORT") is { Length: > 0 } path)
            {
                File.WriteAllText(path, report.ToString());
            }
        }
        finally
        {
            session?.Dispose();
            if (Environment.GetEnvironmentVariable("GIST_PERF_REPORT") is { Length: > 0 } partial)
            {
                try { File.WriteAllText(partial, report.ToString()); } catch (Exception) { }
            }
            try { Directory.Delete(root, recursive: true); } catch (Exception) { }
        }
    }

    internal static bool Poll(Func<bool> probe, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            try { if (probe()) return true; } catch (Exception) { /* tree changing */ }
            Thread.Sleep(5);
        }
        return false;
    }
}
