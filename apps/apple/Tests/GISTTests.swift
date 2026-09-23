import XCTest
import CryptoKit
import Security
@testable import GIST

/// Real (not mocked) integration tests for `CoreClient`, run against a real
/// `GistCore` (SQLite + filesystem storage) pointed at a fresh scratch
/// directory per test. This mirrors how `gist-core`'s own Rust tests work —
/// no mocking framework, just a real temp-dir-backed instance — rather than
/// introducing a mock layer GIST doesn't otherwise use.
///
/// `CoreClient(dbPath:storageDir:)` is a test-only, additive construction
/// path (see `CoreClient.swift`); `.shared`'s production initializer and
/// call sites are untouched.
@MainActor
final class GISTTests: XCTestCase {
    private var tempDir: URL!
    private var storageDir: String!
    private var client: CoreClient!
    /// (service, account) pairs of test-scoped Keychain items created via
    /// `testKeyProvider()` -- deleted in `tearDownWithError` so nothing
    /// test-related is left behind in the real Keychain, mirroring
    /// `KeychainKeyProviderIntegrationTests`'s cleanup.
    private var createdKeychainItems: [(service: String, account: String)] = []

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("GISTTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        let dbPath = tempDir.appendingPathComponent("gist.sqlite3").path
        storageDir = tempDir.appendingPathComponent("storage", isDirectory: true).path
        client = CoreClient(dbPath: dbPath, storageDir: storageDir)
    }

    override func tearDownWithError() throws {
        client = nil
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
        tempDir = nil
        storageDir = nil
        for item in createdKeychainItems {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: item.service,
                kSecAttrAccount as String: item.account,
            ]
            SecItemDelete(query as CFDictionary)
        }
        createdKeychainItems = []
        try super.tearDownWithError()
    }

    // MARK: - Fixture helpers

    /// Copies the `basic_ascii.txt` fixture (bundled into the test target as
    /// a Copy Bundle Resources entry — see `project.yml`'s `GISTTests`
    /// target — rather than referenced by a relative path into the
    /// top-level `fixtures/` directory, which isn't guaranteed to resolve
    /// from the test bundle's runtime working directory) to a fresh
    /// per-test scratch path, so it can be imported as if user-selected.
    private func importableFixtureURL(function: String = #function) throws -> URL {
        guard let bundled = Bundle(for: Self.self).url(forResource: "basic_ascii", withExtension: "txt") else {
            throw XCTSkip("basic_ascii.txt fixture missing from test bundle resources")
        }
        let sanitizedName = function.filter(\.isLetter)
        let destination = tempDir.appendingPathComponent("fixture-\(sanitizedName).txt")
        try FileManager.default.copyItem(at: bundled, to: destination)
        return destination
    }

    /// Independently predicts the path of the ADR-006 sandboxed copy
    /// `gist-store::Store::store_original_copy` writes for `sourceURL`'s
    /// content: `<storageDir>/originals/<sha256-hex>.<ext>`. There is no FFI
    /// accessor for `source_copy_path` (`FfiLibraryItem` only exposes the
    /// original `source_path`), so this recomputes the same content-addressed
    /// filename gist-store uses, to assert on the copy's existence directly
    /// on disk without touching any Rust code.
    private func predictedSandboxedCopyPath(forContentAt sourceURL: URL) throws -> URL {
        let data = try Data(contentsOf: sourceURL)
        let digest = SHA256.hash(data: data)
        let hash = digest.map { String(format: "%02x", $0) }.joined()
        let ext = sourceURL.pathExtension.lowercased()
        let filename = ext.isEmpty ? hash : "\(hash).\(ext)"
        return URL(fileURLWithPath: storageDir)
            .appendingPathComponent("originals", isDirectory: true)
            .appendingPathComponent(filename)
    }

    // MARK: - Import + refresh

    func testImportFileAppearsInItemsAfterRefresh() async throws {
        let fixture = try importableFixtureURL()
        XCTAssertTrue(client.items.isEmpty, "library should start empty against a fresh scratch db")

        await client.importFile(url: fixture)

        XCTAssertNil(client.error)
        XCTAssertEqual(client.items.count, 1)
        XCTAssertEqual(client.items.first?.sourcePath, fixture.path)
    }

    /// Regression test for the bug `PENDING_APPLE_CHANGES.md`'s 2026-09-21
    /// entry flagged: `importFile`/`removeItems`/`importUrl`/`encryptItems`
    /// all set `error` on failure, then unconditionally called a trailing
    /// `refresh()` whose *success* path cleared `error` again -- wiping a
    /// real failure before the UI ever showed it. The Windows port found and
    /// fixed the identical shape (`RefreshAsync`/`ReloadItemsAsync`);
    /// mirrored here as `CoreClient.refresh()`/`reloadItems(clearErrorOnSuccess:)`.
    func testImportFailureErrorSurvivesTheFollowUpRefresh() async throws {
        let missingFile = tempDir.appendingPathComponent("does-not-exist.txt")
        XCTAssertFalse(FileManager.default.fileExists(atPath: missingFile.path))

        await client.importFile(url: missingFile)

        XCTAssertNotNil(client.error, "a failed import must still report an error after its trailing refresh")
        XCTAssertTrue(client.items.isEmpty)
    }

    // MARK: - Search

    func testSearchFindsImportedItemByContent() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        XCTAssertEqual(client.items.count, 1)

        // "adipiscing" appears exactly once in basic_ascii.txt's first
        // paragraph ("consectetur adipiscing elit").
        await client.search(query: "adipiscing")

        XCTAssertNil(client.error)
        XCTAssertEqual(client.searchResults.count, 1)
        XCTAssertEqual(client.searchResults.first?.id, client.items.first?.id)
    }

    func testEmptyOrWhitespaceQueryClearsSearchResultsWithoutRoundTrip() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        await client.search(query: "adipiscing")
        XCTAssertFalse(client.searchResults.isEmpty, "sanity check: search must have found something first")

        await client.search(query: "   ")

        XCTAssertTrue(client.searchResults.isEmpty, "whitespace-only query should clear searchResults")
    }

    // MARK: - Remove (ADR-006 / F-A5: sandboxed copy vs. original file)

    func testRemoveItemsWithDeleteSourceFilesDeletesSandboxedCopyButNeverTheOriginal() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }

        let copyPath = try predictedSandboxedCopyPath(forContentAt: fixture)
        XCTAssertTrue(FileManager.default.fileExists(atPath: copyPath.path), "ADR-006 sandboxed copy should exist after import")
        XCTAssertTrue(FileManager.default.fileExists(atPath: fixture.path), "original file should exist before removal")

        await client.removeItems(ids: [item.id], deleteSourceFiles: true)

        XCTAssertNil(client.error)
        XCTAssertTrue(client.items.isEmpty, "item should be gone from the library after removal")
        XCTAssertFalse(
            FileManager.default.fileExists(atPath: copyPath.path),
            "deleteSourceFiles: true should delete GIST's own sandboxed ADR-006 copy"
        )
        // Security-relevant: the user's real, original file must NEVER be
        // deleted by GIST, regardless of deleteSourceFiles. A failure here
        // would be a real regression against ADR-006 / F-A5, not busywork.
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: fixture.path),
            "the original source file must never be touched by removeItems"
        )
    }

    func testRemoveItemsWithoutDeleteSourceFilesKeepsSandboxedCopyAndOriginal() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }

        let copyPath = try predictedSandboxedCopyPath(forContentAt: fixture)
        XCTAssertTrue(FileManager.default.fileExists(atPath: copyPath.path))

        await client.removeItems(ids: [item.id], deleteSourceFiles: false)

        XCTAssertNil(client.error)
        XCTAssertTrue(client.items.isEmpty)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: copyPath.path),
            "deleteSourceFiles: false should leave GIST's sandboxed copy in place"
        )
        XCTAssertTrue(FileManager.default.fileExists(atPath: fixture.path))
    }

    /// Covers the 2026-09-21 ADR-006 addendum's shared-copy reference check,
    /// adopted automatically through `gist-core` with no Swift change (see
    /// `PENDING_APPLE_CHANGES.md`'s `feat/w2-complete-delete` entry — this is
    /// the "add a Swift test mirroring the shared-copy case" action item).
    /// Two imports of byte-identical content dedup to one ADR-006 stored
    /// copy; removing one item must not delete a copy the other surviving
    /// item still references, but must delete it once the last referencing
    /// item is removed.
    func testRemoveItemsKeepsSharedCopyUntilLastReferencingItemIsRemoved() async throws {
        let sourceContent = try importableFixtureURL()
        let firstImport = tempDir.appendingPathComponent("shared-copy-a.txt")
        let secondImport = tempDir.appendingPathComponent("shared-copy-b.txt")
        try FileManager.default.copyItem(at: sourceContent, to: firstImport)
        try FileManager.default.copyItem(at: sourceContent, to: secondImport)

        await client.importFile(url: firstImport)
        await client.importFile(url: secondImport)
        XCTAssertNil(client.error)
        XCTAssertEqual(client.items.count, 2, "both imports should have succeeded")

        let copyPath = try predictedSandboxedCopyPath(forContentAt: firstImport)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: copyPath.path),
            "identical content should dedup to one ADR-006 stored copy"
        )

        guard let firstItem = client.items.first(where: { $0.sourcePath == firstImport.path }) else {
            XCTFail("expected the first import to be present")
            return
        }
        await client.removeItems(ids: [firstItem.id], deleteSourceFiles: true)

        XCTAssertNil(client.error)
        XCTAssertEqual(client.items.count, 1, "the second item, still referencing the shared copy, must survive")
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: copyPath.path),
            "the shared copy must survive removal while another item still references it"
        )

        guard let secondItem = client.items.first else {
            XCTFail("expected the second import to remain")
            return
        }
        await client.removeItems(ids: [secondItem.id], deleteSourceFiles: true)

        XCTAssertNil(client.error)
        XCTAssertTrue(client.items.isEmpty)
        XCTAssertFalse(
            FileManager.default.fileExists(atPath: copyPath.path),
            "the shared copy should be deleted once the last referencing item is removed"
        )
        // Neither user-facing original file is ever touched, regardless of
        // dedup/reference-count bookkeeping on GIST's own sandboxed copy.
        XCTAssertTrue(FileManager.default.fileExists(atPath: firstImport.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: secondImport.path))
    }

    // MARK: - Per-item encryption (ADR-014)

    /// Reuses `KeychainKeyProvider`'s `init(service:account:)` test seam
    /// (added for `KeychainKeyProviderIntegrationTests`) so this test never
    /// touches the real production Keychain item -- a UUID-suffixed service
    /// name can never collide with `"com.gist.macos.encryption-at-rest"`.
    private func testKeyProvider() -> KeychainKeyProvider {
        let service = "com.gist.macos.encryption-at-rest.TEST-\(UUID().uuidString)"
        let account = "document-content-key-TEST"
        createdKeychainItems.append((service: service, account: account))
        return KeychainKeyProvider(service: service, account: account)
    }

    /// Uses `client`, the suite's default `CoreClient(dbPath:storageDir:)`
    /// -- deliberately genuinely keyless, NOT the shape `.shared` actually
    /// uses in production (see `testEncryptItemsThenReadBackSucceedsOnAReadCapableClient`
    /// below for that). This still correctly documents that a genuinely
    /// keyless `GistCore` can't decrypt content it encrypts.
    func testEncryptItemsEncryptsThenSecondCallIsIdempotentNoOp() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }
        XCTAssertFalse(item.contentEncrypted, "freshly imported item must start unencrypted")

        let keyProvider = testKeyProvider()

        let summary = await client.encryptItems(ids: [item.id], keyProvider: keyProvider)
        XCTAssertNil(client.error)
        XCTAssertEqual(summary.encryptedCount, 1)
        XCTAssertEqual(summary.alreadyEncryptedCount, 0)
        XCTAssertEqual(summary.failedCount, 0)

        // `refresh()` (called internally by `encryptItems`) must reflect the
        // new flag -- this is SQL metadata, readable even though this
        // client's own GistCore has no key provider of its own (see
        // `CoreClient.encryptItems`'s doc comment on that consequence).
        XCTAssertEqual(client.items.count, 1)
        XCTAssertTrue(client.items.first?.contentEncrypted ?? false)

        // Second call with the same key provider: safe, idempotent no-op.
        let summary2 = await client.encryptItems(ids: [item.id], keyProvider: keyProvider)
        XCTAssertNil(client.error)
        XCTAssertEqual(summary2.encryptedCount, 0)
        XCTAssertEqual(summary2.alreadyEncryptedCount, 1)
        XCTAssertEqual(summary2.failedCount, 0)
    }

    /// **Closes the read-after-encrypt gap (ADR-014) -- this is the test
    /// that matches `CoreClient.shared`'s actual real-world configuration.**
    /// Builds a `CoreClient` via `init(dbPath:storageDir:keyProvider:)`
    /// (the `newWithReadKey`-backed constructor `.shared`'s production
    /// `init()` itself now uses), encrypts a freshly-imported item through
    /// it, and confirms `loadDocument`/`startRsvp` -- the two real
    /// content-reading call sites the app's Flow View and RSVP screens use
    /// -- both succeed afterward through that SAME `CoreClient` instance,
    /// with no `client.error` set and the decoded content intact. Before
    /// the fix this documents, both calls failed with `MissingKeyProvider`.
    func testEncryptItemsThenReadBackSucceedsOnAReadCapableClient() async throws {
        let keyProvider = testKeyProvider()
        let readCapableClient = CoreClient(
            dbPath: tempDir.appendingPathComponent("read-capable.sqlite3").path,
            storageDir: tempDir.appendingPathComponent("read-capable-storage", isDirectory: true).path,
            keyProvider: keyProvider
        )

        let fixture = try importableFixtureURL()
        await readCapableClient.importFile(url: fixture)
        guard let item = readCapableClient.items.first else {
            XCTFail("expected an imported item")
            return
        }
        XCTAssertFalse(item.contentEncrypted, "a read-capable client must still import as plaintext by default")

        let summary = await readCapableClient.encryptItems(ids: [item.id], keyProvider: keyProvider)
        XCTAssertNil(readCapableClient.error)
        XCTAssertEqual(summary.encryptedCount, 1)
        XCTAssertTrue(readCapableClient.items.first?.contentEncrypted ?? false)

        // The gap this test closes: reading the just-encrypted item's
        // actual content back through the SAME client instance.
        let document = await readCapableClient.loadDocument(itemId: item.id)
        XCTAssertNil(readCapableClient.error, "loadDocument must succeed after encrypt on a read-capable client")
        XCTAssertNotNil(document, "document content must be decodable after encrypt-then-read")

        let session = await readCapableClient.startRsvp(itemId: item.id, wpm: 300)
        XCTAssertNil(readCapableClient.error, "startRsvp must succeed after encrypt on a read-capable client")
        XCTAssertNotNil(session, "RSVP session must be buildable after encrypt-then-read")
    }

    /// Security-relevant: the ADR-006 sandboxed original-file copy is
    /// explicitly out of scope for this feature (see ADR-014) -- it must be
    /// byte-for-byte untouched by `encryptItems`.
    func testEncryptItemsNeverTouchesTheSandboxedOriginalCopy() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }

        let copyPath = try predictedSandboxedCopyPath(forContentAt: fixture)
        let beforeBytes = try Data(contentsOf: copyPath)

        _ = await client.encryptItems(ids: [item.id], keyProvider: testKeyProvider())

        let afterBytes = try Data(contentsOf: copyPath)
        XCTAssertEqual(beforeBytes, afterBytes, "encryptItems must never touch the ADR-006 sandboxed copy")
    }

    // MARK: - Collections

    func testCreateListAddRemoveCollectionRoundTrips() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }

        XCTAssertTrue(client.collections.isEmpty)
        await client.createCollection(name: "Test Collection")

        XCTAssertNil(client.error)
        XCTAssertEqual(client.collections.count, 1)
        guard let collection = client.collections.first else {
            XCTFail("expected a created collection")
            return
        }
        XCTAssertEqual(collection.name, "Test Collection")

        await client.addItemToCollection(itemId: item.id, collectionId: collection.id)
        XCTAssertNil(client.error)

        await client.removeItemFromCollection(itemId: item.id, collectionId: collection.id)
        XCTAssertNil(client.error)
    }

    // MARK: - Tags

    func testAddListRemoveTagRoundTrips() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }

        var tags = await client.listTagsForItem(itemId: item.id)
        XCTAssertTrue(tags.isEmpty)

        await client.addTag(itemId: item.id, tagName: "favorites")
        tags = await client.listTagsForItem(itemId: item.id)
        XCTAssertEqual(tags, ["favorites"])

        await client.removeTag(itemId: item.id, tagName: "favorites")
        tags = await client.listTagsForItem(itemId: item.id)
        XCTAssertTrue(tags.isEmpty)
    }

    // MARK: - Tag-based filtering

    func testListAllTagsAndListItemsByTagRoundTrip() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let item = client.items.first else {
            XCTFail("expected an imported item")
            return
        }

        await client.listAllTags()
        XCTAssertTrue(client.allTags.isEmpty)

        await client.addTag(itemId: item.id, tagName: "favorites")
        await client.listAllTags()
        XCTAssertEqual(client.allTags, ["favorites"])

        let tagged = await client.listItemsByTag(tagName: "favorites")
        XCTAssertEqual(tagged.map(\.id), [item.id])

        let untagged = await client.listItemsByTag(tagName: "nonexistent")
        XCTAssertTrue(untagged.isEmpty)
    }
}

/// `LibraryFiltering` is pure logic factored out of `LibraryView` precisely
/// so it can be tested here without standing up SwiftUI view state (a
/// view's `@State` can't be set from outside the view, and this pass
/// deliberately does not build a full XCUITest target for that).
final class LibraryFilteringTests: XCTestCase {
    func testIsSearchActiveIsFalseForEmptyOrWhitespaceOnlyText() {
        XCTAssertFalse(LibraryFiltering.isSearchActive(searchText: ""))
        XCTAssertFalse(LibraryFiltering.isSearchActive(searchText: "   "))
    }

    func testIsSearchActiveIsTrueForNonEmptyText() {
        XCTAssertTrue(LibraryFiltering.isSearchActive(searchText: "abc"))
        XCTAssertTrue(LibraryFiltering.isSearchActive(searchText: "  abc  "))
    }

    func testDisplayedItemsReturnsFullListWhenSearchInactive() {
        let items = [LibraryItemVM(id: "1", title: "A", authors: [], sourcePath: nil)]
        let searchResults = [LibraryItemVM(id: "2", title: "B", authors: [], sourcePath: nil)]

        let result = LibraryFiltering.displayedItems(searchText: "", items: items, searchResults: searchResults)

        XCTAssertEqual(result.map(\.id), ["1"])
    }

    func testDisplayedItemsReturnsSearchResultsWhenSearchActive() {
        let items = [LibraryItemVM(id: "1", title: "A", authors: [], sourcePath: nil)]
        let searchResults = [LibraryItemVM(id: "2", title: "B", authors: [], sourcePath: nil)]

        let result = LibraryFiltering.displayedItems(searchText: "query", items: items, searchResults: searchResults)

        XCTAssertEqual(result.map(\.id), ["2"])
    }

    func testLibrarySelectionTitles() {
        XCTAssertEqual(LibrarySelection.all.title, "Library")

        let collection = CollectionVM(id: "c1", name: "Sci-Fi", createdAt: 0)
        XCTAssertEqual(LibrarySelection.collection(collection).title, "Sci-Fi")
    }

    /// `LibrarySelection`'s `Hashable`/`Equatable` conformance is synthesized
    /// via `CollectionVM`'s own conformance, which compares *every* stored
    /// field -- not just `id`. That matters because `SidebarView`'s
    /// `List(selection:)` matches selection state against row `.tag(...)`
    /// values by equality: if this ever silently degraded to an id-only
    /// comparison (e.g. by hand-rolling `Equatable` on `CollectionVM` later),
    /// a renamed collection could compare equal to its stale pre-rename
    /// value and the sidebar could show wrong content. Two collections
    /// sharing an id but differing in name should NOT compare equal today.
    func testLibrarySelectionEqualityIsComponentWise() {
        let a = CollectionVM(id: "same-id", name: "A", createdAt: 1)
        let b = CollectionVM(id: "same-id", name: "B", createdAt: 1)

        XCTAssertNotEqual(LibrarySelection.collection(a), LibrarySelection.collection(b))
        XCTAssertEqual(LibrarySelection.collection(a), LibrarySelection.collection(a))
        XCTAssertNotEqual(LibrarySelection.all, LibrarySelection.collection(a))
    }

    func testLibrarySelectionUsableAsSetElement() {
        // Sanity check that Hashable actually works end-to-end (not just
        // Equatable), since SidebarView relies on hashing for List(selection:).
        let collection = CollectionVM(id: "c1", name: "Sci-Fi", createdAt: 0)
        let selections: Set<LibrarySelection> = [.all, .collection(collection), .all]
        XCTAssertEqual(selections.count, 2)
    }

    func testTagEditorTargetIdentity() {
        // .sheet(item:) identifies the presented sheet by `id` alone, so two
        // targets sharing an id must be treated as "the same sheet" even if
        // other fields differ.
        let t1 = TagEditorTarget(id: "item-1", title: "Foo")
        let t2 = TagEditorTarget(id: "item-1", title: "Bar")
        XCTAssertEqual(t1.id, t2.id)
        XCTAssertNotEqual(t1, t2)
    }

    // MARK: - Sorting

    private static let unsorted: [LibraryItemVM] = [
        LibraryItemVM(id: "1", title: "Banana", authors: ["Zeta"], sourcePath: nil),
        LibraryItemVM(id: "2", title: "apple", authors: ["Alpha"], sourcePath: nil),
        LibraryItemVM(id: "3", title: "Cherry", authors: [], sourcePath: nil),
    ]

    /// `.dateAddedNewest` must be a true no-op (not merely "looks the same
    /// today") since every FFI list call already returns newest-first --
    /// re-sorting here would be redundant work at best and silently wrong if
    /// a caller's array ever isn't already ordered that way.
    func testSortedDateAddedNewestPreservesOriginalOrder() {
        let result = LibraryFiltering.sorted(Self.unsorted, by: .dateAddedNewest)
        XCTAssertEqual(result.map(\.id), ["1", "2", "3"])
    }

    func testSortedDateAddedOldestReversesOrder() {
        let result = LibraryFiltering.sorted(Self.unsorted, by: .dateAddedOldest)
        XCTAssertEqual(result.map(\.id), ["3", "2", "1"])
    }

    func testSortedTitleAZIsCaseInsensitive() {
        let result = LibraryFiltering.sorted(Self.unsorted, by: .titleAZ)
        // "apple" (lowercase) must sort with "Banana"/"Cherry" by letter, not
        // after them by ASCII case.
        XCTAssertEqual(result.map(\.title), ["apple", "Banana", "Cherry"])
    }

    func testSortedTitleZAIsReverseOfTitleAZ() {
        let result = LibraryFiltering.sorted(Self.unsorted, by: .titleZA)
        XCTAssertEqual(result.map(\.title), ["Cherry", "Banana", "apple"])
    }

    func testSortedAuthorAZTreatsNoAuthorAsEmptyString() {
        let result = LibraryFiltering.sorted(Self.unsorted, by: .authorAZ)
        // "" (Cherry, no author) sorts before "Alpha" before "Zeta".
        XCTAssertEqual(result.map(\.id), ["3", "2", "1"])
    }
}
