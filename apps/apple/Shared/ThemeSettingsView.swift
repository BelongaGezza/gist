import SwiftUI

/// Minimal, single-view appearance settings surface. Deliberately not a
/// full preferences window (see CLAUDE.md's M2 theme-engine task guidance
/// -- one focused view is enough for M2); reachable from the sidebar
/// toolbar via `SidebarView`.
struct ThemeSettingsView: View {
    @EnvironmentObject var themeManager: ThemeManager
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Picker(
                    "Theme",
                    selection: Binding(
                        get: { themeManager.selection },
                        set: { themeManager.setSelection($0) }
                    )
                ) {
                    ForEach(ThemeSelection.allCases) { selection in
                        Text(selection.displayName).tag(selection)
                    }
                }
                .pickerStyle(.inline)
                .labelsHidden()
            }
            .formStyle(.grouped)
            .navigationTitle("Appearance")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .frame(minWidth: 320, minHeight: 280)
    }
}
