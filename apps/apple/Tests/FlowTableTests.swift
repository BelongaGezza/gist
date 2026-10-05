import SwiftUI
import XCTest
@testable import GIST

/// M6/R3: `FlowBlockVM.table` decoding, its text linearisation (which must
/// agree byte for byte with Rust's `section_text` -- ADR-003 addendum), the
/// pure grid-layout/accessibility helpers behind `FlowTableView`, and a real
/// Rust<->Swift round trip with annotations in and after a table.
@MainActor
final class FlowTableTests: XCTestCase {
    /// The exact JSON `serde_json` produces for the Rust golden block --
    /// pinned on the Rust side by
    /// `section_text_with_a_table_matches_the_cross_language_golden`
    /// (`gist-core`). Keep the two literals identical.
    private let tableBlockJSON = #"{"Table":{"rows":[["Fruit","Colour"],["Apple",""]],"header_row":true}}"#
    /// The Rust golden `section_text` for [paragraph, table, paragraph].
    private let goldenSectionText = "Intro text.\n\nFruit\tColour\nApple\t\n\nOutro text after."

    private func sectionDocumentJSON() -> Data {
        let json = """
        {
            "id": "doc1",
            "metadata": {"title": "Tables", "author": null},
            "sections": [{
                "id": "s0", "heading": null,
                "blocks": [
                    {"Paragraph": {"runs": [{"text": "Intro text.", "bold": false, "italic": false, "code": false}]}},
                    \(tableBlockJSON),
                    {"Paragraph": {"runs": [{"text": "Outro text after.", "bold": false, "italic": false, "code": false}]}}
                ]
            }]
        }
        """
        return Data(json.utf8)
    }

    // MARK: - Decoding

    func testTableBlockDecodesRustJSONUnderPlainAndSnakeCaseDecoders() throws {
        let data = Data(tableBlockJSON.utf8)

        let plain = try JSONDecoder().decode(FlowBlockVM.self, from: data)
        let snake: FlowBlockVM = try {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase  // what CoreClient.loadDocument uses
            return try decoder.decode(FlowBlockVM.self, from: data)
        }()

        for block in [plain, snake] {
            guard case .table(let rows, let headerRow, _) = block else {
                return XCTFail("expected .table, got \(block)")
            }
            XCTAssertEqual(rows, [["Fruit", "Colour"], ["Apple", ""]])
            XCTAssertTrue(headerRow, "header_row must survive both decoder configurations")
        }
    }

    func testTableBlockWithoutHeaderRowKeyDefaultsToFalse() throws {
        let data = Data(#"{"Table":{"rows":[["a"]]}}"#.utf8)
        guard case .table(_, let headerRow, _) = try JSONDecoder().decode(FlowBlockVM.self, from: data) else {
            return XCTFail("expected .table")
        }
        XCTAssertFalse(headerRow)
    }

    // MARK: - Linearisation / cross-language golden

    func testTableSectionTextMatchesRustGolden() throws {
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: sectionDocumentJSON())
        let section = document.sections[0]
        // Identical literal to `TABLE_SECTION_TEXT_GOLDEN` in gist-core.
        XCTAssertEqual(section.concatenatedPlainText, goldenSectionText)
        // blockByteOffset must agree with where each block really starts.
        XCTAssertEqual(section.blockByteOffset(at: 1), "Intro text.\n\n".utf8.count)
        XCTAssertEqual(
            section.blockByteOffset(at: 2), "Intro text.\n\nFruit\tColour\nApple\t\n\n".utf8.count)
        // Ragged rows / empty cells keep their separators.
        XCTAssertEqual(
            FlowBlockVM.table(rows: [["a", "", "c"], ["d"]], headerRow: false).plainText, "a\t\tc\nd")
    }

    /// Mirrors Rust's `image_plain_text_is_alt_only_never_caption`: a caption
    /// must not leak into `plainText`, or every later block's byte offset in
    /// `concatenatedPlainText` drifts from `section_text` (ADR-003).
    func testImagePlainTextIsAltOnlyNeverCaption() throws {
        let json = """
        {"id": "s0", "heading": null, "blocks": [
            {"Image": {"src": "x.png", "alt": "a dog", "caption": "Figure 1"}},
            {"Image": {"src": "y.png", "alt": null, "caption": "Figure 2"}},
            {"Paragraph": {"runs": [{"text": "after", "bold": false, "italic": false, "code": false}]}}
        ]}
        """
        let section = try JSONDecoder().decode(FlowSectionVM.self, from: Data(json.utf8))
        XCTAssertEqual(section.blocks[0].plainText, "a dog")
        XCTAssertEqual(section.blocks[1].plainText, "")
        XCTAssertEqual(section.concatenatedPlainText, "a dog\n\n\n\nafter")
        XCTAssertEqual(section.blockByteOffset(at: 2), "a dog\n\n\n\n".utf8.count)
    }

    func testTableSpeakableTextIsRowByRowAndSkipsEmptyCells() {
        let table = FlowBlockVM.table(rows: [["Fruit", "Colour"], ["Apple", ""], ["", ""]], headerRow: true)
        XCTAssertEqual(table.speakableText, "Fruit, Colour. Apple.")
        // Non-table blocks are unchanged.
        let list = FlowBlockVM.list(ordered: false, items: ["a", "b"])
        XCTAssertEqual(list.speakableText, list.plainText)
    }

    func testTtsExtractionKeepsOneEntryPerBlockIncludingTables() throws {
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: sectionDocumentJSON())
        let blocks = TtsTextExtraction.speakableBlocks(for: document)
        XCTAssertEqual(blocks, ["Intro text.", "Fruit, Colour. Apple.", "Outro text after."])
    }

    // MARK: - Layout helpers

    func testCellRangesAreOffsetsIntoPlainText() {
        let rows = [["Fruit", "Colour"], ["Apple", ""]]
        let plain = FlowBlockVM.table(rows: rows, headerRow: true).plainText
        let ranges = TableCellLayout.cellRanges(rows: rows)

        for (r, row) in rows.enumerated() {
            for (c, text) in row.enumerated() {
                let range = ranges[r][c]
                let start = plain.index(plain.startIndex, offsetBy: range.lowerBound)
                let end = plain.index(plain.startIndex, offsetBy: range.upperBound)
                XCTAssertEqual(String(plain[start..<end]), text, "cell (\(r),\(c))")
            }
        }
    }

    func testClipShiftsToCellLocalOffsetsAndRejectsNonOverlap() {
        let cell = 10..<15
        XCTAssertEqual(TableCellLayout.clip(12..<14, to: cell), 2..<4)
        XCTAssertEqual(TableCellLayout.clip(8..<12, to: cell), 0..<2)  // starts before the cell
        XCTAssertEqual(TableCellLayout.clip(13..<20, to: cell), 3..<5)  // runs past the cell
        XCTAssertNil(TableCellLayout.clip(0..<10, to: cell))  // touching is not overlapping
        XCTAssertNil(TableCellLayout.clip(15..<20, to: cell))
        XCTAssertNil(TableCellLayout.clip(10..<10, to: 10..<10))  // empty cell
    }

    func testColumnCountAndWidthsHandleRaggedAndEmptyTables() {
        XCTAssertEqual(TableCellLayout.columnCount(rows: [["a"], ["a", "b", "c"]]), 3)
        XCTAssertEqual(TableCellLayout.columnCount(rows: []), 0)

        let widths = TableCellLayout.columnWidths(
            rows: [["x", String(repeating: "w", count: 500)], ["", "y"]], fontSize: 17)
        XCTAssertEqual(widths.count, 2)
        XCTAssertEqual(widths[0], 72, "short column is floored at the minimum width")
        XCTAssertEqual(widths[1], 280, "a huge cell is capped (it wraps) rather than stretching the column")
    }

    // MARK: - Accessibility labels

    func testCellLabelQualifiesDataCellsWithTheirColumnHeader() {
        let rows = [["Fruit", "Colour"], ["Apple", ""]]
        XCTAssertEqual(TableAccessibility.cellLabel(rows: rows, headerRow: true, row: 0, column: 0), "Fruit")
        XCTAssertEqual(
            TableAccessibility.cellLabel(rows: rows, headerRow: true, row: 1, column: 0), "Fruit: Apple")
        XCTAssertEqual(
            TableAccessibility.cellLabel(rows: rows, headerRow: true, row: 1, column: 1), "Colour: empty")
        // No header row -> plain text, no qualification.
        XCTAssertEqual(TableAccessibility.cellLabel(rows: rows, headerRow: false, row: 1, column: 0), "Apple")
        // A ragged/missing cell does not crash.
        XCTAssertEqual(TableAccessibility.cellLabel(rows: [["a"]], headerRow: false, row: 0, column: 3), "empty")
    }

    func testCellPositionAndTableLabelAreOneBased() {
        XCTAssertEqual(
            TableAccessibility.cellPosition(rowCount: 4, columnCount: 3, row: 0, column: 2),
            "Row 1 of 4, column 3 of 3")
        XCTAssertEqual(TableAccessibility.tableLabel(rowCount: 4, columnCount: 3), "Table, 4 rows, 3 columns")
    }

    // MARK: - Real Rust round trip with annotations in and after a table

    private var tempDir: URL!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("FlowTableTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        client = CoreClient(
            dbPath: tempDir.appendingPathComponent("gist.sqlite3").path,
            storageDir: tempDir.appendingPathComponent("storage", isDirectory: true).path)
    }

    override func tearDownWithError() throws {
        client = nil
        if let tempDir { try? FileManager.default.removeItem(at: tempDir) }
        try super.tearDownWithError()
    }

    /// Imports `with_table.docx` through the real core, then anchors
    /// highlights (computed from Swift's `concatenatedPlainText`) inside a
    /// table cell and in the paragraph after the table. Rust re-verifies
    /// each against ITS `section_text`; `.valid` for both proves the two
    /// flattenings agree on real data, not just on the hand-written golden.
    func testRealImportedTableDecodesAndAnnotationsInAndAfterItStayValid() async throws {
        guard let bundled = Bundle(for: Self.self).url(forResource: "with_table", withExtension: "docx") else {
            throw XCTSkip("with_table.docx fixture missing from test bundle resources")
        }
        let source = tempDir.appendingPathComponent("with_table.docx")
        try FileManager.default.copyItem(at: bundled, to: source)

        await client.importFile(url: source)
        guard let item = client.items.first else { return XCTFail("expected an imported item") }
        guard let document = await client.loadDocument(itemId: item.id), let section = document.sections.first
        else { return XCTFail("expected a loaded document") }

        // Structure: paragraph, ONE table (not N loose paragraphs), paragraph.
        XCTAssertEqual(section.blocks.count, 3)
        guard case .table(let rows, let headerRow, _) = section.blocks[1] else {
            return XCTFail("expected block 1 to be a table, got \(section.blocks[1])")
        }
        XCTAssertTrue(headerRow)
        XCTAssertEqual(rows[2], ["Banana", "", "12"], "empty cell keeps its column")

        let text = section.concatenatedPlainText
        var ids: [String] = []
        for needle in ["Dark red almost black", "After the table."] {
            guard let range = text.range(of: needle) else { return XCTFail("missing \(needle)") }
            let start = text.utf8.distance(from: text.utf8.startIndex, to: range.lowerBound)
            let len = needle.utf8.count
            let (prefixHash, quoteHash) = AnnotationAnchoring.hashes(fullText: text, start: start, len: len)
            let id = await client.createAnnotation(
                itemId: item.id, kind: .highlight, blockId: section.id, start: start, len: len,
                prefixHash: prefixHash, quoteHash: quoteHash, noteText: nil)
            ids.append(try XCTUnwrap(id))
        }

        let results = await client.reanchorAnnotations(itemId: item.id)
        XCTAssertEqual(results.count, 2)
        for result in results {
            XCTAssertEqual(
                result.status, .valid,
                "Rust and Swift disagree on the table flattening: annotation \(result.annotation.id) misanchored")
        }
    }

    /// M7/R7: imports `with_merged_cells.docx` through the real core, checks
    /// the decoded spans and that annotations in/after the merged table stay
    /// `.valid` (Rust and Swift flatten the covered slots identically).
    func testRealImportedMergedTableDecodesSpansAndAnnotationsStayValid() async throws {
        guard let bundled = Bundle(for: Self.self).url(forResource: "with_merged_cells", withExtension: "docx")
        else { throw XCTSkip("with_merged_cells.docx fixture missing from test bundle resources") }
        let source = tempDir.appendingPathComponent("with_merged_cells.docx")
        try FileManager.default.copyItem(at: bundled, to: source)

        await client.importFile(url: source)
        guard let item = client.items.first else { return XCTFail("expected an imported item") }
        guard let document = await client.loadDocument(itemId: item.id), let section = document.sections.first
        else { return XCTFail("expected a loaded document") }

        XCTAssertEqual(section.blocks.count, 3)
        guard case .table(let rows, _, let spans) = section.blocks[1] else {
            return XCTFail("expected block 1 to be a table, got \(section.blocks[1])")
        }
        XCTAssertEqual(rows[0], ["Sales", "", "Notes"])
        XCTAssertEqual(rows[2], ["South", "80", ""])
        XCTAssertEqual(
            Set(spans),
            [
                TableSpanVM(row: 0, col: 0, rowspan: 1, colspan: 2),
                TableSpanVM(row: 1, col: 2, rowspan: 2, colspan: 1),
                TableSpanVM(row: 3, col: 0, rowspan: 1, colspan: 2),
            ])

        let text = section.concatenatedPlainText
        for needle in ["Strong", "180", "After the table."] {
            guard let range = text.range(of: needle) else { return XCTFail("missing \(needle)") }
            let start = text.utf8.distance(from: text.utf8.startIndex, to: range.lowerBound)
            let len = needle.utf8.count
            let (prefixHash, quoteHash) = AnnotationAnchoring.hashes(fullText: text, start: start, len: len)
            let id = await client.createAnnotation(
                itemId: item.id, kind: .highlight, blockId: section.id, start: start, len: len,
                prefixHash: prefixHash, quoteHash: quoteHash, noteText: nil)
            XCTAssertNotNil(id)
        }
        let results = await client.reanchorAnnotations(itemId: item.id)
        XCTAssertEqual(results.count, 3)
        for result in results {
            XCTAssertEqual(result.status, .valid, "annotation \(result.annotation.id) misanchored")
        }
    }

    // MARK: - Merged cells (M7/R7)

    /// The SECOND cross-language golden: identical literals to
    /// `MERGED_BLOCK_JSON_GOLDEN` / `MERGED_SECTION_TEXT_GOLDEN` in gist-core.
    private let mergedBlockJSON =
        #"{"Table":{"rows":[["Sales","","Notes"],["North","100","Strong"],["South","80",""]],"header_row":true,"spans":[{"row":0,"col":0,"rowspan":1,"colspan":2},{"row":1,"col":2,"rowspan":2,"colspan":1}]}}"#
    private let mergedGoldenSectionText =
        "Intro text.\n\nSales\t\tNotes\nNorth\t100\tStrong\nSouth\t80\t\n\nOutro text after."

    private func decodeBlock(_ json: String, snakeCase: Bool) throws -> FlowBlockVM {
        let decoder = JSONDecoder()
        if snakeCase { decoder.keyDecodingStrategy = .convertFromSnakeCase }
        return try decoder.decode(FlowBlockVM.self, from: Data(json.utf8))
    }

    func testMergedTableDecodesSpansUnderPlainAndSnakeCaseDecoders() throws {
        for snake in [false, true] {
            guard case .table(let rows, let headerRow, let spans) = try decodeBlock(mergedBlockJSON, snakeCase: snake)
            else { return XCTFail("expected .table") }
            XCTAssertEqual(rows.count, 3)
            XCTAssertTrue(headerRow)
            XCTAssertEqual(
                spans,
                [
                    TableSpanVM(row: 0, col: 0, rowspan: 1, colspan: 2),
                    TableSpanVM(row: 1, col: 2, rowspan: 2, colspan: 1),
                ], "snakeCase=\(snake)")
        }
    }

    func testPreSpansBlobDecodesWithEmptySpans() throws {
        guard case .table(_, _, let spans) = try decodeBlock(tableBlockJSON, snakeCase: false) else {
            return XCTFail("expected .table")
        }
        XCTAssertTrue(spans.isEmpty)
    }

    func testMergedSectionTextMatchesSecondRustGolden() throws {
        let json = """
        {"id": "s0", "heading": null, "blocks": [
            {"Paragraph": {"runs": [{"text": "Intro text.", "bold": false, "italic": false, "code": false}]}},
            \(mergedBlockJSON),
            {"Paragraph": {"runs": [{"text": "Outro text after.", "bold": false, "italic": false, "code": false}]}}
        ]}
        """
        let section = try JSONDecoder().decode(FlowSectionVM.self, from: Data(json.utf8))
        XCTAssertEqual(section.concatenatedPlainText, mergedGoldenSectionText)
    }

    private let mergedRows = [
        ["Sales", "", "Notes"], ["North", "100", "Strong"], ["South", "80", ""], ["Grand total", "", "180"],
    ]
    private var mergedSpans: [TableSpanVM] {
        [
            TableSpanVM(row: 0, col: 0, rowspan: 1, colspan: 2),
            TableSpanVM(row: 1, col: 2, rowspan: 2, colspan: 1),
            TableSpanVM(row: 3, col: 0, rowspan: 1, colspan: 2),
        ]
    }

    func testSpanMapClassifiesSlots() {
        let map = TableSpanMap(rows: mergedRows, spans: mergedSpans)
        XCTAssertEqual(map.slot(row: 0, column: 0), .cell)
        XCTAssertEqual(map.slot(row: 0, column: 1), .coveredRight)
        XCTAssertEqual(map.slot(row: 1, column: 2), .cell)
        XCTAssertEqual(map.slot(row: 2, column: 2), .coveredBelow(originCol: 2, colspan: 1))
        XCTAssertEqual(map.slot(row: 3, column: 1), .coveredRight)
        // Only origins/ordinary cells are announced; covered slots are not.
        XCTAssertFalse(map.isAnnounced(row: 0, column: 1))
        XCTAssertFalse(map.isAnnounced(row: 2, column: 2))
        XCTAssertTrue(map.isAnnounced(row: 2, column: 0))
        // Row-span border logic: origin has no bottom edge, last filler does.
        XCTAssertTrue(map.horizontalEdges(row: 1, column: 2).top)
        XCTAssertFalse(map.horizontalEdges(row: 1, column: 2).bottom)
        XCTAssertFalse(map.horizontalEdges(row: 2, column: 2).top)
        XCTAssertTrue(map.horizontalEdges(row: 2, column: 2).bottom)
    }

    func testSpanMapSanitisesHostileSpans() {
        let rows = [["a", "b"], ["c", "d"]]
        let hostile = [
            TableSpanVM(row: 9, col: 0, rowspan: 2, colspan: 2),  // origin outside grid
            TableSpanVM(row: 0, col: 0, rowspan: 0, colspan: 1),  // invalid
            TableSpanVM(row: 0, col: 0, rowspan: 1, colspan: 1),  // not a merge
            TableSpanVM(row: -1, col: 0, rowspan: 2, colspan: 1),
            TableSpanVM(row: 0, col: 1, rowspan: Int.max, colspan: Int.max),  // clipped to grid
            TableSpanVM(row: 0, col: 0, rowspan: 2, colspan: 2),  // overlaps the clipped one: dropped
        ]
        let map = TableSpanMap(rows: rows, spans: hostile)
        XCTAssertEqual(map.span(row: 0, column: 1), TableSpanVM(row: 0, col: 1, rowspan: 2, colspan: 1))
        XCTAssertNil(map.span(row: 0, column: 0))
        XCTAssertEqual(map.slot(row: 1, column: 1), .coveredBelow(originCol: 1, colspan: 1))
        // Empty and ragged tables never crash.
        XCTAssertEqual(TableSpanMap(rows: [], spans: hostile).columnCount, 0)
        XCTAssertEqual(TableSpanMap(rows: [[]], spans: hostile).slot(row: 0, column: 0), .cell)
    }

    func testSpannedWidthAndColumnWidthsIgnoreMergedCells() {
        let widths: [CGFloat] = [100, 80, 90]
        XCTAssertEqual(TableCellLayout.spannedWidth(widths: widths, column: 0, colspan: 1), 100)
        XCTAssertEqual(
            TableCellLayout.spannedWidth(widths: widths, column: 0, colspan: 2),
            180 + TableCellLayout.cellHorizontalPadding)
        XCTAssertEqual(
            TableCellLayout.spannedWidth(widths: widths, column: 1, colspan: 99),
            170 + TableCellLayout.cellHorizontalPadding, "colspan is clipped to the grid")
        let rows = [["A very long merged heading across two columns", ""], ["x", "y"]]
        let merged = TableCellLayout.columnWidths(
            rows: rows, fontSize: 14, spans: [TableSpanVM(row: 0, col: 0, rowspan: 1, colspan: 2)])
        let unmerged = TableCellLayout.columnWidths(rows: rows, fontSize: 14)
        XCTAssertLessThan(merged[0], unmerged[0], "a merged cell must not widen its first column")
    }

    func testMergedCellAccessibilityPositionsAndLabels() {
        // Whole-region position strings (state the covered extent).
        XCTAssertEqual(
            TableAccessibility.cellPosition(rowCount: 4, columnCount: 3, row: 0, column: 0, rowspan: 1, colspan: 2),
            "Row 1 of 4, columns 1 to 2 of 3")
        XCTAssertEqual(
            TableAccessibility.cellPosition(rowCount: 4, columnCount: 3, row: 1, column: 2, rowspan: 2, colspan: 1),
            "Rows 2 to 3 of 4, column 3 of 3")
        XCTAssertEqual(
            TableAccessibility.cellPosition(rowCount: 4, columnCount: 3, row: 1, column: 0, rowspan: 2, colspan: 2),
            "Rows 2 to 3 of 4, columns 1 to 2 of 3")
        XCTAssertEqual(
            TableAccessibility.cellPosition(rowCount: 4, columnCount: 3, row: 1, column: 1),
            "Row 2 of 4, column 2 of 3", "unmerged cells read exactly as before")
        // A data cell under a spanning header is qualified by that header.
        XCTAssertEqual(
            TableAccessibility.cellLabel(rows: mergedRows, headerRow: true, row: 1, column: 1, spans: mergedSpans),
            "Sales: 100")
        // Without spans the covered header slot would be empty (the M6 behaviour).
        XCTAssertEqual(
            TableAccessibility.cellLabel(rows: mergedRows, headerRow: true, row: 1, column: 1), "100")
    }

    func testAnnotationOffsetsStillResolveInsideMergedTableCells() {
        // Covered slots are empty cells with empty ranges: highlights on the
        // origin cell's text are unaffected by the merge.
        let ranges = TableCellLayout.cellRanges(rows: mergedRows)
        let plain = FlowBlockVM.table(rows: mergedRows, headerRow: true, spans: mergedSpans).plainText
        let r = ranges[1][2]  // "Strong"
        let start = plain.index(plain.startIndex, offsetBy: r.lowerBound)
        let end = plain.index(plain.startIndex, offsetBy: r.upperBound)
        XCTAssertEqual(String(plain[start..<end]), "Strong")
        XCTAssertTrue(ranges[2][2].isEmpty)
    }
}
