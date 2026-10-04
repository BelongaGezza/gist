import Foundation

/// One page of a multi-page OCR scan, as tracked by the review screen
/// (role R8, `development-plan-v2.md` §3.3). `text` is mutable -- OCR is
/// imperfect, and a person can correct it before `OcrImportState.commit`
/// ever calls into Rust. See this file's and `VisionOcrEngine.swift`'s top
/// notes for how "edit before commit" is reconciled with
/// `Core::import_image_with_ocr` committing its whole multi-page document
/// in one call, with no separate edit-then-commit step of its own.
struct OcrReviewPage: Identifiable, Equatable, Sendable {
    let pageIndex: Int
    let sourceURL: URL
    var text: String
    let confidence: Float

    var id: Int { pageIndex }

    var isLowConfidence: Bool { OcrConfidence.isLowConfidence(confidence) }
}

/// Confidence-threshold-to-highlight mapping, factored out as pure logic
/// (no Vision, no SwiftUI) so it's unit-testable on its own -- see
/// `OcrImportTests`.
enum OcrConfidence {
    /// Below this, a page's recognized text is flagged in the review screen
    /// as likely needing correction. 0.5 is a deliberately conservative
    /// midpoint: clean printed text Vision recognizes confidently usually
    /// scores well above this in practice, while a noticeably lower score
    /// typically means a blurry/skewed photo, an unusual font, or a
    /// mostly-blank page.
    static let lowConfidenceThreshold: Float = 0.5

    static func isLowConfidence(_ confidence: Float) -> Bool {
        confidence < lowConfidenceThreshold
    }
}

/// Mirrors the byte-size cap `Core::import_image_with_ocr` enforces
/// Rust-side (ADR-009 Addendum 2, `ParseLimits::default().max_bytes`), so
/// the review screen can reject an oversized page immediately -- before
/// spending any wall-clock time running Vision on it, and before ever
/// crossing into Rust -- with a specific, immediate message instead of a
/// generic import failure much later. This is a UX nicety, not the
/// enforcement point: the real gate is still the one inside
/// `Core::import_image_with_ocr` itself, so a file that changes size
/// between this check and the real commit call (the same TOCTOU shape as
/// `F13`) is still safely rejected there, just with a less immediate
/// message.
enum OcrFileSizeCheck {
    /// `ParseLimits::default().max_bytes` (`crates/gist-model/src/lib.rs`) --
    /// 256 MiB. Not re-derived from FFI (there's no accessor for it), so if
    /// that default ever changes, this constant must be updated by hand to
    /// match; it is deliberately the *same* value, not a separately-chosen
    /// UI limit.
    static let maxBytes = 256 * 1024 * 1024

    static func oversizedFiles(among urls: [URL], maxBytes: Int = OcrFileSizeCheck.maxBytes) -> [URL] {
        urls.filter { url in
            guard
                let size = try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int
            else {
                return false
            }
            return size > maxBytes
        }
    }
}

/// Drives the OCR review screen's state machine (role R8): pick pages ->
/// scan (Vision, cancellable) -> review/edit -> commit (one
/// `GistCore.importImageWithOcr` call) -> done/failed/cancelled.
enum OcrImportPhase: Equatable {
    case idle
    /// Rendering a scanned PDF's pages to temp images (M6 R2), before the
    /// normal Vision scan. Cancellable.
    case rendering(completed: Int, total: Int)
    case scanning(completed: Int, total: Int)
    case reviewing
    case importing
    case done(itemId: String, pageConfidences: [Float])
    case cancelled
    case failed(String)
}

@MainActor
final class OcrImportState: ObservableObject {
    @Published private(set) var phase: OcrImportPhase = .idle {
        didSet {
            // Temp PDF renders never outlive a terminal state.
            switch phase {
            case .cancelled, .failed, .done: cleanupPdfTemp()
            default: break
            }
        }
    }
    @Published var pages: [OcrReviewPage] = []

    private var scanTask: Task<Void, Never>?
    /// Temp page images rendered from a scanned PDF (M6 R2). Owned here and
    /// removed on every terminal path: commit success or failure, cancel,
    /// reset, and the sheet disappearing (`discard()`).
    private var renderedPdf: RenderedPdfPages?
    private var pdfAccessURL: URL?

    /// Validates page sizes up front (`OcrFileSizeCheck`), then kicks off a
    /// cancellable per-page Vision scan (`VisionPageRecognizer`, in
    /// VisionOcrEngine.swift) off the main thread via `Task.detached`.
    /// `pages`/`.reviewing` are only published once every page has been
    /// scanned (rather than incrementally), so there's never a
    /// half-reviewable state visible to the review screen.
    ///
    /// `maxBytes` defaults to the real Rust-mirroring cap
    /// (`OcrFileSizeCheck.maxBytes`) for every production call site
    /// (`OcrImportSheet`'s file importer callback); it's an explicit
    /// parameter only so `OcrImportTests` can exercise the oversized-file
    /// rejection path with small test files instead of needing a real
    /// 256 MiB fixture, the same test-seam idiom as `CoreClient`'s
    /// alternate `init`s and `ThemeManager`'s `initialSystemIsDark`.
    func beginScan(urls: [URL], maxBytes: Int = OcrFileSizeCheck.maxBytes) {
        let oversized = OcrFileSizeCheck.oversizedFiles(among: urls, maxBytes: maxBytes)
        guard oversized.isEmpty else {
            let names = oversized.map(\.lastPathComponent).joined(separator: ", ")
            let limitMb = maxBytes / (1024 * 1024)
            // (R5b localisation) `.failed` carries plain `String`, not
            // `Text()` -- wrap so the fixed English text reaches the
            // catalog. `names` (the user's own file names) stays as data.
            phase = .failed(
                String(
                    localized: "These file(s) exceed GIST's \(limitMb) MB per-page limit and were not scanned: \(names)"
                )
            )
            return
        }
        guard !urls.isEmpty else { return }

        scanTask?.cancel()
        pages = []
        phase = .scanning(completed: 0, total: urls.count)

        scanTask = Task {
            var results: [OcrReviewPage] = []
            for (index, url) in urls.enumerated() {
                if Task.isCancelled {
                    phase = .cancelled
                    return
                }
                do {
                    let recognized = try await Task.detached(priority: .userInitiated) {
                        try VisionPageRecognizer.recognizeText(at: url)
                    }.value
                    if Task.isCancelled {
                        phase = .cancelled
                        return
                    }
                    results.append(
                        OcrReviewPage(
                            pageIndex: index,
                            sourceURL: url,
                            text: recognized.text,
                            confidence: recognized.confidence
                        )
                    )
                    phase = .scanning(completed: index + 1, total: urls.count)
                } catch {
                    // (R5b localisation) Wrap the fixed English prefix;
                    // `error.localizedDescription` is already
                    // system-provided text, not ours to translate here.
                    phase = .failed(
                        String(
                            localized: "Couldn't recognize text on page \(index + 1) (\(url.lastPathComponent)): "
                        )
                            + error.localizedDescription
                    )
                    return
                }
            }
            pages = results
            phase = .reviewing
        }
    }

    /// Requests cancellation of an in-flight scan. Checked between pages
    /// (same "checked between pipeline stages" convention as
    /// `gist_core::ImportObserver`) -- a page already mid-recognition when
    /// this is called still finishes, but no further page starts.
    func cancelScan() {
        scanTask?.cancel()
    }

    // MARK: - PDF source (M6 R2)

    /// Entry point for an image-only PDF: renders its pages to a per-import
    /// temp directory one at a time (`PdfPageRenderer` -- page cap checked
    /// before any render, memory bounded), then hands the page images to the
    /// normal `beginScan` flow, so the existing Vision scan + review screen +
    /// `importImageWithOcr` commit are reused unchanged (ADR-009). Never logs
    /// the source path.
    func beginPdf(url: URL) {
        scanTask?.cancel()
        cleanupPdfTemp()
        pages = []
        phase = .rendering(completed: 0, total: 0)
        // The picker's sandbox access may have been released since the first
        // import attempt; take our own for the render's duration.
        if url.startAccessingSecurityScopedResource() { pdfAccessURL = url }
        scanTask = Task {
            do {
                let rendered = try await Task.detached(priority: .userInitiated) {
                    try PdfPageRenderer.render(
                        url: url,
                        progress: { done, total in
                            Task { @MainActor [weak self] in
                                guard let self, case .rendering = self.phase else { return }
                                self.phase = .rendering(completed: done, total: total)
                            }
                        },
                        isCancelled: { Task.isCancelled }
                    )
                }.value
                releasePdfAccess()
                if Task.isCancelled {
                    rendered.cleanup()
                    phase = .cancelled
                    return
                }
                renderedPdf = rendered
                scanTask = nil
                beginScan(urls: rendered.pageURLs)
            } catch {
                releasePdfAccess()
                phase = Self.phase(forRenderError: error)
            }
        }
    }

    /// Maps a render failure to a presentable terminal phase. Typed errors
    /// only; a locked/malformed PDF gets the same honest wording as the
    /// import alerts rather than a crash or a raw error dump.
    nonisolated static func phase(forRenderError error: Error) -> OcrImportPhase {
        guard let render = error as? PdfRenderError else {
            return .failed(String(localized: "Couldn't read this PDF."))
        }
        switch render {
        case .cancelled:
            return .cancelled
        case .locked:
            return .failed(
                String(
                    localized:
                        "This PDF is password-protected and can't be imported. GIST never attempts to bypass protection."
                )
            )
        case .cannotOpen:
            return .failed(String(localized: "This PDF couldn't be opened. It may be damaged."))
        case .tooManyPages(let count, let max):
            return .failed(String(localized: "This PDF has \(count) pages; GIST's limit is \(max)."))
        case .pageRenderFailed(let index):
            return .failed(String(localized: "Couldn't render page \(index + 1) of this PDF."))
        case .pageTooLarge(let index):
            return .failed(String(localized: "Page \(index + 1) of this PDF is too large to scan."))
        case .totalSizeExceeded:
            return .failed(String(localized: "This PDF is too large to scan on this device."))
        }
    }

    /// Releases any temp page images and cancels in-flight work. Called when
    /// the sheet goes away.
    func discard() {
        scanTask?.cancel()
        cleanupPdfTemp()
    }

    private func releasePdfAccess() {
        pdfAccessURL?.stopAccessingSecurityScopedResource()
        pdfAccessURL = nil
    }

    private func cleanupPdfTemp() {
        releasePdfAccess()
        renderedPdf?.cleanup()
        renderedPdf = nil
    }

    /// True while temp page images from a PDF are held (test seam).
    var holdsPdfTempFiles: Bool { renderedPdf != nil }

    func updateText(forPage pageIndex: Int, text: String) {
        guard let i = pages.firstIndex(where: { $0.pageIndex == pageIndex }) else { return }
        pages[i].text = text
    }

    /// Resets back to `.idle` -- used by the terminal states' "Try Again"
    /// action, so reopening the picker doesn't show stale pages/progress
    /// from a previous attempt.
    func reset() {
        scanTask?.cancel()
        scanTask = nil
        cleanupPdfTemp()
        pages = []
        phase = .idle
    }

    /// Commits the reviewed pages via `CoreClient.importScannedDocument`,
    /// using a `ReviewedOcrEngine` (VisionOcrEngine.swift) built from this
    /// state's *current* `pages` -- i.e. whatever a person has edited by
    /// this point, not necessarily the original Vision output.
    func commit(core: CoreClient) async {
        phase = .importing
        let orderedPages = pages.sorted { $0.pageIndex < $1.pageIndex }
        let engine = ReviewedOcrEngine(pages: orderedPages)
        let paths = orderedPages.map { $0.sourceURL.path }
        let outcome = await core.importScannedDocument(pagePaths: paths, engine: engine)
        // The committed text and page images are copied into the store by
        // Rust (ADR-006 §4); the temp renders are no longer needed either way.
        defer { cleanupPdfTemp() }
        switch outcome {
        case .success(let itemId, let pageConfidences):
            phase = .done(itemId: itemId, pageConfidences: pageConfidences)
        case .failure(let message):
            phase = .failed(message)
        }
    }
}
