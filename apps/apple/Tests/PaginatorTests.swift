import XCTest
@testable import GIST

/// Pure pagination logic (ADR-023): no display, no SwiftUI.
@MainActor
final class PaginatorTests: XCTestCase {
    private func text(lines: Int, lineHeight: CGFloat = 10, keep: Bool = false) -> PageBlockLayout {
        // Each line starts 10 characters after the previous one.
        PageBlockLayout(
            kind: .lines(lineStarts: (0..<lines).map { $0 * 10 }, lineHeight: lineHeight),
            charCount: lines * 10,
            keepWithNext: keep
        )
    }

    private func atomic(_ height: CGFloat, chars: Int = 5) -> PageBlockLayout {
        PageBlockLayout(kind: .atomic(height: height), charCount: chars)
    }

    private func counts(_ blocks: [PageBlockLayout]) -> [Int] { blocks.map(\.charCount) }

    /// Every character of every block is on exactly one page, in order.
    private func assertCovers(_ pages: [PageRange], blocks: [PageBlockLayout], file: StaticString = #filePath, line: UInt = #line) {
        var expected = PagePosition(blockIndex: 0, offset: 0)
        for page in pages {
            XCTAssertEqual(page.start, expected, "pages must be contiguous", file: file, line: line)
            XCTAssertTrue(page.start < page.end, "no empty page", file: file, line: line)
            expected = page.end
        }
        XCTAssertEqual(expected, PagePosition(blockIndex: blocks.count, offset: 0), file: file, line: line)
    }

    func testEmptyDocumentHasNoPages() {
        XCTAssertEqual(Paginator.paginate(blocks: [], pageHeight: 500, blockSpacing: 16), [])
        XCTAssertNil(Paginator.pageIndex(containing: PagePosition(blockIndex: 0, offset: 0), in: []))
    }

    func testIsDeterministic() {
        let blocks = [text(lines: 30), atomic(80), text(lines: 7), text(lines: 3, keep: true), text(lines: 12)]
        let a = Paginator.paginate(blocks: blocks, pageHeight: 120, blockSpacing: 8)
        let b = Paginator.paginate(blocks: blocks, pageHeight: 120, blockSpacing: 8)
        XCTAssertEqual(a, b)
        assertCovers(a, blocks: blocks)
    }

    func testSingleHugeParagraphSplitsAtLineBoundaries() {
        let blocks = [text(lines: 100)]  // 1000 pt tall
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 100, blockSpacing: 16)
        XCTAssertEqual(pages.count, 10)
        assertCovers(pages, blocks: blocks)
        for page in pages.dropLast() {
            XCTAssertEqual(page.start.offset % 10, 0, "breaks only at line starts")
        }
        XCTAssertEqual(pages[1].start, PagePosition(blockIndex: 0, offset: 100))
    }

    func testPiecesOfASplitParagraphTileTheBlock() {
        let blocks = [text(lines: 25)]
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 100, blockSpacing: 16)
        var covered = 0
        for page in pages {
            for piece in Paginator.pieces(of: page, blockCharCounts: counts(blocks)) {
                XCTAssertEqual(piece.from, covered)
                covered = piece.to
            }
        }
        XCTAssertEqual(covered, 250)
    }

    func testOversizedAtomicBlockOwnsItsPage() {
        let blocks = [text(lines: 2), atomic(900), text(lines: 2)]
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 100, blockSpacing: 16)
        assertCovers(pages, blocks: blocks)
        XCTAssertEqual(pages.count, 3)
        XCTAssertEqual(pages[1].start.blockIndex, 1)
        XCTAssertEqual(pages[1].end, PagePosition(blockIndex: 2, offset: 0))
    }

    func testAtomicThatFitsSharesAPage() {
        let blocks = [text(lines: 2), atomic(40)]
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 100, blockSpacing: 16)
        XCTAssertEqual(pages.count, 1)
    }

    func testImageAndEmptyBlocksAreIncludedOnTheirPage() {
        let blocks = [atomic(40, chars: 0), text(lines: 1)]
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 100, blockSpacing: 16)
        let pieces = pages.flatMap { Paginator.pieces(of: $0, blockCharCounts: counts(blocks)) }
        XCTAssertEqual(pieces.map(\.blockIndex), [0, 1])
    }

    func testTinyAndHostilePageSizesTerminateWithNonEmptyPages() {
        let blocks = [text(lines: 50), atomic(500), text(lines: 5, keep: true), text(lines: 5)]
        for height in [CGFloat(0), -5, 1, .nan, .infinity, 39] {
            let pages = Paginator.paginate(blocks: blocks, pageHeight: height, blockSpacing: .nan)
            XCTAssertFalse(pages.isEmpty)
            assertCovers(pages, blocks: blocks)
        }
    }

    func testZeroAndNonFiniteLineHeightsDoNotLoop() {
        let blocks = [PageBlockLayout(kind: .lines(lineStarts: [0, 5, 9], lineHeight: 0), charCount: 12),
                      PageBlockLayout(kind: .lines(lineStarts: [], lineHeight: .nan), charCount: 0)]
        assertCovers(Paginator.paginate(blocks: blocks, pageHeight: 50, blockSpacing: 4), blocks: blocks)
    }

    func testHeadingIsKeptWithFollowingText() {
        // Page fits 100pt: 8 lines (80) + spacing 10 + heading 10 = 100 fits,
        // but heading + next line would not, so the heading moves.
        let blocks = [text(lines: 8), text(lines: 1, keep: true), text(lines: 3)]
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 100, blockSpacing: 10)
        assertCovers(pages, blocks: blocks)
        XCTAssertEqual(pages[1].start, PagePosition(blockIndex: 1, offset: 0))
    }

    func testPageIndexContainingAnchorAndReflowKeepsAnchor() {
        let blocks = [text(lines: 100)]
        let anchor = PagePosition(blockIndex: 0, offset: 437)  // mid-line
        let wide = Paginator.paginate(blocks: blocks, pageHeight: 200, blockSpacing: 16)
        let narrow = Paginator.paginate(blocks: blocks, pageHeight: 70, blockSpacing: 16)
        for pages in [wide, narrow] {
            let i = try! XCTUnwrap(Paginator.pageIndex(containing: anchor, in: pages))
            XCTAssertTrue(pages[i].start <= anchor)
            XCTAssertTrue(anchor < pages[i].end)
        }
        XCTAssertNotEqual(wide.count, narrow.count)
        // Out-of-range anchors map to the first/last page.
        XCTAssertEqual(Paginator.pageIndex(containing: PagePosition(blockIndex: 99, offset: 0), in: wide), wide.count - 1)
        XCTAssertEqual(Paginator.pageIndex(containing: PagePosition(blockIndex: 0, offset: 0), in: wide), 0)
    }

    func testPageTurnNavigationBounds() {
        XCTAssertEqual(Paginator.clampedPage(0, delta: -1, count: 5), 0)
        XCTAssertEqual(Paginator.clampedPage(4, delta: 1, count: 5), 4)
        XCTAssertEqual(Paginator.clampedPage(2, delta: 1, count: 5), 3)
        XCTAssertEqual(Paginator.clampedPage(2, delta: 99, count: 5), 4)
        XCTAssertEqual(Paginator.clampedPage(0, delta: 1, count: 0), 0)
    }

    // MARK: - Anchor store

    private func suite(_ name: String) -> UserDefaults {
        let n = "com.gist.tests.paged.\(name).\(UUID().uuidString)"
        let d = UserDefaults(suiteName: n)!
        addTeardownBlock { d.removePersistentDomain(forName: n) }
        return d
    }

    func testAnchorRoundTripAndPerItem() {
        let d = suite("anchor")
        XCTAssertNil(PagedPositionStore.load(itemId: "a", defaults: d))
        PagedPositionStore.save(itemId: "a", position: PagePosition(blockIndex: 7, offset: 42), defaults: d)
        PagedPositionStore.save(itemId: "b", position: PagePosition(blockIndex: 1, offset: 0), defaults: d)
        XCTAssertEqual(PagedPositionStore.load(itemId: "a", defaults: d), PagePosition(blockIndex: 7, offset: 42))
        XCTAssertEqual(PagedPositionStore.load(itemId: "b", defaults: d), PagePosition(blockIndex: 1, offset: 0))
    }

    func testAnchorClampingOnSaveAndAgainstDocument() {
        let d = suite("anchor-clamp")
        PagedPositionStore.save(itemId: "a", position: PagePosition(blockIndex: -3, offset: -9), defaults: d)
        XCTAssertEqual(PagedPositionStore.load(itemId: "a", defaults: d), PagePosition(blockIndex: 0, offset: 0))
        d.set(["b": "junk"], forKey: "pagedPosition.c")
        XCTAssertNil(PagedPositionStore.load(itemId: "c", defaults: d))
        let clamped = PagePosition(blockIndex: 50, offset: 500).clamped(blockCharCounts: [10, 20])
        XCTAssertEqual(clamped, PagePosition(blockIndex: 1, offset: 20))
        XCTAssertEqual(PagePosition(blockIndex: 5, offset: 5).clamped(blockCharCounts: []), PagePosition(blockIndex: 0, offset: 0))
    }

    func testPagedStoreNeverTouchesFlowScrollStore() {
        let d = suite("separate")
        FlowScrollPositionStore.save(itemId: "a", fraction: 0.5, defaults: d)
        PagedPositionStore.save(itemId: "a", position: PagePosition(blockIndex: 9, offset: 1), defaults: d)
        XCTAssertEqual(FlowScrollPositionStore.load(itemId: "a", defaults: d), 0.5, accuracy: 1e-9)
    }

    func testLayoutModeSettingDefaultsToScrollAndPersists() {
        let d = suite("mode")
        let s = ReadingLayoutSettings(defaults: d)
        XCTAssertEqual(s.mode, .scroll)
        s.setMode(.pages)
        XCTAssertEqual(ReadingLayoutSettings(defaults: d).mode, .pages)
    }

    // MARK: - Mode switch carry-over

    func testCarryFractionRoundTripsThroughBothModesForEveryBlock() {
        for count in [2, 3, 7, 10, 101, 997] {
            for index in 0..<count {
                let carry = ReadingPositionMapping.carryFraction(forBlockIndex: index, blockCount: count)
                // paged side
                XCTAssertEqual(ReadingPositionMapping.blockIndex(forFraction: carry, blockCount: count), index)
                // flow side restores with truncation: Int(fraction * last)
                XCTAssertEqual(Int(carry * Double(count - 1)), index, "count \(count) index \(index)")
            }
        }
        XCTAssertEqual(ReadingPositionMapping.blockIndex(forFraction: .nan, blockCount: 5), 0)
        XCTAssertEqual(ReadingPositionMapping.carryFraction(forBlockIndex: 0, blockCount: 1), 0)
    }

    // MARK: - Document index: search, TOC jump, annotation anchors

    private func document() throws -> FlowDocumentVM {
        let json = """
        {"id":"d","metadata":{"title":"T","author":null},"sections":[
         {"id":"s0","heading":[1,"One"],"blocks":[
           {"Heading":{"level":1,"text":"One"}},
           {"Paragraph":{"runs":[{"text":"alpha beta","bold":false,"italic":false,"code":false}]}}]},
         {"id":"s1","heading":[1,"Two"],"blocks":[
           {"Paragraph":{"runs":[{"text":"gamma beta","bold":false,"italic":false,"code":false}]}}]}]}
        """
        return try JSONDecoder().decode(FlowDocumentVM.self, from: Data(json.utf8))
    }

    func testSearchMatchMapsToTheContainingPage() throws {
        let index = PagedDocumentIndex(document: try document())
        let matches = index.matches(for: "beta")
        XCTAssertEqual(matches.map(\.blockIndex), [1, 2])
        // One block per page.
        let blocks = [text(lines: 1), text(lines: 1), text(lines: 1)]
        let pages = Paginator.paginate(blocks: blocks, pageHeight: 40, blockSpacing: 40)
        XCTAssertEqual(pages.count, 3)
        let match = matches[1]
        XCTAssertEqual(
            Paginator.pageIndex(containing: PagePosition(blockIndex: match.blockIndex, offset: match.range.lowerBound), in: pages), 2)
    }

    func testSectionJumpTargetsFirstBlockOfSection() throws {
        let index = PagedDocumentIndex(document: try document())
        XCTAssertEqual(index.firstBlockIndex(ofSection: "s0"), 0)
        XCTAssertEqual(index.firstBlockIndex(ofSection: "s1"), 2)
        XCTAssertNil(index.firstBlockIndex(ofSection: "nope"))
    }

    func testAnnotationAnchorsAreIndependentOfPagination() throws {
        let doc = try document()
        let index = PagedDocumentIndex(document: doc)
        // s1's only block starts at byte 0; "beta" inside it starts at 6.
        let position = try XCTUnwrap(index.position(forAnchorSection: "s1", byteStart: 6))
        XCTAssertEqual(position, PagePosition(blockIndex: 2, offset: 6))
        // s0: heading "One" (3 bytes) + "\n\n" -> paragraph starts at byte 5; +6 = "beta".
        XCTAssertEqual(index.position(forAnchorSection: "s0", byteStart: 11), PagePosition(blockIndex: 1, offset: 6))
        // Round trip back to the ADR-003 coordinate.
        let back = try XCTUnwrap(index.anchor(for: PagePosition(blockIndex: 1, offset: 6)))
        XCTAssertEqual(back.sectionId, "s0")
        XCTAssertEqual(back.byteStart, 11)
        // The (section, byte) coordinate is the same under any page size.
        for height: CGFloat in [40, 400] {
            let pages = Paginator.paginate(blocks: [text(lines: 1), text(lines: 1), text(lines: 1)], pageHeight: height, blockSpacing: 40)
            XCTAssertNotNil(Paginator.pageIndex(containing: position, in: pages))
            XCTAssertEqual(index.anchor(for: position)?.byteStart, 6)
        }
    }

    // MARK: - Measurer (AppKit) invariants

    func testMeasurerLineStartsAreMonotonicAndWiderMeansFewerLines() {
        let long = String(repeating: "lorem ipsum dolor sit amet ", count: 60)
        let block = FlowBlockVM.paragraph(runs: [FlowTextRunVM(text: long, bold: false, italic: false, code: false)])
        let typo = TypographySettings()
        let narrow = PageBlockMeasurer.layout(for: block, width: 200, typography: typo)
        let wide = PageBlockMeasurer.layout(for: block, width: 600, typography: typo)
        guard case .lines(let n, let nh) = narrow.kind, case .lines(let w, _) = wide.kind else { return XCTFail() }
        XCTAssertEqual(n.first, 0)
        XCTAssertEqual(n, n.sorted())
        XCTAssertTrue((n.last ?? 0) < narrow.charCount)
        XCTAssertGreaterThan(n.count, w.count)
        XCTAssertGreaterThan(nh, 0)
        var bigger = typo
        bigger.fontSize = 28
        guard case .lines(let b, _) = PageBlockMeasurer.layout(for: block, width: 200, typography: bigger).kind else { return XCTFail() }
        XCTAssertGreaterThan(b.count, n.count)
    }

    func testMeasuredReflowAfterTypographyChangeKeepsTheAnchor() {
        let long = String(repeating: "word ", count: 2000)
        let blocks = [FlowBlockVM.paragraph(runs: [FlowTextRunVM(text: long, bold: false, italic: false, code: false)])]
        var typo = TypographySettings()
        let anchor = PagePosition(blockIndex: 0, offset: 5000)
        for size in [13.0, 17.0, 28.0] {
            typo.fontSize = size
            let layouts = PageBlockMeasurer.layouts(for: blocks, width: 500, typography: typo)
            let pages = Paginator.paginate(blocks: layouts, pageHeight: 400, blockSpacing: 16)
            let i = try! XCTUnwrap(Paginator.pageIndex(containing: anchor, in: pages))
            XCTAssertTrue(pages[i].start <= anchor && anchor < pages[i].end)
        }
    }
}
