import SwiftUI

struct ContentView: View {
    @EnvironmentObject var core: CoreClient
    @State private var navigationPath: [String] = []
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
                    .navigationDestination(for: String.self) { itemId in
                        RsvpView(itemId: itemId)
                    }
            }
        }
        .onChange(of: sidebarSelection) { _, _ in
            // Switching sidebar roots pops any pushed RSVP view -- otherwise
            // the stack could show a reader pushed from a since-abandoned
            // root, which reads as a bug (right title, wrong content behind it).
            navigationPath.removeAll()
        }
        .task { await core.refresh() }
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
