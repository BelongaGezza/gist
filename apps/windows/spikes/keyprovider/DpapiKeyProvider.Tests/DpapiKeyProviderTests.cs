using System.Security.Cryptography;
using System.Text;
using Xunit;

namespace Gist.KeyProvider.Tests;

// Real DPAPI, real filesystem, temp dir per test.
public sealed class DpapiKeyProviderTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "gist-kp-test-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try { Directory.Delete(_dir, true); } catch (DirectoryNotFoundException) { }
    }

    private DpapiKeyProvider New(byte[]? entropy = null) => new(_dir, entropy);

    private static bool ContainsSubsequence(byte[] hay, byte[] needle)
    {
        for (int i = 0; i + needle.Length <= hay.Length; i++)
            if (hay.AsSpan(i, needle.Length).SequenceEqual(needle)) return true;
        return false;
    }

    [Fact]
    public void Create_Returns32Bytes_AndPersists()
    {
        var p = New();
        var key = p.GetOrCreateKey();
        Assert.Equal(32, key.Length);
        Assert.True(File.Exists(p.KeyFilePath));
        Assert.NotEqual(new byte[32], key);
    }

    [Fact]
    public void SecondCall_AndSecondInstance_ReturnIdenticalKey()
    {
        var p = New();
        var a = p.GetOrCreateKey();
        Assert.Equal(a, p.GetOrCreateKey());
        Assert.Equal(a, New().GetOrCreateKey());
    }

    [Fact]
    public void Concurrent_SameInstanceAndManyInstances_ConvergeOnOneKeyAndOneFile()
    {
        const int n = 32;
        var results = new byte[n][];
        var shared = New();
        using var gate = new ManualResetEventSlim(false);
        var threads = Enumerable.Range(0, n).Select(i => new Thread(() =>
        {
            var p = i % 2 == 0 ? shared : New();
            gate.Wait();
            results[i] = p.GetOrCreateKey();
        })).ToList();
        threads.ForEach(t => t.Start());
        gate.Set();
        threads.ForEach(t => t.Join());

        Assert.All(results, r => Assert.Equal(results[0], r));
        Assert.Equal(32, results[0].Length);
        Assert.Single(Directory.GetFiles(_dir)); // no leaked temp files
        Assert.Equal(results[0], New().GetOrCreateKey());
    }

    [Fact]
    public void Concurrent_Parallel_ConvergeAcrossManyRounds()
    {
        for (int round = 0; round < 5; round++)
        {
            var d = Path.Combine(_dir, "r" + round);
            var keys = new System.Collections.Concurrent.ConcurrentBag<byte[]>();
            Parallel.For(0, 24, new ParallelOptions { MaxDegreeOfParallelism = 24 },
                _ => keys.Add(new DpapiKeyProvider(d).GetOrCreateKey()));
            var first = keys.First();
            Assert.All(keys, k => Assert.Equal(first, k));
            Assert.Single(Directory.GetFiles(d));
        }
    }

    [Fact]
    public void CorruptFile_ThrowsTyped_FileUntouched_NoNewKey()
    {
        var p = New();
        p.GetOrCreateKey();
        var bytes = File.ReadAllBytes(p.KeyFilePath);
        bytes[^3] ^= 0xFF; // tamper inside the DPAPI blob
        File.WriteAllBytes(p.KeyFilePath, bytes);
        var before = File.ReadAllBytes(p.KeyFilePath);

        Assert.Throws<KeyStoreCorruptException>(() => New().GetOrCreateKey());
        Assert.Equal(before, File.ReadAllBytes(p.KeyFilePath));
        Assert.Single(Directory.GetFiles(_dir));
    }

    [Fact]
    public void GarbageFile_BadHeader_Throws()
    {
        Directory.CreateDirectory(_dir);
        var p = New();
        var junk = Encoding.ASCII.GetBytes("not a key file at all, just text");
        File.WriteAllBytes(p.KeyFilePath, junk);
        Assert.Throws<KeyStoreCorruptException>(() => p.GetOrCreateKey());
        Assert.Equal(junk, File.ReadAllBytes(p.KeyFilePath));
    }

    [Fact]
    public void TruncatedFile_Throws_AndIsNotReplaced()
    {
        var p = New();
        p.GetOrCreateKey();
        var bytes = File.ReadAllBytes(p.KeyFilePath);
        var cut = bytes[..(bytes.Length / 2)];
        File.WriteAllBytes(p.KeyFilePath, cut);
        Assert.Throws<KeyStoreCorruptException>(() => New().GetOrCreateKey());
        Assert.Equal(cut, File.ReadAllBytes(p.KeyFilePath));
    }

    [Fact]
    public void HeaderOnlyFile_Throws()
    {
        var p = New();
        p.GetOrCreateKey();
        File.WriteAllBytes(p.KeyFilePath, "GKP1"u8.ToArray());
        Assert.Throws<KeyStoreCorruptException>(() => New().GetOrCreateKey());
    }

    [Fact]
    public void EmptyFile_Throws_AndIsNotReplaced()
    {
        Directory.CreateDirectory(_dir);
        var p = New();
        File.WriteAllBytes(p.KeyFilePath, Array.Empty<byte>());
        Assert.Throws<KeyStoreCorruptException>(() => p.GetOrCreateKey());
        Assert.Empty(File.ReadAllBytes(p.KeyFilePath));
    }

    [Fact]
    public void DifferentEntropy_Throws_AndFileUntouched()
    {
        var a = New("entropy-A"u8.ToArray());
        var key = a.GetOrCreateKey();
        var before = File.ReadAllBytes(a.KeyFilePath);

        Assert.Throws<KeyStoreCorruptException>(() => New("entropy-B"u8.ToArray()).GetOrCreateKey());
        Assert.Equal(before, File.ReadAllBytes(a.KeyFilePath));
        Assert.Equal(key, New("entropy-A"u8.ToArray()).GetOrCreateKey());
    }

    [Fact]
    public void WrongLengthKey_Throws()
    {
        // Validly DPAPI-protected but 16-byte payload under the default entropy.
        Directory.CreateDirectory(_dir);
        var p = New();
        var blob = ProtectedData.Protect(new byte[16], "GIST.KeyProvider.v1"u8.ToArray(), DataProtectionScope.CurrentUser);
        File.WriteAllBytes(p.KeyFilePath, "GKP1"u8.ToArray().Concat(blob).ToArray());
        Assert.Throws<KeyStoreCorruptException>(() => p.GetOrCreateKey());
    }

    [Fact]
    public void KeyNeverAppearsInPlaintextInFile()
    {
        var p = New();
        var key = p.GetOrCreateKey();
        var file = File.ReadAllBytes(p.KeyFilePath);
        Assert.False(ContainsSubsequence(file, key));
        Assert.False(ContainsSubsequence(file, key[..8])); // even a prefix
    }

    [Fact]
    public void ExceptionMessages_DoNotContainKeyBytes()
    {
        var p = New();
        var key = p.GetOrCreateKey();
        File.WriteAllBytes(p.KeyFilePath, "GKP1xxxx"u8.ToArray());
        var ex = Assert.Throws<KeyStoreCorruptException>(() => p.GetOrCreateKey());
        Assert.DoesNotContain(Convert.ToHexString(key), ex.ToString(), StringComparison.OrdinalIgnoreCase);
    }
}
