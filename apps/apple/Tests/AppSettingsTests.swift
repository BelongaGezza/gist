import XCTest
@testable import GIST

/// Exercises the persisted-default `ObservableObject`s backing the Settings
/// scene (`AppSettings.swift`). Same convention as `ThemeManagerTests`: every
/// test uses a dedicated `UserDefaults` suite (never `.standard`), so running
/// the suite never touches a real user's prefs, and each test tears its
/// suite down afterwards.
@MainActor
final class AppSettingsTests: XCTestCase {
    private func makeSuite(_ name: String) -> UserDefaults {
        let suiteName = "com.gist.tests.appsettings.\(name).\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
        }
        return defaults
    }

    // MARK: - ReadingSettings

    func testReadingSettingsDefaultsToRsvpWhenNothingPersisted() {
        let defaults = makeSuite("reading-default")
        let settings = ReadingSettings(defaults: defaults)

        XCTAssertEqual(settings.defaultMode, .rsvp)
    }

    func testReadingSettingsPersistsAcrossFreshInstance() {
        let defaults = makeSuite("reading-persistence")
        let first = ReadingSettings(defaults: defaults)
        first.setDefaultMode(.flow)

        let second = ReadingSettings(defaults: defaults)

        XCTAssertEqual(second.defaultMode, .flow)
    }

    func testReadingModeDefaultBuildsExpectedDestination() {
        XCTAssertEqual(ReadingModeDefault.rsvp.destination(for: "abc"), .rsvp(itemId: "abc"))
        XCTAssertEqual(ReadingModeDefault.flow.destination(for: "abc"), .flow(itemId: "abc"))
    }

    // MARK: - TypographyDefaults

    func testTypographyDefaultsMatchTypographySettingsOwnDefaultsWhenNothingPersisted() {
        let defaults = makeSuite("typography-default")
        let settings = TypographyDefaults(defaults: defaults)
        let plain = TypographySettings()

        XCTAssertEqual(settings.fontSize, plain.fontSize)
        XCTAssertEqual(settings.fontDesign, plain.fontDesign)
        XCTAssertEqual(settings.lineSpacing, plain.lineSpacing)
        XCTAssertEqual(settings.current, plain)
    }

    func testTypographyDefaultsPersistAcrossFreshInstance() {
        let defaults = makeSuite("typography-persistence")
        let first = TypographyDefaults(defaults: defaults)
        first.fontSize = 22
        first.fontDesign = .serif
        first.lineSpacing = .relaxed

        let second = TypographyDefaults(defaults: defaults)

        XCTAssertEqual(second.fontSize, 22)
        XCTAssertEqual(second.fontDesign, .serif)
        XCTAssertEqual(second.lineSpacing, .relaxed)
        XCTAssertEqual(
            second.current,
            TypographySettings(fontSize: 22, fontDesign: .serif, lineSpacing: .relaxed)
        )
    }

    func testTypographyDefaultsIgnoresOutOfRangeStoredFontSize() {
        // Simulates a corrupted/out-of-range prefs value (e.g. from a future
        // downgrade or a stray manual edit) -- should fall back to
        // `TypographySettings`'s own default rather than propagate garbage.
        let defaults = makeSuite("typography-out-of-range")
        defaults.set(999.0, forKey: "com.gist.settings.typography.fontSize")

        let settings = TypographyDefaults(defaults: defaults)

        XCTAssertEqual(settings.fontSize, TypographySettings().fontSize)
    }

    // MARK: - RsvpDefaults

    func testRsvpDefaultsDefaultWpmIs250WhenNothingPersisted() {
        let defaults = makeSuite("rsvp-default")
        let settings = RsvpDefaults(defaults: defaults)

        XCTAssertEqual(settings.defaultWpm, 250)
        XCTAssertTrue(settings.pauseOnPunctuation)
    }

    func testRsvpDefaultsPersistAcrossFreshInstance() {
        let defaults = makeSuite("rsvp-persistence")
        let first = RsvpDefaults(defaults: defaults)
        first.defaultWpm = 400
        first.pauseOnPunctuation = false

        let second = RsvpDefaults(defaults: defaults)

        XCTAssertEqual(second.defaultWpm, 400)
        XCTAssertFalse(second.pauseOnPunctuation)
    }

    func testRsvpDefaultsClampsWpmToSliderBounds() {
        let defaults = makeSuite("rsvp-clamp")
        let settings = RsvpDefaults(defaults: defaults)

        settings.defaultWpm = 50
        XCTAssertEqual(settings.defaultWpm, 100)

        settings.defaultWpm = 5000
        XCTAssertEqual(settings.defaultWpm, 1000)
    }

    // MARK: - ImportDefaults

    func testImportDefaultsAutoEncryptDefaultsToFalse() {
        let defaults = makeSuite("import-default")
        let settings = ImportDefaults(defaults: defaults)

        XCTAssertFalse(settings.autoEncryptOnImport)
    }

    func testImportDefaultsPersistAcrossFreshInstance() {
        let defaults = makeSuite("import-persistence")
        let first = ImportDefaults(defaults: defaults)
        first.autoEncryptOnImport = true

        let second = ImportDefaults(defaults: defaults)

        XCTAssertTrue(second.autoEncryptOnImport)
    }

    // MARK: - StorageSettings

    func testStorageSettingsDeleteSourceFilesDefaultsToTrue() {
        // Matches the exact hardcoded behavior `LibraryView`'s single
        // "Remove" button had before this setting existed (see
        // `StorageSettings.deleteSourceFilesOnRemoval`'s doc comment) -- a
        // fresh install must behave identically.
        let defaults = makeSuite("storage-default")
        let settings = StorageSettings(defaults: defaults)

        XCTAssertTrue(settings.deleteSourceFilesOnRemoval)
    }

    func testStorageSettingsPersistsAcrossFreshInstance() {
        let defaults = makeSuite("storage-persistence")
        let first = StorageSettings(defaults: defaults)
        first.deleteSourceFilesOnRemoval = false

        let second = StorageSettings(defaults: defaults)

        XCTAssertFalse(second.deleteSourceFilesOnRemoval)
    }
}
