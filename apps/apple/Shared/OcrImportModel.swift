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
    case scanning(completed: Int, total: Int)
    case reviewing
    case importing
    case done(itemId: String, pageConfidences: [Float])
    case cancelled
    case failed(String)
}

@MainActor
final class OcrImportState: ObservableObject {
    @Published private(set) var phase: OcrImportPhase = .idle
    @Published var pages: [OcrReviewPage] = []

    private var scanTask: Task<Void, Never>?

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
        switch outcome {
        case .success(let itemId, let pageConfidences):
            phase = .done(itemId: itemId, pageConfidences: pageConfidences)
        case .failure(let message):
            phase = .failed(message)
        }
    }
}
