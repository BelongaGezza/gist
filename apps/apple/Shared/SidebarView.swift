import SwiftUI

/// Sidebar navigation: a fixed "Library" entry (all items) plus the user's
/// collections. Selection is driven purely by `List(selection:)` -- same
/// click-to-select constraint documented on `LibraryView.itemList` applies
/// here, though it's less of a trap for a sidebar since rows have no
/// competing gesture recognizers or navigation links to begin with.
struct SidebarView: View {
    @EnvironmentObject var core: CoreClient
    @Binding var selection: LibrarySelection?

    var body: some View {
        List(selection: $selection) {
            Label("Library", systemImage: "books.vertical")
                .tag(LibrarySelection.all)

            if !core.collections.isEmpty {
                Section("Collections") {
                    ForEach(core.collections) { collection in
                        Label(collection.name, systemImage: "folder")
                            .tag(LibrarySelection.collection(collection))
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .navigationTitle("GIST")
        .task { await core.listCollections() }
    }
}
