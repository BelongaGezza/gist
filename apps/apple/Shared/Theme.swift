import SwiftUI
#if os(macOS)
import AppKit
#endif

/// The user-facing theme choice, persisted verbatim. `.system` is the
/// "OS-follow" option -- it resolves to `.light` or `.dark` at render time
/// (see `ThemeManager.resolvedTheme`). Sepia and OLED are always explicit
/// choices: they never participate in OS-follow, since a reader who picked
/// a warm sepia page or a true-black OLED screen wants it regardless of
/// what the system appearance is doing.
enum ThemeSelection: String, CaseIterable, Identifiable, Codable {
    case system
    case light
    case dark
    case sepia
    case oled

    var id: String { rawValue }

    // (R5b localisation) Read via `Text(selection.displayName)` in
    // ThemeSettingsView, which takes the returned value, not a literal.
    var displayName: String {
        switch self {
        case .system: return String(localized: "Follow System")
        case .light: return String(localized: "Light")
        case .dark: return String(localized: "Dark")
        case .sepia: return String(localized: "Sepia")
        case .oled: return String(localized: "OLED (True Black)")
        }
    }
}

/// A concrete, resolved theme -- what views actually render. Every
/// `ThemeSelection` case maps 1:1 to one of these except `.system`, which
/// `ThemeManager` resolves to `.light` or `.dark` based on the current
/// system appearance.
enum Theme: String, CaseIterable {
    case light
    case dark
    case sepia
    case oled

    /// Background color for full-screen reading surfaces (library list,
    /// RSVP word display). OLED is pure `#000000`, deliberately distinct
    /// from Dark's slightly-lifted background -- true black is the entire
    /// point of an OLED mode (battery + eye strain on OLED panels), so it
    /// must never just reuse Dark's palette.
    var background: Color {
        switch self {
        case .light: return Color(red: 1.0, green: 1.0, blue: 1.0)
        case .dark: return Color(red: 0.110, green: 0.110, blue: 0.118)
        case .sepia: return Color(red: 0.957, green: 0.925, blue: 0.847)
        case .oled: return Color.black
        }
    }

    /// Primary text/foreground color.
    var foreground: Color {
        switch self {
        case .light: return Color(red: 0.102, green: 0.102, blue: 0.102)
        case .dark: return Color(red: 0.949, green: 0.949, blue: 0.969)
        case .sepia: return Color(red: 0.357, green: 0.275, blue: 0.212)
        case .oled: return Color(red: 0.949, green: 0.949, blue: 0.969)
        }
    }

    /// Accent color for interactive controls (buttons, sliders, selection).
    var accent: Color {
        switch self {
        case .light: return .blue
        case .dark: return .blue
        case .sepia: return Color(red: 0.545, green: 0.353, blue: 0.169)
        case .oled: return .blue
        }
    }

    /// The `ColorScheme` a resolved theme should force via
    /// `.preferredColorScheme`, so system chrome (buttons, text fields,
    /// scrollbars) matches instead of clashing with a light-on-dark or
    /// dark-on-light custom palette.
    var colorScheme: ColorScheme {
        switch self {
        case .light, .sepia: return .light
        case .dark, .oled: return .dark
        }
    }
}

/// Owns the user's theme selection: persists it (UserDefaults -- no need
/// for anything fancier per CLAUDE.md's M2 guidance) and resolves it to a
/// concrete `Theme` for views to consume via `@EnvironmentObject`. Mirrors
/// `CoreClient`'s shape (`@MainActor final class ... : ObservableObject`,
/// a `.shared` singleton) but deliberately holds no Rust/FFI state -- theme
/// is pure UI state and has no business living in `CoreClient`.
@MainActor
final class ThemeManager: ObservableObject {
    static let shared = ThemeManager()

    private static let storageKey = "com.gist.themeSelection"

    @Published private(set) var selection: ThemeSelection
    /// Whether the OS is currently in dark mode. Only consulted when
    /// `selection == .system`. Kept as its own published property (rather
    /// than resolving `NSApp.effectiveAppearance` inline in a computed var)
    /// so it can be seeded deterministically in unit tests instead of
    /// depending on whatever appearance the test machine happens to be in.
    @Published private(set) var systemIsDark: Bool

    private let defaults: UserDefaults
    #if os(macOS)
    private var appearanceObservation: NSKeyValueObservation?
    #endif

    /// - Parameters:
    ///   - defaults: Defaults store to persist into. Tests should pass a
    ///     dedicated `UserDefaults(suiteName:)` instance rather than
    ///     `.standard`, so test runs never touch a real user's prefs.
    ///   - initialSystemIsDark: Overrides the initial `systemIsDark` reading
    ///     instead of asking `NSApp`. Tests use this to make OS-follow
    ///     resolution deterministic; production code leaves it `nil` so the
    ///     real system appearance is read.
    init(defaults: UserDefaults = .standard, initialSystemIsDark: Bool? = nil) {
        self.defaults = defaults
        if let raw = defaults.string(forKey: Self.storageKey),
           let saved = ThemeSelection(rawValue: raw) {
            selection = saved
        } else {
            selection = .system
        }

        if let initialSystemIsDark {
            systemIsDark = initialSystemIsDark
        } else {
            #if os(macOS)
            systemIsDark = NSApp?.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            #else
            systemIsDark = false
            #endif
        }

        observeSystemAppearance()
    }

    /// Updates the selection and persists it immediately.
    func setSelection(_ newValue: ThemeSelection) {
        selection = newValue
        defaults.set(newValue.rawValue, forKey: Self.storageKey)
    }

    /// The concrete theme views should render. Resolves `.system` against
    /// `systemIsDark`; every other selection maps directly to its `Theme`.
    var resolvedTheme: Theme {
        switch selection {
        case .light: return .light
        case .dark: return .dark
        case .sepia: return .sepia
        case .oled: return .oled
        case .system: return systemIsDark ? .dark : .light
        }
    }

    #if os(macOS)
    private func observeSystemAppearance() {
        appearanceObservation = NSApp?.observe(\.effectiveAppearance, options: [.new]) { [weak self] app, _ in
            let isDark = app.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            Task { @MainActor in
                self?.systemIsDark = isDark
            }
        }
    }
    #else
    private func observeSystemAppearance() {}
    #endif
}
