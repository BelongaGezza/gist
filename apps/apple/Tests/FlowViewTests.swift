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

    // MARK: - Annotations (ADR-003): FNV1a / AnnotationAnchoring

    func testFNV1aHashOfEmptyBytesIsTheStandardOffsetBasis() {
        // The FNV-1a loop body never executes for zero input bytes, so the
        // result is exactly the offset basis constant -- a well-known,
        // hardcodable test vector, not an implementation-specific guess.
        XCTAssertEqual(FNV1a.hash([UInt8]()), 0xcbf2_9ce4_8422_2325)
        XCTAssertEqual(FNV1a.hash(""), 0xcbf2_9ce4_8422_2325)
    }

    func testFNV1aHashIsDeterministicAndDiffersForDifferentInput() {
        XCTAssertEqual(FNV1a.hash("hello"), FNV1a.hash("hello"))
        XCTAssertNotEqual(FNV1a.hash("hello"), FNV1a.hash("hellp"))
    }

    func testAnnotationAnchoringQuoteHashMatchesExactlyTheAnchoredSpan() {
        let full = String(repeating: "x", count: 40) + "TARGET"
        let (_, quoteHash) = AnnotationAnchoring.hashes(fullText: full, start: 40, len: 6)
        XCTAssertEqual(quoteHash, FNV1a.hash("TARGET"))
    }

    func testAnnotationAnchoringPrefixHashClampsToThirtyCharactersOfContext() {
        let full = String(repeating: "x", count: 40) + "TARGET"
        let (prefixHash, _) = AnnotationAnchoring.hashes(fullText: full, start: 40, len: 6)
        // Only the 30 characters immediately before `start` are hashed, not
        // all 40 preceding characters.
        XCTAssertEqual(prefixHash, FNV1a.hash(String(repeating: "x", count: 30)))
    }

    func testAnnotationAnchoringPrefixHashUsesWhateverContextExistsWhenLessThanThirty() {
        let (prefixHash, _) = AnnotationAnchoring.hashes(fullText: "abcTARGET", start: 3, len: 6)
        XCTAssertEqual(prefixHash, FNV1a.hash("abc"))
    }

    func testAnnotationAnchoringHashesOfAZeroLengthAnchorAtStartOfEmptyTextAreBothTheOffsetBasis() {
        let (prefixHash, quoteHash) = AnnotationAnchoring.hashes(fullText: "", start: 0, len: 0)
        XCTAssertEqual(prefixHash, 0xcbf2_9ce4_8422_2325)
        XCTAssertEqual(quoteHash, 0xcbf2_9ce4_8422_2325)
    }

    // MARK: - HighlightColor (client-side note_text convention)

    func testHighlightColorEncodeDecodeRoundTripsForEveryCase() {
        for color in HighlightColor.allCases {
            XCTAssertEqual(HighlightColor.decode(from: HighlightColor.encode(color)), color)
        }
    }

    func testHighlightColorDecodeDefaultsToYellowForNilOrUnrecognizedText() {
        XCTAssertEqual(HighlightColor.decode(from: nil), .yellow)
        XCTAssertEqual(HighlightColor.decode(from: "not a color encoding"), .yellow)
        XCTAssertEqual(HighlightColor.decode(from: "color:ultraviolet"), .yellow, "an unknown future colour name must fall back, not crash")
    }

    // MARK: - AnnotationVM

    func testAnnotationVMHighlightColorOnlyAppliesToHighlightKind() {
        let highlight = AnnotationVM(
            id: "1", itemId: "i", kind: .highlight, blockId: "b", start: 0, len: 1,
            prefixHash: 0, quoteHash: 0, noteText: HighlightColor.encode(.pink), createdAt: 0, updatedAt: 0
        )
        XCTAssertEqual(highlight.highlightColor, .pink)

        let note = AnnotationVM(
            id: "2", itemId: "i", kind: .note, blockId: "b", start: 0, len: 1,
            prefixHash: 0, quoteHash: 0, noteText: HighlightColor.encode(.pink), createdAt: 0, updatedAt: 0
        )
        XCTAssertNil(note.highlightColor, "a .note's noteText is real prose, never a colour encoding")
    }

    func testAnnotationVMDisplayNoteTextOnlyAppliesToNoteKindWithNonEmptyText() {
        let note = AnnotationVM(
            id: "1", itemId: "i", kind: .note, blockId: "b", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: "hello", createdAt: 0, updatedAt: 0
        )
        XCTAssertEqual(note.displayNoteText, "hello")

        let emptyNote = AnnotationVM(
            id: "2", itemId: "i", kind: .note, blockId: "b", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: "", createdAt: 0, updatedAt: 0
        )
        XCTAssertNil(emptyNote.displayNoteText)

        let highlight = AnnotationVM(
            id: "3", itemId: "i", kind: .highlight, blockId: "b", start: 0, len: 1,
            prefixHash: 0, quoteHash: 0, noteText: "color:blue", createdAt: 0, updatedAt: 0
        )
        XCTAssertNil(highlight.displayNoteText)

        let bookmark = AnnotationVM(
            id: "4", itemId: "i", kind: .bookmark, blockId: "b", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: nil, createdAt: 0, updatedAt: 0
        )
        XCTAssertNil(bookmark.displayNoteText)
    }

    func testAnnotationVMAnchorEqualityIsComponentWiseIgnoringIdKindAndHashes() {
        let a = AnnotationVM(
            id: "1", itemId: "i", kind: .highlight, blockId: "b", start: 5, len: 10,
            prefixHash: 0, quoteHash: 0, noteText: nil, createdAt: 0, updatedAt: 0
        )
        let b = AnnotationVM(
            id: "2", itemId: "i", kind: .note, blockId: "b", start: 5, len: 10,
            prefixHash: 999, quoteHash: 999, noteText: "x", createdAt: 1, updatedAt: 1
        )
        let c = AnnotationVM(
            id: "3", itemId: "i", kind: .note, blockId: "b", start: 6, len: 10,
            prefixHash: 0, quoteHash: 0, noteText: nil, createdAt: 0, updatedAt: 0
        )

        XCTAssertEqual(a.anchor, b.anchor, "same block/start/len is the same anchor regardless of id/kind/hashes")
        XCTAssertNotEqual(a.anchor, c.anchor)
    }

    func testAnnotationVMFromFfiMapsAllFields() {
        let ffi = FfiAnnotation(
            id: "id1", itemId: "item1", kind: .bookmark, blockId: "block1",
            start: 7, len: 3, prefixHash: 42, quoteHash: 43, noteText: "n",
            createdAt: 100, updatedAt: 200
        )
        let vm = AnnotationVM(ffi: ffi)

        XCTAssertEqual(vm.id, "id1")
        XCTAssertEqual(vm.itemId, "item1")
        XCTAssertEqual(vm.kind, .bookmark)
        XCTAssertEqual(vm.blockId, "block1")
        XCTAssertEqual(vm.start, 7)
        XCTAssertEqual(vm.len, 3)
        XCTAssertEqual(vm.prefixHash, 42)
        XCTAssertEqual(vm.quoteHash, 43)
        XCTAssertEqual(vm.noteText, "n")
        XCTAssertEqual(vm.createdAt, 100)
        XCTAssertEqual(vm.updatedAt, 200)
    }

    // MARK: - selectableWords (word tokenizer for highlight selection)

    func testSelectableWordsSplitsOnWhitespaceWithCorrectByteRanges() {
        let words = selectableWords(in: "Hello world")
        XCTAssertEqual(words.map(\.text), ["Hello", "world"])
        XCTAssertEqual(words[0].byteRange, 0..<5)
        XCTAssertEqual(words[1].byteRange, 6..<11)
    }

    func testSelectableWordsHandlesMultiByteCharactersWithCorrectByteOffsets() {
        // "café" is 5 UTF-8 bytes ("é" is 2 bytes) -- "bar" must start at
        // byte 6 (after "café" + the separating space), not character 5.
        let words = selectableWords(in: "café bar")
        XCTAssertEqual(words.map(\.text), ["café", "bar"])
        XCTAssertEqual(words[0].byteRange, 0..<5)
        XCTAssertEqual(words[1].byteRange, 6..<9)
    }

    func testSelectableWordsIgnoresLeadingTrailingAndRepeatedWhitespace() {
        let words = selectableWords(in: "  one   two  ")
        XCTAssertEqual(words.map(\.text), ["one", "two"])
    }

    func testSelectableWordsOfEmptyOrWhitespaceOnlyStringIsEmpty() {
        XCTAssertTrue(selectableWords(in: "").isEmpty)
        XCTAssertTrue(selectableWords(in: "   ").isEmpty)
    }

    // MARK: - FlowSectionVM annotation-anchoring helpers + FlowDocumentVM snippets

    /// Builds a document JSON with one or more sections, each holding plain
    /// (unstyled) paragraph blocks -- unlike `documentJSON(headings:)` above
    /// (which uses empty `blocks` arrays for TOC-only tests), this exercises
    /// `FlowSectionVM.concatenatedPlainText`/`blockByteOffset(at:)` and
    /// `FlowDocumentVM.annotatedText`/`sectionLabel`, which need real block
    /// text.
    private func paragraphDocumentJSON(sections: [(id: String, heading: String?, paragraphs: [String])]) -> Data {
        let sectionsJSON = sections.map { section -> String in
            let headingJSON = section.heading.map { "[1, \"\($0)\"]" } ?? "null"
            let blocksJSON = section.paragraphs.map { text in
                """
                {"Paragraph": {"runs": [{"text": "\(text)", "bold": false, "italic": false, "code": false}]}}
                """
            }.joined(separator: ",")
            return """
            {"id": "\(section.id)", "heading": \(headingJSON), "blocks": [\(blocksJSON)]}
            """
        }.joined(separator: ",")
        let json = """
        {
            "id": "doc1",
            "metadata": {"title": "Annotated Doc", "author": null},
            "sections": [\(sectionsJSON)]
        }
        """
        return Data(json.utf8)
    }

    func testConcatenatedPlainTextJoinsBlocksWithDoubleNewlineAndBlockByteOffsetIsCumulative() throws {
        let json = paragraphDocumentJSON(sections: [(id: "s0", heading: nil, paragraphs: ["Hello", "World"])])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let section = document.sections[0]

        // Must match gist_core::anchoring::section_text's "\n\n" join exactly
        // -- see concatenatedPlainText's doc comment.
        XCTAssertEqual(section.concatenatedPlainText, "Hello\n\nWorld")
        XCTAssertEqual(section.blockByteOffset(at: 0), 0)
        XCTAssertEqual(section.blockByteOffset(at: 1), "Hello\n\n".utf8.count)
    }

    func testAnnotatedTextResolvesSpanFromSectionConcatenatedTextAndSectionLabelUsesHeading() throws {
        let json = paragraphDocumentJSON(sections: [(id: "s0", heading: "Intro", paragraphs: ["Hello", "World"])])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let section = document.sections[0]
        let start = section.blockByteOffset(at: 1)
        let annotation = AnnotationVM(
            id: "a1", itemId: "item1", kind: .highlight, blockId: "s0", start: start, len: "World".utf8.count,
            prefixHash: 0, quoteHash: 0, noteText: nil, createdAt: 0, updatedAt: 0
        )

        XCTAssertEqual(document.annotatedText(for: annotation), "World")
        XCTAssertEqual(document.sectionLabel(for: annotation), "Intro")
    }

    func testSectionLabelFallsBackToOneBasedIndexWhenSectionHasNoHeading() throws {
        let json = paragraphDocumentJSON(sections: [(id: "s0", heading: nil, paragraphs: ["Hi"])])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let annotation = AnnotationVM(
            id: "a1", itemId: "i", kind: .note, blockId: "s0", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: "n", createdAt: 0, updatedAt: 0
        )
        XCTAssertEqual(document.sectionLabel(for: annotation), "Section 1")
    }

    func testAnnotatedTextAndSectionLabelHandleAnUnknownBlockIdGracefully() throws {
        let json = paragraphDocumentJSON(sections: [(id: "s0", heading: nil, paragraphs: ["Hi"])])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let annotation = AnnotationVM(
            id: "a1", itemId: "i", kind: .bookmark, blockId: "does-not-exist", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: nil, createdAt: 0, updatedAt: 0
        )
        XCTAssertNil(document.annotatedText(for: annotation))
        XCTAssertEqual(document.sectionLabel(for: annotation), "Unknown section")
    }

    // MARK: - AnnotationMarkdownExporter

    func testMarkdownExporterGroupsHighlightsNotesAndBookmarksWithAttachedNoteNestedUnderItsHighlight() throws {
        let json = paragraphDocumentJSON(sections: [
            (id: "s0", heading: "Chapter One", paragraphs: ["The quick brown fox jumps"]),
            (id: "s1", heading: "Chapter Two", paragraphs: ["Another paragraph entirely"]),
        ])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let highlightStart = 4  // byte offset of "quick"

        let highlight = AnnotationVM(
            id: "h1", itemId: "item", kind: .highlight, blockId: "s0", start: highlightStart, len: "quick".utf8.count,
            prefixHash: 0, quoteHash: 0, noteText: HighlightColor.encode(.green), createdAt: 1, updatedAt: 1
        )
        let attachedNote = AnnotationVM(
            id: "n1", itemId: "item", kind: .note, blockId: "s0", start: highlightStart, len: "quick".utf8.count,
            prefixHash: 0, quoteHash: 0, noteText: "nice adjective", createdAt: 2, updatedAt: 2
        )
        let standaloneNote = AnnotationVM(
            id: "n2", itemId: "item", kind: .note, blockId: "s1", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: "a standalone thought", createdAt: 3, updatedAt: 3
        )
        let bookmark = AnnotationVM(
            id: "b1", itemId: "item", kind: .bookmark, blockId: "s1", start: 0, len: 0,
            prefixHash: 0, quoteHash: 0, noteText: nil, createdAt: 4, updatedAt: 4
        )

        let markdown = AnnotationMarkdownExporter.markdown(for: [highlight, attachedNote, standaloneNote, bookmark], in: document)

        XCTAssertTrue(markdown.contains("# Annotations — Annotated Doc"))
        XCTAssertTrue(markdown.contains("[Green]"))
        XCTAssertTrue(markdown.contains("Chapter One"))
        XCTAssertTrue(markdown.contains("quick"))
        XCTAssertTrue(markdown.contains("## Bookmarks"))

        guard
            let highlightsIndex = markdown.range(of: "## Highlights")?.lowerBound,
            let notesIndex = markdown.range(of: "## Notes")?.lowerBound,
            let attachedNoteIndex = markdown.range(of: "nice adjective")?.lowerBound,
            let standaloneNoteIndex = markdown.range(of: "a standalone thought")?.lowerBound
        else {
            XCTFail("expected all sections/notes to be present in the exported Markdown")
            return
        }
        XCTAssertTrue(
            highlightsIndex < attachedNoteIndex && attachedNoteIndex < notesIndex,
            "a note attached to a highlight must be nested under Highlights, not listed again under Notes"
        )
        XCTAssertTrue(standaloneNoteIndex > notesIndex)
    }

    func testMarkdownExporterShowsPlaceholderWhenThereAreNoAnnotations() throws {
        let json = paragraphDocumentJSON(sections: [(id: "s0", heading: nil, paragraphs: ["Hi"])])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        let markdown = AnnotationMarkdownExporter.markdown(for: [], in: document)
        XCTAssertTrue(markdown.contains("_No annotations yet._"))
    }
}
