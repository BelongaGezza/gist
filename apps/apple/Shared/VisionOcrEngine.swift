import Foundation
import ImageIO
import Vision

/// Errors specific to the OCR review screen's scan phase (reading/
/// recognizing a page image client-side), as opposed to `GistError`, which
/// covers the later Rust-side `Core::import_image_with_ocr` commit call.
enum OcrScanError: LocalizedError {
    case unreadableImage(URL)

    // (R5b localisation) `errorDescription` ultimately surfaces as plain
    // `String` text (via `error.localizedDescription` in
    // `OcrImportModel.swift`), never through `Text()` directly -- wrap so
    // the fixed English text reaches the catalog. `url.lastPathComponent`
    // is the user's own file name (data, not translatable).
    var errorDescription: String? {
        switch self {
        case .unreadableImage(let url):
            return String(localized: "Couldn't read \(url.lastPathComponent) as an image.")
        }
    }
}

/// Runs Apple's on-device `Vision` text recognizer (`VNRecognizeTextRequest`)
/// against one page image, for the OCR review screen's scan phase (role
/// R8). This is a *preview* pass only -- it never touches `gist-core`/FFI.
/// Its results (and whatever a person edits afterward in the review screen)
/// are later handed back to Rust by `ReviewedOcrEngine`, below, which is
/// what actually conforms to the FFI `OcrEngine` callback interface.
///
/// Why split into two engines instead of one live Vision-backed `OcrEngine`
/// wired straight to `GistCore.importImageWithOcr`: `Core::
/// import_image_with_ocr` commits its whole multi-page document in one
/// Rust-side call, with no separate "edit the recognized text, then commit"
/// step of its own (per this role's brief). Any "let a person fix OCR
/// mistakes first" UX therefore has to happen entirely client-side, before
/// that one commit call is ever made -- so recognition has to run once,
/// up front, outside of `importImageWithOcr` entirely (this type), with a
/// second, non-recognizing pass (`ReviewedOcrEngine`) supplying the
/// (possibly edited) results when the real commit call happens.
enum VisionPageRecognizer {
    /// Synchronous, and can take real wall-clock time per page -- callers
    /// must not run this on the main thread. `OcrImportState.beginScan`
    /// dispatches each call via `Task.detached`.
    static func recognizeText(at url: URL) throws -> (text: String, confidence: Float) {
        guard
            let imageSource = CGImageSourceCreateWithURL(url as CFURL, nil),
            let cgImage = CGImageSourceCreateImageAtIndex(imageSource, 0, nil)
        else {
            throw OcrScanError.unreadableImage(url)
        }

        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true

        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        try handler.perform([request])

        guard let observations = request.results, !observations.isEmpty else {
            return ("", 0)
        }

        var lines: [String] = []
        var confidences: [Float] = []
        for observation in observations {
            guard let candidate = observation.topCandidates(1).first else { continue }
            lines.append(candidate.string)
            confidences.append(candidate.confidence)
        }

        let text = lines.joined(separator: "\n")
        let averageConfidence = confidences.isEmpty
            ? 0
            : confidences.reduce(0, +) / Float(confidences.count)
        return (text, averageConfidence)
    }
}

/// `OcrEngine` conformance (the `gist-ffi` callback interface generated from
/// `#[uniffi::export(callback_interface)] trait OcrEngine`) used only at the
/// final commit step -- this is deliberately not a live recognizer. By the
/// time `OcrImportState.commit` constructs one of these, OCR has already
/// run once via `VisionPageRecognizer` (above) and a person has had the
/// chance to correct the text in the review screen; `recognizePage` simply
/// hands each page's final text and confidence back to
/// `Core::import_image_with_ocr` when Rust calls it in page order,
/// deliberately ignoring the pre-processed `imageBytes` parameter Rust
/// passes in -- that PNG has already served its purpose (giving Vision
/// something bounded/normalized to look at) during the scan phase;
/// re-running recognition on it here would silently discard any edits.
final class ReviewedOcrEngine: OcrEngine, @unchecked Sendable {
    // `@unchecked Sendable`: `pagesByIndex` is a `let`-bound dictionary of
    // `OcrReviewPage` (itself `Sendable`), fully populated in `init` and
    // never mutated afterward, so this class has no actual shared mutable
    // state for Sendable to protect against -- the checked-Sendable
    // synthesis Swift offers for structs isn't available to classes, hence
    // the explicit opt-out here rather than a real concurrency risk.
    private let pagesByIndex: [Int: OcrReviewPage]

    init(pages: [OcrReviewPage]) {
        pagesByIndex = Dictionary(uniqueKeysWithValues: pages.map { ($0.pageIndex, $0) })
    }

    func recognizePage(pageIndex: UInt32, imageBytes: Data) -> OcrPageResult? {
        guard let page = pagesByIndex[Int(pageIndex)] else { return nil }
        return OcrPageResult(pageIndex: pageIndex, text: page.text, confidence: page.confidence)
    }
}
