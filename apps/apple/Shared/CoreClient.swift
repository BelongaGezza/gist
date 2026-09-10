import Foundation
import Combine

/// Single touch-point for all FFI calls.
@MainActor
final class CoreClient: ObservableObject {
    static let shared = CoreClient()

    @Published var items: [LibraryItemVM] = []
    @Published var isLoading = false
    @Published var error: String?
    /// Set (instead of `error`) when an import fails specifically because the
    /// source file is DRM-protected, so the view layer can present a
    /// dedicated DRM alert rather than a generic import-failure message.
    @Published var drmProtectedFile: URL?

    private let core: GistCore?

    private init() {
        do {
            let supportDir = try FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            ).appendingPathComponent("GIST", isDirectory: true)

            let dbPath = supportDir.appendingPathComponent("gist.sqlite3").path
            let storageDir = supportDir.appendingPathComponent("storage", isDirectory: true).path

            core = try GistCore(dbPath: dbPath, storageDir: storageDir)
        } catch {
            self.core = nil
            self.error = "Failed to initialise GIST core: \(error)"
        }
    }

    func refresh() async {
        guard let core else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            let ffiItems = try core.listItems(offset: 0, limit: 500)
            items = ffiItems.map { item in
                LibraryItemVM(
                    id: item.id,
                    title: item.title ?? "Untitled",
                    authors: item.authors,
                    sourcePath: item.sourcePath
                )
            }
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// Import any supported file (txt, epub, docx) via the generic FFI
    /// import path, which sniffs the format from magic bytes / extension.
    func importFile(url: URL) async {
        guard let core else { return }
        drmProtectedFile = nil
        do {
            _ = try core.importFile(path: url.path)
            error = nil
        } catch let gistError as GistError {
            switch gistError {
            case .DrmProtected:
                drmProtectedFile = url
            case .Core, .InternalPanic:
                error = "\(gistError)"
            }
        } catch {
            self.error = "\(error)"
        }
        await refresh()
    }

    /// Fetches an RSVP session for `itemId` and decodes it. `startRsvp` is a
    /// one-shot FFI call — the returned token stream and pacing config are
    /// then driven entirely client-side by `RsvpPlayer` (see RsvpView.swift),
    /// per the "SwiftUI shell drives it" model documented on
    /// `gist_rsvp::RsvpSession`. Returns `nil` on failure and sets `error`.
    func startRsvp(itemId: String, wpm: UInt32) async -> RsvpSessionVM? {
        guard let core else { return nil }
        do {
            let json = try core.startRsvp(itemId: itemId, wpm: wpm)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(RsvpSessionVM.self, from: Data(json.utf8))
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    /// Persists the current token index so playback can resume later.
    func saveProgress(itemId: String, tokenIndex: Int) async {
        guard let core else { return }
        do {
            try core.saveProgress(itemId: itemId, tokenIndex: UInt64(tokenIndex))
        } catch {
            self.error = "\(error)"
        }
    }
}

struct LibraryItemVM: Identifiable {
    let id: String
    let title: String
    let authors: [String]
    let sourcePath: String?
}
