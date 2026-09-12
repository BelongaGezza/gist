import SwiftUI

/// Identifies which item a presented `TagEditorView` sheet targets. A plain
/// `(id, title)` tuple isn't `Identifiable`, which `.sheet(item:)` requires,
/// hence this tiny wrapper -- shared by `LibraryView` and
/// `CollectionDetailView`, the two call sites that present the sheet.
struct TagEditorTarget: Identifiable, Hashable {
    let id: String
    let title: String
}

/// Minimal sheet for viewing, adding, and removing tags on a single library
/// item, backed by `CoreClient.listTagsForItem`/`addTag`/`removeTag`.
/// Deliberately single-item only, like the existing toolbar "Open"/"Tags"
/// buttons (enabled only when exactly one item is selected) -- unlike bulk
/// removal or "add to collection", tagging N items at once doesn't have an
/// obvious single UI (what does the tag list show when items disagree?), so
/// this stays scoped to one item rather than guessing a bulk semantics.
struct TagEditorView: View {
    @EnvironmentObject var core: CoreClient
    @Environment(\.dismiss) private var dismiss

    let itemId: String
    let itemTitle: String

    @State private var tags: [String] = []
    @State private var newTag = ""
    @State private var isLoading = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Tags for \u{201C}\(itemTitle)\u{201D}")
                .font(.headline)

            Group {
                if isLoading {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                } else if tags.isEmpty {
                    Text("No tags yet.")
                        .foregroundStyle(.secondary)
                } else {
                    List {
                        ForEach(tags, id: \.self) { tag in
                            HStack {
                                Text(tag)
                                Spacer()
                                Button(role: .destructive) {
                                    Task { await remove(tag) }
                                } label: {
                                    Image(systemName: "xmark.circle.fill")
                                }
                                .buttonStyle(.borderless)
                            }
                        }
                    }
                }
            }
            .frame(minHeight: 120)

            HStack {
                TextField("New tag", text: $newTag)
                    .onSubmit { Task { await add() } }
                Button("Add") { Task { await add() } }
                    .disabled(newTag.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }

            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding()
        .frame(width: 320)
        .task { await load() }
    }

    private func load() async {
        isLoading = true
        tags = await core.listTagsForItem(itemId: itemId)
        isLoading = false
    }

    private func add() async {
        let name = newTag.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        newTag = ""
        await core.addTag(itemId: itemId, tagName: name)
        await load()
    }

    private func remove(_ tag: String) async {
        await core.removeTag(itemId: itemId, tagName: tag)
        await load()
    }
}
