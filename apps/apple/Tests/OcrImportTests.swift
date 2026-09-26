import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import GIST

/// Tests for the OCR review screen (role R8): the pure confidence/size-limit
/// logic factored out of `OcrImportView.swift`/`OcrImportModel.swift`, plus
/// real end-to-end `CoreClient.importScannedDocument` round trips against a
/// real `GistCore` (SQLite + filesystem) pointed at a fresh temp directory —
/// same real-core, no-mocking convention as `GISTTests`. Deliberately a
/// dedicated file rather than added to the already-large `GISTTests.swift`.
///
/// None of these tests exercise `VisionPageRecognizer` itself (real Vision
/// text recognition on a plain white test square would have nothing to
/// recognize, and isn't the interesting part to verify here) — the
/// end-to-end tests instead build a `ReviewedOcrEngine` directly from
/// hand-written `OcrReviewPage` values, exactly as `OcrImportState.commit`
/// does once a person has reviewed (and possibly edited) Vision's real
/// output. That's the actual integration seam this role owns: getting
/// already-reviewed text through `GistCore.importImageWithOcr` and into the
/// library correctly.
@MainActor
final class OcrImportTests: XCTestCase {
    private var tempDir: URL!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("OcrImportTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        let dbPath = tempDir.appendingPathComponent("gist.sqlite3").path
        let storageDir = tempDir.appendingPathComponent("storage", isDirectory: true).path
        client = CoreClient(dbPath: dbPath, storageDir: storageDir)
    }

    override func tearDownWithError() throws {
        client = nil
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
        tempDir = nil
        try super.tearDownWithError()
    }

    // MARK: - Test image helper

    /// Writes a minimal valid PNG (a plain white square) to `url`, via
    /// CoreGraphics/ImageIO directly — the same frameworks
    /// `VisionPageRecognizer` itself uses to read images — rather than
    /// bundling a binary fixture. `gist_imageprep::prepare_image` has no
    /// minimum-dimension requirement, so a tiny square is sufficient to
    /// exercise the real Rust pipeline (decode -> greyscale -> resize ->
    /// re-encode) without a real scanned-document fixture.
    private func makeTestImageFile(at url: URL, size: Int = 20) throws {
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        guard let context = CGContext(
            data: nil,
            width: size,
            height: size,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            throw XCTSkip("couldn't create CGContext for test image")
        }
        context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: size, height: size))
        guard let cgImage = context.makeImage() else {
            throw XCTSkip("couldn't render test CGImage")
        }
        guard let destination = CGImageDestinationCreateWithURL(
            url as CFURL, UTType.png.identifier as CFString, 1, nil
        ) else {
            throw XCTSkip("couldn't create PNG destination")
        }
        CGImageDestinationAddImage(destination, cgImage, nil)
        guard CGImageDestinationFinalize(destination) else {
            throw XCTSkip("couldn't finalize PNG")
        }
    }

    // MARK: - OcrConfidence

    func testOcrConfidenceThresholdBoundary() {
        XCTAssertTrue(OcrConfidence.isLowConfidence(0.0))
        XCTAssertTrue(OcrConfidence.isLowConfidence(0.49))
        XCTAssertFalse(OcrConfidence.isLowConfidence(0.5))
        XCTAssertFalse(OcrConfidence.isLowConfidence(1.0))
    }

    func testOcrReviewPageIsLowConfidenceDerivedProperty() {
        let low = OcrReviewPage(
            pageIndex: 0, sourceURL: URL(fileURLWithPath: "/tmp/a.png"), text: "", confidence: 0.2
        )
        let high = OcrReviewPage(
            pageIndex: 1, sourceURL: URL(fileURLWithPath: "/tmp/b.png"), text: "", confidence: 0.9
        )
        XCTAssertTrue(low.isLowConfidence)
        XCTAssertFalse(high.isLowConfidence)
    }

    // MARK: - OcrFileSizeCheck

    func testOcrFileSizeCheckFlagsOnlyOversizedFiles() throws {
        let smallURL = tempDir.appendingPathComponent("small.bin")
        let bigURL = tempDir.appendingPathComponent("big.bin")
        try Data(count: 10).write(to: smallURL)
        try Data(count: 200).write(to: bigURL)

        let oversized = OcrFileSizeCheck.oversizedFiles(among: [smallURL, bigURL], maxBytes: 100)
        XCTAssertEqual(oversized, [bigURL])
    }

    func testOcrFileSizeCheckDefaultMatchesRustParseLimitsMaxBytes() {
        // Mirrors `ParseLimits::default().max_bytes` in
        // crates/gist-model/src/lib.rs (256 MiB) — see ADR-009 Addendum 2.
        XCTAssertEqual(OcrFileSizeCheck.maxBytes, 256 * 1024 * 1024)
    }

    // MARK: - ReviewedOcrEngine

    func testReviewedOcrEngineReturnsStoredTextAndConfidenceIgnoringImageBytes() {
        let page = OcrReviewPage(
            pageIndex: 2,
            sourceURL: URL(fileURLWithPath: "/tmp/x.png"),
            text: "edited text",
            confidence: 0.75
        )
        let engine = ReviewedOcrEngine(pages: [page])

        // Deliberately passing unrelated bytes -- the engine must ignore
        // them and return the reviewed text regardless (see this type's doc
        // comment for why re-recognizing here would discard edits).
        let result = engine.recognizePage(pageIndex: 2, imageBytes: Data([1, 2, 3]))

        XCTAssertEqual(result?.pageIndex, 2)
        XCTAssertEqual(result?.text, "edited text")
        XCTAssertEqual(result?.confidence, 0.75)
    }

    func testReviewedOcrEngineReturnsNilForUnknownPageIndex() {
        let engine = ReviewedOcrEngine(pages: [])
        XCTAssertNil(engine.recognizePage(pageIndex: 0, imageBytes: Data()))
    }

    // MARK: - OcrImportState

    func testBeginScanWithNoURLsIsANoOp() {
        let state = OcrImportState()
        state.beginScan(urls: [])
        XCTAssertEqual(state.phase, .idle)
    }

    func testBeginScanRejectsOversizedFileWithoutSpawningAScan() throws {
        let bigURL = tempDir.appendingPathComponent("big.bin")
        try Data(count: 1000).write(to: bigURL)

        let state = OcrImportState()
        state.beginScan(urls: [bigURL], maxBytes: 500)

        guard case .failed(let message) = state.phase else {
            XCTFail("expected .failed, got \(state.phase)")
            return
        }
        XCTAssertTrue(message.contains("big.bin"))
        XCTAssertTrue(state.pages.isEmpty)
    }

    func testResetClearsPagesAndReturnsToIdle() {
        let state = OcrImportState()
        state.pages = [
            OcrReviewPage(
                pageIndex: 0, sourceURL: URL(fileURLWithPath: "/tmp/x.png"), text: "x", confidence: 0.9
            ),
        ]
        state.reset()
        XCTAssertEqual(state.phase, .idle)
        XCTAssertTrue(state.pages.isEmpty)
    }

    func testUpdateTextEditsOnlyTheTargetedPage() {
        let state = OcrImportState()
        state.pages = [
            OcrReviewPage(
                pageIndex: 0, sourceURL: URL(fileURLWithPath: "/tmp/a.png"), text: "a", confidence: 0.9
            ),
            OcrReviewPage(
                pageIndex: 1, sourceURL: URL(fileURLWithPath: "/tmp/b.png"), text: "b", confidence: 0.9
            ),
        ]

        state.updateText(forPage: 1, text: "edited")

        XCTAssertEqual(state.pages[0].text, "a")
        XCTAssertEqual(state.pages[1].text, "edited")
    }

    func testUpdateTextWithUnknownPageIndexIsANoOp() {
        let state = OcrImportState()
        state.pages = [
            OcrReviewPage(
                pageIndex: 0, sourceURL: URL(fileURLWithPath: "/tmp/a.png"), text: "a", confidence: 0.9
            ),
        ]
        state.updateText(forPage: 99, text: "should not appear anywhere")
        XCTAssertEqual(state.pages.count, 1)
        XCTAssertEqual(state.pages[0].text, "a")
    }

    // MARK: - End-to-end (real GistCore, no mocking)

    /// The real integration seam this role owns: a reviewed, possibly-edited
    /// set of pages goes through `GistCore.importImageWithOcr` and lands in
    /// the library as a real multi-section document, with per-page
    /// confidences preserved in order.
    func testCommitImportsReviewedPagesAndTransitionsToDone() async throws {
        let page0URL = tempDir.appendingPathComponent("page0.png")
        let page1URL = tempDir.appendingPathComponent("page1.png")
        try makeTestImageFile(at: page0URL)
        try makeTestImageFile(at: page1URL)

        let state = OcrImportState()
        state.pages = [
            OcrReviewPage(pageIndex: 0, sourceURL: page0URL, text: "Hello page one", confidence: 0.92),
            OcrReviewPage(pageIndex: 1, sourceURL: page1URL, text: "Hello page two", confidence: 0.40),
        ]

        await state.commit(core: client)

        guard case .done(let itemId, let confidences) = state.phase else {
            XCTFail("expected .done, got \(state.phase)")
            return
        }
        XCTAssertFalse(itemId.isEmpty)
        XCTAssertEqual(confidences, [0.92, 0.40])

        await client.refresh()
        XCTAssertTrue(client.items.contains { $0.id == itemId })
    }

    /// `CoreClient.importScannedDocument` directly (rather than going
    /// through `OcrImportState.commit`), confirming the outcome type itself
    /// carries the right data on success.
    func testImportScannedDocumentReturnsSuccessWithItemIdAndConfidences() async throws {
        let pageURL = tempDir.appendingPathComponent("page.png")
        try makeTestImageFile(at: pageURL)

        let page = OcrReviewPage(pageIndex: 0, sourceURL: pageURL, text: "Committed text", confidence: 0.8)
        let engine = ReviewedOcrEngine(pages: [page])

        let outcome = await client.importScannedDocument(pagePaths: [pageURL.path], engine: engine)

        guard case .success(let itemId, let confidences) = outcome else {
            XCTFail("expected .success, got \(outcome)")
            return
        }
        XCTAssertFalse(itemId.isEmpty)
        XCTAssertEqual(confidences, [0.8])
    }

    /// A file over the byte-size cap must surface as a real, presentable
    /// `.failure` from `Core::import_image_with_ocr`'s
    /// `ImportError::ResourceLimitExceeded` (ADR-009 Addendum 2) — not a
    /// crash, and not silently swallowed. This calls
    /// `CoreClient.importScannedDocument` directly with a real oversized
    /// file so the assertion is against the real Rust-side cap, not this
    /// UI's own client-side `OcrFileSizeCheck` pre-check.
    func testImportScannedDocumentSurfacesResourceLimitExceededAsFailure() async throws {
        let oversizedURL = tempDir.appendingPathComponent("oversized.bin")
        // One byte over `ParseLimits::default().max_bytes` would be the
        // minimal reproduction, but allocating/writing a 256 MiB+1 file on
        // every test run is wasteful; a sparse file of that size is
        // sufficient since only `fs::metadata(path)?.len()` is consulted
        // before any byte is actually read.
        let handle = FileManager.default.createFile(atPath: oversizedURL.path, contents: nil)
        XCTAssertTrue(handle, "failed to create sparse test file")
        let fileHandle = try FileHandle(forWritingTo: oversizedURL)
        try fileHandle.truncate(atOffset: UInt64(OcrFileSizeCheck.maxBytes) + 1)
        try fileHandle.close()

        let page = OcrReviewPage(pageIndex: 0, sourceURL: oversizedURL, text: "irrelevant", confidence: 1.0)
        let engine = ReviewedOcrEngine(pages: [page])

        let outcome = await client.importScannedDocument(pagePaths: [oversizedURL.path], engine: engine)

        guard case .failure(let message) = outcome else {
            XCTFail("expected .failure, got \(outcome)")
            return
        }
        XCTAssertTrue(
            message.localizedCaseInsensitiveContains("resource limit")
                || message.localizedCaseInsensitiveContains("limit exceeded"),
            "expected a resource-limit error, got: \(message)"
        )
    }
}
