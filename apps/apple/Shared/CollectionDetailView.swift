import SwiftUI

/// Shows the items in a single collection, via
/// `CoreClient.listItemsInCollection` (`GistCore::list_items_in_collection`).
/// Mirrors `LibraryView`'s selection/row/removal patterns where it clearly
/// reduces duplication (shared `LibraryRowContent`, same toolbar-driven
/// "Open" idiom, same tag editor sheet) without trying to unify the two into
/// one generic view -- removal semantics differ (remove from *this
/// collection* vs. remove from the *library*), so that part stays separate.
///
/// Rows carry no `NavigationLink`/`onTapGesture`, same reasoning as
/// `LibraryView.itemList`'s doc comment: either one silently breaks macOS's
/// native `List(selection:)` click-to-select.
struct CollectionDetailView: View {
    @EnvironmentObject var core: CoreClient
    let collection: CollectionVM
    @Binding var navigationPath: [String]

    @State private var items: [LibraryItemVM] = []
    @State private var isLoading = false
    @State private var selection = Set<String>()
    @State private var tagEditorTarget: TagEditorTarget?
    @State private var showRemoveConfirm = false

    var body: some View {
        Group {
            if items.isEmpty && !isLoading {
                emptyState
            } else {
                itemList
            }
        }
        .navigationTitle(collection.name)
        // Keyed on the collection's id so switching collections in the
        // sidebar reloads this view's items rather than reusing stale state.
        .task(id: collection.id) { await load() }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    if let id = selection.first {
                        navigationPath.append(id)
                    }
                } label: {
                    Label("Open", systemImage: "book")
                }
                .disabled(selection.count != 1)
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    if let id = selection.first, let item = items.first(where: { $0.id == id }) {
                        tagEditorTarget = TagEditorTarget(id: item.id, title: item.title)
                    }
                } label: {
                    Label("Tags", systemImage: "tag")
                }
                .disabled(selection.count != 1)
            }
            ToolbarItem(placement: .primaryAction) {
                Button(role: .destructive) { showRemoveConfirm = true } label: {
                    Label("Remove from Collection", systemImage: "folder.badge.minus")
                }
                .disabled(selection.isEmpty)
            }
        }
        .sheet(item: $tagEditorTarget) { target in
            TagEditorView(itemId: target.id, itemTitle: target.title)
        }
        .alert(
            selection.count == 1
                ? "Remove 1 item from \u{201C}\(collection.name)\u{201D}?"
                : "Remove \(selection.count) items from \u{201C}\(collection.name)\u{201D}?",
            isPresented: $showRemoveConfirm
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Remove", role: .destructive) {
                let ids = Array(selection)
                selection.removeAll()
                Task {
                    for id in ids {
                        await core.removeItemFromCollection(itemId: id, collectionId: collection.id)
                    }
                    await load()
                }
            }
        } message: {
            Text("This only removes the item from this collection -- it stays in your library.")
        }
        .alert(
            "Error",
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
            Image(systemName: "folder")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No items in this collection")
                .font(.title2)
            Text("Add items from the Library view's \u{201C}Add to Collection\u{201D} menu.")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var itemList: some View {
        List(items, selection: $selection) { item in
            LibraryRowContent(item: item)
                .contextMenu {
                    Button("Open in Reader") {
                        navigationPath.append(item.id)
                    }
                    Button("Manage Tags\u{2026}") {
                        tagEditorTarget = TagEditorTarget(id: item.id, title: item.title)
                    }
                    Divider()
                    Button("Remove from Collection", role: .destructive) {
                        selection = [item.id]
                        showRemoveConfirm = true
                    }
                }
        }
    }

    private func load() async {
        isLoading = true
        items = await core.listItemsInCollection(collectionId: collection.id)
        isLoading = false
    }
}
