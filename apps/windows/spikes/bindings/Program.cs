using System.Text.Json;
using uniffi.gist_ffi;

// Every check prints PASS/FAIL; any FAIL makes the process exit non-zero.
int failures = 0;
void Check(string name, bool ok, string detail = "")
{
    Console.WriteLine($"{(ok ? "PASS" : "FAIL")}  {name}{(detail.Length > 0 ? "  -- " + detail : "")}");
    if (!ok) failures++;
}

var repo = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../../.."));
var fixtures = Path.Combine(repo, "fixtures");
var work = Path.Combine(Path.GetTempPath(), "gist-spike-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(work);

var keyProvider = new FixedKeyProvider();
GistCore core = GistCore.NewWithReadKey(
    Path.Combine(work, "gist.sqlite3"), Path.Combine(work, "storage"), keyProvider);

try
{
    core.Health();
    Check("constructor NewWithReadKey + Health()", true);

    // Import + records + string[] + Option<string>
    var id = core.ImportFile(Path.Combine(fixtures, "txt", "basic_ascii.txt"));
    Check("ImportFile returns an id", id.Length > 0, id);
    var items = core.ListItems(0, 50);
    Check("ListItems returns record array", items.Length == 1 && items[0].Id == id,
        $"title={items[0].Title ?? "<null>"} authors=[{string.Join(",", items[0].Authors)}] encrypted={items[0].ContentEncrypted}");

    // FTS search incl. partial-word prefix match (F19 regression)
    Check("SearchItems full word", core.SearchItems("consectetur", 10).Length == 1);
    Check("SearchItems partial word 'consec'", core.SearchItems("consec", 10).Length == 1);
    Check("SearchItems hostile input does not throw", Try(() => core.SearchItems("\"foo* OR (", 10)));

    // JSON-string endpoints
    using (var rsvp = JsonDocument.Parse(core.StartRsvp(id, 300)))
    {
        var tokens = rsvp.RootElement.GetProperty("tokens").GetArrayLength();
        var wpm = rsvp.RootElement.GetProperty("config").GetProperty("wpm").GetUInt32();
        Check("StartRsvp JSON decodes", tokens > 10 && wpm == 300, $"tokens={tokens} wpm={wpm}");
    }
    using (var doc = JsonDocument.Parse(core.GetDocumentJson(id)))
    {
        var kinds = new SortedSet<string>();
        foreach (var s in doc.RootElement.GetProperty("sections").EnumerateArray())
            foreach (var b in s.GetProperty("blocks").EnumerateArray())
                foreach (var p in b.EnumerateObject()) kinds.Add(p.Name);   // serde externally-tagged enum
        Check("GetDocumentJson decodes; block tags are externally tagged", kinds.Count > 0, string.Join(",", kinds));
    }
    core.SaveProgress(id, 5);
    Check("SaveProgress", true);

    // Collections + tags
    var col = core.CreateCollection("Spike");
    core.AddItemToCollection(id, col);
    Check("Collections round trip", core.ListCollections().Any(c => c.Id == col && c.Name == "Spike")
        && core.ListItemsInCollection(col).Length == 1);
    core.AddTag(id, "alpha");
    Check("Tags round trip", core.ListTagsForItem(id).SequenceEqual(new[] { "alpha" })
        && core.ListAllTags().Contains("alpha") && core.ListItemsByTag("alpha").Length == 1);

    // Typed exceptions
    var drm = Catch(() => core.ImportFile(Path.Combine(fixtures, "epub", "adversarial", "drm_protected.epub")));
    Check("DRM epub -> GistException.DrmProtected (typed, not string-matched)", drm is GistException.DrmProtected,
        drm?.GetType().Name ?? "no exception");
    var missing = Catch(() => core.ImportFile(Path.Combine(work, "nope.txt")));
    Check("Missing file -> GistException (not a crash)", missing is GistException, missing?.GetType().Name ?? "none");

    // Encryption: callback interface invoked, and read-after-encrypt works through a read-capable client (ADR-014)
    var before = keyProvider.Calls;
    var enc = core.EncryptItems(new[] { id }, keyProvider);
    Check("EncryptItems -> Encrypted", enc.Length == 1 && enc[0].Outcome == FfiEncryptOutcome.Encrypted, enc[0].Error ?? "");
    Check("KeyProvider callback was invoked from Rust", keyProvider.Calls > before, $"calls={keyProvider.Calls}");
    Check("Read-after-encrypt via GetDocumentJson", Try(() => core.GetDocumentJson(id)));
    Check("List shows ContentEncrypted", core.ListItems(0, 10)[0].ContentEncrypted);

    // Threading: FFI from many threads at once (plan: CoreClient uses Task.Run)
    var errs = 0;
    Parallel.For(0, 64, _ => { try { core.ListItems(0, 10); core.SearchItems("lorem", 5); } catch { Interlocked.Increment(ref errs); } });
    Check("64 concurrent FFI calls", errs == 0, $"errors={errs}");

    // Removal never touches the user's original
    var orig = Path.Combine(fixtures, "txt", "basic_ascii.txt");
    core.RemoveItems(new[] { id }, true);
    Check("RemoveItems; user's original untouched", core.ListItems(0, 10).Length == 0 && File.Exists(orig));

    // Panic containment: a wrong-length key must surface as InternalPanic, not crash the process.
    var bad = GistCore.NewWithReadKey(Path.Combine(work, "b.sqlite3"), Path.Combine(work, "b"), new ShortKeyProvider());
    var id2 = bad.ImportFile(Path.Combine(fixtures, "txt", "basic_ascii.txt"));
    var panic = Catch(() => bad.EncryptItems(new[] { id2 }, new ShortKeyProvider()));
    Check("Bad callback (16-byte key) -> GistException.InternalPanic, process survives", panic is GistException.InternalPanic, panic?.GetType().Name ?? "returned normally");
}
finally
{
    core.Dispose();
    try { Directory.Delete(work, true); } catch { }
}

Console.WriteLine(failures == 0 ? "\nSPIKE RESULT: ALL PASS" : $"\nSPIKE RESULT: {failures} FAILURE(S)");
return failures == 0 ? 0 : 1;

static bool Try(Action a) { try { a(); return true; } catch { return false; } }
static Exception? Catch(Action a) { try { a(); return null; } catch (Exception e) { return e; } }

sealed class FixedKeyProvider : KeyProvider
{
    private readonly byte[] key = System.Security.Cryptography.RandomNumberGenerator.GetBytes(32);
    public int Calls;
    public byte[] GetOrCreateKey() { Interlocked.Increment(ref Calls); return key; }
}
sealed class ShortKeyProvider : KeyProvider
{
    public byte[] GetOrCreateKey() => new byte[16];
}
