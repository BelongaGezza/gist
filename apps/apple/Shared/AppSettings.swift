import SwiftUI

// This file collects the persisted-default `ObservableObject`s backing the
// app's `Settings` scene (see `SettingsView.swift`). Each mirrors
// `ThemeManager`'s established shape (`Theme.swift`): a `@MainActor final
// class ... : ObservableObject`, a `.shared` singleton, plain `UserDefaults`
// persistence (no need for anything fancier per CLAUDE.md's M2 guidance),
// and an `init(defaults:)` test seam so `AppSettingsTests` can exercise
// persistence against a dedicated `UserDefaults(suiteName:)` instance rather
// than `.standard`, exactly like `ThemeManagerTests`.
//
// Each object is deliberately independent (not one giant `AppSettings`
// blob) so a view that only cares about one domain -- `RsvpView` reading a
// default WPM, `FlowReaderContainer` seeding its typography state -- doesn't
// need to import or depend on unrelated settings, matching how `ThemeManager`
// is already separate from `CoreClient`.

// MARK: - Reading

/// Which reading destination `LibraryView`'s ambiguous "open" actions (the
/// toolbar Open button; a future default double-click/Return action) should
/// use when the user hasn't explicitly picked one via the context menu's
/// separate "Open in Reader" / "Open in Flow View" items -- those stay
/// explicit per-action choices, unaffected by this default.
enum ReadingModeDefault: String, CaseIterable, Identifiable, Codable {
    case rsvp
    case flow

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .rsvp: return "RSVP"
        case .flow: return "Flow View"
        }
    }

    /// Builds the `ReadingDestination` (see `ContentView.swift`) this default
    /// resolves to for a given item id.
    func destination(for itemId: String) -> ReadingDestination {
        switch self {
        case .rsvp: return .rsvp(itemId: itemId)
        case .flow: return .flow(itemId: itemId)
        }
    }
}

@MainActor
final class ReadingSettings: ObservableObject {
    static let shared = ReadingSettings()

    private static let defaultModeKey = "com.gist.settings.reading.defaultMode"

    @Published private(set) var defaultMode: ReadingModeDefault

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        if let raw = defaults.string(forKey: Self.defaultModeKey),
            let mode = ReadingModeDefault(rawValue: raw)
        {
            defaultMode = mode
        } else {
            defaultMode = .rsvp
        }
    }

    func setDefaultMode(_ mode: ReadingModeDefault) {
        defaultMode = mode
        defaults.set(mode.rawValue, forKey: Self.defaultModeKey)
    }
}

// MARK: - Typography

/// Persisted defaults for the flow view's "Aa" typography menu
/// (`TypographySettings`/`ReadingFontDesign`/`LineSpacingOption`, all defined
/// in `FlowDocumentModel.swift`), so a reader can set a persistent baseline
/// without first opening a document and adjusting it there. `current` mirrors
/// `TypographySettings`'s own field defaults exactly when nothing has been
/// persisted yet, so a fresh install behaves identically to before this
/// settings scene existed.
@MainActor
final class TypographyDefaults: ObservableObject {
    static let shared = TypographyDefaults()

    private static let fontSizeKey = "com.gist.settings.typography.fontSize"
    private static let fontDesignKey = "com.gist.settings.typography.fontDesign"
    private static let lineSpacingKey = "com.gist.settings.typography.lineSpacing"

    @Published var fontSize: Double {
        didSet { defaults.set(fontSize, forKey: Self.fontSizeKey) }
    }
    @Published var fontDesign: ReadingFontDesign {
        didSet { defaults.set(fontDesign.rawValue, forKey: Self.fontDesignKey) }
    }
    @Published var lineSpacing: LineSpacingOption {
        didSet { defaults.set(lineSpacing.rawValue, forKey: Self.lineSpacingKey) }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        let storedFontSize = defaults.double(forKey: Self.fontSizeKey)
        fontSize =
            TypographySettings.range.contains(storedFontSize)
            ? storedFontSize : TypographySettings().fontSize

        if let raw = defaults.string(forKey: Self.fontDesignKey),
            let design = ReadingFontDesign(rawValue: raw)
        {
            fontDesign = design
        } else {
            fontDesign = TypographySettings().fontDesign
        }

        if let raw = defaults.string(forKey: Self.lineSpacingKey),
            let spacing = LineSpacingOption(rawValue: raw)
        {
            lineSpacing = spacing
        } else {
            lineSpacing = TypographySettings().lineSpacing
        }
    }

    /// A `TypographySettings` value built from the current persisted
    /// defaults -- what `FlowReaderContainer` seeds a freshly-opened
    /// document's per-session typography state from.
    var current: TypographySettings {
        TypographySettings(fontSize: fontSize, fontDesign: fontDesign, lineSpacing: lineSpacing)
    }
}

// MARK: - RSVP

@MainActor
final class RsvpDefaults: ObservableObject {
    static let shared = RsvpDefaults()

    private static let defaultWpmKey = "com.gist.settings.rsvp.defaultWpm"
    /// Placeholder-shaped key for a punctuation-pause toggle. **Not wired to
    /// any pacing behavior in this pass** -- `RsvpWallClockEngine.
    /// tokenDurationMs` (RsvpView.swift) currently always applies
    /// `RsvpConfigVM`'s sentence/comma/paragraph/numeral pause multipliers
    /// unconditionally; there is no on/off switch anywhere in the pacing
    /// engine (Swift or Rust) for this setting to gate yet. Added now, ahead
    /// of the wiring, so a concurrent RSVP-polish workstream has a stable
    /// name/key to reconcile against instead of inventing its own -- see
    /// this role's handback report for the exact key/name to check against
    /// whatever that role lands.
    private static let pauseOnPunctuationKey = "com.gist.settings.rsvp.pauseOnPunctuation"

    /// Default reading speed a freshly-opened RSVP session starts at.
    /// Clamped to `RsvpView`'s existing slider bounds (100...1000) and
    /// defaults to 250, matching `RsvpPlayer.load`'s previous hardcoded
    /// `initialWpm` default exactly -- a fresh install's behavior is
    /// unchanged until a user visits Settings.
    @Published var defaultWpm: Int {
        didSet {
            let clamped = min(max(defaultWpm, 100), 1000)
            if clamped != defaultWpm { defaultWpm = clamped }
            defaults.set(defaultWpm, forKey: Self.defaultWpmKey)
        }
    }

    /// See `pauseOnPunctuationKey`'s doc comment -- persisted, surfaced in
    /// Settings, but not yet load-bearing on any pacing decision.
    @Published var pauseOnPunctuation: Bool {
        didSet { defaults.set(pauseOnPunctuation, forKey: Self.pauseOnPunctuationKey) }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let storedWpm = defaults.object(forKey: Self.defaultWpmKey) as? Int
        defaultWpm = storedWpm.map { min(max($0, 100), 1000) } ?? 250
        if defaults.object(forKey: Self.pauseOnPunctuationKey) != nil {
            pauseOnPunctuation = defaults.bool(forKey: Self.pauseOnPunctuationKey)
        } else {
            pauseOnPunctuation = true
        }
    }
}

// MARK: - Import

@MainActor
final class ImportDefaults: ObservableObject {
    static let shared = ImportDefaults()

    private static let autoEncryptOnImportKey = "com.gist.settings.import.autoEncryptOnImport"

    /// When true, `CoreClient.importFile`/`importUrl` encrypt a newly
    /// imported item at rest (ADR-011/014) immediately after a successful
    /// import, via the same `encryptItems` path the Library's "Encrypt"
    /// action uses. Defaults to `false` -- new imports land plaintext by
    /// default today (see `CoreClient.init`'s ADR-014 doc comment); this is
    /// an opt-in, not a behavior change for anyone who doesn't visit
    /// Settings.
    @Published var autoEncryptOnImport: Bool {
        didSet { defaults.set(autoEncryptOnImport, forKey: Self.autoEncryptOnImportKey) }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        autoEncryptOnImport = defaults.bool(forKey: Self.autoEncryptOnImportKey)
    }
}

// MARK: - Storage

@MainActor
final class StorageSettings: ObservableObject {
    static let shared = StorageSettings()

    private static let deleteSourceFilesOnRemovalKey =
        "com.gist.settings.storage.deleteSourceFilesOnRemoval"

    /// The default `deleteSourceFiles` value `LibraryView`'s single "Remove"
    /// action passes to `CoreClient.removeItems`. **This promotes the value
    /// that used to be hardcoded `true` at that one call site** -- see this
    /// role's handback report for the full history: `LibraryView` used to
    /// offer a genuine per-action choice between "Remove from Library"
    /// (`false`) and "Also Delete Original File" (`true`); commit `8ba23dd`
    /// (2026-09-23), following ADR-006's 2026-09-21 addendum, collapsed that
    /// to a single hardcoded-`true` "Remove" button, reasoning that the old
    /// choice was misleading -- `Core::sweep_orphaned_files` deletes any
    /// unreferenced `originals/` copy on the next launch regardless, so
    /// "keep" only ever meant "until next launch," never a real keep.
    ///
    /// That reasoning is still correct and applies equally to this
    /// UserDefaults-backed setting: setting this to `false` does **not**
    /// give a durable "always keep GIST's copy" guarantee -- it only delays
    /// deletion of an unreferenced stored copy until the next app launch's
    /// orphan sweep. `SettingsView`'s Storage tab states this explicitly
    /// rather than repeating the exact illusory-choice framing the ADR
    /// rejected. Defaults to `true`, matching current shipped behavior
    /// exactly -- a fresh install is unaffected until a user opens Settings.
    @Published var deleteSourceFilesOnRemoval: Bool {
        didSet {
            defaults.set(deleteSourceFilesOnRemoval, forKey: Self.deleteSourceFilesOnRemovalKey)
        }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        if defaults.object(forKey: Self.deleteSourceFilesOnRemovalKey) != nil {
            deleteSourceFilesOnRemoval = defaults.bool(forKey: Self.deleteSourceFilesOnRemovalKey)
        } else {
            deleteSourceFilesOnRemoval = true
        }
    }
}
