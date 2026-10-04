using System.Diagnostics;
using System.Globalization;
using System.Text;
using Gist.Core.Client;
using Gist.Core.Flow;
using Gist.Core.Tests.TestSupport;
using Xunit.Abstractions;

namespace Gist.Core.Tests.Flow;

/// <summary>Reports Skipped unless <c>GIST_RUN_PERF=1</c>.</summary>
public sealed class PerfFactAttribute : FactAttribute
{
    public const string EnvVar = "GIST_RUN_PERF";

    public PerfFactAttribute()
    {
        if (Environment.GetEnvironmentVariable(EnvVar) != "1")
        {
            Skip = $"Perf measurement is opt-in: set {EnvVar}=1 (GIST_PERF_REPORT=<file> saves the report; run with -c Release for representative numbers).";
        }
    }
}

/// <summary>
/// W5 exit criterion, core half: measures the real import -> get_document_json -> decode -> find -> TOC path on a
/// generated ≥100k-word ePub. Numbers are reported, not asserted tightly: the only assertions are generous
/// sanity ceilings so a catastrophic regression (quadratic decode, say) still fails.
/// </summary>
public sealed class FlowPerfTests : IDisposable
{
    private readonly ITestOutputHelper _output;
    private readonly TempWorkspace _workspace = new();

    public FlowPerfTests(ITestOutputHelper output) => _output = output;

    public void Dispose() => _workspace.Dispose();

    private static double Ms(Stopwatch sw) => sw.Elapsed.TotalMilliseconds;

    [PerfFact]
    public async Task Large_document_core_path_is_measured()
    {
        var gen = PerfDocumentGenerator.GenerateEpub(_workspace.Root);
        var report = new StringBuilder();
        void Line(string s)
        {
            report.AppendLine(s);
            _output.WriteLine(s);
        }

        Line($"W5 core measurement ({(IsDebug() ? "Debug" : "Release")} build of the test assembly; the Rust core is whatever gist_ffi.dll is staged)");
        Line(string.Create(CultureInfo.InvariantCulture, $"document: {gen.WordCount} words, {gen.ChapterCount} chapters, {gen.HeadingCount} headings, {gen.BlockCount} blocks, epub {new FileInfo(gen.EpubPath).Length / 1024} KiB"));

        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, new FakeKeyProvider());
        Assert.True(await client.InitializeAsync());

        var sw = Stopwatch.StartNew();
        var id = await client.ImportFileAsync(gen.EpubPath);
        sw.Stop();
        Assert.NotNull(id);
        Line(string.Create(CultureInfo.InvariantCulture, $"import (ImportFileAsync, epub parse + store + FTS tokens): {Ms(sw):F0} ms"));

        // get_document_json: 3 runs (first includes cold paths).
        string json = "";
        var jsonTimes = new List<double>();
        for (var i = 0; i < 3; i++)
        {
            sw.Restart();
            json = (await client.GetDocumentJsonAsync(id!))!;
            sw.Stop();
            jsonTimes.Add(Ms(sw));
        }
        Assert.False(string.IsNullOrEmpty(json));
        Line(string.Create(CultureInfo.InvariantCulture, $"get_document_json: {string.Join(" / ", jsonTimes.Select(t => t.ToString("F0", CultureInfo.InvariantCulture)))} ms (runs 1/2/3); {json.Length / 1024} KiB ({json.Length} UTF-16 chars)"));

        // Decode: 3 runs, with managed allocation (thread-allocated bytes) and retained size.
        FlowDocument? doc = null;
        var decodeTimes = new List<double>();
        var allocs = new List<long>();
        for (var i = 0; i < 3; i++)
        {
            var before = GC.GetAllocatedBytesForCurrentThread();
            sw.Restart();
            var r = FlowDocumentDecoder.Decode(json);
            sw.Stop();
            allocs.Add(GC.GetAllocatedBytesForCurrentThread() - before);
            decodeTimes.Add(Ms(sw));
            Assert.True(r.Succeeded, r.Error.ToString());
            doc = r.Document;
        }
        Line(string.Create(CultureInfo.InvariantCulture, $"FlowDocumentDecoder.Decode: {string.Join(" / ", decodeTimes.Select(t => t.ToString("F0", CultureInfo.InvariantCulture)))} ms; allocated {string.Join(" / ", allocs.Select(a => (a / 1048576.0).ToString("F1", CultureInfo.InvariantCulture)))} MiB per run"));

        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        var heapBefore = GC.GetTotalMemory(true);
        var kept = FlowDocumentDecoder.Decode(json).Document!;
        var heapAfter = GC.GetTotalMemory(true);
        Line(string.Create(CultureInfo.InvariantCulture, $"retained managed heap for one decoded document (json string released: no): ~{(heapAfter - heapBefore) / 1048576.0:F1} MiB"));
        GC.KeepAlive(kept);

        Assert.NotNull(doc);
        Line(string.Create(CultureInfo.InvariantCulture, $"decoded: {doc!.Sections.Count} sections, {doc.Entries.Count} block entries, {doc.TableOfContents.Count} TOC entries, skipped blocks {doc.SkippedBlockCount}"));

        // TOC / entries are built in the FlowDocument constructor; time a rebuild via decode is already included.
        // Time constructing a second document from the decoded sections to isolate TOC + Entries building.
        sw.Restart();
        var rebuilt = new FlowDocument(doc.Id, doc.Title, doc.Author, doc.Sections, doc.SkippedBlockCount);
        sw.Stop();
        Line(string.Create(CultureInfo.InvariantCulture, $"FlowDocument construction from decoded sections (TOC + entry flattening): {Ms(sw):F2} ms ({rebuilt.TableOfContents.Count} TOC entries)"));

        // Find: common / rare / none. 5 runs each, report min and max.
        foreach (var (label, query) in new[] { ("common 'the'", "the"), ("common 'a'", "a"), ("rare", PerfDocumentGenerator.RareWord), ("no match", "qqqqnotpresent") })
        {
            var times = new List<double>();
            int count = 0;
            bool truncated = false;
            for (var i = 0; i < 5; i++)
            {
                sw.Restart();
                var m = FlowSearch.FindAll(doc, query, out truncated);
                sw.Stop();
                times.Add(Ms(sw));
                count = m.Count;
            }
            Line(string.Create(CultureInfo.InvariantCulture, $"FindAll {label}: {count} matches{(truncated ? " (truncated at cap)" : "")}; min {times.Min():F1} ms, max {times.Max():F1} ms over 5 runs"));
        }

        // Per-keystroke cost (what the UI debounces): incremental prefixes of a typed word.
        sw.Restart();
        foreach (var q in new[] { "w", "wa", "wat", "wate", "water" })
        {
            FlowSearch.FindAll(doc, q, out _);
        }
        sw.Stop();
        Line(string.Create(CultureInfo.InvariantCulture, $"FindAll typing 'water' keystroke by keystroke (5 queries): {Ms(sw):F1} ms total"));

        // Repeated open: does the get_document_json -> decode path (what each reopen of the reader does) leak
        // native or managed memory? Force full GCs between samples so only genuinely retained memory shows.
        var priv = new List<double>();
        for (var i = 0; i < 12; i++)
        {
            var j = (await client.GetDocumentJsonAsync(id!))!;
            var d = FlowDocumentDecoder.Decode(j).Document!;
            GC.KeepAlive(d);
            j = null!;
            d = null!;
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
            using var p = Process.GetCurrentProcess();
            priv.Add(p.PrivateMemorySize64 / 1048576.0);
        }
        Line("repeated get_document_json + Decode, full GC after each; process private MiB after each: "
            + string.Join(" ", priv.Select(v => v.ToString("F0", CultureInfo.InvariantCulture))));

        var proc = Process.GetCurrentProcess();
        Line(string.Create(CultureInfo.InvariantCulture, $"test process peak working set {proc.PeakWorkingSet64 / 1048576.0:F0} MiB, peak commit (paged) {proc.PeakPagedMemorySize64 / 1048576.0:F0} MiB (includes xunit + native core)"));

        if (Environment.GetEnvironmentVariable("GIST_PERF_REPORT") is { Length: > 0 } path)
        {
            File.WriteAllText(path, report.ToString());
        }

        // Generous sanity ceilings only: a quadratic regression would blow these by orders of magnitude.
        Assert.True(decodeTimes.Min() < 5_000, $"decode {decodeTimes.Min():F0} ms");
        Assert.True(doc.TableOfContents.Count >= gen.HeadingCount - 1);
    }

    private static bool IsDebug()
    {
#if DEBUG
        return true;
#else
        return false;
#endif
    }
}
