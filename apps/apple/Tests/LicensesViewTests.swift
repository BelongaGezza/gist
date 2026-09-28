import SwiftUI
import XCTest
@testable import GIST

/// R6: license attribution screen. Confirms the bundled `ThirdPartyNotices.txt`
/// resource (Copy Bundle Resources entry wired into `project.yml`, see
/// `LicensesView.swift`) is genuinely present and loadable in a built app --
/// not a dead link -- and that the view itself constructs without crashing,
/// matching this codebase's existing light-touch UI test conventions (pure
/// logic extracted to a `static`/testable function, plus one render-without-
/// crashing check for the view value itself; no XCUITest/window-server
/// dependency, same as `ThemeManagerTests`/`FlowViewTests`).
final class LicensesViewTests: XCTestCase {
    func testLoadNoticesTextReturnsBundledContentNotTheMissingResourceFallback() {
        let text = LicensesView.loadNoticesText()

        XCTAssertFalse(text.isEmpty)
        XCTAssertTrue(text.contains("Third-Party Notices"))
        // Spot-check a couple of crates that should always be present --
        // confirms this is the real generated list, not an empty/truncated
        // or stale bundled file.
        XCTAssertTrue(text.contains("rusqlite"))
        XCTAssertTrue(text.contains("uniffi"))
        // The fallback string used when the bundle resource can't be found.
        XCTAssertFalse(text.contains("unavailable in this build"))
    }

    func testLoadNoticesTextFallsBackGracefullyForABundleWithNoResource() {
        // A bundle that definitely doesn't contain ThirdPartyNotices.txt
        // (the test bundle itself, not the host app bundle) exercises the
        // missing-resource fallback path without needing to construct a
        // broken app bundle.
        let text = LicensesView.loadNoticesText(bundle: Bundle(for: LicensesViewTests.self))

        XCTAssertTrue(text.contains("unavailable in this build"))
    }

    @MainActor
    func testLicensesViewBodyRendersWithoutCrashing() {
        let view = LicensesView()
        // Forcing SwiftUI to evaluate `body` is enough to catch a crash from
        // a force-unwrap or similar in the view tree's construction -- no
        // window server / XCUITest needed, matching this codebase's existing
        // convention of not hosting real windows in unit tests.
        _ = view.body
    }
}
