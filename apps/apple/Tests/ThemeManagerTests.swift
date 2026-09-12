import SwiftUI
import XCTest
@testable import GIST

/// Exercises `ThemeManager`'s persistence and OS-follow resolution logic.
/// Every test uses a dedicated `UserDefaults` suite (never `.standard`), so
/// running the suite never pollutes a real user's prefs, and each test
/// tears its suite down afterwards.
@MainActor
final class ThemeManagerTests: XCTestCase {
    private func makeSuite(_ name: String) -> UserDefaults {
        let suiteName = "com.gist.tests.\(name).\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
        }
        return defaults
    }

    func testOsFollowResolvesToDarkWhenSystemIsDark() {
        let defaults = makeSuite("os-follow-dark")
        let manager = ThemeManager(defaults: defaults, initialSystemIsDark: true)

        XCTAssertEqual(manager.selection, .system)
        XCTAssertEqual(manager.resolvedTheme, .dark)
    }

    func testOsFollowResolvesToLightWhenSystemIsLight() {
        let defaults = makeSuite("os-follow-light")
        let manager = ThemeManager(defaults: defaults, initialSystemIsDark: false)

        XCTAssertEqual(manager.selection, .system)
        XCTAssertEqual(manager.resolvedTheme, .light)
    }

    func testExplicitSepiaSelectionOverridesOsFollow() {
        let defaults = makeSuite("explicit-sepia")
        // System is dark, but an explicit Sepia choice must win outright --
        // Sepia/OLED never participate in OS-follow.
        let manager = ThemeManager(defaults: defaults, initialSystemIsDark: true)

        manager.setSelection(.sepia)

        XCTAssertEqual(manager.selection, .sepia)
        XCTAssertEqual(manager.resolvedTheme, .sepia)
    }

    func testExplicitOledSelectionOverridesOsFollow() {
        let defaults = makeSuite("explicit-oled")
        let manager = ThemeManager(defaults: defaults, initialSystemIsDark: false)

        manager.setSelection(.oled)

        XCTAssertEqual(manager.selection, .oled)
        XCTAssertEqual(manager.resolvedTheme, .oled)
    }

    func testPersistedSelectionSurvivesFreshInstanceReadingSameSuite() {
        let defaults = makeSuite("persistence")

        let first = ThemeManager(defaults: defaults, initialSystemIsDark: false)
        first.setSelection(.oled)

        let second = ThemeManager(defaults: defaults, initialSystemIsDark: false)

        XCTAssertEqual(second.selection, .oled)
        XCTAssertEqual(second.resolvedTheme, .oled)
    }

    func testDefaultSelectionIsSystemWhenNothingPersisted() {
        let defaults = makeSuite("default-selection")
        let manager = ThemeManager(defaults: defaults, initialSystemIsDark: false)

        XCTAssertEqual(manager.selection, .system)
    }

    func testOledBackgroundIsPureBlackDistinctFromDark() {
        // OLED's whole point is true black -- guard against it ever being
        // collapsed into Dark's (slightly lifted) background color.
        XCTAssertEqual(Theme.oled.background, Color.black)
        XCTAssertNotEqual(Theme.dark.background, Theme.oled.background)
    }
}
