import SwiftUI

@main
struct GISTApp: App {
    @StateObject private var core = CoreClient.shared

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(core)
        }
        .commands {
            CommandGroup(replacing: .newItem) { }
        }
    }
}
