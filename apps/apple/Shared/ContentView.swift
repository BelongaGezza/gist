import SwiftUI

struct ContentView: View {
    @EnvironmentObject var core: CoreClient

    var body: some View {
        NavigationSplitView {
            SidebarView()
        } detail: {
            NavigationStack {
                LibraryView()
                    .navigationDestination(for: String.self) { itemId in
                        RsvpView(itemId: itemId)
                    }
            }
        }
        .task { await core.refresh() }
    }
}
