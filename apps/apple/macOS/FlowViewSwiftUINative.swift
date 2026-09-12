import SwiftUI

/// Prototype A for CLAUDE.md's Q8: renders the document as one SwiftUI
/// `Text`/`AttributedString` per block, inside a `LazyVStack` in a
/// `ScrollView`. See `FlowViewTextKit2.swift` for the TextKit 2 alternative
/// this is being compared against.
///
/// Search highlighting works by finding case-insensitive match ranges
/// against each block's plain text (`FlowBlockVM.plainText`, character-
/// offset based — see `String.rangesOfSubstring`), then re-applying those
/// same integer offsets against the `AttributedString` built for display.
/// This only works because both are built from the *same underlying
/// characters in the same order* — for a `.paragraph` block that means
/// concatenating `TextRun`s in original order before either the plain-text
/// search or the styled `AttributedString` is built, so the two never fall
/// out of sync with each other.
struct FlowViewSwiftUINative: ReadingLayout {
    let document: FlowDocumentVM
    @Binding var typography: TypographySettings
    @ObservedObject var search: SearchState
    @ObservedObject var navigation: SectionNavigator

    /// Section+block entries in document order, flattened once at init time
    /// (each gets its own synthetic id — see `FlowBlockEntry`) rather than
    /// recomputed on every body evaluation.
    private let flatBlocks: [FlowBlockEntry]

    @State private var matches: [(entryId: UUID, range: Range<Int>)] = []

    init(document: FlowDocumentVM, typography: Binding<TypographySettings>, search: SearchState, navigation: SectionNavigator) {
        self.document = document
        self._typography = typography
        self.search = search
        self.navigation = navigation
        self.flatBlocks = document.sections.flatMap { section in
            section.blocks.map { FlowBlockEntry(sectionId: section.id, block: $0) }
        }
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(flatBlocks) { entry in
                        blockView(entry)
                            .id(entry.id)
                    }
                }
                .padding(24)
                .frame(maxWidth: 700, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .onAppear { recomputeMatches() }
            .onChange(of: search.query) { _, _ in
                recomputeMatches()
                scrollToCurrentMatch(proxy: proxy)
            }
            .onChange(of: search.currentMatchIndex) { _, _ in
                scrollToCurrentMatch(proxy: proxy)
            }
            .onChange(of: navigation.pendingSectionId) { _, sectionId in
                guard let sectionId, let target = flatBlocks.first(where: { $0.sectionId == sectionId }) else { return }
                withAnimation { proxy.scrollTo(target.id, anchor: .top) }
            }
        }
    }

    // MARK: - Search

    private func recomputeMatches() {
        guard !search.query.isEmpty else {
            matches = []
            search.matchCount = 0
            search.currentMatchIndex = 0
            return
        }
        var results: [(entryId: UUID, range: Range<Int>)] = []
        for entry in flatBlocks {
            for range in entry.block.plainText.rangesOfSubstring(search.query) {
                results.append((entry.id, range))
            }
        }
        matches = results
        search.matchCount = results.count
        search.currentMatchIndex = 0
    }

    private func scrollToCurrentMatch(proxy: ScrollViewProxy) {
        guard matches.indices.contains(search.currentMatchIndex) else { return }
        withAnimation {
            proxy.scrollTo(matches[search.currentMatchIndex].entryId, anchor: .center)
        }
    }

    /// Matches belonging to one block entry, tagged with their position in
    /// the global `matches` array (`offset`) so highlighting can tell the
    /// current match apart from other matches in the same block.
    private func matchesForBlock(_ entryId: UUID) -> [(offset: Int, range: Range<Int>)] {
        matches.enumerated().compactMap { offset, match in
            match.entryId == entryId ? (offset, match.range) : nil
        }
    }

    // MARK: - Block rendering

    @ViewBuilder
    private func blockView(_ entry: FlowBlockEntry) -> some View {
        let blockMatches = matchesForBlock(entry.id)
        switch entry.block {
        case .heading(let level, let text):
            Text(highlighted(plain: text, matches: blockMatches))
                .font(headingFont(level: level))
        case .paragraph(let runs):
            Text(highlighted(runs: runs, matches: blockMatches))
                .lineSpacing(4)
        case .list(let ordered, let items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    HStack(alignment: .top, spacing: 8) {
                        Text(ordered ? "\(index + 1)." : "\u{2022}")
                            .foregroundStyle(.secondary)
                        Text(item)
                    }
                }
            }
            .font(.system(size: typography.fontSize))
        case .image(_, let alt, let caption):
            VStack(alignment: .leading, spacing: 4) {
                Image(systemName: "photo")
                    .font(.system(size: 32))
                    .foregroundStyle(.secondary)
                if let alt, !alt.isEmpty {
                    Text(alt).font(.caption).foregroundStyle(.secondary)
                }
                if let caption, !caption.isEmpty {
                    Text(caption).font(.caption).italic()
                }
            }
        }
    }

    private func headingFont(level: Int) -> Font {
        switch level {
        case ...1: return .system(size: typography.fontSize + 11, weight: .bold)
        case 2: return .system(size: typography.fontSize + 6, weight: .bold)
        default: return .system(size: typography.fontSize + 3, weight: .semibold)
        }
    }

    // MARK: - AttributedString construction

    /// Builds a plain (unstyled beyond font size) `AttributedString` for
    /// headings/lists/images, with search-match backgrounds applied.
    private func highlighted(plain text: String, matches: [(offset: Int, range: Range<Int>)]) -> AttributedString {
        var attr = AttributedString(text)
        applyHighlights(&attr, matches: matches)
        return attr
    }

    /// Builds a paragraph's `AttributedString` by concatenating its
    /// `TextRun`s with their own bold/italic/code styling, then applies
    /// search-match backgrounds on top — see the type-level doc comment for
    /// why the two offset spaces line up.
    private func highlighted(runs: [FlowTextRunVM], matches: [(offset: Int, range: Range<Int>)]) -> AttributedString {
        var attr = AttributedString()
        for run in runs {
            var piece = AttributedString(run.text)
            var font = Font.system(size: typography.fontSize)
            if run.code {
                font = .system(size: typography.fontSize, design: .monospaced)
            } else {
                if run.bold { font = font.bold() }
                if run.italic { font = font.italic() }
            }
            piece.font = font
            attr += piece
        }
        applyHighlights(&attr, matches: matches)
        return attr
    }

    private func applyHighlights(_ attr: inout AttributedString, matches: [(offset: Int, range: Range<Int>)]) {
        for (offset, range) in matches {
            guard
                let lower = attr.characters.index(attr.startIndex, offsetBy: range.lowerBound, limitedBy: attr.endIndex),
                let upper = attr.characters.index(attr.startIndex, offsetBy: range.upperBound, limitedBy: attr.endIndex)
            else { continue }
            attr[lower..<upper].backgroundColor = offset == search.currentMatchIndex
                ? Color.orange.opacity(0.7)
                : Color.yellow.opacity(0.5)
        }
    }
}
