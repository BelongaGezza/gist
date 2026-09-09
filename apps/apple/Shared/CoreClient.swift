import Foundation
import Combine

/// Single touch-point for all FFI calls.
@MainActor
final class CoreClient: ObservableObject {
    static let shared = CoreClient()

    @Published var items: [LibraryItemVM] = []
    @Published var isLoading = false
    @Published var error: String?

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

    func importTxt(url: URL) async {
        guard let core else { return }
        do {
            _ = try core.importTxt(path: url.path)
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
