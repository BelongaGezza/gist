import Foundation
import Combine

/// Single touch-point for all FFI calls.
@MainActor
final class CoreClient: ObservableObject {
    static let shared = CoreClient()

    @Published var items: [LibraryItemVM] = []
    @Published var searchResults: [LibraryItemVM] = []
    @Published var isSearching = false
    @Published var collections: [CollectionVM] = []
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
            items = mapItems(ffiItems)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    private func mapItems(_ ffiItems: [FfiLibraryItem]) -> [LibraryItemVM] {
        ffiItems.map { item in
            LibraryItemVM(
                id: item.id,
                title: item.title ?? "Untitled",
                authors: item.authors,
                sourcePath: item.sourcePath
            )
        }
    }

    /// Runs an FTS5 search via `GistCore::search_items` and publishes into
    /// `searchResults`. An empty/whitespace-only query clears the results
    /// rather than round-tripping to FFI, since `LibraryView` treats a
    /// non-empty `searchResults` set as "showing search, not the full list."
    func search(query: String) async {
        guard let core else { return }
        guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            searchResults = []
            return
        }
        isSearching = true
        defer { isSearching = false }
        do {
            let ffiItems = try core.searchItems(query: query, limit: 200)
            searchResults = mapItems(ffiItems)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// Removes items from the library. `deleteSourceFiles` controls whether
    /// GIST's sandboxed ADR-006 copy is also deleted (the user's real,
    /// original file is never touched either way — see CLAUDE.md's
    /// copy-on-import note).
    func removeItems(ids: [String], deleteSourceFiles: Bool) async {
        guard let core else { return }
        do {
            try core.removeItems(ids: ids, deleteSourceFiles: deleteSourceFiles)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        await refresh()
    }

    /// Imports a web page by URL via `Core::import_url`, following the same
    /// DRM/error-handling shape as `importFile` (DRM is unreachable in
    /// practice for a web-fetch import, but the shared `GistError` type
    /// still carries the case, so we still branch on it for consistency).
    func importUrl(urlString: String) async {
        guard let core else { return }
        drmProtectedFile = nil
        do {
            _ = try core.importUrl(url: urlString)
            error = nil
        } catch let gistError as GistError {
            switch gistError {
            case .DrmProtected:
                drmProtectedFile = URL(string: urlString)
            case .Core, .InternalPanic:
                error = "\(gistError)"
            }
        } catch {
            self.error = "\(error)"
        }
        await refresh()
    }

    // MARK: - Collections & tags

    func createCollection(name: String) async {
        guard let core else { return }
        do {
            _ = try core.createCollection(name: name)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        await listCollections()
    }

    func listCollections() async {
        guard let core else { return }
        do {
            let ffiCollections = try core.listCollections()
            collections = ffiCollections.map {
                CollectionVM(id: $0.id, name: $0.name, createdAt: $0.createdAt)
            }
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func addItemToCollection(itemId: String, collectionId: String) async {
        guard let core else { return }
        do {
            try core.addItemToCollection(itemId: itemId, collectionId: collectionId)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func removeItemFromCollection(itemId: String, collectionId: String) async {
        guard let core else { return }
        do {
            try core.removeItemFromCollection(itemId: itemId, collectionId: collectionId)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func addTag(itemId: String, tagName: String) async {
        guard let core else { return }
        do {
            try core.addTag(itemId: itemId, tagName: tagName)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func removeTag(itemId: String, tagName: String) async {
        guard let core else { return }
        do {
            try core.removeTag(itemId: itemId, tagName: tagName)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func listTagsForItem(itemId: String) async -> [String] {
        guard let core else { return [] }
        do {
            return try core.listTagsForItem(itemId: itemId)
        } catch {
            self.error = "\(error)"
            return []
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

struct CollectionVM: Identifiable {
    let id: String
    let name: String
    let createdAt: Int64
}
