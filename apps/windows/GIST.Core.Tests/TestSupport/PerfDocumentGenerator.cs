using System.IO.Compression;
using System.Text;

namespace Gist.Core.Tests.TestSupport;

/// <summary>
/// Deterministic generator for the W5 large-document measurement: a multi-chapter ePub (h1/h2/h3 headings, prose
/// paragraphs, bullet and numbered lists) of at least <see cref="MinWords"/> words. Fixed seed, so every run
/// measures the same document. Generated into a temp directory and never committed. This file is linked into
/// GIST.App.UITests as well, so the Core measurement and the UI measurement use the identical document.
/// </summary>
public static class PerfDocumentGenerator
{
    public const int MinWords = 100_000;
    public const string Title = "Perf Large Document";
    public const string RareWord = "zephyrquartz";
    public const string RareChapterHeading = "Chapter 40: The Rare Word Chapter";

    private const int Chapters = 70;
    private const int Seed = 20261004;

    private static readonly string[] Vocabulary =
    [
        "the", "of", "and", "a", "to", "in", "is", "that", "it", "was", "for", "on", "are", "with", "as", "his",
        "they", "be", "at", "one", "have", "this", "from", "or", "had", "by", "hot", "word", "but", "what", "some",
        "we", "can", "out", "other", "were", "all", "there", "when", "up", "use", "your", "how", "said", "an",
        "each", "she", "which", "do", "their", "time", "if", "will", "way", "about", "many", "then", "them",
        "write", "would", "like", "so", "these", "her", "long", "make", "thing", "see", "him", "two", "has",
        "look", "more", "day", "could", "go", "come", "did", "number", "sound", "no", "most", "people", "my",
        "over", "know", "water", "than", "call", "first", "who", "may", "down", "side", "been", "now", "find",
        "orchard", "lantern", "harbour", "meadow", "compass", "violin", "granite", "willow", "ember", "trellis",
    ];

    public sealed record Result(string EpubPath, int WordCount, int ChapterCount, int HeadingCount, int BlockCount);

    public static Result GenerateEpub(string directory)
    {
        Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, "perf-large-document.epub");
        if (File.Exists(path)) File.Delete(path);
        var rng = new Random(Seed);
        var words = 0;
        var headings = 0;
        var blocks = 0;

        using (var zip = ZipFile.Open(path, ZipArchiveMode.Create))
        {
            // The mimetype entry must be first and stored.
            var mt = zip.CreateEntry("mimetype", CompressionLevel.NoCompression);
            using (var w = new StreamWriter(mt.Open(), new UTF8Encoding(false))) w.Write("application/epub+zip");
            Add(zip, "META-INF/container.xml",
                "<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>");

            var manifest = new StringBuilder();
            var spine = new StringBuilder();
            for (var c = 1; c <= Chapters; c++)
            {
                var sb = new StringBuilder();
                sb.Append("<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Chapter ").Append(c).Append("</title></head><body>");
                var heading = c == 40 ? RareChapterHeading : $"Chapter {c}: The {Cap(Pick(rng))} {Cap(Pick(rng))}";
                sb.Append("<h1>").Append(heading).Append("</h1>");
                headings++;
                blocks++;
                words += CountWords(heading);
                for (var s = 1; s <= 3; s++)
                {
                    var h2 = $"Section {c}.{s} — {Cap(Pick(rng))} {Pick(rng)}";
                    sb.Append("<h2>").Append(h2).Append("</h2>");
                    headings++;
                    blocks++;
                    words += CountWords(h2);
                    for (var p = 0; p < 4; p++)
                    {
                        var para = Paragraph(rng, 10 + rng.Next(6), c == 40 && s == 2 && p == 1);
                        sb.Append("<p>").Append(para).Append("</p>");
                        blocks++;
                        words += CountWords(para);
                    }
                    if (s == 2)
                    {
                        var h3 = $"Subsection {c}.2.1 — {Cap(Pick(rng))}";
                        sb.Append("<h3>").Append(h3).Append("</h3>");
                        headings++;
                        blocks++;
                        words += CountWords(h3);
                        sb.Append("<ul>");
                        for (var i = 0; i < 4; i++)
                        {
                            var li = Paragraph(rng, 1, false);
                            sb.Append("<li>").Append(li).Append("</li>");
                            words += CountWords(li);
                        }
                        sb.Append("</ul>");
                        blocks++;
                        sb.Append("<ol>");
                        for (var i = 0; i < 3; i++)
                        {
                            var li = Paragraph(rng, 1, false);
                            sb.Append("<li>").Append(li).Append("</li>");
                            words += CountWords(li);
                        }
                        sb.Append("</ol>");
                        blocks++;
                    }
                }
                sb.Append("</body></html>");
                Add(zip, $"OEBPS/chapter{c}.xhtml", sb.ToString());
                manifest.Append($"<item id=\"c{c}\" href=\"chapter{c}.xhtml\" media-type=\"application/xhtml+xml\"/>");
                spine.Append($"<itemref idref=\"c{c}\"/>");
            }

            Add(zip, "OEBPS/content.opf",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" unique-identifier=\"uid\">"
                + "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>" + Title + "</dc:title><dc:creator>Generator</dc:creator><dc:language>en</dc:language>"
                + "<dc:identifier id=\"uid\">urn:uuid:00000000-0000-0000-0000-0000000000f0</dc:identifier></metadata>"
                + "<manifest>" + manifest + "</manifest><spine>" + spine + "</spine></package>");
        }

        if (words < MinWords) throw new InvalidOperationException($"generator produced only {words} words");
        return new Result(path, words, Chapters, headings, blocks);
    }

    private static string Pick(Random rng) => Vocabulary[rng.Next(Vocabulary.Length)];

    private static string Cap(string s) => char.ToUpperInvariant(s[0]) + s[1..];

    private static int CountWords(string s) => s.Split(' ', StringSplitOptions.RemoveEmptyEntries).Length;

    private static string Paragraph(Random rng, int sentences, bool plantRare)
    {
        var sb = new StringBuilder();
        for (var i = 0; i < sentences; i++)
        {
            var n = 8 + rng.Next(12);
            for (var w = 0; w < n; w++)
            {
                var word = Pick(rng);
                if (w == 0) word = Cap(word);
                if (plantRare && i == 0 && w == 3) word = RareWord;
                sb.Append(word);
                if (w < n - 1) sb.Append(w % 9 == 8 ? ", " : " ");
            }
            sb.Append(i < sentences - 1 ? ". " : ".");
        }
        return sb.ToString();
    }

    private static void Add(ZipArchive zip, string name, string content)
    {
        var e = zip.CreateEntry(name, CompressionLevel.Optimal);
        using var w = new StreamWriter(e.Open(), new UTF8Encoding(false));
        w.Write(content);
    }
}
