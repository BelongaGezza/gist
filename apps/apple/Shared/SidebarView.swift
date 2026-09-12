import SwiftUI

struct SidebarView: View {
    @EnvironmentObject var themeManager: ThemeManager
    @State private var showThemeSettings = false

    var body: some View {
        List {
            Label("Library", systemImage: "books.vertical")
        }
        .listStyle(.sidebar)
        .navigationTitle("GIST")
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button {
                    showThemeSettings = true
                } label: {
                    Label("Appearance", systemImage: "paintpalette")
                }
            }
        }
        .sheet(isPresented: $showThemeSettings) {
            ThemeSettingsView()
        }
    }
}
