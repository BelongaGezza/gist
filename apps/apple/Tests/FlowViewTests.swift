import SwiftUI
import XCTest
@testable import GIST

/// Exercises the flow view's pure/logic-level pieces that don't require
/// standing up `FlowViewSwiftUINative`'s SwiftUI body: scroll-position
/// persistence, `ReadingProgress`'s clamping, typography option mappings,
/// and the plain-text search-range helper `FlowViewSwiftUINative` relies on.
@MainActor
final class FlowViewTests: XCTestCase {
    private func makeSuite(_ name: String) -> UserDefaults {
        let suiteName = "com.gist.tests.\(name).\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
        }
        return defaults
    }

    // MARK: - FlowScrollPositionStore

    func testScrollPositionRoundTripsPerItem() {
        let defaults = makeSuite("scroll-position-roundtrip")

        XCTAssertEqual(FlowScrollPositionStore.load(itemId: "item-1", defaults: defaults), 0)

        FlowScrollPositionStore.save(itemId: "item-1", fraction: 0.42, defaults: defaults)
        FlowScrollPositionStore.save(itemId: "item-2", fraction: 0.9, defaults: defaults)

        XCTAssertEqual(FlowScrollPositionStore.load(itemId: "item-1", defaults: defaults), 0.42, accuracy: 0.0001)
        XCTAssertEqual(FlowScrollPositionStore.load(itemId: "item-2", defaults: defaults), 0.9, accuracy: 0.0001)
    }

    func testScrollPositionSaveClampsOutOfRangeValues() {
        let defaults = makeSuite("scroll-position-clamp")

        FlowScrollPositionStore.save(itemId: "item", fraction: 1.5, defaults: defaults)
        XCTAssertEqual(FlowScrollPositionStore.load(itemId: "item", defaults: defaults), 1)

        FlowScrollPositionStore.save(itemId: "item", fraction: -0.5, defaults: defaults)
        XCTAssertEqual(FlowScrollPositionStore.load(itemId: "item", defaults: defaults), 0)
    }

    // MARK: - ReadingProgress

    func testReadingProgressClampsInitialFraction() {
        XCTAssertEqual(ReadingProgress(initialFraction: 1.5).initialFraction, 1)
        XCTAssertEqual(ReadingProgress(initialFraction: -0.2).initialFraction, 0)
        XCTAssertEqual(ReadingProgress(initialFraction: 0.5).initialFraction, 0.5)
    }

    func testReadingProgressTreatsNonFiniteInitialFractionAsZero() {
        XCTAssertEqual(ReadingProgress(initialFraction: .nan).initialFraction, 0)
        XCTAssertEqual(ReadingProgress(initialFraction: .infinity).initialFraction, 0)
    }

    func testReadingProgressSeedsFractionFromInitialFraction() {
        let progress = ReadingProgress(initialFraction: 0.3)
        XCTAssertEqual(progress.fraction, 0.3)
    }

    // MARK: - Typography option mappings

    func testReadingFontDesignMapsToDistinctSwiftUIDesigns() {
        let mapped = Set(ReadingFontDesign.allCases.map(\.fontDesign))
        XCTAssertEqual(mapped.count, ReadingFontDesign.allCases.count, "each case should map to a distinct Font.Design")
    }

    func testLineSpacingOptionsAreOrderedCompactToRelaxed() {
        XCTAssertLessThan(LineSpacingOption.compact.extraPoints, LineSpacingOption.regular.extraPoints)
        XCTAssertLessThan(LineSpacingOption.regular.extraPoints, LineSpacingOption.relaxed.extraPoints)
    }

    // MARK: - String.rangesOfSubstring

    func testRangesOfSubstringFindsCaseInsensitiveNonOverlappingMatches() {
        let ranges = "The cat sat on the mat".rangesOfSubstring("at")
        XCTAssertEqual(ranges.count, 3)
        for range in ranges {
            XCTAssertEqual(range.count, 2)
        }
    }

    func testRangesOfSubstringReturnsEmptyForNoMatchOrEmptyQuery() {
        XCTAssertTrue("hello world".rangesOfSubstring("xyz").isEmpty)
        XCTAssertTrue("hello world".rangesOfSubstring("").isEmpty)
    }

    func testRangesOfSubstringOffsetsAreCharacterBasedNotByteBased() {
        // "café" -- é is a single Character but multiple UTF-8 bytes; the
        // offset of "fé" must be counted in characters (2) for this to line
        // up correctly against an AttributedString built from the same text.
        let ranges = "café".rangesOfSubstring("fé")
        XCTAssertEqual(ranges, [2..<4])
    }
}
