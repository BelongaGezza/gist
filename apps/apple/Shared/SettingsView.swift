import SwiftUI

/// The app's native macOS Settings/Preferences window (`Settings { }` scene,
/// wired up in `GISTApp.swift`, opened via ⌘, or the app menu's "Settings…"
/// item -- the standard macOS mechanism, not a bespoke window). A tabbed
/// `Form`-per-domain layout, the conventional shape for a multi-section
/// macOS preferences window.
///
/// Each tab's persisted state lives in its own `ObservableObject` in
/// `AppSettings.swift` (`ReadingSettings`/`TypographyDefaults`/
/// `RsvpDefaults`/`ImportDefaults`/`StorageSettings`), each a `.shared`
/// singleton mirroring `ThemeManager`'s shape. This view reads them directly
/// via `@ObservedObject` on the singleton rather than requiring them to be
/// injected as environment objects from `GISTApp`/`ContentView` -- keeps this
/// whole settings surface a self-contained addition with no changes needed
/// to the existing environment-object wiring chain.
struct SettingsView: View {
    var body: some View {
        TabView {
            ReadingSettingsTab()
                .tabItem { Label("Reading", systemImage: "book") }
            TypographySettingsTab()
                .tabItem { Label("Typography", systemImage: "textformat.size") }
            RsvpSettingsTab()
                .tabItem { Label("RSVP", systemImage: "text.word.spacing") }
            ImportSettingsTab()
                .tabItem { Label("Import", systemImage: "square.and.arrow.down") }
            StorageSettingsTab()
                .tabItem { Label("Storage", systemImage: "internaldrive") }
            AboutSettingsTab()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 480, height: 360)
    }
}

// MARK: - Reading

private struct ReadingSettingsTab: View {
    @ObservedObject private var settings = ReadingSettings.shared

    var body: some View {
        Form {
            Picker(
                "Default reading mode",
                selection: Binding(
                    get: { settings.defaultMode },
                    set: { settings.setDefaultMode($0) }
                )
            ) {
                ForEach(ReadingModeDefault.allCases) { mode in
                    Text(mode.displayName).tag(mode)
                }
            }
            .pickerStyle(.radioGroup)

            Text(
                "Used when opening an item without picking a mode explicitly (the Library toolbar's Open button). \"Open in Reader\" and \"Open in Flow View\" in the context menu always override this."
            )
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
        .padding()
    }
}

// MARK: - Typography

private struct TypographySettingsTab: View {
    @ObservedObject private var defaults = TypographyDefaults.shared

    var body: some View {
        Form {
            Section("Size") {
                HStack {
                    Slider(
                        value: $defaults.fontSize,
                        in: TypographySettings.range,
                        step: 1
                    )
                    Text("\(Int(defaults.fontSize)) pt")
                        .monospacedDigit()
                        .frame(width: 48, alignment: .trailing)
                }
            }

            Section("Font") {
                Picker("Font", selection: $defaults.fontDesign) {
                    ForEach(ReadingFontDesign.allCases) { design in
                        Text(design.label).tag(design)
                    }
                }
                .pickerStyle(.radioGroup)
            }

            Section("Line Spacing") {
                Picker("Line Spacing", selection: $defaults.lineSpacing) {
                    ForEach(LineSpacingOption.allCases) { option in
                        Text(option.label).tag(option)
                    }
                }
                .pickerStyle(.radioGroup)
            }

            Text("Applies to newly opened documents in Flow View. Already-open sessions keep whatever the \"Aa\" menu was last set to for that session.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
        .padding()
    }
}

// MARK: - RSVP

private struct RsvpSettingsTab: View {
    @ObservedObject private var defaults = RsvpDefaults.shared

    var body: some View {
        Form {
            Section("Speed") {
                HStack {
                    Slider(
                        value: Binding(
                            get: { Double(defaults.defaultWpm) },
                            set: { defaults.defaultWpm = Int($0.rounded()) }
                        ),
                        in: 100...1000,
                        step: 10
                    )
                    Text("\(defaults.defaultWpm) WPM")
                        .monospacedDigit()
                        .frame(width: 72, alignment: .trailing)
                }
            }

            Section {
                Toggle("Pause longer on punctuation", isOn: $defaults.pauseOnPunctuation)
                Text(
                    "Not yet wired to playback -- GIST's pacing engine currently always applies its sentence/comma/paragraph pauses. This preference is saved for when a per-session on/off switch lands."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}

// MARK: - Import

private struct ImportSettingsTab: View {
    @ObservedObject private var defaults = ImportDefaults.shared

    var body: some View {
        Form {
            Toggle("Automatically encrypt newly imported items", isOn: $defaults.autoEncryptOnImport)
            Text(
                "Encrypts each item's content at rest (ADR-011/014) right after it's imported. It stays fully readable afterward -- this only protects the stored file on disk, and never touches your original file."
            )
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
        .padding()
    }
}

// MARK: - Storage

private struct StorageSettingsTab: View {
    @ObservedObject private var settings = StorageSettings.shared

    /// Read-only display of where GIST stores its library -- computed the
    /// same way `CoreClient.init()` computes its own storage directory, but
    /// independently, since this is purely informational and shouldn't
    /// require threading a value out of `CoreClient`.
    private var storageLocation: String {
        guard
            let supportDir = try? FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: false
            )
        else {
            // (R5b localisation) Read via `Text(storageLocation)`, which
            // takes the returned value, not a literal.
            return String(localized: "Unavailable")
        }
        return supportDir.appendingPathComponent("GIST", isDirectory: true).path
    }

    var body: some View {
        Form {
            Section {
                Toggle("Delete GIST's stored copy when removing items", isOn: $settings.deleteSourceFilesOnRemoval)
                Text(
                    "Removing an item from the Library always deletes GIST's own database record for it. This controls whether GIST's separate, sandboxed stored copy (never your original file) is deleted immediately too. Turning this off only delays that deletion until the next launch's automatic cleanup -- an unreferenced stored copy doesn't stay indefinitely either way."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }

            Section("Library Location") {
                Text(storageLocation)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        }
        .formStyle(.grouped)
        .padding()
    }
}

// MARK: - About

private struct AboutSettingsTab: View {
    // (R5b localisation) Read via `Text(versionString)`, which takes the
    // returned value, not a literal.
    private var versionString: String {
        let shortVersion = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String
        switch (shortVersion, build) {
        case let (.some(v), .some(b)): return String(localized: "Version \(v) (\(b))")
        case let (.some(v), nil): return String(localized: "Version \(v)")
        default: return String(localized: "Version unavailable (debug build)")
        }
    }

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "book.pages")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("GIST")
                .font(.title2)
                .bold()
            Text(versionString)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text("An RSVP-style reading app.")
                .font(.callout)
            Text("Licensed under the MIT License.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Link("View source on GitHub", destination: URL(string: "https://github.com/BelongaGezza/gist")!)
                .font(.caption)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }
}
