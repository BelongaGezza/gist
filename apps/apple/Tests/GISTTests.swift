import XCTest
import CryptoKit
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
