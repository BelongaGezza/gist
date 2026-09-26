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

        // Standard macOS Settings/Preferences window (⌘,). See
        // `SettingsView.swift` for the tabbed Reading/Typography/RSVP/
        // Import/Storage/About layout; each tab's persisted state lives in
        // its own `.shared` singleton (`AppSettings.swift`), so no
        // environment objects need injecting here.
        Settings {
            SettingsView()
        }
    }
}
