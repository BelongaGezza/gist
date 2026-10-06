import Foundation

/// Turns the typed resource-limit cases of `GistError` (M7 R3) into an
/// honest, specific, path-free message for the import alert.
///
/// Matching is on the enum case only, never on message text: the Rust side
/// classifies which limit was hit (`gist_model::LimitKind`) and surfaces one
/// flat `GistError` case per kind. The message names the file the *user*
/// picked (its last path component, which is their own data) but never any
/// path or limit number from the core.
enum ImportLimitMessage {
    /// A user-presentable message if `error` is one of the resource-limit
    /// cases, otherwise `nil` (so callers keep their existing handling for
    /// every other case).
    static func message(for error: GistError, name: String?) -> String? {
        let subject = name ?? String(localized: "This file")
        switch error {
        case .ResourceLimitTooLarge:
            return String(localized: "\(subject) is too large for GIST's import limits. Try a smaller file.")
        case .ResourceLimitTooManyPages:
            return String(localized: "\(subject) has more pages than GIST can import. Try splitting it into smaller documents.")
        case .ResourceLimitTooManyEntries:
            return String(localized: "\(subject) contains too many internal parts for GIST's import limits, so it may be damaged or unusually complex.")
        case .ResourceLimitTooDeeplyNested:
            return String(localized: "\(subject) is structured too deeply for GIST's import limits, so it may be damaged or unusually complex.")
        case .ResourceLimitContentTooLarge:
            return String(localized: "The content of \(subject) is too large for GIST's import limits.")
        case .ResourceLimitTableTooLarge:
            return String(localized: "\(subject) contains a table that is too large for GIST's import limits.")
        case .ResourceLimitOther:
            return String(localized: "\(subject) exceeds GIST's import limits.")
        case .Core, .DrmProtected, .ChecksumMismatch, .PdfEncrypted, .PdfNoTextLayer,
             .PdfUnavailable, .InternalPanic:
            return nil
        }
    }
}
