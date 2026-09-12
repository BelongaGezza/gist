import SwiftUI

/// Where `LibraryView`'s "Open in…" actions can navigate. A plain `String`
/// (bare item id) used to be enough when RSVP was the only reading mode;
/// the flow-view prototypes (CLAUDE.md's Q8) add two more destinations for
/// the *same* item id, so the id alone is no longer sufficient to pick a
/// destination view.
enum ReadingDestination: Hashable {
    case rsvp(itemId: String)
    case flowSwiftUI(itemId: String)
    case flowTextKit2(itemId: String)
}

struct ContentView: View {
    @EnvironmentObject var core: CoreClient
    @State private var navigationPath: [ReadingDestination] = []

    var body: some View {
        NavigationSplitView {
            SidebarView()
        } detail: {
            NavigationStack(path: $navigationPath) {
                LibraryView(navigationPath: $navigationPath)
                    .navigationDestination(for: ReadingDestination.self) { destination in
                        switch destination {
                        case .rsvp(let itemId):
                            RsvpView(itemId: itemId)
                        case .flowSwiftUI(let itemId):
                            FlowReaderContainer<FlowViewSwiftUINative>(itemId: itemId)
                        case .flowTextKit2(let itemId):
                            FlowReaderContainer<FlowViewTextKit2>(itemId: itemId)
                        }
                    }
            }
        }
        .task { await core.refresh() }
    }
}
