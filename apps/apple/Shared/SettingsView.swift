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
                    // A bare `Slider(value:in:step:)` with no `title:`
                    // closure has no accessible name of its own -- VoiceOver
                    // would announce only a bare percentage with no
                    // indication of what it controls.
                    .accessibilityLabel("Default font size")
                    .accessibilityValue("\(Int(defaults.fontSize)) points")
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
                    .accessibilityLabel("Default reading speed")
                    .accessibilityValue("\(defaults.defaultWpm) words per minute")
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
    @ObservedObject private var core = CoreClient.shared

    @State private var usage: StorageUsage?
    @State private var isComputingUsage = false
    @State private var isVerifyingIntegrity = false
    @State private var integritySummary: LibraryIntegritySummary?

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

            Section("Storage Usage") {
                if let usage {
                    LabeledContent("Original files", value: usage.originalsFormatted)
                    LabeledContent("Reading data", value: usage.blobFormatted)
                    LabeledContent("Total", value: usage.totalFormatted)
                } else if isComputingUsage {
                    HStack {
                        ProgressView()
                        Text("Calculating…")
                            .foregroundStyle(.secondary)
                    }
                } else {
                    Text("Not yet calculated.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Button("Recalculate") {
                    Task { await refreshUsage() }
                }
                .disabled(isComputingUsage)
            }

            Section("Library Integrity") {
                Text(
                    "Checks every item's stored files against their recorded checksums (ADR-013). This only reports results -- it never deletes or changes anything."
                )
                .font(.caption)
                .foregroundStyle(.secondary)

                if isVerifyingIntegrity {
                    HStack {
                        ProgressView()
                        Text("Verifying…")
                            .foregroundStyle(.secondary)
                    }
                } else if let summary = integritySummary {
                    Label {
                        Text(summary.message)
                    } icon: {
                        Image(systemName: summary.hasFailures ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    }
                    .foregroundStyle(summary.hasFailures ? .orange : .secondary)

                    if let detail = summary.detailMessage {
                        Text(detail)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                Button("Verify Library Integrity") {
                    Task { await runIntegrityCheck() }
                }
                .disabled(isVerifyingIntegrity)
            }
        }
        .formStyle(.grouped)
        .padding()
        .task { await refreshUsage() }
    }

    private func refreshUsage() async {
        isComputingUsage = true
        usage = await Task.detached(priority: .utility) { StorageUsage.compute() }.value
        isComputingUsage = false
    }

    private func runIntegrityCheck() async {
        isVerifyingIntegrity = true
        let outcomes = await core.verifyLibraryIntegrity()
        integritySummary = LibraryIntegritySummary(outcomes: outcomes)
        isVerifyingIntegrity = false
    }
}

/// Disk-usage breakdown for the app's storage directory
/// (`<Application Support>/GIST/storage` -- the same root `CoreClient`
/// points `GistCore` at), broken into the two categories `docs/
/// m4-agent-roles.md`'s R4 spec calls for: the ADR-006 `originals/`
/// sandboxed-copy directory, and everything else directly in `storage/`
/// (the ADR-007 `<id>.json`/`<id>.tokens.json` IR blobs plus their ADR-013
/// `.blake3` checksum sidecars -- there is no separate "IR blob
/// subdirectory" on disk, they live flat alongside `originals/`).
///
/// Computed via plain `FileManager` directory enumeration rather than a new
/// FFI export -- per the role spec, this is simple enough not to need
/// walking the DB, and it's purely a reporting concern the Rust core has no
/// reason to track itself.
private struct StorageUsage {
    let originalsBytes: Int64
    let blobBytes: Int64

    var totalBytes: Int64 { originalsBytes + blobBytes }

    private static let formatter: ByteCountFormatter = {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        return formatter
    }()

    var originalsFormatted: String { Self.formatter.string(fromByteCount: originalsBytes) }
    var blobFormatted: String { Self.formatter.string(fromByteCount: blobBytes) }
    var totalFormatted: String { Self.formatter.string(fromByteCount: totalBytes) }

    /// Walks `<Application Support>/GIST/storage` off the main actor
    /// (called via `Task.detached` from `StorageSettingsTab.refreshUsage`).
    /// Returns an all-zero breakdown, not `nil`, if the directory doesn't
    /// exist yet (e.g. a fresh install with nothing imported) -- that's "no
    /// usage yet," not a failure to report.
    static func compute() -> StorageUsage {
        guard
            let supportDir = try? FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: false
            )
        else {
            return StorageUsage(originalsBytes: 0, blobBytes: 0)
        }

        let storageDir = supportDir
            .appendingPathComponent("GIST", isDirectory: true)
            .appendingPathComponent("storage", isDirectory: true)

        var isDirectory: ObjCBool = false
        guard
            FileManager.default.fileExists(atPath: storageDir.path, isDirectory: &isDirectory),
            isDirectory.boolValue
        else {
            return StorageUsage(originalsBytes: 0, blobBytes: 0)
        }

        let originalsDir = storageDir.appendingPathComponent("originals", isDirectory: true)
        let originalsBytes = recursiveFileSize(of: originalsDir)
        let blobBytes = topLevelFileSize(of: storageDir, excludingDirectoryNamed: "originals")
        return StorageUsage(originalsBytes: originalsBytes, blobBytes: blobBytes)
    }

    /// Sums the size of every regular file directly inside `directory`,
    /// skipping subdirectories (`originals/`, accounted for separately by
    /// `recursiveFileSize`). This is where `<id>.json`/`<id>.tokens.json`
    /// (ADR-007) and their `.blake3` checksum sidecars (ADR-013) live.
    private static func topLevelFileSize(of directory: URL, excludingDirectoryNamed excluded: String) -> Int64 {
        guard
            let entries = try? FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: [.fileSizeKey, .isRegularFileKey],
                options: [.skipsHiddenFiles]
            )
        else { return 0 }

        var total: Int64 = 0
        for entry in entries where entry.lastPathComponent != excluded {
            guard
                let values = try? entry.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey]),
                values.isRegularFile == true,
                let size = values.fileSize
            else { continue }
            total += Int64(size)
        }
        return total
    }

    /// Sums every regular file's size under `directory`, recursively --
    /// `originals/` is flat today, but this stays correct if that ever
    /// changes.
    private static func recursiveFileSize(of directory: URL) -> Int64 {
        guard
            let enumerator = FileManager.default.enumerator(
                at: directory,
                includingPropertiesForKeys: [.fileSizeKey, .isRegularFileKey],
                options: [.skipsHiddenFiles]
            )
        else { return 0 }

        var total: Int64 = 0
        for case let fileURL as URL in enumerator {
            guard
                let values = try? fileURL.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey]),
                values.isRegularFile == true,
                let size = values.fileSize
            else { continue }
            total += Int64(size)
        }
        return total
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

    /// R6: license attribution screen. A `@State` flag + `.sheet`, matching
    /// `SidebarView`'s `showThemeSettings`/`ThemeSettingsView` pattern,
    /// rather than a `NavigationLink` -- this `Form`-less tab has no
    /// `NavigationStack` of its own to push into.
    @State private var showLicenses = false

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
            Button("Third-Party Notices…") { showLicenses = true }
                .font(.caption)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
        .sheet(isPresented: $showLicenses) {
            LicensesView()
        }
    }
}
