import SwiftUI
import UniformTypeIdentifiers

/// Supported import file types: plain text/markdown, ePub, and DOCX.
/// Falls back gracefully if a UTType identifier isn't registered on the
/// running system (rather than crashing the file picker).
private let importableContentTypes: [UTType] = [
    .plainText,
    .text,
    UTType("org.idpf.epub-container"),
    UTType("org.openxmlformats.wordprocessingml.document"),
].compactMap { $0 }

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
            allowedContentTypes: importableContentTypes,
            allowsMultipleSelection: false
        ) { result in
            if case .success(let urls) = result, let url = urls.first {
                Task { await core.importFile(url: url) }
            }
        }
        .alert(
            "DRM-Protected Document",
            isPresented: Binding(
                get: { core.drmProtectedFile != nil },
                set: { if !$0 { core.drmProtectedFile = nil } }
            )
        ) {
            Button("OK", role: .cancel) { core.drmProtectedFile = nil }
        } message: {
            Text(
                "\(core.drmProtectedFile?.lastPathComponent ?? "This file") is protected by DRM and can't be imported. GIST never attempts to circumvent copy protection."
            )
        }
        .alert(
            "Import Failed",
            isPresented: Binding(
                get: { core.error != nil },
                set: { if !$0 { core.error = nil } }
            )
        ) {
            Button("OK", role: .cancel) { core.error = nil }
        } message: {
            Text(core.error ?? "")
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "book.closed")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No books yet")
                .font(.title2)
            Text("Import a .txt, .epub, or .docx file to get started")
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
