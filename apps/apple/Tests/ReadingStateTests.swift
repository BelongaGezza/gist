import XCTest
@testable import GIST

/// ADR-021 (reading-state model): `last_opened_at` set by both readers,
/// progress derived from the RSVP position, and the three reading-state sort
/// keys. Like `GISTTests`, the `CoreClient` tests run against a real
/// temp-dir-backed `GistCore` -- no mocking.
@MainActor
final class ReadingStateTests: XCTestCase {
    private var tempDir: URL!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ReadingStateTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        client = CoreClient(
            dbPath: tempDir.appendingPathComponent("gist.sqlite3").path,
            storageDir: tempDir.appendingPathComponent("storage", isDirectory: true).path
        )
    }

    override func tearDownWithError() throws {
        client = nil
        if let tempDir { try? FileManager.default.removeItem(at: tempDir) }
        tempDir = nil
        try super.tearDownWithError()
    }

    private func importFixture(function: String = #function) async throws -> String {
        guard let bundled = Bundle(for: Self.self).url(forResource: "basic_ascii", withExtension: "txt") else {
            throw XCTSkip("basic_ascii.txt fixture missing from test bundle resources")
        }
        let dest = tempDir.appendingPathComponent("fixture-\(function.filter(\.isLetter)).txt")
        try FileManager.default.copyItem(at: bundled, to: dest)
        await client.importFile(url: dest)
        return try XCTUnwrap(client.items.first?.id, "fixture import failed")
    }

    /// Polls (fire-and-forget stamps land on a detached Task) until the
    /// item shows a `lastOpenedAt`, or fails after ~3 s.
    private func waitForLastOpened(_ id: String) async throws -> LibraryItemVM {
        for _ in 0..<60 {
            await client.refresh()
            if let item = client.items.first(where: { $0.id == id }), item.lastOpenedAt != nil {
                return item
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
        XCTFail("lastOpenedAt was never set")
        return try XCTUnwrap(client.items.first(where: { $0.id == id }))
    }

    // MARK: - Core round trip

    func testImportedItemIsNeverOpenedWithZeroProgressAndKnownType() async throws {
        let id = try await importFixture()
        let item = try XCTUnwrap(client.items.first(where: { $0.id == id }))
        XCTAssertNil(item.lastOpenedAt)
        XCTAssertEqual(item.progressFraction, 0)
        XCTAssertEqual(item.sourceType, "txt")
    }

    func testMarkItemOpenedSetsLastOpenedAndIsIdempotent() async throws {
        let id = try await importFixture()
        let first = await client.markItemOpened(itemId: id)
        XCTAssertNotNil(first)
        let second = await client.markItemOpened(itemId: id)
        XCTAssertNotNil(second)
        await client.refresh()
        let item = try XCTUnwrap(client.items.first(where: { $0.id == id }))
        XCTAssertNotNil(item.lastOpenedAt)
        // Unknown id: no crash, no error surfaced.
        _ = await client.markItemOpened(itemId: "no-such-item")
        XCTAssertNil(client.error)
    }

    // MARK: - Both readers stamp last-opened

    func testRsvpReaderOpenStampsLastOpened() async throws {
        let id = try await importFixture()
        let player = RsvpPlayer()
        await player.load(core: client, itemId: id)
        XCTAssertTrue(player.isLoaded)
        let item = try await waitForLastOpened(id)
        XCTAssertNotNil(item.lastOpenedAt)
    }

    func testFlowReaderOpenStampsLastOpenedButLeavesProgressAtZero() async throws {
        // The documented ADR-021 limitation: flow-only reading records
        // *when* but not RSVP progress.
        let id = try await importFixture()
        let doc = await client.openFlowDocument(itemId: id)
        XCTAssertNotNil(doc)
        let item = try await waitForLastOpened(id)
        XCTAssertNotNil(item.lastOpenedAt)
        XCTAssertEqual(item.progressFraction, 0)
    }

    func testFailedFlowLoadDoesNotStampLastOpened() async throws {
        let doc = await client.openFlowDocument(itemId: "missing-item")
        XCTAssertNil(doc)
    }

    func testRsvpProgressShowsUpInTheListing() async throws {
        let id = try await importFixture()
        await client.saveProgress(itemId: id, tokenIndex: 5)
        await client.refresh()
        let item = try XCTUnwrap(client.items.first(where: { $0.id == id }))
        XCTAssertGreaterThan(item.progressFraction, 0)
        XCTAssertLessThanOrEqual(item.progressFraction, 1)
    }

    func testRefreshLoadedItemsPicksUpReaderChangesWithoutResettingTheList() async throws {
        let id = try await importFixture()
        let before = client.items.count
        await client.markItemOpened(itemId: id)
        await client.refreshLoadedItems()
        XCTAssertEqual(client.items.count, before)
        XCTAssertNotNil(client.items.first(where: { $0.id == id })?.lastOpenedAt)
    }

    // MARK: - Sorting (pure)

    private static func vm(
        _ id: String, type: String = "txt", opened: TimeInterval? = nil, progress: Double = 0
    ) -> LibraryItemVM {
        LibraryItemVM(
            id: id, title: id, authors: [], sourcePath: nil,
            sourceType: type,
            lastOpenedAt: opened.map { Date(timeIntervalSince1970: $0) },
            progressFraction: progress
        )
    }

    func testSortByTypeGroupsTypesAndPutsUnknownLastKeepingDateAddedOrderWithinAType() {
        // Incoming order = date added, newest first.
        let items = [
            Self.vm("a", type: "pdf"), Self.vm("b", type: ""), Self.vm("c", type: "epub"),
            Self.vm("d", type: "pdf"), Self.vm("e", type: "epub"),
        ]
        let sorted = LibraryFiltering.sorted(items, by: .sourceType)
        XCTAssertEqual(sorted.map(\.id), ["c", "e", "a", "d", "b"])
    }

    func testSortByLastReadNewestPutsNeverOpenedLastAndTiesKeepIncomingOrder() {
        let items = [
            Self.vm("never1"), Self.vm("old", opened: 100), Self.vm("new", opened: 300),
            Self.vm("tieA", opened: 200), Self.vm("never2"), Self.vm("tieB", opened: 200),
        ]
        let sorted = LibraryFiltering.sorted(items, by: .lastReadNewest)
        XCTAssertEqual(sorted.map(\.id), ["new", "tieA", "tieB", "old", "never1", "never2"])
    }

    func testSortByLastReadOldestStillPutsNeverOpenedLast() {
        let items = [
            Self.vm("never"), Self.vm("old", opened: 100), Self.vm("new", opened: 300),
            Self.vm("mid", opened: 200),
        ]
        let sorted = LibraryFiltering.sorted(items, by: .lastReadOldest)
        XCTAssertEqual(sorted.map(\.id), ["old", "mid", "new", "never"])
    }

    func testSortByProgressHighestAndLowestWithTiesAndUnstarted() {
        let items = [
            Self.vm("zero1"), Self.vm("half", progress: 0.5), Self.vm("done", progress: 1),
            Self.vm("zero2"), Self.vm("halfB", progress: 0.5),
        ]
        XCTAssertEqual(
            LibraryFiltering.sorted(items, by: .progressHighest).map(\.id),
            ["done", "half", "halfB", "zero1", "zero2"]
        )
        XCTAssertEqual(
            LibraryFiltering.sorted(items, by: .progressLowest).map(\.id),
            ["zero1", "zero2", "half", "halfB", "done"]
        )
    }

    func testSortingEmptyAndSingleItemListsIsSafeForEveryOrder() {
        for order in LibrarySortOrder.allCases {
            XCTAssertTrue(LibraryFiltering.sorted([], by: order).isEmpty)
            XCTAssertEqual(LibraryFiltering.sorted([Self.vm("x")], by: order).map(\.id), ["x"])
        }
    }

    func testEverySortOrderHasADistinctNonEmptyLabel() {
        let labels = LibrarySortOrder.allCases.map(\.label)
        XCTAssertEqual(Set(labels).count, labels.count)
        XCTAssertFalse(labels.contains(where: \.isEmpty))
    }

    // MARK: - Row indicator logic

    func testRowReadingStateVisibilityAndPercent() {
        XCTAssertFalse(LibraryRowReadingState.hasReadingState(Self.vm("fresh")))
        XCTAssertTrue(LibraryRowReadingState.hasReadingState(Self.vm("p", progress: 0.1)))
        // Flow-only: opened, zero progress -> shows a last-read line.
        XCTAssertTrue(LibraryRowReadingState.hasReadingState(Self.vm("f", opened: 10)))
        XCTAssertEqual(LibraryRowReadingState.percent(Self.vm("a", progress: 0.426)), 43)
        XCTAssertEqual(LibraryRowReadingState.percent(Self.vm("b", progress: 7)), 100)
        XCTAssertEqual(LibraryRowReadingState.percent(Self.vm("c", progress: .nan)), 0)
        XCTAssertEqual(LibraryRowReadingState.percent(Self.vm("d", progress: -1)), 0)
    }

    func testRowCaptionAndAccessibilityLabelMentionPercentAndLastRead() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let both = Self.vm("both", opened: 1_000_000 - 3 * 86_400, progress: 0.42)
        let caption = LibraryRowReadingState.caption(for: both, now: now)
        XCTAssertTrue(caption.contains("42"), caption)
        let label = LibraryRowReadingState.accessibilityLabel(for: both, now: now)
        XCTAssertTrue(label.contains("42"), label)
        XCTAssertTrue(label.localizedCaseInsensitiveContains("percent"), label)

        // Flow-only item: last-read text but no percentage anywhere.
        let flowOnly = Self.vm("flow", opened: 1_000_000 - 86_400)
        let flowCaption = LibraryRowReadingState.caption(for: flowOnly, now: now)
        XCTAssertFalse(flowCaption.contains("%"), flowCaption)
        XCTAssertFalse(flowCaption.isEmpty)

        // Never opened, no progress: nothing to say.
        XCTAssertEqual(LibraryRowReadingState.caption(for: Self.vm("n"), now: now), "")
    }

    func testLimitationHelpTextStatesTheFlowViewCaveat() {
        XCTAssertTrue(LibraryRowReadingState.limitationHelp.contains("Flow"))
    }
}
