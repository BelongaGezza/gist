import SwiftUI

// ── Decoded document model ───────────────────────────────────────────────────
//
// Client-side mirror of `gist_model::Document`'s JSON shape, fetched via
// `GistCore.getDocumentJson` (see `CoreClient.loadDocument`). This is the
// block-structured IR — headings/paragraphs/images/lists — as opposed to
// RSVP's flat token stream (`RsvpSessionVM` in RsvpView.swift). Built for the
// M2 flow view (see CLAUDE.md's Q8, decided 2026-09-12 in favor of
// SwiftUI-native): `FlowViewSwiftUINative` renders this model.
//
// `Block` and `Section.heading` use serde's default externally-tagged/tuple
// representations, which don't match Swift's synthesized `Decodable`, so
// both get hand-written `init(from:)`.

struct FlowDocumentVM: Decodable {
    let id: String
    let metadata: FlowMetadataVM
    let sections: [FlowSectionVM]

    /// Table of contents: one entry per section that has a heading, in
    /// document order. Each entry carries the heading's level (`h1`-`h6`,
    /// straight from `gist_model::Section.heading`'s `(u8, String)` tuple —
    /// no Rust-side change was needed, the level was already in the JSON and
    /// simply wasn't surfaced in the UI) so the hosting container can nest
    /// the rendered list by indentation; see `TocEntry.indentLevel`.
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

    /// Nesting depth for rendering: `h1` is flush (0), `h2` indented once
    /// (1), `h3` indented twice (2), and so on. `gist-model`'s
    /// `Block::Heading { level: u8, .. }` doesn't itself constrain the range
    /// (parsers are expected to emit `1...6`, but nothing enforces it), so
    /// this clamps rather than producing negative padding for an
    /// out-of-range `level < 1`.
    var indentLevel: Int { max(level - 1, 0) }
}

struct FlowSectionVM: Decodable {
    let id: String
    let heading: FlowHeadingVM?
    let blocks: [FlowBlockVM]

    /// Concatenation of every block's `plainText`, each joined by `"\n\n"`,
    /// in document order. This is the byte-offset addressing space this
    /// reading view uses for ADR-003's `(block_id, start, len)` annotation
    /// anchor -- see `AnnotationAnchoring`'s doc comment (`AnnotationModel
    /// .swift`) for why: `gist_model::Block` has no id of its own, only
    /// `Section` does, so `block_id` is always a *section* id, and a
    /// section can hold more than one block. Addressing into the section's
    /// whole concatenated text (rather than one block's text in isolation)
    /// gives every byte offset within a section a single, unambiguous
    /// meaning regardless of how many blocks it contains, and lets a
    /// highlight's `prefix_hash` context correctly span a block boundary
    /// (e.g. a highlight starting at the very first word of a paragraph,
    /// whose preceding context is the end of the previous block).
    ///
    /// **The `"\n\n"` separator is not a free client-side choice** -- it
    /// must match `gist_core::anchoring::section_text`'s own join exactly,
    /// byte for byte, because that Rust function is what
    /// `Core::reanchor_annotations` slices `start..start+len` out of when
    /// verifying/re-anchoring an annotation on load. A separator mismatch
    /// between the two sides doesn't fail loudly -- it just makes Rust
    /// compute `prefix_hash`/`quote_hash` against the wrong substring the
    /// moment a section has more than one block, so every annotation past
    /// the first block in its section silently reads as shifted or
    /// orphaned even when nothing in the document changed. (Caught during
    /// M3 integration, 2026-09-26: this file originally joined with a
    /// single `"\n"`, disagreeing with `section_text`'s `"\n\n"` -- fixed to
    /// match. If `section_text`'s separator ever changes, this must change
    /// with it in the same commit.)
    var concatenatedPlainText: String {
        blocks.map(\.plainText).joined(separator: "\n\n")
    }

    /// The byte offset within `concatenatedPlainText` where block `index`'s
    /// own text begins. Must add the same 2-byte `"\n\n"` separator length
    /// `concatenatedPlainText` joins with -- see its doc comment.
    func blockByteOffset(at index: Int) -> Int {
        blocks.prefix(index).reduce(0) { $0 + $1.plainText.utf8.count + 2 }
    }

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
    /// This block's position among its own section's `blocks` array --
    /// needed (alongside `sectionId`) to compute its byte offset within
    /// `FlowSectionVM.concatenatedPlainText` via `blockByteOffset(at:)`,
    /// for annotation anchoring (see that property's doc comment).
    let blockIndexInSection: Int
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

/// Adjustable typography for the flow view: size, font design, and line
/// spacing -- the three controls a reading app's "Aa" panel conventionally
/// exposes (see e.g. Safari Reader/Apple Books). Deliberately does not add
/// letter-spacing, hyphenation, or a reading-width control; those can follow
/// later without changing this type's shape.
struct TypographySettings: Equatable {
    var fontSize: Double = 17
    var fontDesign: ReadingFontDesign = .system
    var lineSpacing: LineSpacingOption = .regular

    static let range: ClosedRange<Double> = 13...28
}

/// Font family choice for reading text. Wraps `Font.Design` (which isn't
/// `CaseIterable`) so the typography menu can build a `Picker` off
/// `allCases`. Code spans (`FlowTextRunVM.code`) always render monospaced
/// regardless of this setting -- that's a content distinction, not a user
/// preference.
enum ReadingFontDesign: String, CaseIterable, Identifiable, Equatable {
    case system
    case serif
    case rounded

    var id: String { rawValue }

    // (R5b localisation) Read via `Text(design.label)` in the typography
    // menu, which takes the returned value, not a literal.
    var label: String {
        switch self {
        case .system: return String(localized: "Default")
        case .serif: return String(localized: "Serif")
        case .rounded: return String(localized: "Rounded")
        }
    }

    var fontDesign: Font.Design {
        switch self {
        case .system: return .default
        case .serif: return .serif
        case .rounded: return .rounded
        }
    }
}

/// Extra spacing added between lines within a paragraph, on top of the
/// font's own natural leading. Values are added points, not a multiplier,
/// matching `Text.lineSpacing(_:)`'s own unit.
enum LineSpacingOption: String, CaseIterable, Identifiable, Equatable {
    case compact
    case regular
    case relaxed

    var id: String { rawValue }

    // (R5b localisation) Read via `Text(option.label)` in the typography
    // menu, which takes the returned value, not a literal.
    var label: String {
        switch self {
        case .compact: return String(localized: "Compact")
        case .regular: return String(localized: "Regular")
        case .relaxed: return String(localized: "Relaxed")
        }
    }

    var extraPoints: Double {
        switch self {
        case .compact: return 2
        case .regular: return 6
        case .relaxed: return 12
        }
    }
}

/// Shared in-document search state, driven by the hosting container's search
/// field and find-next/previous buttons. Each `ReadingLayout` implementation
/// computes its own matches against its own rendered text (see
/// `FlowViewSwiftUINative`) and publishes the count back here so the toolbar
/// can show "3 of 27" regardless of which layout is active.
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

/// Tracks how far through the document the reader has scrolled, as a
/// `0...1` fraction rather than anything layout-specific (a block index, a
/// pixel offset) -- that keeps it meaningful across whatever `ReadingLayout`
/// is hosting it, including a future paginated view (Q3) where "fraction"
/// would mean "page N of M" instead of "block N of M". `initialFraction` is
/// where a `ReadingLayout` should scroll to once on first appear, seeded by
/// `FlowReaderContainer` from `FlowScrollPositionStore`'s persisted value for
/// this item; `fraction` is then kept live by the layout as the user scrolls,
/// and the container persists it back out on change.
@MainActor
final class ReadingProgress: ObservableObject {
    @Published var fraction: Double
    let initialFraction: Double

    init(initialFraction: Double) {
        let clamped = initialFraction.isFinite ? min(max(initialFraction, 0), 1) : 0
        self.initialFraction = clamped
        self.fraction = clamped
    }
}

/// Persists each item's flow-view scroll position (a `0...1` fraction, see
/// `ReadingProgress`) locally via `UserDefaults`, keyed per item id. This is
/// deliberately independent of `gist-store`'s `reading_progress` table/
/// `CoreClient.saveProgress` -- that table's `token_index` column is RSVP-
/// specific (a position in the flat token stream), and conflating it with a
/// scroll fraction over the block-structured document would mean one number
/// meaning two different things depending on which reading mode last wrote
/// it. A client-side-only store is enough for this: it never needs to sync
/// across devices to be useful, and revisiting it as a real backend field is
/// a schema decision (Q10), not something to force through UserDefaults'
/// shape today.
enum FlowScrollPositionStore {
    private static func key(itemId: String) -> String { "flowScrollPosition.\(itemId)" }

    static func load(itemId: String, defaults: UserDefaults = .standard) -> Double {
        let value = defaults.double(forKey: key(itemId: itemId))
        return value.isFinite ? min(max(value, 0), 1) : 0
    }

    static func save(itemId: String, fraction: Double, defaults: UserDefaults = .standard) {
        defaults.set(min(max(fraction, 0), 1), forKey: key(itemId: itemId))
    }
}

/// Live annotation state for the flow view (ADR-003: highlights, notes,
/// bookmarks), owned by `FlowReaderContainer` exactly like `SearchState`/
/// `SectionNavigator`/`ReadingProgress` above -- a shared, mutable
/// `ObservableObject` the container seeds via `CoreClient.listAnnotations`
/// and that any `ReadingLayout` conformer (currently just
/// `FlowViewSwiftUINative`) both renders from *and* mutates directly when it
/// creates/deletes an annotation, rather than round-tripping every change
/// back up through the container. `AnnotationsSidebarView` (the browse/
/// export UI, presented as a sheet) observes the same instance so it always
/// reflects whatever the reading view just did, and vice versa.
@MainActor
final class AnnotationState: ObservableObject {
    @Published var items: [AnnotationVM] = []

    /// Set by `AnnotationsSidebarView`'s "Jump" action to request a
    /// scroll-to. Mirrors `SectionNavigator.pendingSectionId`'s "container-
    /// owned shared state; the hosting layout observes and reacts" idiom,
    /// just keyed on an annotation id instead of a section id.
    @Published var pendingJumpAnnotationId: String?

    /// Refetches `items`, replacing whatever was there, via
    /// `core.reanchorAnnotations(itemId:)` rather than the plain
    /// `listAnnotations` -- this re-verifies every annotation's anchor
    /// against the document's *current* content (ADR-003) before the
    /// sidebar/flow view render, silently correcting a shifted anchor
    /// (`.reanchored`) and stamping `AnnotationVM.anchorStatus` so an
    /// `.orphaned` one can show a visible indicator (see
    /// `AnnotationsSidebarView`). Used both for the initial load and any
    /// time a caller would rather re-derive the full list than reason about
    /// a local edit (e.g. after an update-note round trip) -- re-running
    /// re-anchoring on every reload is deliberately cheap/idempotent (an
    /// already-`.valid` annotation just re-verifies as `.valid`), not
    /// gated behind "only on first open."
    func reload(itemId: String, core: CoreClient) async {
        items = await core.reanchorAnnotations(itemId: itemId).map(\.annotation)
    }
}

// ── ReadingLayout protocol ───────────────────────────────────────────────────

/// Common contract for a reading-mode implementation that renders a parsed
/// document's blocks with adjustable typography, in-document search, and TOC
/// navigation. `FlowViewSwiftUINative` (SwiftUI `Text`/`AttributedString` in
/// a `LazyVStack`) conforms to this -- originally built alongside a second,
/// TextKit-2-based conformer so the two could be compared for CLAUDE.md's
/// Q8; that question was decided 2026-09-12 in favor of SwiftUI-native for
/// v1.0, and the TextKit 2 prototype was removed. The protocol stays generic
/// (not folded into `FlowViewSwiftUINative` directly) since Q3's paginated
/// view (open for v1.1) is a plausible future second conformer.
///
/// Kept intentionally small: a `ReadingLayout` only needs to know how to
/// render `document` at `typography`'s settings, react to `search`'s query /
/// current-match-index, react to `navigation`'s requested section, and keep
/// `progress`'s fraction in sync with where the user has scrolled to (and
/// scroll to `progress.initialFraction` once on first appear). The TOC's
/// *content* is a pure function of the document
/// (`FlowDocumentVM.tableOfContents`) so it isn't part of this protocol —
/// every implementation gets the entry list for free from the same document
/// value; only "jump to a section" needs a per-implementation reaction.
protocol ReadingLayout: View {
    init(
        document: FlowDocumentVM,
        typography: Binding<TypographySettings>,
        search: SearchState,
        navigation: SectionNavigator,
        progress: ReadingProgress,
        annotations: AnnotationState
    )
}
