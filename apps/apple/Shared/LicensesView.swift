import SwiftUI

/// Third-party notices / licences screen, reachable from Settings -> About
/// (`AboutSettingsTab` in `SettingsView.swift`). Mirrors `ThemeSettingsView`'s
/// shape: a single-purpose `.sheet`-presented view with its own
/// `NavigationStack` and a "Done" confirmation-action toolbar button, rather
/// than folding this into the About tab's own `Form` -- the notices text is
/// long (187 Rust crates, see `docs/THIRD-PARTY.md`), so it needs its own
/// scrollable surface, not another `Form` section.
///
/// Content is genuinely bundled, not a dead link: `ThirdPartyNotices.txt`
/// (under `Resources/`, wired into `project.yml`'s Copy Bundle Resources for
/// `GISTmacOS`, the same pattern used for the `GISTTests` fixture per
/// `CLAUDE.md`'s M2 notes) is generated from `docs/THIRD-PARTY.md`'s Rust
/// dependency table and loaded at runtime via `Bundle.main`, so the app
/// carries its own copy rather than pointing out to GitHub.
struct LicensesView: View {
    @Environment(\.dismiss) private var dismiss

    /// Loaded once per presentation rather than cached as a stored property,
    /// since this view is only ever instantiated when the sheet is shown.
    private var noticesText: String {
        Self.loadNoticesText()
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                Text(noticesText)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
            }
            .navigationTitle("Third-Party Notices")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .frame(minWidth: 480, minHeight: 480)
    }

    /// Reads the bundled notices file. Separated out as a `static` function
    /// (rather than inline in `noticesText`) so `LicensesViewTests` can call
    /// it directly without instantiating the SwiftUI view.
    static func loadNoticesText(bundle: Bundle = .main) -> String {
        guard
            let url = bundle.url(forResource: "ThirdPartyNotices", withExtension: "txt"),
            let contents = try? String(contentsOf: url, encoding: .utf8)
        else {
            // Should be unreachable in a correctly built app (the resource
            // is a Copy Bundle Resources entry, not optional) -- but a
            // missing/misconfigured bundle resource should degrade to a
            // readable message rather than crash the Settings window.
            return String(
                localized: "Third-party notices are unavailable in this build. See docs/THIRD-PARTY.md in the GIST source repository."
            )
        }
        return contents
    }
}
