import SwiftUI

struct ContentView: View {
    @EnvironmentObject var core: CoreClient

    var body: some View {
        NavigationSplitView {
            SidebarView()
        } detail: {
            LibraryView()
        }
        .task { await core.refresh() }
    }
}
