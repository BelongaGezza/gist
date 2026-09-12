import SwiftUI

struct ContentView: View {
    @EnvironmentObject var core: CoreClient
    @State private var navigationPath: [String] = []

    var body: some View {
        NavigationSplitView {
            SidebarView()
        } detail: {
            NavigationStack(path: $navigationPath) {
                LibraryView(navigationPath: $navigationPath)
                    .navigationDestination(for: String.self) { itemId in
                        RsvpView(itemId: itemId)
                    }
            }
        }
        .task { await core.refresh() }
    }
}
