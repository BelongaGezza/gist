import AppKit
import PDFKit
import XCTest
@testable import GIST

/// PDF import UI + OCR-fallback routing (M6 R2). Real `GistCore` against a
/// fresh temp dir, real bundled pdfium, real PDFKit rendering -- no mocking
/// framework. The one thing not driven here is Vision OCR on real scans:
/// where a test needs recognised text it supplies hand-written
/// `OcrReviewPage` values (the same boundary `OcrImportTests` uses).
@MainActor
final class PdfUiTests: XCTestCase {
    private var tempDir: URL!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PdfUiTests-\(UUID().uuidString)", isDirectory: true)
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

    private func fixture(_ name: String) throws -> URL {
        guard let url = Bundle(for: Self.self).url(forResource: name, withExtension: "pdf") else {
            throw XCTSkip("\(name).pdf missing from test bundle resources")
        }
        return url
    }

    /// A fresh, empty directory to use as `PdfPageRenderer`'s temp root, so
    /// "nothing was created / everything was removed" is directly observable.
    private func makeRenderRoot() throws -> URL {
        let root = tempDir.appendingPathComponent("render-root-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }

    private func entries(in dir: URL) -> [String] {
        (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []
    }

    private func makeBlankPdf(pages: Int) -> PDFDocument {
        let document = PDFDocument()
        let image = NSImage(size: NSSize(width: 120, height: 160), flipped: false) { rect in
            NSColor.white.setFill()
            rect.fill()
            return true
        }
        for i in 0..<pages {
            if let page = PDFPage(image: image) { document.insert(page, at: i) }
        }
        return document
    }

    // MARK: - Import routing through CoreClient (typed errors -> UI state)

    func testTextPdfImportAppearsInLibraryAndOpensInRsvpAndFlow() async throws {
        await client.importFile(url: try fixture("pdf_plain_text"))
        XCTAssertNil(client.error)
        XCTAssertNil(client.pdfEncryptedFile)
        XCTAssertNil(client.pdfOcrCandidate)
        XCTAssertEqual(client.items.count, 1)
        let id = try XCTUnwrap(client.items.first?.id)

        let session = await client.startRsvp(itemId: id, wpm: 300)
        XCTAssertNotNil(session)
        XCTAssertFalse(session?.tokens.isEmpty ?? true)

        let document = await client.loadDocument(itemId: id)
        XCTAssertNotNil(document)
        XCTAssertFalse(document?.sections.isEmpty ?? true)
    }

    func testEncryptedPdfSetsEncryptedStateNotGenericError() async throws {
        let url = try fixture("pdf_encrypted")
        await client.importFile(url: url)
        XCTAssertEqual(client.pdfEncryptedFile, url)
        XCTAssertNil(client.error, "typed PdfEncrypted must not fall through to the generic error alert")
        XCTAssertNil(client.pdfOcrCandidate)
        XCTAssertNil(client.pdfUnavailableFile)
        XCTAssertTrue(client.items.isEmpty)
    }

    func testImageOnlyPdfRoutesToOcrCandidateNotError() async throws {
        let url = try fixture("pdf_image_only")
        await client.importFile(url: url)
        XCTAssertEqual(client.pdfOcrCandidate, url)
        XCTAssertNil(client.error, "a scan is routed to OCR, not reported as a failure")
        XCTAssertNil(client.pdfEncryptedFile)
        XCTAssertTrue(client.items.isEmpty)
    }

    func testNextImportClearsPreviousPdfState() async throws {
        await client.importFile(url: try fixture("pdf_encrypted"))
        XCTAssertNotNil(client.pdfEncryptedFile)
        await client.importFile(url: try fixture("pdf_plain_text"))
        XCTAssertNil(client.pdfEncryptedFile)
        XCTAssertEqual(client.items.count, 1)
    }

    // MARK: - PdfPageRenderer

    func testPixelSizeAppliesDpiAndClampsLongestSide() {
        // US Letter at 200 DPI.
        let letter = PdfPageRenderer.pixelSize(forPointSize: CGSize(width: 612, height: 792))
        XCTAssertEqual(letter.width, 1700, accuracy: 1)
        XCTAssertEqual(letter.height, 2200, accuracy: 1)
        // A huge page is scaled down so the longest side is the cap.
        let huge = PdfPageRenderer.pixelSize(forPointSize: CGSize(width: 14_400, height: 7_200))
        XCTAssertEqual(max(huge.width, huge.height), PdfPageRenderer.maxPixelDimension)
        XCTAssertEqual(huge.width / huge.height, 2, accuracy: 0.01)
        // Degenerate sizes still yield at least 1 pixel.
        let tiny = PdfPageRenderer.pixelSize(forPointSize: CGSize(width: 0.001, height: 0.001))
        XCTAssertGreaterThanOrEqual(tiny.width, 1)
    }

    /// M6 R7: a non-finite media box must never produce a NaN pixel size
    /// (`Int(NaN)` traps in `renderJPEG`).
    func testPixelSizeIsAlwaysFiniteForHostileGeometry() {
        for size in [
            CGSize(width: CGFloat.infinity, height: 100), CGSize(width: CGFloat.nan, height: 100),
            CGSize(width: -CGFloat.infinity, height: CGFloat.infinity), CGSize(width: 1e300, height: 1),
        ] {
            let px = PdfPageRenderer.pixelSize(forPointSize: size)
            XCTAssertTrue(px.width.isFinite && px.height.isFinite, "\(size) -> \(px)")
            XCTAssertGreaterThanOrEqual(px.width, 1)
            XCTAssertLessThanOrEqual(max(px.width, px.height), PdfPageRenderer.maxPixelDimension)
        }
    }

    func testRendersRealImageOnlyPdfToBoundedJpegFiles() throws {
        let root = try makeRenderRoot()
        var progressCalls: [(Int, Int)] = []
        let rendered = try PdfPageRenderer.render(
            url: try fixture("pdf_image_only"),
            tempRoot: root,
            progress: { progressCalls.append(($0, $1)) }
        )
        XCTAssertFalse(rendered.pageURLs.isEmpty)
        XCTAssertEqual(progressCalls.last?.0, rendered.pageURLs.count)
        for url in rendered.pageURLs {
            XCTAssertEqual(url.pathExtension, "jpg")
            let size = try XCTUnwrap(
                try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int
            )
            XCTAssertGreaterThan(size, 0)
            XCTAssertLessThanOrEqual(size, PdfPageRenderer.maxPageBytes)
            let image = try XCTUnwrap(NSImage(contentsOf: url))
            let rep = try XCTUnwrap(image.representations.first)
            XCTAssertLessThanOrEqual(CGFloat(rep.pixelsWide), PdfPageRenderer.maxPixelDimension)
            XCTAssertLessThanOrEqual(CGFloat(rep.pixelsHigh), PdfPageRenderer.maxPixelDimension)
        }
        XCTAssertEqual(entries(in: root).count, 1, "exactly one per-import temp directory")
        rendered.cleanup()
        XCTAssertTrue(entries(in: root).isEmpty, "cleanup() removes the temp directory")
    }

    func testPageCapRejectsBeforeAnyTempDirectoryOrRender() throws {
        let root = try makeRenderRoot()
        let document = makeBlankPdf(pages: 5)
        XCTAssertEqual(document.pageCount, 5)
        var rendered = 0
        XCTAssertThrowsError(
            try PdfPageRenderer.render(
                document: document,
                tempRoot: root,
                maxPages: 4,
                progress: { _, _ in rendered += 1 }
            )
        ) { error in
            XCTAssertEqual(error as? PdfRenderError, .tooManyPages(count: 5, max: 4))
        }
        XCTAssertEqual(rendered, 0)
        XCTAssertTrue(entries(in: root).isEmpty, "no temp directory is created when the cap rejects")
    }

    func testCancellationMidwayCleansUpEverything() throws {
        let root = try makeRenderRoot()
        let document = makeBlankPdf(pages: 4)
        var done = 0
        XCTAssertThrowsError(
            try PdfPageRenderer.render(
                document: document,
                tempRoot: root,
                progress: { completed, _ in done = completed },
                isCancelled: { done >= 2 }
            )
        ) { error in
            XCTAssertEqual(error as? PdfRenderError, .cancelled)
        }
        XCTAssertEqual(done, 2, "stopped after the page that was in flight, not after all four")
        XCTAssertTrue(entries(in: root).isEmpty, "failure path removes already-written pages")
    }

    func testPerPageByteCapFailureCleansUp() throws {
        let root = try makeRenderRoot()
        XCTAssertThrowsError(
            try PdfPageRenderer.render(document: makeBlankPdf(pages: 2), tempRoot: root, maxPageBytes: 1)
        ) { error in
            XCTAssertEqual(error as? PdfRenderError, .pageTooLarge(pageIndex: 0))
        }
        XCTAssertTrue(entries(in: root).isEmpty)
    }

    func testTotalSizeBudgetFailureCleansUp() throws {
        let root = try makeRenderRoot()
        XCTAssertThrowsError(
            try PdfPageRenderer.render(document: makeBlankPdf(pages: 3), tempRoot: root, maxTotalBytes: 1)
        ) { error in
            XCTAssertEqual(error as? PdfRenderError, .totalSizeExceeded)
        }
        XCTAssertTrue(entries(in: root).isEmpty)
    }

    func testMalformedPdfFileSurfacesCannotOpenWithoutCrash() throws {
        let root = try makeRenderRoot()
        let bad = tempDir.appendingPathComponent("garbage.pdf")
        try Data("this is not a pdf".utf8).write(to: bad)
        XCTAssertThrowsError(try PdfPageRenderer.render(url: bad, tempRoot: root)) { error in
            XCTAssertEqual(error as? PdfRenderError, .cannotOpen)
        }
        XCTAssertTrue(entries(in: root).isEmpty)
    }

    func testLockedPdfThrowsLockedBeforeRendering() throws {
        let root = try makeRenderRoot()
        XCTAssertThrowsError(try PdfPageRenderer.render(url: try fixture("pdf_encrypted"), tempRoot: root)) { error in
            // PDFKit either reports the document as locked, or (for an
            // owner-password-only file it can open) renders it; the fixture
            // is user-password protected so it must be locked.
            XCTAssertEqual(error as? PdfRenderError, .locked)
        }
        XCTAssertTrue(entries(in: root).isEmpty)
    }

    // MARK: - Error mapping + end-to-end commit

    func testRenderErrorsMapToPresentablePhases() {
        guard case .cancelled = OcrImportState.phase(forRenderError: PdfRenderError.cancelled) else {
            return XCTFail("cancelled should map to .cancelled")
        }
        for error in [
            PdfRenderError.locked, .cannotOpen, .tooManyPages(count: 3000, max: 2000),
            .pageRenderFailed(pageIndex: 1), .pageTooLarge(pageIndex: 0), .totalSizeExceeded,
        ] {
            guard case .failed(let message) = OcrImportState.phase(forRenderError: error) else {
                return XCTFail("\(error) should map to .failed")
            }
            XCTAssertFalse(message.isEmpty)
        }
    }

    /// Image-only PDF -> rendered temp pages -> (stubbed Vision) reviewed
    /// text -> the real `importImageWithOcr` commit -> the item appears in
    /// the library, and the state machine removes its temp renders.
    func testScannedPdfRenderCommitEndToEndAndTempCleanup() async throws {
        let state = OcrImportState()
        state.beginPdf(url: try fixture("pdf_image_only"))
        let deadline = Date().addingTimeInterval(90)
        while Date() < deadline {
            if case .reviewing = state.phase { break }
            if case .failed = state.phase { break }
            try await Task.sleep(nanoseconds: 100_000_000)
        }
        // Vision may find no text on a blank-ish fixture; either way pages
        // must have been rendered. Stub the OCR boundary only.
        guard case .reviewing = state.phase else {
            return XCTFail("expected .reviewing after render+scan, got \(state.phase)")
        }
        XCTAssertTrue(state.holdsPdfTempFiles)
        let pages = state.pages.map {
            OcrReviewPage(pageIndex: $0.pageIndex, sourceURL: $0.sourceURL, text: "scanned pdf page \($0.pageIndex)", confidence: 0.9)
        }
        let urls = pages.map(\.sourceURL)
        XCTAssertFalse(urls.isEmpty)
        for (i, page) in pages.enumerated() { state.updateText(forPage: i, text: page.text) }

        await state.commit(core: client)
        guard case .done = state.phase else { return XCTFail("expected .done, got \(state.phase)") }
        XCTAssertEqual(client.items.count, 1)
        XCTAssertFalse(state.holdsPdfTempFiles)
        for url in urls {
            XCTAssertFalse(FileManager.default.fileExists(atPath: url.path), "temp render removed after commit")
        }
    }

    func testDiscardAfterRenderRemovesTempFiles() async throws {
        let state = OcrImportState()
        state.beginPdf(url: try fixture("pdf_image_only"))
        let deadline = Date().addingTimeInterval(90)
        while Date() < deadline {
            if case .reviewing = state.phase { break }
            if case .failed = state.phase { break }
            try await Task.sleep(nanoseconds: 100_000_000)
        }
        let urls = state.pages.map(\.sourceURL)
        state.discard()
        XCTAssertFalse(state.holdsPdfTempFiles)
        for url in urls { XCTAssertFalse(FileManager.default.fileExists(atPath: url.path)) }
    }

    func testMalformedPdfThroughStateMachineFailsCleanly() async throws {
        let bad = tempDir.appendingPathComponent("garbage.pdf")
        try Data("not a pdf".utf8).write(to: bad)
        let state = OcrImportState()
        state.beginPdf(url: bad)
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline {
            if case .failed = state.phase { break }
            try await Task.sleep(nanoseconds: 50_000_000)
        }
        guard case .failed = state.phase else { return XCTFail("expected .failed, got \(state.phase)") }
        XCTAssertFalse(state.holdsPdfTempFiles)
    }
}
