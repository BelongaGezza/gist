import SwiftUI

struct SidebarView: View {
    var body: some View {
        List {
            Label("Library", systemImage: "books.vertical")
        }
        .listStyle(.sidebar)
        .navigationTitle("GIST")
    }
}
