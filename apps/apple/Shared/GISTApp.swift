import SwiftUI

@main
struct GISTApp: App {
    @StateObject private var core = CoreClient.shared
    @StateObject private var themeManager = ThemeManager.shared

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(core)
                .environmentObject(themeManager)
        }
        .commands {
            CommandGroup(replacing: .newItem) { }
        }
    }
}
