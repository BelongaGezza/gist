import XCTest
@testable import GIST

/// Typed resource-limit errors (M7 R3). Real `GistCore` against a fresh temp
/// dir; all matching is on the typed `GistError` case, never on message text.
@MainActor
final class ImportLimitTests: XCTestCase {
    private var tempDir: URL!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ImportLimitTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        client = CoreClient(
            dbPath: tempDir.appendingPathComponent("gist.sqlite3").path,
            storageDir: tempDir.appendingPathComponent("storage", isDirectory: true).path
        )
    }

    override func tearDownWithError() throws {
        client = nil
        if let tempDir { try? FileManager.default.removeItem(at: tempDir) }
        try super.tearDownWithError()
    }

    /// A sparse file one byte over the default 256 MiB input cap.
    private func makeOversizedTxt() throws -> URL {
        let url = tempDir.appendingPathComponent("SECRET-oversized.txt")
        XCTAssertTrue(FileManager.default.createFile(atPath: url.path, contents: nil))
        let handle = try FileHandle(forWritingTo: url)
        try handle.truncate(atOffset: 256 * 1024 * 1024 + 1)
        try handle.close()
        return url
    }

    func testOversizedTxtSurfacesTypedLimitMessageNotGenericError() async throws {
        let url = try makeOversizedTxt()
        await client.importFile(url: url)

        let message = try XCTUnwrap(client.importLimitMessage)
        XCTAssertTrue(message.contains("SECRET-oversized.txt"), "names the user's own file: \(message)")
        XCTAssertNil(client.error, "a limit rejection must not also raise the generic error alert")
        XCTAssertNil(client.drmProtectedFile)
        XCTAssertNil(client.pdfEncryptedFile)
        XCTAssertNil(client.pdfOcrCandidate)
        XCTAssertFalse(message.contains(tempDir.path), "no filesystem path may reach the alert")
    }

    func testNextImportClearsPreviousLimitMessage() async throws {
        await client.importFile(url: try makeOversizedTxt())
        XCTAssertNotNil(client.importLimitMessage)

        let ok = tempDir.appendingPathComponent("ok.txt")
        try "A short, perfectly fine document.".write(to: ok, atomically: true, encoding: .utf8)
        await client.importFile(url: ok)
        XCTAssertNil(client.importLimitMessage)
        XCTAssertNil(client.error)
    }

    func testNonLimitUrlFailureKeepsGenericErrorPath() async {
        await client.importUrl(urlString: "http://example.com/not-https")
        XCTAssertNil(client.importLimitMessage)
        XCTAssertNotNil(client.error)
    }

    func testEveryLimitCaseHasADistinctSpecificMessage() {
        // Every flat `GistError` case carries the Rust `#[error]` text as
        // `message`; the UI must never depend on it, so pass an empty one.
        let m = ""
        let cases: [GistError] = [
            .ResourceLimitTooLarge(message: m), .ResourceLimitTooManyPages(message: m),
            .ResourceLimitTooManyEntries(message: m), .ResourceLimitTooDeeplyNested(message: m),
            .ResourceLimitContentTooLarge(message: m), .ResourceLimitTableTooLarge(message: m),
            .ResourceLimitOther(message: m),
        ]
        var seen = Set<String>()
        for error in cases {
            let message = ImportLimitMessage.message(for: error, name: "Book.pdf")
            XCTAssertNotNil(message, "\(error)")
            XCTAssertTrue(message?.contains("Book.pdf") ?? false)
            XCTAssertTrue(seen.insert(message ?? "").inserted, "duplicate message for \(error)")
        }
        XCTAssertTrue(
            ImportLimitMessage.message(for: .ResourceLimitTooManyPages(message: ""), name: "x")?.contains("pages") ?? false)
    }

    func testNonLimitErrorsHaveNoLimitMessage() {
        let others: [GistError] = [
            .Core(message: "boom"), .DrmProtected(message: ""), .ChecksumMismatch(message: "p"),
            .PdfEncrypted(message: ""), .PdfNoTextLayer(message: ""), .PdfUnavailable(message: ""),
            .InternalPanic(message: ""),
        ]
        for error in others {
            XCTAssertNil(ImportLimitMessage.message(for: error, name: "x"), "\(error)")
        }
    }
}
