import SwiftUI

// ── Decoded document model ───────────────────────────────────────────────────
//
// Client-side mirror of `gist_model::Document`'s JSON shape, fetched via
// `GistCore.getDocumentJson` (see `CoreClient.loadDocument`). This is the
// block-structured IR — headings/paragraphs/images/lists — as opposed to
// RSVP's flat token stream (`RsvpSessionVM` in RsvpView.swift). Built for the
// M2 flow-view prototypes (see CLAUDE.md's Q8): both `FlowViewSwiftUINative`
// and `FlowViewTextKit2` render this same model.
//
// `Block` and `Section.heading` use serde's default externally-tagged/tuple
// representations, which don't match Swift's synthesized `Decodable`, so
// both get hand-written `init(from:)`.

struct FlowDocumentVM: Decodable {
    let id: String
    let metadata: FlowMetadataVM
    let sections: [FlowSectionVM]

    /// Table of contents: one entry per section that has a heading, in
    /// document order. Deliberately not virtualised/nested by level — a
    /// flat jump list is enough to compare the two rendering approaches.
    var tableOfContents: [TocEntry] {
        sections.enumerated().compactMap { index, section in
            guard let heading = section.heading else { return nil }
            return TocEntry(sectionIndex: index, sectionId: section.id, level: heading.level, title: heading.text)
        }
    }
}

struct FlowMetadataVM: Decodable {
    let title: String
    let author: String?
}

struct TocEntry: Identifiable, Hashable {
    let sectionIndex: Int
    let sectionId: String
    let level: Int
    let title: String
    var id: String { sectionId }
}

struct FlowSectionVM: Decodable {
    let id: String
    let heading: FlowHeadingVM?
    let blocks: [FlowBlockVM]

    private enum CodingKeys: String, CodingKey { case id, heading, blocks }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        blocks = try container.decode([FlowBlockVM].self, forKey: .blocks)
        if try container.decodeNil(forKey: .heading) {
            heading = nil
        } else {
            // Rust's `Option<(u8, String)>` serialises as a 2-element JSON array.
            var tuple = try container.nestedUnkeyedContainer(forKey: .heading)
            let level = try tuple.decode(Int.self)
            let text = try tuple.decode(String.self)
            heading = FlowHeadingVM(level: level, text: text)
        }
    }
}

struct FlowHeadingVM {
    let level: Int
    let text: String
}

/// A block plus the id of the section it belongs to and a synthetic,
/// per-view-instance identity — the flattened, `Identifiable` shape
/// `FlowViewSwiftUINative` renders and scrolls by (`ForEach`,
/// `ScrollViewReader.scrollTo`, search-match bookkeeping).
struct FlowBlockEntry: Identifiable {
    let id = UUID()
    let sectionId: String
    let block: FlowBlockVM
}

/// Mirrors `gist_model::Block`. Serde's default enum representation is
/// externally tagged: `{"Heading": {"level":1,"text":"..."}}`.
///
/// Deliberately has no identity of its own — Swift enums can't carry a
/// stored property (there's no uniform storage slot across cases the way a
/// struct has), so per-block identity for `ForEach`/`ScrollViewReader`
/// anchors lives on `FlowBlockEntry` below instead, assigned once when a
/// document's blocks are flattened.
enum FlowBlockVM: Decodable {
    case heading(level: Int, text: String)
    case paragraph(runs: [FlowTextRunVM])
    case image(src: String, alt: String?, caption: String?)
    case list(ordered: Bool, items: [String])

    /// Plain text used for search matching and (for non-paragraph blocks)
    /// as a fallback render — mirrors `gist_model::Block::plain_text`.
    var plainText: String {
        switch self {
        case .heading(_, let text): return text
        case .paragraph(let runs): return runs.map(\.text).joined()
        case .image(_, let alt, let caption): return alt ?? caption ?? ""
        case .list(_, let items): return items.joined(separator: " ")
        }
    }

    private enum RootKey: String, CodingKey { case Heading, Paragraph, Image, List }
    private enum HeadingKeys: String, CodingKey { case level, text }
    private enum ParagraphKeys: String, CodingKey { case runs }
    private enum ImageKeys: String, CodingKey { case src, alt, caption }
    private enum ListKeys: String, CodingKey { case ordered, items }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: RootKey.self)
        if container.contains(.Heading) {
            let inner = try container.nestedContainer(keyedBy: HeadingKeys.self, forKey: .Heading)
            self = .heading(
                level: try inner.decode(Int.self, forKey: .level),
                text: try inner.decode(String.self, forKey: .text)
            )
        } else if container.contains(.Paragraph) {
            let inner = try container.nestedContainer(keyedBy: ParagraphKeys.self, forKey: .Paragraph)
            self = .paragraph(runs: try inner.decode([FlowTextRunVM].self, forKey: .runs))
        } else if container.contains(.Image) {
            let inner = try container.nestedContainer(keyedBy: ImageKeys.self, forKey: .Image)
            self = .image(
                src: try inner.decode(String.self, forKey: .src),
                alt: try inner.decodeIfPresent(String.self, forKey: .alt),
                caption: try inner.decodeIfPresent(String.self, forKey: .caption)
            )
        } else if container.contains(.List) {
            let inner = try container.nestedContainer(keyedBy: ListKeys.self, forKey: .List)
            self = .list(
                ordered: try inner.decode(Bool.self, forKey: .ordered),
                items: try inner.decode([String].self, forKey: .items)
            )
        } else {
            throw DecodingError.dataCorrupted(
                .init(codingPath: decoder.codingPath, debugDescription: "Unknown Block variant")
            )
        }
    }
}

struct FlowTextRunVM: Decodable {
    let text: String
    let bold: Bool
    let italic: Bool
    let code: Bool
}

extension String {
    /// Case-insensitive, non-overlapping match ranges of `query` within
    /// `self`, expressed as *character offsets* (not `String.Index`) so
    /// callers can apply them against an `AttributedString` built
    /// separately (e.g. per-run, for `FlowViewSwiftUINative`) without
    /// juggling two different index spaces.
    func rangesOfSubstring(_ query: String) -> [Range<Int>] {
        guard !query.isEmpty else { return [] }
        var results: [Range<Int>] = []
        var searchStart = startIndex
        while searchStart < endIndex,
            let found = range(of: query, options: .caseInsensitive, range: searchStart..<endIndex)
        {
            let lower = distance(from: startIndex, to: found.lowerBound)
            let upper = distance(from: startIndex, to: found.upperBound)
            results.append(lower..<upper)
            searchStart = found.upperBound
        }
        return results
    }
}

// ── Shared reading controls ─────────────────────────────────────────────────

/// Adjustable typography shared by both `ReadingLayout` implementations.
/// Deliberately just a font size for this prototype — the point is
/// comparing the two rendering strategies, not building out the full
/// typography panel the real flow view will eventually need.
struct TypographySettings: Equatable {
    var fontSize: Double = 17

    static let range: ClosedRange<Double> = 13...28
}

/// Shared in-document search state, driven by the hosting container's search
/// field and find-next/previous buttons. Each `ReadingLayout` implementation
/// computes its own matches against its own rendered text (see
/// FlowViewSwiftUINative/FlowViewTextKit2) and publishes the count back here
/// so the toolbar can show "3 of 27" regardless of which layout is active.
@MainActor
final class SearchState: ObservableObject {
    @Published var query: String = ""
    @Published var matchCount: Int = 0
    @Published var currentMatchIndex: Int = 0

    func findNext() {
        guard matchCount > 0 else { return }
        currentMatchIndex = (currentMatchIndex + 1) % matchCount
    }

    func findPrevious() {
        guard matchCount > 0 else { return }
        currentMatchIndex = (currentMatchIndex - 1 + matchCount) % matchCount
    }
}

/// Requests a jump to a section (from the shared TOC menu the hosting
/// container builds off `FlowDocumentVM.tableOfContents`). A separate tiny
/// object rather than a field on `SearchState`, following the same
/// container-owns-shared-state idiom already used for search: the container
/// sets `pendingSectionId`, each `ReadingLayout` implementation observes it
/// and scrolls, in whatever way fits its own rendering (SwiftUI:
/// `ScrollViewReader.scrollTo`; TextKit2: compute the section's character
/// offset and `scrollRangeToVisible`).
@MainActor
final class SectionNavigator: ObservableObject {
    @Published var pendingSectionId: String?
}

// ── ReadingLayout protocol ───────────────────────────────────────────────────

/// Common contract for a reading-mode implementation that renders a parsed
/// document's blocks with adjustable typography, in-document search, and TOC
/// navigation. `FlowViewSwiftUINative` (SwiftUI `Text`/`AttributedString` in
/// a `LazyVStack`) and `FlowViewTextKit2` (`NSTextView`/TextKit 2 via
/// `NSViewRepresentable`) both conform to this — the point of building two
/// implementations of one protocol is to compare them on equal footing for
/// CLAUDE.md's Q8 (SwiftUI Text vs TextKit 2 for the flow view).
///
/// Kept intentionally small: a `ReadingLayout` only needs to know how to
/// render `document` at `typography`'s size, react to `search`'s query /
/// current-match-index, and react to `navigation`'s requested section. The
/// TOC's *content* is a pure function of the document
/// (`FlowDocumentVM.tableOfContents`) so it isn't part of this protocol —
/// both implementations get the entry list for free from the same document
/// value; only "jump to a section" needs a per-implementation reaction.
protocol ReadingLayout: View {
    init(
        document: FlowDocumentVM,
        typography: Binding<TypographySettings>,
        search: SearchState,
        navigation: SectionNavigator
    )
}
