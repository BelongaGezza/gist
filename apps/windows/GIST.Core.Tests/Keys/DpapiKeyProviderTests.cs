using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using Gist.Core.Keys;
using Gist.Core.Storage;

namespace Gist.Core.Tests.Keys;

// Real DPAPI, real filesystem, temp dir per test. Nothing here touches the machine's own
// %LOCALAPPDATA%\GIST key or ACLs: every provider is pointed at a throwaway directory that
// Dispose deletes.
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

    // ---- Promoted from apps/windows/spikes/keyprovider (13 tests, unchanged behaviour) ----

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

    // ---- W1: failure classification (review finding Q9) ----

    [Theory]
    [InlineData(unchecked((int)0x8007000D))] // ERROR_INVALID_DATA — failed integrity check
    [InlineData(unchecked((int)0x80070057))] // ERROR_INVALID_PARAMETER — not a DPAPI blob at all
    public void Classifier_ProvenBadBlob_IsCorrupt(int hresult)
    {
        var classified = KeyStoreFailureClassifier.Classify(new CryptographicException("x") { HResult = hresult });
        Assert.IsType<KeyStoreCorruptException>(classified);
    }

    [Theory]
    [InlineData(unchecked((int)0x80070002))] // ERROR_FILE_NOT_FOUND — DPAPI master key missing
    [InlineData(unchecked((int)0x80070005))] // ERROR_ACCESS_DENIED
    [InlineData(unchecked((int)0x8009000B))] // NTE_BAD_KEY_STATE
    [InlineData(unchecked((int)0x8007013D))] // ERROR_MR_MID_NOT_FOUND — profile not fully loaded
    [InlineData(unchecked((int)0x80004005))] // E_FAIL — unknown: must not be called corruption
    public void Classifier_TransientOrUnknown_IsUnavailableAndRetryable(int hresult)
    {
        var classified = KeyStoreFailureClassifier.Classify(new CryptographicException("x") { HResult = hresult });
        var unavailable = Assert.IsType<KeyStoreUnavailableException>(classified);
        Assert.IsAssignableFrom<KeyProviderException>(unavailable);
    }

    [Fact]
    public void Classifier_PreservesTheUnderlyingErrorWithoutLeakingIt()
    {
        var inner = new CryptographicException("native detail") { HResult = unchecked((int)0x80070002) };
        var classified = KeyStoreFailureClassifier.Classify(inner);
        Assert.Same(inner, classified.InnerException);
        Assert.Contains("retry", classified.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void AllTypedFailures_ShareOneCatchableBaseType()
    {
        // The app's error handling catches KeyProviderException once; a new subtype must not escape it.
        Assert.IsAssignableFrom<KeyProviderException>(new KeyStoreCorruptException("c"));
        Assert.IsAssignableFrom<KeyProviderException>(new KeyStoreUnavailableException("u"));
        Assert.IsAssignableFrom<KeyProviderException>(new KeyStoreIoException("i"));
    }

    [Fact]
    public void AnyFailure_NeverDeletesOverwritesOrRecreatesTheKeyFile()
    {
        // The invariant, exercised over every failure shape reachable from outside the process.
        var p = New();
        p.GetOrCreateKey();
        var good = File.ReadAllBytes(p.KeyFilePath);

        var damaged = new (string Name, byte[] Bytes)[]
        {
            ("tampered", Tamper(good)),
            ("truncated", good[..(good.Length / 2)]),
            ("header-only", "GKP1"u8.ToArray()),
            ("not-a-blob", "GKP1????"u8.ToArray()),
            ("empty", Array.Empty<byte>()),
            ("alien-header", Encoding.ASCII.GetBytes("XXXX and some payload bytes")),
        };

        foreach (var (name, bytes) in damaged)
        {
            File.WriteAllBytes(p.KeyFilePath, bytes);
            var writtenAt = File.GetLastWriteTimeUtc(p.KeyFilePath);

            Assert.Throws<KeyStoreCorruptException>(() => New().GetOrCreateKey());

            Assert.True(File.Exists(p.KeyFilePath), name + ": key file was deleted");
            Assert.Equal(bytes, File.ReadAllBytes(p.KeyFilePath));
            Assert.Equal(writtenAt, File.GetLastWriteTimeUtc(p.KeyFilePath));
            Assert.Single(Directory.GetFiles(_dir)); // and nothing new was written beside it
        }

        static byte[] Tamper(byte[] source)
        {
            var copy = (byte[])source.Clone();
            copy[^3] ^= 0xFF;
            return copy;
        }
    }

    // ---- W1: stale temp-file reaping (review finding Q9) ----

    [Fact]
    public void StaleTempFile_FromACrashedCreate_IsReaped()
    {
        Directory.CreateDirectory(_dir);
        var stale = Path.Combine(_dir, DpapiKeyProvider.FileName + "." + Guid.NewGuid().ToString("N") + ".tmp");
        File.WriteAllBytes(stale, new byte[64]);
        File.SetLastWriteTimeUtc(stale, DateTime.UtcNow.AddHours(-2));

        New().GetOrCreateKey();

        Assert.False(File.Exists(stale));
        Assert.Single(Directory.GetFiles(_dir)); // only the key file remains
    }

    [Fact]
    public void FreshTempFile_PossiblyALiveCreator_IsLeftAlone()
    {
        Directory.CreateDirectory(_dir);
        var fresh = Path.Combine(_dir, DpapiKeyProvider.FileName + "." + Guid.NewGuid().ToString("N") + ".tmp");
        File.WriteAllBytes(fresh, new byte[64]);

        New().GetOrCreateKey();

        Assert.True(File.Exists(fresh));
    }

    [Fact]
    public void Reaping_NeverTouchesTheKeyFileOrForeignFiles()
    {
        var p = New();
        var key = p.GetOrCreateKey();
        File.SetLastWriteTimeUtc(p.KeyFilePath, DateTime.UtcNow.AddYears(-1)); // old, but never a reap target

        var foreign = Path.Combine(_dir, DpapiKeyProvider.FileName + ".backup.tmp"); // matches the glob, not the pattern
        File.WriteAllBytes(foreign, new byte[8]);
        File.SetLastWriteTimeUtc(foreign, DateTime.UtcNow.AddHours(-2));
        var unrelated = Path.Combine(_dir, "notes.txt");
        File.WriteAllText(unrelated, "keep me");
        File.SetLastWriteTimeUtc(unrelated, DateTime.UtcNow.AddHours(-2));

        Assert.Equal(key, New().GetOrCreateKey());

        Assert.True(File.Exists(p.KeyFilePath));
        Assert.True(File.Exists(foreign));
        Assert.True(File.Exists(unrelated));
    }

    // ---- W1: ACL hardening (review finding Q9) ----

    [Fact]
    public void Create_RestrictsKeyDirectoryToTheCurrentUser()
    {
        var p = New();
        p.GetOrCreateKey();

        var rules = ReadRules(new DirectoryInfo(_dir).GetAccessControl(), out var isProtected);
        Assert.True(isProtected, "inheritance was not removed from the key directory");
        Assert.All(rules, r => Assert.Equal(CurrentUserSid, r.IdentityReference));
        Assert.Contains(rules, r => r.AccessControlType == AccessControlType.Allow
                                    && r.FileSystemRights.HasFlag(FileSystemRights.FullControl));
    }

    [Fact]
    public void KeyFile_InheritsTheRestrictedDirectoryAcl()
    {
        // The file's ACL is not set directly (doing so made a racing reader fail with a sharing
        // violation); it comes from the directory's inheritable rule, which must resolve to the
        // current user alone.
        var p = New();
        p.GetOrCreateKey();

        var rules = ReadRules(new FileInfo(p.KeyFilePath).GetAccessControl(), out _);
        Assert.NotEmpty(rules);
        Assert.All(rules, r => Assert.Equal(CurrentUserSid, r.IdentityReference));
        Assert.Contains(rules, r => r.AccessControlType == AccessControlType.Allow
                                    && r.FileSystemRights.HasFlag(FileSystemRights.Read));
    }

    [Fact]
    public void Hardening_IsIdempotent_AndStillReadsTheSameKey()
    {
        var p = New();
        var key = p.GetOrCreateKey();
        var acl = Describe(_dir);

        Assert.Equal(key, New().GetOrCreateKey()); // a second instance re-applies maintenance
        Assert.Equal(key, New().GetOrCreateKey());
        Assert.Equal(acl, Describe(_dir));
    }

    [Fact]
    public void ExistingUnhardenedDirectory_IsHardenedOnNextUse()
    {
        // A key directory created before this hardening existed (or by another tool) must be fixed up,
        // without the key file being rewritten.
        Directory.CreateDirectory(_dir);
        var inherited = new DirectoryInfo(_dir).GetAccessControl();
        Assert.False(inherited.AreAccessRulesProtected); // precondition: a plain temp dir inherits

        var key = New().GetOrCreateKey();
        var before = File.ReadAllBytes(new DpapiKeyProvider(_dir).KeyFilePath);

        Assert.Equal(key, New().GetOrCreateKey());
        Assert.True(new DirectoryInfo(_dir).GetAccessControl().AreAccessRulesProtected);
        Assert.Equal(before, File.ReadAllBytes(new DpapiKeyProvider(_dir).KeyFilePath));
    }

    // ---- W1: the key lives under the same root as the store (review finding Q9) ----

    [Fact]
    public void KeyFile_LandsUnderTheSameRootAsTheDatabaseAndStorage()
    {
        var paths = GistStoragePaths.ForRoot(Path.Combine(_dir, "root"));
        paths.EnsureCreated();

        var p = new DpapiKeyProvider(paths.KeyDir);
        p.GetOrCreateKey();

        Assert.True(File.Exists(p.KeyFilePath));
        Assert.StartsWith(paths.Root + Path.DirectorySeparatorChar, p.KeyFilePath, StringComparison.Ordinal);
        Assert.Equal(paths.Root, Path.GetDirectoryName(Path.GetDirectoryName(p.KeyFilePath)));
        Assert.Equal(paths.Root, Path.GetDirectoryName(paths.DbPath));
    }

    [Fact]
    public void TwoDifferentRoots_HoldTwoIndependentKeys()
    {
        // A debug build pointed at a scratch root must never read the production root's key.
        var a = GistStoragePaths.ForRoot(Path.Combine(_dir, "prod"));
        var b = GistStoragePaths.ForRoot(Path.Combine(_dir, "debug"));
        a.EnsureCreated();
        b.EnsureCreated();

        var keyA = new DpapiKeyProvider(a.KeyDir).GetOrCreateKey();
        var keyB = new DpapiKeyProvider(b.KeyDir).GetOrCreateKey();

        Assert.NotEqual(keyA, keyB);
        Assert.Equal(keyA, new DpapiKeyProvider(a.KeyDir).GetOrCreateKey());
    }

    // ---- helpers ----

    private static IdentityReference CurrentUserSid => WindowsIdentity.GetCurrent().User!;

    private static List<FileSystemAccessRule> ReadRules(FileSystemSecurity security, out bool isProtected)
    {
        isProtected = security.AreAccessRulesProtected;
        return security.GetAccessRules(includeExplicit: true, includeInherited: true, typeof(SecurityIdentifier))
            .Cast<FileSystemAccessRule>().ToList();
    }

    private static string Describe(string directory) =>
        new DirectoryInfo(directory).GetAccessControl().GetSecurityDescriptorSddlForm(AccessControlSections.Access);
}
