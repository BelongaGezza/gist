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
    @State private var showEncryptConfirm = false
    @State private var encryptSummary: EncryptItemsSummary?
    /// Only ever set when `RemoveItemsSummary.hasFailures` -- a clean removal
    /// (the common case) does not interrupt the user with a dialog just to
    /// say it worked. See `CoreClient.removeItems`'s doc comment.
    @State private var removeWarning: RemoveItemsSummary?
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
    /// Backs `.searchFocused` below so ⌘F can programmatically focus the
    /// `.searchable` field -- previously it was reachable only by clicking
    /// into it with the mouse/trackpad (see CLAUDE.md's M2 item-5 note).
    @FocusState private var isSearchFieldFocused: Bool

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

    // `body` used to be one ~270-line expression chaining the content Group,
    // ~9 view modifiers, an 8-item `.toolbar { }`, and 5 `.alert(...)`
    // calls -- a single expression large enough that the Swift type checker
    // timed out on it on GitHub's CI runner (though not, apparently, in
    // every local Xcode run -- see CLAUDE.md's N6 note). Splitting it into
    // `mainContent` + an extracted `@ToolbarContentBuilder` property +
    // two grouped alert-applying methods gives the type checker several
    // much smaller expressions to solve independently instead of one huge
    // one. No behavioral change -- same modifiers, same bindings, same
    // closures, just fewer of them chained in a single expression.
    var body: some View {
        withItemActionAlerts(
            withImportAlerts(
                mainContent
                    .toolbar { toolbarContent }
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
            )
        )
    }

    private var mainContent: some View {
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
        .searchFocused($isSearchFieldFocused)
        .background {
            // Invisible button purely to host the ⌘F shortcut -- standard
            // SwiftUI idiom for binding a keyboard shortcut to an action
            // that isn't itself a visible control. `.searchFocused` (the
            // declarative macOS/iOS 17+ counterpart to `.searchable`) does
            // the actual focus work; no manual NSResponder/first-responder
            // poking involved.
            Button("Focus Search") { isSearchFieldFocused = true }
                .keyboardShortcut("f", modifiers: .command)
                .hidden()
        }
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
    }

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
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
            Button { showEncryptConfirm = true } label: {
                Label("Encrypt", systemImage: "lock")
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
                    // Settings scene's Reading tab default (RSVP vs Flow
                    // View) -- the context menu's explicit "Open in
                    // Reader"/"Open in Flow View" items are unaffected.
                    navigationPath.append(ReadingSettings.shared.defaultMode.destination(for: id))
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

    /// Alerts related to bringing new content into the library: DRM
    /// rejection, a generic error surface, URL-paste import, and creating a
    /// new collection. Grouped separately from `withItemActionAlerts` purely
    /// to keep each modifier-chain expression small for the type checker --
    /// there's no functional relationship between the two groupings beyond
    /// "both are alerts this view presents."
    @ViewBuilder
    private func withImportAlerts<Content: View>(_ content: Content) -> some View {
        content
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
    }

    /// Alerts for actions taken on the current selection: remove, encrypt,
    /// and the encrypt result summary. See `withImportAlerts`'s doc comment
    /// for why these are split into a separate group.
    @ViewBuilder
    private func withItemActionAlerts<Content: View>(_ content: Content) -> some View {
        content
            .alert(
                selection.count == 1 ? "Remove 1 item?" : "Remove \(selection.count) items?",
                isPresented: $showRemoveConfirm
            ) {
                Button("Cancel", role: .cancel) {}
                Button("Remove", role: .destructive) {
                    let ids = Array(selection)
                    selection.removeAll()
                    // Was hardcoded `true` -- now reads the Settings-scene
                    // default (Storage tab), see `StorageSettings
                    // .deleteSourceFilesOnRemoval`'s doc comment for why
                    // `true` remains the out-of-the-box default.
                    let deleteSourceFiles = StorageSettings.shared.deleteSourceFilesOnRemoval
                    Task {
                        let summary = await core.removeItems(ids: ids, deleteSourceFiles: deleteSourceFiles)
                        if summary.hasFailures { removeWarning = summary }
                    }
                }
            } message: {
                // Matches the Windows wording exactly (`RemovePreview.LibraryIrreversibleLine`,
                // docs/windows-ui-spec.md §4.5/§10 item 1) -- removal is now a single, complete
                // delete of everything GIST holds for the item, never the user's own file. See
                // ADR-006's 2026-09-21 addendum for why the old two-button choice was dropped.
                Text(
                    selection.count == 1
                        ? "This permanently deletes this item, and GIST's stored copy of it, from this PC. It can't be undone. The original file you imported is not touched."
                        : "This permanently deletes these items, and GIST's stored copies of them, from this PC. It can't be undone. The original files you imported are not touched."
                )
            }
            .alert(
                selection.count == 1 ? "Encrypt 1 item?" : "Encrypt \(selection.count) items?",
                isPresented: $showEncryptConfirm
            ) {
                Button("Cancel", role: .cancel) {}
                Button("Encrypt") {
                    let ids = Array(selection)
                    Task { encryptSummary = await core.encryptItems(ids: ids) }
                }
            } message: {
                Text(
                    "Encrypts the selected item's content at rest (ADR-011/014). It stays fully readable afterward — RSVP and Flow View both continue to work — this only protects the file on disk. Only GIST's internal document data is affected, never your original file."
                )
            }
            .alert(
                "Encryption Result",
                isPresented: Binding(
                    get: { encryptSummary != nil },
                    set: { if !$0 { encryptSummary = nil } }
                )
            ) {
                Button("OK", role: .cancel) { encryptSummary = nil }
            } message: {
                Text(encryptSummary?.message ?? "")
            }
            .alert(
                "Some Files Could Not Be Deleted",
                isPresented: Binding(
                    get: { removeWarning != nil },
                    set: { if !$0 { removeWarning = nil } }
                )
            ) {
                Button("OK", role: .cancel) { removeWarning = nil }
            } message: {
                Text(removeWarning?.message ?? "")
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
                    Button("Open in Flow View") {
                        navigationPath.append(.flow(itemId: item.id))
                    }
                    Button("Manage Tags…") {
                        tagEditorTarget = TagEditorTarget(id: item.id, title: item.title)
                    }
                    if !item.contentEncrypted {
                        Button("Encrypt…") {
                            selection = [item.id]
                            showEncryptConfirm = true
                        }
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
