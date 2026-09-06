import SwiftUI

struct LibraryView: View {
    @EnvironmentObject var core: CoreClient
    @State private var showImporter = false

    var body: some View {
        Group {
            if core.items.isEmpty && !core.isLoading {
                emptyState
            } else {
                itemList
            }
        }
        .navigationTitle("Library")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { showImporter = true } label: {
                    Label("Import", systemImage: "plus")
                }
            }
        }
        .fileImporter(
            isPresented: $showImporter,
            allowedContentTypes: [.plainText],
            allowsMultipleSelection: false
        ) { result in
            if case .success(let urls) = result, let url = urls.first {
                Task { await core.importTxt(url: url) }
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "book.closed")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No books yet")
                .font(.title2)
            Text("Import a .txt file to get started")
                .foregroundStyle(.secondary)
            Button("Import file…") { showImporter = true }
                .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var itemList: some View {
        List(core.items) { item in
            NavigationLink(value: item.id) {
                VStack(alignment: .leading) {
                    Text(item.title)
                        .font(.headline)
                    if !item.authors.isEmpty {
                        Text(item.authors.joined(separator: ", "))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }
}
