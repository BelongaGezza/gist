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
}

struct LibraryItemVM: Identifiable {
    let id: String
    let title: String
    let authors: [String]
    let sourcePath: String?
}
