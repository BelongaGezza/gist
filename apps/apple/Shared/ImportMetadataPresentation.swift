import Foundation

// Pure presentation logic for the M8 R4 import-fidelity and library-
// completeness surfaces (SC-01 encoding prompt, SC-03 tracked-changes
// notice, SC-06 web metadata, SC-15 word count / reading time). Kept free of
// SwiftUI state so it is unit-testable, like `LibraryFiltering`.

// MARK: - SC-01 encoding choices

/// One selectable text encoding. `label` is the WHATWG label passed straight
/// to `importFileWithEncoding`; `name` is the user-visible name.
struct TextEncodingOption: Hashable, Identifiable {
    let label: String
    let name: String
    var id: String { label }
}

enum TextEncodingChoices {
    /// Curated short list offered in the prompt.
    static let curated: [TextEncodingOption] = [
        .init(label: "utf-8", name: "UTF-8"),
        .init(label: "utf-16le", name: "UTF-16 (little-endian)"),
        .init(label: "utf-16be", name: "UTF-16 (big-endian)"),
        .init(label: "windows-1252", name: "Windows-1252 (Western European)"),
        .init(label: "windows-1251", name: "Windows-1251 (Cyrillic)"),
        .init(label: "iso-8859-2", name: "ISO-8859-2 (Central European)"),
        .init(label: "iso-8859-15", name: "ISO-8859-15 (Western European, Euro)"),
        .init(label: "koi8-r", name: "KOI8-R (Cyrillic)"),
        .init(label: "shift_jis", name: "Shift_JIS (Japanese)"),
        .init(label: "gbk", name: "GBK (Simplified Chinese)"),
        .init(label: "big5", name: "Big5 (Traditional Chinese)"),
        .init(label: "euc-kr", name: "EUC-KR (Korean)"),
    ]

    /// The curated list, with the detected encoding prepended when it is not
    /// already present (so the picker can always show the current guess).
    static func options(including detected: String) -> [TextEncodingOption] {
        if curated.contains(where: { $0.label.caseInsensitiveCompare(detected) == .orderedSame }) {
            return curated
        }
        return [TextEncodingOption(label: detected, name: detected)] + curated
    }

    /// The option label matching `detected` (case-insensitive), falling back
    /// to `detected` itself.
    static func selection(for detected: String) -> String {
        curated.first { $0.label.caseInsensitiveCompare(detected) == .orderedSame }?.label ?? detected
    }

    /// Whether the import flow should stop and ask the user (SC-01).
    static func needsPrompt(uncertain: Bool, lossy: Bool) -> Bool { uncertain || lossy }

    /// File extensions the encoding prompt applies to (plain-text imports).
    static func isPlainTextFile(_ url: URL) -> Bool {
        ["txt", "text", "md", "markdown"].contains(url.pathExtension.lowercased())
    }
}

/// A plain-text import paused awaiting the user's encoding choice.
struct PendingEncodingImport: Identifiable {
    let url: URL
    let detectedName: String
    let lossy: Bool
    var id: URL { url }
}

// MARK: - SC-15 word count / reading time

enum ReadingTimeFormat {
    /// "1,234 words · ≈ 5 min", or nil for an empty document (0 words).
    static func caption(wordCount: Int, seconds: Int) -> String? {
        guard wordCount > 0 else { return nil }
        let words = wordCount.formatted(.number)
        let minutes = minutes(fromSeconds: seconds)
        return String(localized: "\(words) words · ≈ \(minutes) min")
    }

    static func accessibilityLabel(wordCount: Int, seconds: Int) -> String? {
        guard wordCount > 0 else { return nil }
        let words = wordCount.formatted(.number)
        let minutes = minutes(fromSeconds: seconds)
        return String(localized: "\(words) words, about \(minutes) minutes to read")
    }

    /// Whole minutes, rounded up, never below 1 for a non-empty document.
    static func minutes(fromSeconds seconds: Int) -> Int {
        max(1, (max(seconds, 0) + 59) / 60)
    }
}

// MARK: - SC-06 web metadata

/// Presentation model for the item-info sheet (SC-03 / SC-06).
struct ItemInfoVM: Equatable {
    let title: String
    let author: String?
    let sourceType: String
    let trackedChanges: Bool
    let importDate: String?
    let language: String?
    let publishedDate: String?
    let retrievedDate: String?
    let canonicalUrl: String?
}

enum ItemInfoPresentation {
    /// Only http(s) URLs may become a clickable link; anything else (or an
    /// unparseable string) stays plain text. Never opened automatically.
    static func safeLink(_ raw: String?) -> URL? {
        guard let raw = raw?.trimmingCharacters(in: .whitespacesAndNewlines),
            !raw.isEmpty, let url = URL(string: raw),
            let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
            url.host?.isEmpty == false
        else { return nil }
        return url
    }

    /// Locale-formatted date when `raw` is ISO 8601 (date or date-time),
    /// otherwise the raw string unchanged.
    static func displayDate(_ raw: String?) -> String? {
        guard let raw = raw?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty
        else { return nil }
        let full = ISO8601DateFormatter()
        full.formatOptions = [.withInternetDateTime]
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let dateOnly = ISO8601DateFormatter()
        dateOnly.formatOptions = [.withFullDate]
        if let d = full.date(from: raw) ?? fractional.date(from: raw) {
            return d.formatted(date: .abbreviated, time: .shortened)
        }
        if let d = dateOnly.date(from: raw) {
            return d.formatted(date: .abbreviated, time: .omitted)
        }
        return raw
    }

    /// Whether the web-specific section has anything to show.
    static func hasWebDetails(_ info: ItemInfoVM) -> Bool {
        info.publishedDate != nil || info.retrievedDate != nil || info.canonicalUrl != nil
    }
}
