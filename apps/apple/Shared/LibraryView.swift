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

/// Pure filtering logic factored out of `LibraryView` so it's unit-testable
/// without constructing SwiftUI state (a view's `@State` can't be set from
/// outside the view). See `GISTTests` for coverage.
enum LibraryFiltering {
    /// Whether `searchText` is non-empty, i.e. `displayedItems` should render
    /// search results instead of the full item list.
    static func isSearchActive(searchText: String) -> Bool {
        !searchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    static func displayedItems(
        searchText: String,
        items: [LibraryItemVM],
        searchResults: [LibraryItemVM]
    ) -> [LibraryItemVM] {
        isSearchActive(searchText: searchText) ? searchResults : items
    }

    /// Applies `order` to `items`. `.dateAddedNewest` is a no-op: every FFI
    /// list call (`listItems`/`searchItems`/`listItemsByTag`/
    /// `listItemsInCollection`) already returns newest-first straight from
    /// SQL's `ORDER BY created_at DESC`, so reversing that in place gives
    /// oldest-first without a second query.
    static func sorted(_ items: [LibraryItemVM], by order: LibrarySortOrder) -> [LibraryItemVM] {
        switch order {
        case .dateAddedNewest:
            return items
        case .dateAddedOldest:
            return items.reversed()
        case .titleAZ:
            return items.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
        case .titleZA:
            return items.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedDescending }
        case .authorAZ:
            return items.sorted {
                ($0.authors.first ?? "").localizedCaseInsensitiveCompare($1.authors.first ?? "") == .orderedAscending
            }
        }
    }
}

enum LibrarySortOrder: String, CaseIterable, Identifiable {
    case dateAddedNewest
    case dateAddedOldest
    case titleAZ
    case titleZA
    case authorAZ

    var id: String { rawValue }

    var label: String {
        switch self {
        case .dateAddedNewest: return "Date Added (Newest)"
        case .dateAddedOldest: return "Date Added (Oldest)"
        case .titleAZ: return "Title (A–Z)"
        case .titleZA: return "Title (Z–A)"
        case .authorAZ: return "Author (A–Z)"
        }
    }
}

struct LibraryView: View {
    @EnvironmentObject var core: CoreClient
    @EnvironmentObject var themeManager: ThemeManager
    @Binding var navigationPath: [ReadingDestination]
    @State private var showImporter = false
    @State private var searchText = ""
    @State private var searchTask: Task<Void, Never>?
    @State private var selection = Set<String>()
    @State private var showRemoveConfirm = false
    @State private var showUrlImportAlert = false
    @State private var urlToImport = ""
    @State private var showNewCollectionAlert = false
    @State private var newCollectionName = ""
    @State private var tagEditorTarget: TagEditorTarget?
    @State private var sortOrder: LibrarySortOrder = .dateAddedNewest
    /// The tag currently filtering the list, or `nil` for no filter. Mutually
    /// exclusive with an active search (picking a tag clears `searchText`
    /// and vice versa) -- combining "search within this tag" isn't supported,
    /// to keep the two controls' interaction unambiguous.
    @State private var tagFilter: String?
    @State private var tagFilteredItems: [LibraryItemVM] = []

    /// Whether `searchText` is non-empty, i.e. `itemList` should render
    /// `core.searchResults` instead of the full `core.items` list.
    private var isSearchActive: Bool {
        LibraryFiltering.isSearchActive(searchText: searchText)
    }

    private var displayedItems: [LibraryItemVM] {
        let base = tagFilter != nil
            ? tagFilteredItems
            : LibraryFiltering.displayedItems(searchText: searchText, items: core.items, searchResults: core.searchResults)
        return LibraryFiltering.sorted(base, by: sortOrder)
    }

    var body: some View {
        Group {
            if displayedItems.isEmpty && !core.isLoading {
                if tagFilter != nil {
                    AnyView(noTagResultsState)
                } else {
                    isSearchActive ? AnyView(noResultsState) : AnyView(emptyState)
                }
            } else {
                itemList
            }
        }
        .background(themeManager.resolvedTheme.background)
        .foregroundStyle(themeManager.resolvedTheme.foreground)
        .scrollContentBackground(.hidden)
        .navigationTitle("Library")
        .searchable(text: $searchText, prompt: "Search library")
        .onChange(of: searchText) { _, newValue in
            // A tag filter and an active search are mutually exclusive (see
            // `tagFilter`'s doc comment) -- starting a search cancels
            // whichever tag filter was active.
            if LibraryFiltering.isSearchActive(searchText: newValue) {
                tagFilter = nil
            }
            // Debounce: cancel any in-flight wait and start a fresh one, same
            // Task-based cancellation idiom RsvpPlayer.play() uses for its
            // per-token sleep loop (see RsvpView.swift).
            searchTask?.cancel()
            searchTask = Task {
                try? await Task.sleep(nanoseconds: 300_000_000)
                guard !Task.isCancelled else { return }
                await core.search(query: newValue)
            }
        }
        .task { await core.listCollections() }
        .task { await core.listAllTags() }
        // Keyed on tagFilter so picking a different tag (or clearing it)
        // reloads; nil clears tagFilteredItems back to empty since
        // displayedItems ignores it once tagFilter is nil anyway.
        .task(id: tagFilter) {
            if let tagFilter {
                tagFilteredItems = await core.listItemsByTag(tagName: tagFilter)
            } else {
                tagFilteredItems = []
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { showImporter = true } label: {
                    Label("Import File", systemImage: "plus")
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Button { showUrlImportAlert = true } label: {
                    Label("Import URL", systemImage: "link")
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Picker("Sort By", selection: $sortOrder) {
                        ForEach(LibrarySortOrder.allCases) { order in
                            Text(order.label).tag(order)
                        }
                    }
                } label: {
                    Label("Sort", systemImage: "arrow.up.arrow.down")
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button {
                        tagFilter = nil
                    } label: {
                        if tagFilter == nil {
                            Label("All Tags", systemImage: "checkmark")
                        } else {
                            Text("All Tags")
                        }
                    }
                    if !core.allTags.isEmpty {
                        Divider()
                        ForEach(core.allTags, id: \.self) { tag in
                            Button {
                                tagFilter = tag
                                searchText = ""
                            } label: {
                                if tagFilter == tag {
                                    Label(tag, systemImage: "checkmark")
                                } else {
                                    Text(tag)
                                }
                            }
                        }
                    }
                } label: {
                    Label(
                        "Filter",
                        systemImage: tagFilter == nil
                            ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill"
                    )
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    ForEach(core.collections) { collection in
                        Button(collection.name) {
                            let ids = selection
                            Task {
                                for id in ids {
                                    await core.addItemToCollection(itemId: id, collectionId: collection.id)
                                }
                            }
                        }
                    }
                    if !core.collections.isEmpty {
                        Divider()
                    }
                    Button("New Collection…") { showNewCollectionAlert = true }
                } label: {
                    Label("Add to Collection", systemImage: "folder.badge.plus")
                }
                .disabled(selection.isEmpty)
            }
            ToolbarItem(placement: .primaryAction) {
                Button(role: .destructive) { showRemoveConfirm = true } label: {
                    Label("Remove", systemImage: "trash")
                }
                .disabled(selection.isEmpty)
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    if let id = selection.first {
                        navigationPath.append(.rsvp(itemId: id))
                    }
                } label: {
                    Label("Open", systemImage: "book")
                }
                .disabled(selection.count != 1)
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    if let id = selection.first, let item = displayedItems.first(where: { $0.id == id }) {
                        tagEditorTarget = TagEditorTarget(id: item.id, title: item.title)
                    }
                } label: {
                    Label("Tags", systemImage: "tag")
                }
                .disabled(selection.count != 1)
            }
        }
        .sheet(item: $tagEditorTarget) { target in
            TagEditorView(itemId: target.id, itemTitle: target.title)
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
        .alert("Paste URL to Import", isPresented: $showUrlImportAlert) {
            TextField("https://example.com/article", text: $urlToImport)
            Button("Cancel", role: .cancel) { urlToImport = "" }
            Button("Import") {
                let urlString = urlToImport
                urlToImport = ""
                Task { await core.importUrl(urlString: urlString) }
            }
        } message: {
            Text("GIST fetches the page, extracts the readable content, and adds it to your library.")
        }
        .alert("New Collection", isPresented: $showNewCollectionAlert) {
            TextField("Collection name", text: $newCollectionName)
            Button("Cancel", role: .cancel) { newCollectionName = "" }
            Button("Create") {
                let name = newCollectionName
                newCollectionName = ""
                Task { await core.createCollection(name: name) }
            }
        }
        .alert(
            selection.count == 1 ? "Remove 1 item?" : "Remove \(selection.count) items?",
            isPresented: $showRemoveConfirm
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Remove from Library") {
                let ids = Array(selection)
                selection.removeAll()
                Task { await core.removeItems(ids: ids, deleteSourceFiles: false) }
            }
            Button("Also Delete Original File", role: .destructive) {
                let ids = Array(selection)
                selection.removeAll()
                Task { await core.removeItems(ids: ids, deleteSourceFiles: true) }
            }
        } message: {
            Text(
                "\"Remove from Library\" only removes GIST's record and its sandboxed copy — your original file, wherever it lives, is never touched. \"Also Delete Original File\" additionally deletes GIST's own imported copy (see ADR-006); it never deletes the original either."
            )
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

    private var noResultsState: some View {
        VStack(spacing: 16) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No matches")
                .font(.title2)
            Text("No items match \"\(searchText)\"")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var noTagResultsState: some View {
        VStack(spacing: 16) {
            Image(systemName: "tag")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No items")
                .font(.title2)
            Text("No items are tagged \"\(tagFilter ?? "")\"")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// Row content is deliberately NOT a `NavigationLink` here. Wrapping the
    /// whole row in one, inside a `List(selection:)`, is a known macOS
    /// SwiftUI trap: the link swallows the single click as a navigation
    /// push, so the click never lands as a selection and `selection` can
    /// never become non-empty -- the toolbar Remove button then looks
    /// permanently (and inexplicably) disabled.
    ///
    /// A first fix attempt added `.onTapGesture(count: 2)` per row to open
    /// the book on double-click -- that turned out to be a second version of
    /// the same trap: attaching any `onTapGesture` directly to `List` row
    /// content competes with, and in practice suppresses, the List's native
    /// single-click-to-select handling on macOS (user-reported: clicking a
    /// row did nothing, selection never happened at all). Rows are now
    /// *pure* content with no gesture recognizer of their own, so the List's
    /// built-in click-to-select is untouched; opening a book is driven
    /// entirely off the `selection` binding instead -- either the toolbar
    /// "Open" button (enabled when exactly one item is selected) or the
    /// context menu's "Open in Reader" (right-click uses a separate gesture
    /// recognizer, so it never conflicted with selection).
    private var itemList: some View {
        List(displayedItems, selection: $selection) { item in
            LibraryRowContent(item: item)
                .contextMenu {
                    Button("Open in Reader") {
                        navigationPath.append(.rsvp(itemId: item.id))
                    }
                    // Q8 prototypes (CLAUDE.md M2): two rendering approaches
                    // for the not-yet-built flow view, side by side for
                    // comparison.
                    Button("Open in Flow View (SwiftUI)") {
                        navigationPath.append(.flowSwiftUI(itemId: item.id))
                    }
                    Button("Open in Flow View (TextKit 2)") {
                        navigationPath.append(.flowTextKit2(itemId: item.id))
                    }
                    Button("Manage Tags…") {
                        tagEditorTarget = TagEditorTarget(id: item.id, title: item.title)
                    }
                    Divider()
                    Button("Remove…", role: .destructive) {
                        selection = [item.id]
                        showRemoveConfirm = true
                    }
                }
        }
    }
}
