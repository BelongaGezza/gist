import XCTest
@testable import GIST

/// PDF import (M6 R1) against a real `GistCore` and the real, bundled
/// `libpdfium.dylib`. The test host is `GIST.app`, so the Rust core resolves
/// the library at `GIST.app/Contents/Frameworks/libpdfium.dylib` exactly as
/// it does in the shipped app -- a passing text-PDF import here proves the
/// embedded dylib is present, loadable, and wired end-to-end (not just that
/// the Rust unit tests pass against `artifacts/pdfium`).
final class PdfImportTests: XCTestCase {
    private var tempDir: URL!
    private var core: GistCore!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PdfImportTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        core = try GistCore(
            dbPath: tempDir.appendingPathComponent("gist.sqlite3").path,
            storageDir: tempDir.appendingPathComponent("storage", isDirectory: true).path
        )
    }

    override func tearDownWithError() throws {
        core = nil
        if let tempDir { try? FileManager.default.removeItem(at: tempDir) }
        try super.tearDownWithError()
    }

    private func fixturePath(_ name: String) throws -> String {
        guard let url = Bundle(for: Self.self).url(forResource: name, withExtension: "pdf") else {
            throw XCTSkip("\(name).pdf missing from test bundle resources")
        }
        return url.path
    }

    func testEmbeddedPdfiumIsInTheHostAppBundle() throws {
        let dylib = Bundle.main.bundleURL
            .appendingPathComponent("Contents/Frameworks/libpdfium.dylib")
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: dylib.path),
            "libpdfium.dylib must be embedded in the app bundle (project.yml dependency)"
        )
    }

    func testTextPdfImportsThroughTheBundledLibrary() throws {
        let id = try core.importFile(path: try fixturePath("pdf_plain_text"))
        XCTAssertFalse(id.isEmpty)
        let hits = try core.searchItems(query: "rhythm", limit: 10)
        XCTAssertTrue(hits.contains { $0.id == id })
    }

    func testEncryptedPdfThrowsTypedPdfEncrypted() throws {
        XCTAssertThrowsError(try core.importFile(path: try fixturePath("pdf_encrypted"))) { error in
            guard case GistError.PdfEncrypted = error else {
                return XCTFail("expected GistError.PdfEncrypted, got \(error)")
            }
        }
    }

    func testImageOnlyPdfThrowsTypedPdfNoTextLayer() throws {
        XCTAssertThrowsError(try core.importFile(path: try fixturePath("pdf_image_only"))) { error in
            guard case GistError.PdfNoTextLayer = error else {
                return XCTFail("expected GistError.PdfNoTextLayer, got \(error)")
            }
        }
    }
}
