import CoreGraphics
import Foundation

// ── Paginated reading view: pure pagination logic (ADR-023) ─────────────────
//
// Nothing in this file measures text or touches a view. `Paginator` turns
// already-measured block layouts into page boundaries; the AppKit measuring
// step lives in `PageBlockMeasurer` (macOS/). That split keeps the
// page-boundary algorithm deterministic and unit-testable without a display.

/// A position in the document that does not depend on page size or typography:
/// a flat block index (same ordering as `FlowViewSwiftUINative.flatBlocks`
/// and `TtsTextExtraction.speakableBlocks`) plus a *character* (grapheme)
/// offset into that block's `plainText`. ADR-023: this, never a page number,
/// is what the paged view persists.
struct PagePosition: Hashable, Comparable {
    var blockIndex: Int
    var offset: Int

    init(blockIndex: Int, offset: Int) {
        self.blockIndex = blockIndex
        self.offset = offset
    }

    static func < (lhs: PagePosition, rhs: PagePosition) -> Bool {
        lhs.blockIndex != rhs.blockIndex ? lhs.blockIndex < rhs.blockIndex : lhs.offset < rhs.offset
    }

    /// Clamps into a document whose blocks have the given character counts.
    /// An empty document clamps to `(0, 0)`.
    func clamped(blockCharCounts: [Int]) -> PagePosition {
        guard !blockCharCounts.isEmpty else { return PagePosition(blockIndex: 0, offset: 0) }
        let block = min(max(blockIndex, 0), blockCharCounts.count - 1)
        let offset = min(max(self.offset, 0), max(blockCharCounts[block], 0))
        return PagePosition(blockIndex: block, offset: offset)
    }
}

/// One block as the paginator sees it: how much vertical space it needs and
/// where (if anywhere) it may be broken.
struct PageBlockLayout: Equatable {
    enum Kind: Equatable {
        /// Flowing text that may break between lines. `lineStarts` are the
        /// character offsets at which each laid-out line begins (ascending,
        /// first is 0; an empty array is treated as one line).
        case lines(lineStarts: [Int], lineHeight: CGFloat)
        /// Cannot be broken (table, list, image). Taller than a page means it
        /// gets a page of its own and the view scrolls it inside the page.
        case atomic(height: CGFloat)
    }

    var kind: Kind
    /// Character count of the block's `plainText`.
    var charCount: Int
    /// Headings: do not leave this block alone at the bottom of a page.
    var keepWithNext: Bool = false
}

/// Half-open range of a page: `start` is the first position on the page,
/// `end` the first position of the next page (`(blockCount, 0)` for the last).
struct PageRange: Equatable {
    var start: PagePosition
    var end: PagePosition
}

/// The slice of one block shown on a page.
struct PagePiece: Equatable {
    var blockIndex: Int
    /// Character offsets into the block's `plainText`, `from <= to`.
    var from: Int
    var to: Int
}

enum Paginator {
    /// Smallest page height the algorithm will honour. Anything smaller
    /// (including non-finite values) is raised to this so a tiny window can
    /// neither loop forever nor produce zero-size pages.
    static let minimumPageHeight: CGFloat = 40

    static func paginate(blocks: [PageBlockLayout], pageHeight: CGFloat, blockSpacing: CGFloat) -> [PageRange] {
        guard !blocks.isEmpty else { return [] }
        let limit = max(finite(pageHeight, fallback: minimumPageHeight), minimumPageHeight)
        let spacing = max(finite(blockSpacing, fallback: 0), 0)
        let end = PagePosition(blockIndex: blocks.count, offset: 0)

        var pages: [PageRange] = []
        var position = PagePosition(blockIndex: 0, offset: 0)

        while position < end {
            var used: CGFloat = 0
            var placedAny = false
            var cursor = position

            pageLoop: while cursor.blockIndex < blocks.count {
                let block = blocks[cursor.blockIndex]
                let gap: CGFloat = placedAny ? spacing : 0

                // Keep a heading with the first element of what follows it.
                if block.keepWithNext, placedAny, cursor.offset == 0, cursor.blockIndex + 1 < blocks.count {
                    let needed = gap + totalHeight(of: block) + spacing + firstElementHeight(of: blocks[cursor.blockIndex + 1])
                    if used + needed > limit { break pageLoop }
                }

                switch block.kind {
                case .atomic(let rawHeight):
                    let height = max(finite(rawHeight, fallback: 0), 0)
                    if !placedAny {
                        used = height
                        placedAny = true
                        cursor = PagePosition(blockIndex: cursor.blockIndex + 1, offset: 0)
                        // An oversized atomic block owns its page.
                        if height >= limit { break pageLoop }
                    } else if used + gap + height <= limit {
                        used += gap + height
                        cursor = PagePosition(blockIndex: cursor.blockIndex + 1, offset: 0)
                    } else {
                        break pageLoop
                    }

                case .lines(let starts, let rawLineHeight):
                    let lineHeight = max(finite(rawLineHeight, fallback: 1), 1)
                    let normalized = starts.isEmpty ? [0] : starts
                    var line = firstLine(atOrAfter: cursor.offset, in: normalized)
                    let firstLineOnThisPage = line
                    var filled = false
                    while line < normalized.count {
                        let cost = (line == firstLineOnThisPage ? gap : 0) + lineHeight
                        if placedAny, used + cost > limit {
                            cursor = PagePosition(blockIndex: cursor.blockIndex, offset: normalized[line])
                            filled = true
                            break
                        }
                        used += cost
                        placedAny = true
                        line += 1
                    }
                    if filled { break pageLoop }
                    cursor = PagePosition(blockIndex: cursor.blockIndex + 1, offset: 0)
                }
            }

            // The first element of every page is always placed, so progress
            // is guaranteed; this guard is belt and braces against a future
            // edit breaking that invariant.
            if cursor <= position {
                cursor = PagePosition(blockIndex: min(position.blockIndex + 1, blocks.count), offset: 0)
            }
            pages.append(PageRange(start: position, end: cursor))
            position = cursor
        }
        return pages
    }

    /// Index of the page containing `position` (the last page whose start is
    /// at or before it). Positions past the end map to the last page; before
    /// the start to the first. `nil` only when there are no pages.
    static func pageIndex(containing position: PagePosition, in pages: [PageRange]) -> Int? {
        guard !pages.isEmpty else { return nil }
        var low = 0
        var high = pages.count - 1
        var found = 0
        while low <= high {
            let mid = (low + high) / 2
            if pages[mid].start <= position {
                found = mid
                low = mid + 1
            } else {
                high = mid - 1
            }
        }
        return found
    }

    /// The per-block slices a page shows.
    static func pieces(of page: PageRange, blockCharCounts: [Int]) -> [PagePiece] {
        var result: [PagePiece] = []
        var index = page.start.blockIndex
        while index < blockCharCounts.count, index < page.end.blockIndex || (index == page.end.blockIndex && page.end.offset > 0) {
            let count = max(blockCharCounts[index], 0)
            let from = index == page.start.blockIndex ? min(max(page.start.offset, 0), count) : 0
            let to = index == page.end.blockIndex ? min(max(page.end.offset, from), count) : count
            result.append(PagePiece(blockIndex: index, from: from, to: to))
            index += 1
        }
        return result
    }

    /// Clamped page-turn target: `delta` pages from `current`, never outside
    /// `0..<count` (0 when there are no pages).
    static func clampedPage(_ current: Int, delta: Int, count: Int) -> Int {
        guard count > 0 else { return 0 }
        return min(max(current + delta, 0), count - 1)
    }

    // MARK: - Helpers

    private static func finite(_ value: CGFloat, fallback: CGFloat) -> CGFloat {
        value.isFinite ? value : fallback
    }

    private static func firstLine(atOrAfter offset: Int, in starts: [Int]) -> Int {
        starts.firstIndex(where: { $0 >= offset }) ?? max(starts.count - 1, 0)
    }

    private static func totalHeight(of block: PageBlockLayout) -> CGFloat {
        switch block.kind {
        case .atomic(let height): return max(finite(height, fallback: 0), 0)
        case .lines(let starts, let lineHeight):
            return CGFloat(max(starts.count, 1)) * max(finite(lineHeight, fallback: 1), 1)
        }
    }

    private static func firstElementHeight(of block: PageBlockLayout) -> CGFloat {
        switch block.kind {
        case .atomic(let height): return max(finite(height, fallback: 0), 0)
        case .lines(_, let lineHeight): return max(finite(lineHeight, fallback: 1), 1)
        }
    }
}

// ── Position mapping between reading modes ──────────────────────────────────

enum ReadingPositionMapping {
    /// Block index for a `0...1` block fraction (the quantity both the flow
    /// and paged layouts publish through `ReadingProgress`).
    static func blockIndex(forFraction fraction: Double, blockCount: Int) -> Int {
        guard blockCount > 1 else { return 0 }
        let last = Double(blockCount - 1)
        let clamped = fraction.isFinite ? min(max(fraction, 0), 1) : 0
        return min(max(Int((clamped * last).rounded()), 0), blockCount - 1)
    }

    /// The fraction to hand to the *other* mode when switching. A quarter
    /// block of bias keeps the flow view's truncating restore
    /// (`Int(fraction * last)`) from landing one block early through
    /// floating-point error, while this file's rounding still recovers the
    /// same index.
    static func carryFraction(forBlockIndex index: Int, blockCount: Int) -> Double {
        guard blockCount > 1 else { return 0 }
        let last = Double(blockCount - 1)
        let clampedIndex = Double(min(max(index, 0), blockCount - 1))
        return min((clampedIndex + 0.25) / last, 1)
    }

    /// Plain (unbiased) fraction for display and publication.
    static func fraction(forBlockIndex index: Int, blockCount: Int) -> Double {
        guard blockCount > 1 else { return 0 }
        return Double(min(max(index, 0), blockCount - 1)) / Double(blockCount - 1)
    }
}

/// UTF-8 byte offset (ADR-003 annotation space) to Character offset within a
/// block's text, clamped. Same conversion `FlowViewSwiftUINative` uses.
enum PageTextOffsets {
    static func characterOffset(forByteOffset byteOffset: Int, in text: String) -> Int {
        let utf8 = text.utf8
        let clamped = min(max(byteOffset, 0), utf8.count)
        guard let byteIndex = utf8.index(utf8.startIndex, offsetBy: clamped, limitedBy: utf8.endIndex),
            let stringIndex = byteIndex.samePosition(in: text)
        else { return text.count }
        return text.distance(from: text.startIndex, to: stringIndex)
    }

    static func byteOffset(forCharacterOffset offset: Int, in text: String) -> Int {
        let clamped = min(max(offset, 0), text.count)
        let index = text.index(text.startIndex, offsetBy: clamped)
        return text.utf8.distance(from: text.startIndex, to: index)
    }
}

// ── Persistence: the paged view's own position store ────────────────────────

/// Per-item paged reading position, `UserDefaults`-backed. A third, separate
/// meaning of "position" (ADR-023): not `reading_progress`'s RSVP token index
/// and not `FlowScrollPositionStore`'s block fraction. Stores an anchor, never
/// a page number, because page numbers change with window size and typography.
enum PagedPositionStore {
    private static func key(itemId: String) -> String { "pagedPosition.\(itemId)" }

    static func load(itemId: String, defaults: UserDefaults = .standard) -> PagePosition? {
        guard let raw = defaults.dictionary(forKey: key(itemId: itemId)),
            let block = raw["b"] as? Int, let offset = raw["o"] as? Int
        else { return nil }
        return PagePosition(blockIndex: max(block, 0), offset: max(offset, 0))
    }

    static func save(itemId: String, position: PagePosition, defaults: UserDefaults = .standard) {
        defaults.set(
            ["b": max(position.blockIndex, 0), "o": max(position.offset, 0)],
            forKey: key(itemId: itemId)
        )
    }
}

// ── Reading-layout choice (Scroll / Pages) ──────────────────────────────────

enum ReadingLayoutMode: String, CaseIterable, Identifiable {
    case scroll
    case pages

    var id: String { rawValue }

    var label: String {
        switch self {
        case .scroll: return String(localized: "Scroll")
        case .pages: return String(localized: "Pages")
        }
    }
}

/// Persisted default layout for the flow reader. Defaults to `.scroll` so the
/// existing behaviour is unchanged unless the reader opts in.
@MainActor
final class ReadingLayoutSettings: ObservableObject {
    static let shared = ReadingLayoutSettings()
    private static let key = "com.gist.settings.reading.flowLayout"

    @Published private(set) var mode: ReadingLayoutMode
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        mode = defaults.string(forKey: Self.key).flatMap(ReadingLayoutMode.init(rawValue:)) ?? .scroll
    }

    func setMode(_ newMode: ReadingLayoutMode) {
        mode = newMode
        defaults.set(newMode.rawValue, forKey: Self.key)
    }
}

// ── Document index used by the paged layout ─────────────────────────────────

struct PagedMatch: Equatable {
    var blockIndex: Int
    var range: Range<Int>
}

/// Flat-block lookups the paged layout needs, built once per document.
struct PagedDocumentIndex {
    let blockTexts: [String]
    let blockSectionIds: [String]
    let blockIndexInSection: [Int]
    let blockCharCounts: [Int]
    private let firstBlockOfSection: [String: Int]
    private let sectionsById: [String: FlowSectionVM]

    var blockCount: Int { blockTexts.count }

    init(document: FlowDocumentVM) {
        var texts: [String] = []
        var sectionIds: [String] = []
        var indices: [Int] = []
        var first: [String: Int] = [:]
        for section in document.sections {
            for (i, block) in section.blocks.enumerated() {
                if first[section.id] == nil { first[section.id] = texts.count }
                texts.append(block.plainText)
                sectionIds.append(section.id)
                indices.append(i)
            }
        }
        blockTexts = texts
        blockSectionIds = sectionIds
        blockIndexInSection = indices
        blockCharCounts = texts.map(\.count)
        firstBlockOfSection = first
        sectionsById = Dictionary(document.sections.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
    }

    func firstBlockIndex(ofSection id: String) -> Int? { firstBlockOfSection[id] }

    func matches(for query: String) -> [PagedMatch] {
        guard !query.isEmpty else { return [] }
        var results: [PagedMatch] = []
        for (index, text) in blockTexts.enumerated() {
            for range in text.rangesOfSubstring(query) {
                results.append(PagedMatch(blockIndex: index, range: range))
            }
        }
        return results
    }

    /// Converts an ADR-003 anchor `(section id, UTF-8 byte offset into the
    /// section's "\n\n"-joined text)` to a page position. The anchor itself is
    /// never page-relative, so this works at any page size.
    func position(forAnchorSection sectionId: String, byteStart: Int) -> PagePosition? {
        guard let section = sectionsById[sectionId], let firstBlock = firstBlockOfSection[sectionId] else { return nil }
        // Accumulate the start incrementally (same arithmetic as
        // `blockByteOffset(at:)`: previous end + the 2-byte "\n\n" separator).
        // Calling `blockByteOffset(at: i)` per block was O(N^2) in the
        // section's block count.
        var blockStart = 0
        for i in 0..<section.blocks.count {
            let blockEnd = blockStart + blockTexts[firstBlock + i].utf8.count
            if byteStart >= blockStart && byteStart <= blockEnd {
                let flat = firstBlock + i
                let offset = PageTextOffsets.characterOffset(forByteOffset: byteStart - blockStart, in: blockTexts[flat])
                return PagePosition(blockIndex: flat, offset: offset)
            }
            blockStart = blockEnd + 2
        }
        return nil
    }

    /// Inverse of `position(forAnchorSection:byteStart:)` for a page position:
    /// the `(section id, byte offset)` ADR-003 coordinate.
    func anchor(for position: PagePosition) -> (sectionId: String, byteStart: Int)? {
        guard blockTexts.indices.contains(position.blockIndex),
            let section = sectionsById[blockSectionIds[position.blockIndex]]
        else { return nil }
        let inBlock = PageTextOffsets.byteOffset(forCharacterOffset: position.offset, in: blockTexts[position.blockIndex])
        return (section.id, section.blockByteOffset(at: blockIndexInSection[position.blockIndex]) + inBlock)
    }
}
