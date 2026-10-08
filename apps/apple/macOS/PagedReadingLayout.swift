import SwiftUI

/// The optional page-turn reading view (spec Q3, v1.1; ADR-023): a second
/// `ReadingLayout` conformer that cuts the document into fixed-size pages at
/// the current window size and typography, then shows one page at a time.
///
/// Pagination is measured by `PageBlockMeasurer` (AppKit text layout used as a
/// measuring instrument) and computed by the pure `Paginator`; this view only
/// renders a page's `PagePiece`s. The reader's place is kept as a
/// `PagePosition` anchor (flat block index + character offset), never a page
/// number, so a resize or typography change reflows to the same place.
///
/// Not visually verified at the time of writing (no display access in the
/// authoring environment) -- see `docs/qa-manual-clickthrough-m3.md`.
struct PagedReadingLayout: ReadingLayout {
    static var persistsFlowScrollFraction: Bool { false }

    @MainActor static func seedProgress(itemId: String, carryFraction: Double?) -> ReadingProgress {
        if let carry = carryFraction {
            return ReadingProgress(initialFraction: carry)
        }
        if let anchor = PagedPositionStore.load(itemId: itemId) {
            let progress = ReadingProgress(initialFraction: 0)
            progress.initialAnchor = anchor
            return progress
        }
        // First time in paged mode: start where the scrolling view left off
        // (read-only; the flow store is never written from here).
        return ReadingProgress(initialFraction: FlowScrollPositionStore.load(itemId: itemId))
    }

    let document: FlowDocumentVM
    @Binding var typography: TypographySettings
    @ObservedObject var search: SearchState
    @ObservedObject var navigation: SectionNavigator
    @ObservedObject var progress: ReadingProgress
    @ObservedObject var annotations: AnnotationState
    @EnvironmentObject var core: CoreClient
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private let flatBlocks: [FlowBlockVM]
    private let index: PagedDocumentIndex
    private let sectionsById: [String: FlowSectionVM]

    @State private var layouts: [PageBlockLayout] = []
    @State private var pages: [PageRange] = []
    @State private var currentPage = 0
    @State private var anchor: PagePosition
    @State private var isPaginating = true
    @State private var geometry = PageGeometry(textWidth: 600, pageHeight: 400)
    @State private var matches: [PagedMatch] = []
    @State private var composerTarget: PagedComposerTarget?

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
        self.flatBlocks = document.sections.flatMap(\.blocks)
        let index = PagedDocumentIndex(document: document)
        self.index = index
        self.sectionsById = Dictionary(document.sections.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let start = progress.initialAnchor
            ?? PagePosition(
                blockIndex: ReadingPositionMapping.blockIndex(forFraction: progress.initialFraction, blockCount: index.blockCount),
                offset: 0
            )
        _anchor = State(initialValue: start.clamped(blockCharCounts: index.blockCharCounts))
    }

    // MARK: - Geometry

    struct PageGeometry: Equatable {
        var textWidth: CGFloat
        var pageHeight: CGFloat
    }

    private static let horizontalPadding: CGFloat = 24
    private static let verticalPadding: CGFloat = 20
    private static let controlsHeight: CGFloat = 44

    private func geometry(for size: CGSize) -> PageGeometry {
        let width = min(max(size.width - 2 * Self.horizontalPadding, 80), 700)
        // Slack: one extra line-spacing plus a few points, because SwiftUI Text
        // and the AppKit measurer can disagree by a fraction of a line.
        let slack = CGFloat(typography.lineSpacing.extraPoints) + 4
        let height = max(size.height - Self.controlsHeight - 2 * Self.verticalPadding - slack, 40)
        return PageGeometry(textWidth: width.rounded(), pageHeight: height.rounded())
    }

    private struct RepaginationKey: Equatable {
        var geometry: PageGeometry
        var typography: TypographySettings
        var documentId: String
    }

    // MARK: - Body

    var body: some View {
        GeometryReader { proxy in
            let g = geometry(for: proxy.size)
            VStack(spacing: 0) {
                pageArea(geometry: g)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                controls
            }
            .task(id: RepaginationKey(geometry: g, typography: typography, documentId: document.id)) {
                await repaginate(geometry: g)
            }
        }
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.rightArrow) { turn(1); return .handled }
        .onKeyPress(.downArrow) { turn(1); return .handled }
        .onKeyPress(.pageDown) { turn(1); return .handled }
        .onKeyPress(.space) { turn(1); return .handled }
        .onKeyPress(.leftArrow) { turn(-1); return .handled }
        .onKeyPress(.upArrow) { turn(-1); return .handled }
        .onKeyPress(.pageUp) { turn(-1); return .handled }
        .onKeyPress(.home) { goToPage(0); return .handled }
        .onKeyPress(.end) { goToPage(pages.count - 1); return .handled }
        .onAppear { recomputeMatches() }
        .onChange(of: search.query) { _, _ in
            recomputeMatches()
            goToCurrentMatch()
        }
        .onChange(of: search.currentMatchIndex) { _, _ in goToCurrentMatch() }
        .onChange(of: navigation.pendingSectionId) { _, sectionId in
            guard let sectionId, let block = index.firstBlockIndex(ofSection: sectionId) else { return }
            go(to: PagePosition(blockIndex: block, offset: 0))
            navigation.pendingSectionId = nil
        }
        .onChange(of: navigation.pendingBlockIndex) { _, block in
            guard let block else { return }
            // Read-aloud follow-along: only move if the spoken block is not
            // already visible on the current page.
            if pages.indices.contains(currentPage), !pageShows(block: block, page: pages[currentPage]) {
                go(to: PagePosition(blockIndex: block, offset: 0))
            }
            navigation.pendingBlockIndex = nil
        }
        .onChange(of: annotations.pendingJumpAnnotationId) { _, annotationId in
            guard let annotationId,
                let annotation = annotations.items.first(where: { $0.id == annotationId }),
                let position = index.position(forAnchorSection: annotation.blockId, byteStart: annotation.start)
            else { return }
            go(to: position)
            annotations.pendingJumpAnnotationId = nil
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

    // MARK: - Page area

    @ViewBuilder
    private func pageArea(geometry g: PageGeometry) -> some View {
        if isPaginating && pages.isEmpty {
            ProgressView("Paginating…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if pages.isEmpty {
            Text("This document has no content.")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            let page = pages[min(currentPage, pages.count - 1)]
            pageContent(page: page, geometry: g)
                .frame(width: g.textWidth, height: g.pageHeight, alignment: .topLeading)
                .clipped()
                .padding(.vertical, Self.verticalPadding)
                .frame(maxWidth: .infinity)
                .id(currentPage)
                .transition(.opacity)
                .accessibilityElement(children: .contain)
                .accessibilityLabel("Page \(currentPage + 1) of \(pages.count)")
                .accessibilityAction(named: Text("Next Page")) { turn(1) }
                .accessibilityAction(named: Text("Previous Page")) { turn(-1) }
        }
    }

    @ViewBuilder
    private func pageContent(page: PageRange, geometry g: PageGeometry) -> some View {
        let pieces = Paginator.pieces(of: page, blockCharCounts: index.blockCharCounts)
        let stack = VStack(alignment: .leading, spacing: PageBlockMeasurer.blockSpacing) {
            ForEach(pieces, id: \.blockIndex) { piece in
                pieceView(piece)
            }
        }
        if pieces.count == 1, layouts.indices.contains(pieces[0].blockIndex),
            case .atomic(let height) = layouts[pieces[0].blockIndex].kind, height > g.pageHeight
        {
            // Oversized table/list/image: owns its page and scrolls inside it.
            ScrollView(.vertical) { stack }
        } else {
            stack
        }
    }

    private var controls: some View {
        HStack {
            Button { turn(-1) } label: {
                Image(systemName: "chevron.left")
            }
            .disabled(currentPage <= 0)
            .accessibilityLabel("Previous Page")
            Spacer()
            Text("Page \(pages.isEmpty ? 0 : currentPage + 1) of \(pages.count)")
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            Spacer()
            Button { turn(1) } label: {
                Image(systemName: "chevron.right")
            }
            .disabled(currentPage >= pages.count - 1)
            .accessibilityLabel("Next Page")
        }
        .padding(.horizontal, Self.horizontalPadding)
        .frame(height: Self.controlsHeight)
        .background(.thinMaterial)
    }

    // MARK: - Pagination and navigation

    private func repaginate(geometry g: PageGeometry) async {
        // Debounce live window resizing; a newer key cancels this task.
        try? await Task.sleep(nanoseconds: 120_000_000)
        guard !Task.isCancelled else { return }
        let blocks = flatBlocks
        let typo = typography
        let result: (layouts: [PageBlockLayout], pages: [PageRange]) = await Task.detached(priority: .userInitiated) {
            let measured = PageBlockMeasurer.layouts(for: blocks, width: g.textWidth, typography: typo)
            let cut = Paginator.paginate(blocks: measured, pageHeight: g.pageHeight, blockSpacing: PageBlockMeasurer.blockSpacing)
            return (measured, cut)
        }.value
        guard !Task.isCancelled else { return }
        geometry = g
        layouts = result.layouts
        pages = result.pages
        // The anchor is deliberately not moved by a reflow: the page that
        // contains it is shown, so repeated resizes cannot drift the position.
        currentPage = Paginator.pageIndex(containing: anchor, in: pages) ?? 0
        isPaginating = false
        publishProgress()
    }

    private func turn(_ delta: Int) {
        goToPage(Paginator.clampedPage(currentPage, delta: delta, count: pages.count))
    }

    private func goToPage(_ target: Int) {
        guard !pages.isEmpty else { return }
        let clamped = Paginator.clampedPage(target, delta: 0, count: pages.count)
        guard clamped != currentPage else { return }
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) {
            currentPage = clamped
        }
        anchor = pages[clamped].start
        PagedPositionStore.save(itemId: document.id, position: anchor)
        publishProgress()
        AccessibilityNotification.Announcement(String(localized: "Page \(clamped + 1) of \(pages.count)")).post()
    }

    private func go(to position: PagePosition) {
        guard let target = Paginator.pageIndex(containing: position, in: pages), target != currentPage else { return }
        goToPage(target)
    }

    private func pageShows(block: Int, page: PageRange) -> Bool {
        Paginator.pieces(of: page, blockCharCounts: index.blockCharCounts).contains { $0.blockIndex == block }
    }

    private func publishProgress() {
        guard pages.indices.contains(currentPage) else { return }
        progress.fraction = ReadingPositionMapping.fraction(
            forBlockIndex: pages[currentPage].start.blockIndex,
            blockCount: index.blockCount
        )
    }

    // MARK: - Search

    private func recomputeMatches() {
        matches = index.matches(for: search.query)
        search.matchCount = matches.count
        search.currentMatchIndex = 0
    }

    private func goToCurrentMatch() {
        guard matches.indices.contains(search.currentMatchIndex) else { return }
        let match = matches[search.currentMatchIndex]
        go(to: PagePosition(blockIndex: match.blockIndex, offset: match.range.lowerBound))
    }

    // MARK: - Piece rendering

    @ViewBuilder
    private func pieceView(_ piece: PagePiece) -> some View {
        let entryBlock = flatBlocks[piece.blockIndex]
        VStack(alignment: .leading, spacing: 4) {
            if piece.from == 0 { pointIndicators(blockIndex: piece.blockIndex) }
            pieceContent(entryBlock, piece: piece)
        }
        .contextMenu { contextMenuItems(blockIndex: piece.blockIndex) }
    }

    @ViewBuilder
    private func pieceContent(_ block: FlowBlockVM, piece: PagePiece) -> some View {
        let blockMatches = matchesForBlock(piece.blockIndex)
        let highlightRanges = annotationHighlightRanges(blockIndex: piece.blockIndex)
        switch block {
        case .heading(let level, let text):
            Text(slice(styled(plain: text, matches: blockMatches, highlights: highlightRanges), piece: piece))
                .font(headingFont(level: level))
                .fixedSize(horizontal: false, vertical: true)
        case .paragraph(let runs):
            Text(slice(styled(runs: runs, matches: blockMatches, highlights: highlightRanges), piece: piece))
                .lineSpacing(typography.lineSpacing.extraPoints)
                .fixedSize(horizontal: false, vertical: true)
        case .list(let ordered, let items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { i, item in
                    HStack(alignment: .top, spacing: 8) {
                        Text(ordered ? "\(i + 1)." : "\u{2022}").foregroundStyle(.secondary)
                        Text(item)
                    }
                }
            }
            .font(.system(size: typography.fontSize, design: typography.fontDesign.fontDesign))
        case .table(let rows, let headerRow, let spans):
            FlowTableView(
                rows: rows,
                headerRow: headerRow,
                spans: spans,
                fontSize: typography.fontSize,
                fontDesign: typography.fontDesign.fontDesign
            ) { text, cellRange in
                styled(
                    plain: text,
                    matches: blockMatches.compactMap { match in
                        TableCellLayout.clip(match.range, to: cellRange).map { (match.offset, $0) }
                    },
                    highlights: highlightRanges.compactMap { item in
                        TableCellLayout.clip(item.range, to: cellRange).map { ($0, item.color) }
                    }
                )
            }
        case .image(_, let alt, let caption):
            VStack(alignment: .leading, spacing: 4) {
                Image(systemName: "photo").font(.system(size: 32)).foregroundStyle(.secondary)
                if let alt, !alt.isEmpty { Text(alt).font(.caption).foregroundStyle(.secondary) }
                if let caption, !caption.isEmpty { Text(caption).font(.caption).italic() }
            }
        }
    }

    /// Cuts the full-block attributed string to the characters this page owns.
    private func slice(_ attr: AttributedString, piece: PagePiece) -> AttributedString {
        let total = attr.characters.count
        guard piece.from > 0 || piece.to < total else { return attr }
        guard
            let lower = attr.characters.index(attr.startIndex, offsetBy: min(piece.from, total), limitedBy: attr.endIndex),
            let upper = attr.characters.index(attr.startIndex, offsetBy: min(piece.to, total), limitedBy: attr.endIndex),
            lower <= upper
        else { return attr }
        return AttributedString(attr[lower..<upper])
    }

    private func matchesForBlock(_ blockIndex: Int) -> [(offset: Int, range: Range<Int>)] {
        matches.enumerated().compactMap { offset, match in
            match.blockIndex == blockIndex ? (offset, match.range) : nil
        }
    }

    private func headingFont(level: Int) -> Font {
        let design = typography.fontDesign.fontDesign
        switch level {
        case ...1: return .system(size: typography.fontSize + 11, weight: .bold, design: design)
        case 2: return .system(size: typography.fontSize + 6, weight: .bold, design: design)
        default: return .system(size: typography.fontSize + 3, weight: .semibold, design: design)
        }
    }

    // MARK: - Styling (mirrors FlowViewSwiftUINative; ADR-023)

    private func styled(
        plain text: String,
        matches: [(offset: Int, range: Range<Int>)],
        highlights: [(range: Range<Int>, color: Color)]
    ) -> AttributedString {
        var attr = AttributedString(text)
        applyHighlights(&attr, matches: matches, highlights: highlights)
        return attr
    }

    private func styled(
        runs: [FlowTextRunVM],
        matches: [(offset: Int, range: Range<Int>)],
        highlights: [(range: Range<Int>, color: Color)]
    ) -> AttributedString {
        var attr = AttributedString()
        for run in runs {
            var piece = AttributedString(run.text)
            var font = Font.system(size: typography.fontSize, design: typography.fontDesign.fontDesign)
            if run.code {
                font = .system(size: typography.fontSize, design: .monospaced)
            } else {
                if run.bold { font = font.bold() }
                if run.italic { font = font.italic() }
            }
            piece.font = font
            attr += piece
        }
        applyHighlights(&attr, matches: matches, highlights: highlights)
        return attr
    }

    private func applyHighlights(
        _ attr: inout AttributedString,
        matches: [(offset: Int, range: Range<Int>)],
        highlights: [(range: Range<Int>, color: Color)]
    ) {
        for item in highlights {
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

    // MARK: - Annotations (ADR-003 anchors; never page-relative)

    private func annotationHighlightRanges(blockIndex: Int) -> [(range: Range<Int>, color: Color)] {
        guard let section = sectionsById[index.blockSectionIds[blockIndex]] else { return [] }
        let blockText = index.blockTexts[blockIndex]
        let blockStart = section.blockByteOffset(at: index.blockIndexInSection[blockIndex])
        let blockEnd = blockStart + blockText.utf8.count
        return annotations.items.compactMap { annotation in
            guard annotation.kind == .highlight, annotation.blockId == section.id, annotation.len > 0 else { return nil }
            let clippedStart = max(annotation.start, blockStart)
            let clippedEnd = min(annotation.start + annotation.len, blockEnd)
            guard clippedStart < clippedEnd else { return nil }
            let charStart = PageTextOffsets.characterOffset(forByteOffset: clippedStart - blockStart, in: blockText)
            let charEnd = PageTextOffsets.characterOffset(forByteOffset: clippedEnd - blockStart, in: blockText)
            guard charStart < charEnd else { return nil }
            return (charStart..<charEnd, annotation.highlightColor?.color ?? .yellow)
        }
    }

    private func pointAnnotations(blockIndex: Int) -> [AnnotationVM] {
        guard let section = sectionsById[index.blockSectionIds[blockIndex]] else { return [] }
        let blockStart = section.blockByteOffset(at: index.blockIndexInSection[blockIndex])
        let blockEnd = blockStart + index.blockTexts[blockIndex].utf8.count
        return annotations.items.filter { annotation in
            (annotation.kind == .bookmark || annotation.kind == .note) && annotation.len == 0
                && annotation.blockId == section.id
                && annotation.start >= blockStart && annotation.start <= blockEnd
        }
    }

    @ViewBuilder
    private func pointIndicators(blockIndex: Int) -> some View {
        let points = pointAnnotations(blockIndex: blockIndex)
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
            .accessibilityLabel(hasBookmark ? "Bookmark actions" : "Note actions")
        }
    }

    @ViewBuilder
    private func contextMenuItems(blockIndex: Int) -> some View {
        if let section = sectionsById[index.blockSectionIds[blockIndex]] {
            if case .paragraph = flatBlocks[blockIndex] {
                Button {
                    composerTarget = .highlight(
                        section: section,
                        blockIndexInSection: index.blockIndexInSection[blockIndex],
                        blockPlainText: index.blockTexts[blockIndex]
                    )
                } label: {
                    Label("Select Text to Highlight…", systemImage: "highlighter")
                }
            }
            Button {
                composerTarget = .note(
                    sectionId: section.id,
                    contextLabel: section.heading?.text ?? String(localized: "This paragraph"),
                    id: UUID().uuidString
                )
            } label: {
                Label("Add Note Here", systemImage: "note.text.badge.plus")
            }
            Button {
                addBookmark(blockIndex: blockIndex, section: section)
            } label: {
                Label("Add Bookmark Here", systemImage: "bookmark")
            }
        }
    }

    private func addBookmark(blockIndex: Int, section: FlowSectionVM) {
        let blockStart = section.blockByteOffset(at: index.blockIndexInSection[blockIndex])
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
}

private enum PagedComposerTarget: Identifiable {
    case highlight(section: FlowSectionVM, blockIndexInSection: Int, blockPlainText: String)
    case note(sectionId: String, contextLabel: String, id: String)

    var id: String {
        switch self {
        case .highlight(let section, let blockIndexInSection, _): return "highlight-\(section.id)-\(blockIndexInSection)"
        case .note(_, _, let id): return id
        }
    }
}
