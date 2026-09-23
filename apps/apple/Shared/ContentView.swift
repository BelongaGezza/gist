import SwiftUI

/// Where `LibraryView`'s "Open in…" actions can navigate. A plain `String`
/// (bare item id) used to be enough when RSVP was the only reading mode;
/// the flow view (CLAUDE.md's Q8 -- decided 2026-09-12 in favor of the
/// SwiftUI-native prototype for v1.0, see `FlowViewSwiftUINative`) adds a
/// second destination for the *same* item id, so the id alone is no longer
/// sufficient to pick a destination view.
enum ReadingDestination: Hashable {
    case rsvp(itemId: String)
    case flow(itemId: String)
}

struct ContentView: View {
    @EnvironmentObject var core: CoreClient
    @EnvironmentObject var themeManager: ThemeManager
    @State private var navigationPath: [ReadingDestination] = []
    /// Root of the detail pane -- `.all` (the full library) or a single
    /// collection, driven by `SidebarView`'s selection. Kept as a
    /// non-optional `LibrarySelection` here (falling back to `.all` on
    /// deselection) so `detailRoot` doesn't need to handle a `nil` case;
    /// `SidebarView.selection` is `LibrarySelection?` only because
    /// `List(selection:)` requires an optional binding.
    @State private var sidebarSelection: LibrarySelection = .all

    var body: some View {
        NavigationSplitView {
            SidebarView(selection: Binding(
                get: { sidebarSelection },
                set: { sidebarSelection = $0 ?? .all }
            ))
        } detail: {
            NavigationStack(path: $navigationPath) {
                detailRoot
                    .navigationDestination(for: ReadingDestination.self) { destination in
                        switch destination {
                        case .rsvp(let itemId):
                            RsvpView(itemId: itemId)
                        case .flow(let itemId):
                            FlowReaderContainer<FlowViewSwiftUINative>(itemId: itemId)
                        }
                    }
            }
        }
        .onChange(of: sidebarSelection) { _, _ in
            // Switching sidebar roots pops any pushed RSVP view -- otherwise
            // the stack could show a reader pushed from a since-abandoned
            // root, which reads as a bug (right title, wrong content behind it).
            navigationPath.removeAll()
        }
        .task {
            // Reclaim anything a previous removal's best-effort file delete
            // left behind (PENDING_APPLE_CHANGES.md's W2/Q10 adoption entry)
            // before the library loads, so a freshly-launched app never
            // shows stale leftover storage. Runs once per launch.
            await core.sweepOrphanedFiles()
            await core.refresh()
        }
        .tint(themeManager.resolvedTheme.accent)
        .background(themeManager.resolvedTheme.background)
        .preferredColorScheme(
            themeManager.selection == .system ? nil : themeManager.resolvedTheme.colorScheme
        )
    }

    @ViewBuilder
    private var detailRoot: some View {
        switch sidebarSelection {
        case .all:
            LibraryView(navigationPath: $navigationPath)
        case .collection(let collection):
            CollectionDetailView(collection: collection, navigationPath: $navigationPath)
        }
    }
}
