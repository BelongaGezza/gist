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

    // MARK: - TocEntry.indentLevel

    func testTocEntryIndentLevelIsHeadingLevelMinusOne() {
        XCTAssertEqual(TocEntry(sectionIndex: 0, sectionId: "s", level: 1, title: "T").indentLevel, 0)
        XCTAssertEqual(TocEntry(sectionIndex: 0, sectionId: "s", level: 2, title: "T").indentLevel, 1)
        XCTAssertEqual(TocEntry(sectionIndex: 0, sectionId: "s", level: 3, title: "T").indentLevel, 2)
        XCTAssertEqual(TocEntry(sectionIndex: 0, sectionId: "s", level: 6, title: "T").indentLevel, 5)
    }

    func testTocEntryIndentLevelClampsOutOfRangeLevelToFlush() {
        // gist-model's `level: u8` isn't itself range-checked to 1...6; an
        // unexpected 0 (or, in principle, negative if the type ever widened)
        // must render flush rather than with negative padding.
        XCTAssertEqual(TocEntry(sectionIndex: 0, sectionId: "s", level: 0, title: "T").indentLevel, 0)
    }

    // MARK: - FlowDocumentVM.tableOfContents (decode + nesting)

    /// Builds the JSON shape `gist_model::Document` serialises -- see
    /// `FlowSectionVM`/`FlowBlockVM`'s hand-written `init(from:)` in
    /// FlowDocumentModel.swift, which mirror serde's externally-tagged enum
    /// and tuple representations. `heading` sections take `(level, text)`;
    /// headless sections take `nil`. Blocks are irrelevant to the TOC so
    /// each section gets an empty `blocks` array.
    private func documentJSON(headings: [(level: Int, title: String)?]) -> Data {
        let sections = headings.enumerated().map { index, heading -> String in
            let headingJSON = heading.map { "[\($0.level), \"\($0.title)\"]" } ?? "null"
            return """
            {"id": "s\(index)", "heading": \(headingJSON), "blocks": []}
            """
        }
        let json = """
        {
            "id": "doc1",
            "metadata": {"title": "Test Doc", "author": null},
            "sections": [\(sections.joined(separator: ","))]
        }
        """
        return Data(json.utf8)
    }

    func testTableOfContentsIncludesOnlyHeadingSectionsInDocumentOrderWithIndentLevels() throws {
        let json = documentJSON(headings: [
            (1, "Chapter 1"),
            (2, "Section 1.1"),
            nil,  // a headless paragraph-only section, e.g. front matter
            (3, "Subsection 1.1.1"),
            (1, "Chapter 2"),
        ])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let toc = document.tableOfContents

        XCTAssertEqual(toc.count, 4, "the headless section must not appear in the TOC")
        XCTAssertEqual(toc.map(\.title), ["Chapter 1", "Section 1.1", "Subsection 1.1.1", "Chapter 2"])
        XCTAssertEqual(toc.map(\.level), [1, 2, 3, 1])
        XCTAssertEqual(toc.map(\.indentLevel), [0, 1, 2, 0])
        // sectionIndex must point back at the section's real position in
        // `document.sections`, skipping over the headless one (index 2).
        XCTAssertEqual(toc.map(\.sectionIndex), [0, 1, 3, 4])
    }

    func testTableOfContentsIsEmptyWhenDocumentHasNoHeadings() throws {
        let json = documentJSON(headings: [nil, nil, nil])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        XCTAssertTrue(document.tableOfContents.isEmpty)
    }

    func testTableOfContentsWithAllSectionsAtSameLevelHaveEqualIndentAndPreserveOrder() throws {
        let json = documentJSON(headings: [(2, "First"), (2, "Second"), (2, "Third")])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let toc = document.tableOfContents

        XCTAssertEqual(toc.map(\.title), ["First", "Second", "Third"])
        XCTAssertEqual(Set(toc.map(\.indentLevel)), [1], "all same heading level must indent equally")
    }
}
