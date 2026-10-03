import AppKit
import Foundation
import PDFKit

/// Errors from rendering an image-only PDF's pages to temp image files for the
/// existing OCR flow (M6 R2). Typed so the UI never string-matches.
enum PdfRenderError: Error, Equatable {
    /// The PDF has more pages than the cap. Thrown before a single page is
    /// rendered or any temp file is created (security register `N9`).
    case tooManyPages(count: Int, max: Int)
    /// PDFKit says the document is locked (password-protected).
    case locked
    /// PDFKit could not open the file at all (malformed/truncated).
    case cannotOpen
    case pageRenderFailed(pageIndex: Int)
    /// A rendered page file would exceed the per-input byte cap even after
    /// re-encoding at lower quality.
    case pageTooLarge(pageIndex: Int)
    /// The cumulative size of the rendered temp files passed the disk budget.
    case totalSizeExceeded
    case cancelled
}

/// The rendered pages of one PDF, living in a per-import temp directory.
/// The owner must call `cleanup()` when finished with them (success, failure
/// and cancel alike).
struct RenderedPdfPages {
    let directory: URL
    let pageURLs: [URL]

    func cleanup() {
        try? FileManager.default.removeItem(at: directory)
    }
}

/// Renders a PDF's pages to JPEG files, one page at a time, for feeding to
/// `GistCore.importImageWithOcr` through the existing OCR review screen.
///
/// Memory is bounded: each page is rendered inside its own `autoreleasepool`,
/// written to disk, and released before the next begins -- only one page's
/// bitmap exists at a time. Disk use is bounded by `maxTotalBytes`.
enum PdfPageRenderer {
    /// Mirrors `ParseLimits::default().max_pages` (`crates/gist-model`).
    /// There is no FFI accessor for the Rust limits, so -- exactly like
    /// `OcrFileSizeCheck.maxBytes` -- this is a hand-kept mirror of the same
    /// value, not a separately chosen UI limit. The real gate remains in Rust.
    static let maxPages = 2_000
    /// OCR render resolution. 200 DPI is ample for Vision on printed text.
    static let dpi: CGFloat = 200
    /// Hard cap on either pixel dimension of a rendered page (oversized
    /// pages, e.g. posters/maps, are scaled down to fit).
    static let maxPixelDimension: CGFloat = 4_096
    /// Per-page file cap: Rust's per-input cap (`OcrFileSizeCheck.maxBytes`).
    static let maxPageBytes = OcrFileSizeCheck.maxBytes
    /// Cumulative cap on all temp page files for one import.
    static let maxTotalBytes = 2 * 1024 * 1024 * 1024

    /// Pixel size for a page of `pointSize` (PDF points, 1/72 in) at `dpi`,
    /// clamped so neither side exceeds `maxPixelDimension` and each is >= 1.
    static func pixelSize(
        forPointSize pointSize: CGSize,
        dpi: CGFloat = PdfPageRenderer.dpi,
        maxPixelDimension: CGFloat = PdfPageRenderer.maxPixelDimension
    ) -> CGSize {
        let scale = dpi / 72
        var w = max(pointSize.width * scale, 1)
        var h = max(pointSize.height * scale, 1)
        let longest = max(w, h)
        if longest > maxPixelDimension {
            let k = maxPixelDimension / longest
            w *= k
            h *= k
        }
        return CGSize(width: max(w.rounded(), 1), height: max(h.rounded(), 1))
    }

    /// Opens `url` with PDFKit and renders it. `progress(completed, total)`
    /// is called after each page; `isCancelled` is polled between pages.
    /// On any thrown error the temp directory is already removed.
    static func render(
        url: URL,
        tempRoot: URL = FileManager.default.temporaryDirectory,
        maxPages: Int = PdfPageRenderer.maxPages,
        maxPageBytes: Int = PdfPageRenderer.maxPageBytes,
        maxTotalBytes: Int = PdfPageRenderer.maxTotalBytes,
        progress: (Int, Int) -> Void = { _, _ in },
        isCancelled: () -> Bool = { false }
    ) throws -> RenderedPdfPages {
        guard let document = PDFDocument(url: url) else { throw PdfRenderError.cannotOpen }
        return try render(
            document: document,
            tempRoot: tempRoot,
            maxPages: maxPages,
            maxPageBytes: maxPageBytes,
            maxTotalBytes: maxTotalBytes,
            progress: progress,
            isCancelled: isCancelled
        )
    }

    static func render(
        document: PDFDocument,
        tempRoot: URL = FileManager.default.temporaryDirectory,
        maxPages: Int = PdfPageRenderer.maxPages,
        maxPageBytes: Int = PdfPageRenderer.maxPageBytes,
        maxTotalBytes: Int = PdfPageRenderer.maxTotalBytes,
        progress: (Int, Int) -> Void = { _, _ in },
        isCancelled: () -> Bool = { false }
    ) throws -> RenderedPdfPages {
        if document.isLocked { throw PdfRenderError.locked }
        let total = document.pageCount
        // Page cap first -- before the temp dir exists and before any render.
        guard total <= maxPages else { throw PdfRenderError.tooManyPages(count: total, max: maxPages) }
        guard total > 0 else { throw PdfRenderError.cannotOpen }

        let directory = tempRoot.appendingPathComponent("gist-pdf-ocr-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var urls: [URL] = []
        var totalBytes = 0
        do {
            for index in 0..<total {
                if isCancelled() { throw PdfRenderError.cancelled }
                // Render + write + release, one page at a time.
                let data = try autoreleasepool { () throws -> Data in
                    guard let page = document.page(at: index),
                        let jpeg = renderJPEG(page: page, quality: 0.85)
                    else { throw PdfRenderError.pageRenderFailed(pageIndex: index) }
                    if jpeg.count > maxPageBytes {
                        // Retry once at a much lower quality before giving up.
                        guard let small = renderJPEG(page: page, quality: 0.4), small.count <= maxPageBytes else {
                            throw PdfRenderError.pageTooLarge(pageIndex: index)
                        }
                        return small
                    }
                    return jpeg
                }
                totalBytes += data.count
                if totalBytes > maxTotalBytes { throw PdfRenderError.totalSizeExceeded }
                let file = directory.appendingPathComponent(String(format: "page-%05d.jpg", index + 1))
                try data.write(to: file, options: .atomic)
                urls.append(file)
                progress(index + 1, total)
            }
        } catch {
            try? FileManager.default.removeItem(at: directory)
            throw error
        }
        return RenderedPdfPages(directory: directory, pageURLs: urls)
    }

    /// Renders one page into an exactly-sized bitmap on a white background
    /// (PDF pages are transparent) and returns JPEG data, or `nil` on failure.
    private static func renderJPEG(page: PDFPage, quality: CGFloat) -> Data? {
        let box = page.bounds(for: .mediaBox)
        // Account for the page's own rotation when sizing the output.
        let rotated = page.rotation % 180 != 0
        let points = rotated ? CGSize(width: box.height, height: box.width) : box.size
        guard points.width > 0, points.height > 0 else { return nil }
        let px = pixelSize(forPointSize: points)
        guard
            let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil,
                pixelsWide: Int(px.width),
                pixelsHigh: Int(px.height),
                bitsPerSample: 8,
                samplesPerPixel: 4,
                hasAlpha: true,
                isPlanar: false,
                colorSpaceName: .deviceRGB,
                bytesPerRow: 0,
                bitsPerPixel: 0
            ),
            let context = NSGraphicsContext(bitmapImageRep: rep)
        else { return nil }
        // `thumbnail(of:for:)` handles rotation/box transforms for us.
        let image = page.thumbnail(of: px, for: .mediaBox)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        NSColor.white.setFill()
        NSRect(origin: .zero, size: px).fill()
        image.draw(in: NSRect(origin: .zero, size: px))
        NSGraphicsContext.restoreGraphicsState()
        return rep.representation(using: .jpeg, properties: [.compressionFactor: quality])
    }
}
