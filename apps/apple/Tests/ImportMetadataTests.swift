import XCTest
@testable import GIST

/// M8 R4: SC-01 encoding prompt, SC-03 tracked-changes notice, SC-06 item
/// info, SC-15 word count / reading time. Real temp-dir `GistCore`, no mocks.
@MainActor
final class ImportMetadataTests: XCTestCase {
    private var tempDir: URL!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ImportMetadataTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        client = CoreClient(
            dbPath: tempDir.appendingPathComponent("gist.sqlite3").path,
            storageDir: tempDir.appendingPathComponent("storage", isDirectory: true).path)
        client.readingWpmProvider = { 250 }
    }

    override func tearDownWithError() throws {
        client = nil
        try? FileManager.default.removeItem(at: tempDir)
        try super.tearDownWithError()
    }

    /// Copies a bundled fixture into the scratch dir (imports read a real path).
    private func fixture(_ name: String, _ ext: String) throws -> URL {
        guard let bundled = Bundle(for: Self.self).url(forResource: name, withExtension: ext) else {
            throw XCTSkip("\(name).\(ext) fixture missing from test bundle resources")
        }
        let dest = tempDir.appendingPathComponent("\(name).\(ext)")
        try FileManager.default.copyItem(at: bundled, to: dest)
        return dest
    }

    // MARK: - SC-01

    func testEncodingChoicesIncludeDetectedAndSelectCaseInsensitively() {
        XCTAssertEqual(TextEncodingChoices.selection(for: "Windows-1251"), "windows-1251")
        let opts = TextEncodingChoices.options(including: "x-weird")
        XCTAssertEqual(opts.first?.label, "x-weird")
        XCTAssertEqual(opts.count, TextEncodingChoices.curated.count + 1)
        XCTAssertEqual(TextEncodingChoices.options(including: "UTF-8").count, TextEncodingChoices.curated.count)
        XCTAssertTrue(TextEncodingChoices.needsPrompt(uncertain: true, lossy: false))
        XCTAssertTrue(TextEncodingChoices.needsPrompt(uncertain: false, lossy: true))
        XCTAssertFalse(TextEncodingChoices.needsPrompt(uncertain: false, lossy: false))
    }

    func testPlainTextFileDetectionByExtension() {
        XCTAssertTrue(TextEncodingChoices.isPlainTextFile(URL(fileURLWithPath: "/a/b.TXT")))
        XCTAssertTrue(TextEncodingChoices.isPlainTextFile(URL(fileURLWithPath: "/a/b.md")))
        XCTAssertFalse(TextEncodingChoices.isPlainTextFile(URL(fileURLWithPath: "/a/b.docx")))
    }

    func testCertainEncodingImportsSilentlyWithoutPrompt() async throws {
        let url = try fixture("utf16le_bom", "txt")
        await client.importFile(url: url)
        XCTAssertNil(client.pendingEncodingImport, "a BOM is a certain encoding")
        XCTAssertEqual(client.items.count, 1)
        XCTAssertNil(client.error)
    }

    func testLossyFilePromptsAndCancelImportsNothing() async throws {
        let url = try fixture("invalid_utf8_mixed", "txt")
        await client.importFile(url: url)
        let pending = try XCTUnwrap(client.pendingEncodingImport, "invalid UTF-8 must prompt")
        XCTAssertEqual(pending.url, url)
        XCTAssertTrue(client.items.isEmpty, "nothing imported while the prompt is open")

        client.pendingEncodingImport = nil  // cancel
        XCTAssertTrue(client.items.isEmpty)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path), "cancel never touches the file")
    }

    func testChosenEncodingImportsAndClearsPrompt() async throws {
        let url = try fixture("invalid_utf8_mixed", "txt")
        await client.importFile(url: url)
        XCTAssertNotNil(client.pendingEncodingImport)

        await client.importTextFile(url: url, encoding: "windows-1252")
        XCTAssertNil(client.pendingEncodingImport)
        XCTAssertNil(client.error)
        XCTAssertEqual(client.items.count, 1)
    }

    func testCyrillicOverrideImports() async throws {
        let url = try fixture("windows1251_russian", "txt")
        let forced = client.detectEncoding(url: url, encoding: "windows-1251")
        XCTAssertEqual(forced?.lossy, false)
        await client.importTextFile(url: url, encoding: "windows-1251")
        XCTAssertEqual(client.items.count, 1)
        XCTAssertGreaterThan(client.items.first?.wordCount ?? 0, 0)
    }

    func testUnknownEncodingLabelIsATypedErrorNotAnImport() async throws {
        let url = try fixture("windows1252_french", "txt")
        await client.importTextFile(url: url, encoding: "not-a-real-encoding")
        XCTAssertNotNil(client.error)
        XCTAssertTrue(client.items.isEmpty)
    }

    // MARK: - SC-03

    func testTrackedChangesDocxPostsNonBlockingNoticeAndFlag() async throws {
        let url = try fixture("tracked_changes_min", "docx")
        await client.importFile(url: url)
        XCTAssertEqual(client.items.count, 1, "the import still succeeds")
        XCTAssertNil(client.error)
        XCTAssertNotNil(client.importNotice)
        let id = try XCTUnwrap(client.items.first?.id)
        XCTAssertEqual(client.itemInfo(itemId: id)?.trackedChanges, true)
        XCTAssertEqual(client.itemInfo(itemId: id)?.sourceType, "docx")
    }

    func testCleanDocxHasNoNotice() async throws {
        let url = try fixture("with_table", "docx")
        await client.importFile(url: url)
        XCTAssertEqual(client.items.count, 1)
        XCTAssertNil(client.importNotice)
    }

    // MARK: - SC-06

    func testSafeLinkOnlyAllowsHttpAndHttps() {
        XCTAssertNotNil(ItemInfoPresentation.safeLink("https://example.com/a"))
        XCTAssertNotNil(ItemInfoPresentation.safeLink(" http://example.com "))
        XCTAssertNil(ItemInfoPresentation.safeLink("javascript:alert(1)"))
        XCTAssertNil(ItemInfoPresentation.safeLink("file:///etc/passwd"))
        XCTAssertNil(ItemInfoPresentation.safeLink("https://"))
        XCTAssertNil(ItemInfoPresentation.safeLink(""))
        XCTAssertNil(ItemInfoPresentation.safeLink(nil))
    }

    func testDisplayDateFormatsIsoAndPassesThroughOthers() {
        XCTAssertNil(ItemInfoPresentation.displayDate(nil))
        XCTAssertNil(ItemInfoPresentation.displayDate("  "))
        XCTAssertEqual(ItemInfoPresentation.displayDate("sometime in spring"), "sometime in spring")
        XCTAssertNotEqual(ItemInfoPresentation.displayDate("2024-01-02"), "2024-01-02")
        XCTAssertNotEqual(ItemInfoPresentation.displayDate("2024-01-02T03:04:05Z"), "2024-01-02T03:04:05Z")
    }

    func testWebDetailsPresence() {
        let none = ItemInfoVM(
            title: "t", author: nil, sourceType: "txt", trackedChanges: false, importDate: nil,
            language: nil, publishedDate: nil, retrievedDate: nil, canonicalUrl: nil)
        XCTAssertFalse(ItemInfoPresentation.hasWebDetails(none))
        let web = ItemInfoVM(
            title: "t", author: "A", sourceType: "web", trackedChanges: false, importDate: nil,
            language: nil, publishedDate: "2024-01-02", retrievedDate: nil, canonicalUrl: nil)
        XCTAssertTrue(ItemInfoPresentation.hasWebDetails(web))
    }

    func testItemInfoForUnknownIdIsNil() {
        XCTAssertNil(client.itemInfo(itemId: "missing"))
    }

    // MARK: - SC-15

    func testReadingTimeFormatting() {
        XCTAssertNil(ReadingTimeFormat.caption(wordCount: 0, seconds: 0))
        XCTAssertNil(ReadingTimeFormat.accessibilityLabel(wordCount: 0, seconds: 0))
        XCTAssertEqual(ReadingTimeFormat.minutes(fromSeconds: 0), 1)
        XCTAssertEqual(ReadingTimeFormat.minutes(fromSeconds: 60), 1)
        XCTAssertEqual(ReadingTimeFormat.minutes(fromSeconds: 61), 2)
        XCTAssertEqual(ReadingTimeFormat.minutes(fromSeconds: -5), 1)
        let caption = ReadingTimeFormat.caption(wordCount: 1234, seconds: 300)
        XCTAssertTrue(caption?.contains("≈ 5 min") == true, "\(caption ?? "nil")")
    }

    func testImportedItemCarriesWordCountAndReadingSeconds() async throws {
        let url = try fixture("windows1252_french", "txt")
        await client.importFile(url: url)
        await client.importTextFile(url: url, encoding: "windows-1252")  // dedupe-safe either way
        let item = try XCTUnwrap(client.items.first)
        XCTAssertGreaterThan(item.wordCount, 0)
        XCTAssertEqual(
            item.readingSeconds,
            Int(try estimateReadingSeconds(wordCount: UInt32(item.wordCount), wpm: 250)))
    }

    func testReadingSecondsFollowTheConfiguredWpm() async throws {
        let url = try fixture("tracked_changes_min", "docx")
        await client.importFile(url: url)
        let slow = try XCTUnwrap(client.items.first)
        client.readingWpmProvider = { 1000 }
        await client.refreshLoadedItems()
        let fast = try XCTUnwrap(client.items.first)
        XCTAssertEqual(slow.wordCount, fast.wordCount)
        XCTAssertLessThanOrEqual(fast.readingSeconds, slow.readingSeconds)
    }
}
