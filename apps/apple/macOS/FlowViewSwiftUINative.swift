import SwiftUI

/// The flow reading view (CLAUDE.md's Q8, decided 2026-09-12 in favor of
/// SwiftUI-native for v1.0 -- see CLAUDE.md's M2 item 5 for the full
/// rationale: mainly cross-platform reuse and native theme integration,
/// traded against weaker text selection/find than a TextKit 2 prototype had.
/// Renders the document as one SwiftUI `Text`/`AttributedString` per block,
/// inside a `LazyVStack` in a `ScrollView`.
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
    @ObservedObject var progress: ReadingProgress
    /// ADR-003 annotation state (highlights/notes/bookmarks), shared with
    /// `FlowReaderContainer` and `AnnotationsSidebarView` -- see
    /// `AnnotationState`'s doc comment. `document.id` is used as the item id
    /// for every annotation FFI call this view makes: `gist_core::
    /// Core::get_document` looks a document up by, and always returns it
    /// under, the same id as the owning `library_items` row (see
    /// `Core::import_txt`'s `let id = doc.id.clone(); self.store.
    /// insert_item(&doc)`), so `document.id == itemId` always holds.
    @EnvironmentObject var core: CoreClient
    @ObservedObject var annotations: AnnotationState

    /// Section+block entries in document order, flattened once at init time
    /// (each gets its own synthetic id — see `FlowBlockEntry`) rather than
    /// recomputed on every body evaluation.
    private let flatBlocks: [FlowBlockEntry]
    /// Parallel to `document.sections`, for O(1) lookup by section id when
    /// resolving an annotation's `blockId` back to a `FlowSectionVM`.
    private let sectionsById: [String: FlowSectionVM]

    @State private var matches: [(entryId: UUID, range: Range<Int>)] = []
    /// Which block/annotation-creation sheet is currently presented, if any.
    /// A single `Identifiable` enum (rather than several separate
    /// `@State private var show...: Bool` flags) so exactly one sheet can be
    /// on screen at a time via one `.sheet(item:)`, matching
    /// `TagEditorTarget`'s existing idiom elsewhere in this app.
    @State private var composerTarget: AnnotationComposerTarget?
    /// Indices (into `flatBlocks`) of every block `LazyVStack` currently has
    /// on screen, maintained via each row's `onAppear`/`onDisappear`. The
    /// minimum of this set is treated as "the block the reader is at" for
    /// both the progress fraction and keyboard paging -- cheap to maintain
    /// (no scroll-geometry math) and accurate enough at block granularity.
    @State private var visibleBlockIndices: Set<Int> = []
    @State private var hasRestoredInitialPosition = false

    private var currentBlockIndex: Int { visibleBlockIndices.min() ?? 0 }
    private var lastBlockIndex: Int { max(flatBlocks.count - 1, 0) }
    /// How many blocks a Page Up/Down keypress moves by. Block-count-based
    /// rather than pixel/viewport-based -- real "one screen's worth" paging
    /// would need scroll-geometry APIs to measure the viewport, which isn't
    /// worth the added fragility here; jumping a fixed number of blocks is
    /// simple, predictable, and good enough for keyboard paging.
    private static let pageBlockCount = 12

    init(
        document: FlowDocumentVM,
        typography: Binding<TypographySettings>,
        search: SearchState,
        navigation: SectionNavigator,
        progress: ReadingProgress,
        annotations: AnnotationState
    ) {
        self.document = document
        self._typography = typography
        self.search = search
        self.navigation = navigation
        self.progress = progress
        self.annotations = annotations
        self.flatBlocks = document.sections.flatMap { section in
            section.blocks.enumerated().map { index, block in
                FlowBlockEntry(sectionId: section.id, block: block, blockIndexInSection: index)
            }
        }
        self.sectionsById = Dictionary(uniqueKeysWithValues: document.sections.map { ($0.id, $0) })
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(Array(flatBlocks.enumerated()), id: \.element.id) { index, entry in
                        blockView(entry)
                            .id(entry.id)
                            .onAppear { visibleBlockIndices.insert(index) }
                            .onDisappear { visibleBlockIndices.remove(index) }
                    }
                }
                .padding(24)
                .frame(maxWidth: 700, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .focusable()
            .focusEffectDisabled()
            .onKeyPress(.home) { jump(to: 0, proxy: proxy); return .handled }
            .onKeyPress(.end) { jump(to: lastBlockIndex, proxy: proxy); return .handled }
            .onKeyPress(.pageDown) { jump(to: currentBlockIndex + Self.pageBlockCount, proxy: proxy); return .handled }
            .onKeyPress(.pageUp) { jump(to: currentBlockIndex - Self.pageBlockCount, proxy: proxy); return .handled }
            .onKeyPress(.downArrow) { jump(to: currentBlockIndex + 1, proxy: proxy); return .handled }
            .onKeyPress(.upArrow) { jump(to: currentBlockIndex - 1, proxy: proxy); return .handled }
            .onAppear { recomputeMatches() }
            .task {
                // Restore the persisted scroll position once, after the
                // first layout pass has actually placed rows -- doing this
                // synchronously in onAppear can race SwiftUI's initial
                // layout and silently no-op the scrollTo.
                guard !hasRestoredInitialPosition, progress.initialFraction > 0, !flatBlocks.isEmpty else { return }
                hasRestoredInitialPosition = true
                let target = Int(progress.initialFraction * Double(lastBlockIndex))
                jump(to: target, proxy: proxy, animated: false)
            }
            .onChange(of: visibleBlockIndices) { _, _ in
                progress.fraction = lastBlockIndex > 0 ? Double(currentBlockIndex) / Double(lastBlockIndex) : 0
            }
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
            .onChange(of: annotations.pendingJumpAnnotationId) { _, annotationId in
                guard let annotationId, let target = entry(forAnnotationId: annotationId) else { return }
                withAnimation { proxy.scrollTo(target.id, anchor: .center) }
                annotations.pendingJumpAnnotationId = nil
            }
        }
        .sheet(item: $composerTarget) { target in
            switch target {
            case .highlight(let section, let blockIndexInSection, let blockPlainText):
                HighlightSelectionSheet(
                    itemId: document.id,
                    section: section,
                    blockIndexInSection: blockIndexInSection,
                    blockPlainText: blockPlainText,
                    annotations: annotations
                )
            case .note(let sectionId, let contextLabel, _):
                NoteComposerSheet(itemId: document.id, sectionId: sectionId, contextLabel: contextLabel, annotations: annotations)
            }
        }
    }

    // MARK: - Annotations (ADR-003)

    /// Resolves an annotation id back to the specific `FlowBlockEntry` whose
    /// byte-offset window (within its section's `concatenatedPlainText`)
    /// contains the annotation's `start` -- used both for "jump to this
    /// annotation" and, implicitly, for deciding which block a point
    /// annotation (note/bookmark) "belongs to" when rendering its indicator.
    private func entry(forAnnotationId id: String) -> FlowBlockEntry? {
        guard let annotation = annotations.items.first(where: { $0.id == id }),
            let section = sectionsById[annotation.blockId]
        else { return nil }
        return flatBlocks.first { candidate in
            guard candidate.sectionId == annotation.blockId else { return false }
            let blockStart = section.blockByteOffset(at: candidate.blockIndexInSection)
            let blockEnd = blockStart + candidate.block.plainText.utf8.count
            return annotation.start >= blockStart && annotation.start <= blockEnd
        }
    }

    /// Persisted `.highlight` annotations whose anchored span overlaps this
    /// block's own byte-offset window, converted to *character* offsets
    /// local to `entry.block.plainText` (see `characterOffset(forByteOffset:in:)`)
    /// so they can be applied the same way `applyHighlights` already applies
    /// search-match backgrounds.
    private func annotationHighlightRanges(for entry: FlowBlockEntry) -> [(range: Range<Int>, color: Color)] {
        guard let section = sectionsById[entry.sectionId] else { return [] }
        let blockText = entry.block.plainText
        let blockStart = section.blockByteOffset(at: entry.blockIndexInSection)
        let blockByteRange = blockStart..<(blockStart + blockText.utf8.count)

        return annotations.items.compactMap { annotation in
            guard annotation.kind == .highlight, annotation.blockId == entry.sectionId, annotation.len > 0 else {
                return nil
            }
            let clippedStart = max(annotation.start, blockByteRange.lowerBound)
            let clippedEnd = min(annotation.start + annotation.len, blockByteRange.upperBound)
            guard clippedStart < clippedEnd else { return nil }
            let charStart = characterOffset(forByteOffset: clippedStart - blockStart, in: blockText)
            let charEnd = characterOffset(forByteOffset: clippedEnd - blockStart, in: blockText)
            guard charStart < charEnd else { return nil }
            return (charStart..<charEnd, annotation.highlightColor?.color ?? .yellow)
        }
    }

    /// Point annotations (a zero-length `.bookmark` or standalone `.note`)
    /// anchored within this block's byte-offset window -- rendered as a
    /// small indicator menu above the block. A `.note` "attached to" a
    /// highlight (`len > 0`) is deliberately excluded here; it's only shown
    /// in `AnnotationsSidebarView`, nested under its highlight, to keep the
    /// reading view itself uncluttered.
    private func pointAnnotations(for entry: FlowBlockEntry) -> [AnnotationVM] {
        guard let section = sectionsById[entry.sectionId] else { return [] }
        let blockStart = section.blockByteOffset(at: entry.blockIndexInSection)
        let blockEnd = blockStart + entry.block.plainText.utf8.count
        return annotations.items.filter { annotation in
            (annotation.kind == .bookmark || annotation.kind == .note) && annotation.len == 0
                && annotation.blockId == entry.sectionId
                && annotation.start >= blockStart && annotation.start <= blockEnd
        }
    }

    /// Converts a UTF-8 byte offset within `text` to a Character (grapheme
    /// cluster) offset. ADR-003's `start`/`len` are byte offsets, but
    /// `AttributedString.characters.index(offsetBy:)` (used by
    /// `applyHighlights`) counts Characters -- the same space
    /// `String.rangesOfSubstring`'s search-match offsets already use.
    /// Clamped defensively; every caller here derives byte offsets from this
    /// same view's own word-boundary splitting, so a genuinely invalid
    /// (mid-character) offset should not occur in practice.
    private func characterOffset(forByteOffset byteOffset: Int, in text: String) -> Int {
        let utf8 = text.utf8
        let clamped = min(max(byteOffset, 0), utf8.count)
        guard let byteIndex = utf8.index(utf8.startIndex, offsetBy: clamped, limitedBy: utf8.endIndex),
            let stringIndex = byteIndex.samePosition(in: text)
        else {
            return text.count
        }
        return text.distance(from: text.startIndex, to: stringIndex)
    }

    private func addBookmark(at entry: FlowBlockEntry) {
        guard let section = sectionsById[entry.sectionId] else { return }
        let blockStart = section.blockByteOffset(at: entry.blockIndexInSection)
        let (prefixHash, quoteHash) = AnnotationAnchoring.hashes(fullText: section.concatenatedPlainText, start: blockStart, len: 0)
        Task {
            await core.createAnnotation(
                itemId: document.id,
                kind: .bookmark,
                blockId: section.id,
                start: blockStart,
                len: 0,
                prefixHash: prefixHash,
                quoteHash: quoteHash,
                noteText: nil
            )
            await annotations.reload(itemId: document.id, core: core)
        }
    }

    // MARK: - Navigation

    private func jump(to index: Int, proxy: ScrollViewProxy, animated: Bool = true) {
        guard !flatBlocks.isEmpty else { return }
        let clamped = min(max(index, 0), lastBlockIndex)
        let target = flatBlocks[clamped].id
        if animated {
            withAnimation { proxy.scrollTo(target, anchor: .top) }
        } else {
            proxy.scrollTo(target, anchor: .top)
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
        VStack(alignment: .leading, spacing: 4) {
            annotationIndicators(for: entry)
            blockContent(entry)
        }
        .contextMenu { annotationContextMenuItems(for: entry) }
    }

    @ViewBuilder
    private func blockContent(_ entry: FlowBlockEntry) -> some View {
        let blockMatches = matchesForBlock(entry.id)
        let highlightRanges = annotationHighlightRanges(for: entry)
        switch entry.block {
        case .heading(let level, let text):
            Text(highlighted(plain: text, matches: blockMatches, highlightRanges: highlightRanges))
                .font(headingFont(level: level))
        case .paragraph(let runs):
            Text(highlighted(runs: runs, matches: blockMatches, highlightRanges: highlightRanges))
                .lineSpacing(typography.lineSpacing.extraPoints)
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
            .font(.system(size: typography.fontSize, design: typography.fontDesign.fontDesign))
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

    /// Small indicator menu shown above a block that has one or more point
    /// annotations (bookmark/standalone note) anchored to it -- a `Menu`
    /// rather than a per-icon `.popover` so multiple indicators on the same
    /// block never fight over anchoring the same shared piece of state (see
    /// `pointAnnotations(for:)`'s doc comment for what's excluded).
    @ViewBuilder
    private func annotationIndicators(for entry: FlowBlockEntry) -> some View {
        let points = pointAnnotations(for: entry)
        if !points.isEmpty {
            let hasBookmark = points.contains { $0.kind == .bookmark }
            Menu {
                ForEach(points) { point in
                    Button(role: .destructive) {
                        Task {
                            await core.deleteAnnotation(id: point.id)
                            await annotations.reload(itemId: document.id, core: core)
                        }
                    } label: {
                        // (R5b localisation) The `displayNoteText ?? "Note —
                        // Delete"` branch forces the whole ternary
                        // (including the sibling "Bookmark — Delete"
                        // literal) to plain `String`, losing `Label`'s
                        // automatic literal handling for both fallbacks --
                        // wrap each explicitly. `displayNoteText` itself is
                        // the user's own note content (data).
                        Label(
                            point.kind == .bookmark
                                ? String(localized: "Bookmark — Delete")
                                : (point.displayNoteText ?? String(localized: "Note — Delete")),
                            systemImage: point.kind == .bookmark ? "bookmark.fill" : "note.text"
                        )
                    }
                }
            } label: {
                Image(systemName: hasBookmark ? "bookmark.fill" : "note.text")
                    .foregroundStyle(hasBookmark ? Color.orange : Color.blue)
                    .font(.caption)
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
        }
    }

    @ViewBuilder
    private func annotationContextMenuItems(for entry: FlowBlockEntry) -> some View {
        if case .paragraph = entry.block, let section = sectionsById[entry.sectionId] {
            Button {
                composerTarget = .highlight(
                    section: section,
                    blockIndexInSection: entry.blockIndexInSection,
                    blockPlainText: entry.block.plainText
                )
            } label: {
                Label("Select Text to Highlight…", systemImage: "highlighter")
            }
        }
        Button {
            composerTarget = .note(sectionId: entry.sectionId, contextLabel: sectionContextLabel(for: entry), id: UUID().uuidString)
        } label: {
            Label("Add Note Here", systemImage: "note.text.badge.plus")
        }
        Button {
            addBookmark(at: entry)
        } label: {
            Label("Add Bookmark Here", systemImage: "bookmark")
        }
    }

    /// A short label describing where a standalone note is being added --
    /// shown inside `NoteComposerSheet` for context, since that sheet has no
    /// other way to show the user what they're annotating.
    // (R5b localisation) `section.heading?.text` is real document content
    // (data) when present; only the two fallback literals below are fixed
    // UI strings, so only those are wrapped.
    private func sectionContextLabel(for entry: FlowBlockEntry) -> String {
        guard let section = sectionsById[entry.sectionId] else {
            return String(localized: "This location")
        }
        return section.heading?.text ?? String(localized: "This paragraph")
    }

    private func headingFont(level: Int) -> Font {
        let design = typography.fontDesign.fontDesign
        switch level {
        case ...1: return .system(size: typography.fontSize + 11, weight: .bold, design: design)
        case 2: return .system(size: typography.fontSize + 6, weight: .bold, design: design)
        default: return .system(size: typography.fontSize + 3, weight: .semibold, design: design)
        }
    }

    // MARK: - AttributedString construction

    /// Builds a plain (unstyled beyond font size) `AttributedString` for
    /// headings/lists/images, with persisted-highlight and search-match
    /// backgrounds applied.
    private func highlighted(
        plain text: String,
        matches: [(offset: Int, range: Range<Int>)],
        highlightRanges: [(range: Range<Int>, color: Color)]
    ) -> AttributedString {
        var attr = AttributedString(text)
        applyHighlights(&attr, matches: matches, highlightRanges: highlightRanges)
        return attr
    }

    /// Builds a paragraph's `AttributedString` by concatenating its
    /// `TextRun`s with their own bold/italic/code styling, then applies
    /// persisted-highlight and search-match backgrounds on top — see the
    /// type-level doc comment for why the two offset spaces line up.
    private func highlighted(
        runs: [FlowTextRunVM],
        matches: [(offset: Int, range: Range<Int>)],
        highlightRanges: [(range: Range<Int>, color: Color)]
    ) -> AttributedString {
        var attr = AttributedString()
        for run in runs {
            var piece = AttributedString(run.text)
            var font = Font.system(size: typography.fontSize, design: typography.fontDesign.fontDesign)
            if run.code {
                // Code spans always render monospaced -- that's a content
                // distinction (this is literal code), not something the
                // reader's font-design preference should override.
                font = .system(size: typography.fontSize, design: .monospaced)
            } else {
                if run.bold { font = font.bold() }
                if run.italic { font = font.italic() }
            }
            piece.font = font
            attr += piece
        }
        applyHighlights(&attr, matches: matches, highlightRanges: highlightRanges)
        return attr
    }

    /// Applies persisted-annotation highlight backgrounds first (a base
    /// layer, in each highlight's own colour), then search-match backgrounds
    /// on top (always orange/yellow, regardless of any highlight underneath)
    /// so the active in-document search is never visually lost inside an
    /// existing highlight.
    private func applyHighlights(
        _ attr: inout AttributedString,
        matches: [(offset: Int, range: Range<Int>)],
        highlightRanges: [(range: Range<Int>, color: Color)]
    ) {
        for item in highlightRanges {
            guard
                let lower = attr.characters.index(attr.startIndex, offsetBy: item.range.lowerBound, limitedBy: attr.endIndex),
                let upper = attr.characters.index(attr.startIndex, offsetBy: item.range.upperBound, limitedBy: attr.endIndex)
            else { continue }
            attr[lower..<upper].backgroundColor = item.color.opacity(0.45)
        }
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

/// Identifies which annotation-creation sheet is presented, and for which
/// block/section -- a single enum (rather than several separate optional
/// `@State` sheet targets) so at most one composer sheet is ever shown at a
/// time, matching `TagEditorTarget`'s existing idiom elsewhere in this app.
private enum AnnotationComposerTarget: Identifiable {
    case highlight(section: FlowSectionVM, blockIndexInSection: Int, blockPlainText: String)
    /// `id` is a caller-supplied nonce (a fresh `UUID` per "Add Note Here"
    /// invocation) rather than something derived from `sectionId` alone,
    /// since Swift enum cases can't carry a default parameter value the way
    /// a function can -- callers always pass a fresh one explicitly.
    case note(sectionId: String, contextLabel: String, id: String)

    var id: String {
        switch self {
        case .highlight(let section, let blockIndexInSection, _):
            return "highlight-\(section.id)-\(blockIndexInSection)"
        case .note(_, _, let id):
            return id
        }
    }
}
