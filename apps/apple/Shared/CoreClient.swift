import Foundation
import Combine

/// Single touch-point for all FFI calls.
/// In M0, this uses stub data until the xcframework is linked.
@MainActor
final class CoreClient: ObservableObject {
    static let shared = CoreClient()

    @Published var items: [LibraryItemVM] = []
    @Published var isLoading = false
    @Published var error: String?

    private init() {}

    func refresh() async {
        isLoading = true
        defer { isLoading = false }
        // TODO: call GistCore.listItems() once xcframework is linked
        items = []
    }

    func importTxt(url: URL) async {
        // TODO: call GistCore.importTxt() once xcframework is linked
        await refresh()
    }
}

struct LibraryItemVM: Identifiable {
    let id: String
    let title: String
    let authors: [String]
    let sourcePath: String?
}
