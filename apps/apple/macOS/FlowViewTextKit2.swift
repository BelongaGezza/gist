import AppKit
import SwiftUI

/// Prototype B for CLAUDE.md's Q8: renders the whole document as one
/// `NSAttributedString` in an `NSTextView` backed by TextKit 2
/// (`NSTextLayoutManager`, via `NSTextView(usingTextLayoutManager: true)`),
/// wrapped for SwiftUI via `NSViewRepresentable`. See
/// `FlowViewSwiftUINative.swift` for the SwiftUI-native alternative this is
/// being compared against.
///
/// Search reuses `NSTextView`'s own selection/scroll machinery instead of
/// hand-rolled highlighting: a match becomes a real selection range plus
/// `showFindIndicator(for:)` (the same yellow "found" bezel Spotlight-style
/// find bars use), which also means text selection/copy is native `NSText`
/// behaviour for free — see the Q8 writeup for how this compares to the
/// SwiftUI version's per-block `AttributedString` highlighting.
struct FlowViewTextKit2: NSViewRepresentable, ReadingLayout {
    let document: FlowDocumentVM
    @Binding var typography: TypographySettings
    @ObservedObject var search: SearchState
    @ObservedObject var navigation: SectionNavigator

    init(document: FlowDocumentVM, typography: Binding<TypographySettings>, search: SearchState, navigation: SectionNavigator) {
        self.document = document
        self._typography = typography
        self.search = search
        self.navigation = navigation
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = NSTextView(usingTextLayoutManager: true)
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = false
        textView.textContainerInset = NSSize(width: 24, height: 24)
        textView.textContainer?.widthTracksTextView = true
        textView.autoresizingMask = [.width]

        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.documentView = textView
        scrollView.drawsBackground = false

        rebuildContent(textView, context: context)
        return scrollView
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {
        guard let textView = nsView.documentView as? NSTextView else { return }
        let coordinator = context.coordinator

        if coordinator.lastFontSize != typography.fontSize {
            rebuildContent(textView, context: context)
        }
        if coordinator.lastQuery != search.query {
            coordinator.lastQuery = search.query
            performSearch(textView, context: context)
        }
        if coordinator.lastMatchIndex != search.currentMatchIndex {
            coordinator.lastMatchIndex = search.currentMatchIndex
            scrollToCurrentMatch(textView, context: context)
        }
        if let pending = navigation.pendingSectionId, pending != coordinator.lastNavigatedSectionId {
            coordinator.lastNavigatedSectionId = pending
            if let range = coordinator.sectionRanges[pending] {
                let anchor = NSRange(location: range.location, length: min(range.length, 1))
                textView.scrollRangeToVisible(range)
                textView.showFindIndicator(for: anchor)
            }
        }
    }

    // MARK: - Content

    private func rebuildContent(_ textView: NSTextView, context: Context) {
        let (attributed, sectionRanges) = Self.buildAttributedString(document: document, fontSize: typography.fontSize)
        textView.textStorage?.setAttributedString(attributed)
        context.coordinator.sectionRanges = sectionRanges
        context.coordinator.lastFontSize = typography.fontSize
        // Font size changed the whole layout, so any previously computed
        // match ranges are stale — force a re-search on the rebuilt text.
        context.coordinator.lastQuery = ""
    }

    private static func buildAttributedString(
        document: FlowDocumentVM,
        fontSize: Double
    ) -> (NSAttributedString, [String: NSRange]) {
        let result = NSMutableAttributedString()
        var sectionRanges: [String: NSRange] = [:]
        let size = CGFloat(fontSize)
        let bodyFont = NSFont.systemFont(ofSize: size)
        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.lineSpacing = 4
        paragraphStyle.paragraphSpacing = 14

        for section in document.sections {
            let sectionStart = result.length
            for block in section.blocks {
                switch block {
                case .heading(let level, let text):
                    let headingSize = size + CGFloat(max(11 - level * 3, 2))
                    let font = NSFont.boldSystemFont(ofSize: headingSize)
                    result.append(
                        NSAttributedString(
                            string: text + "\n\n",
                            attributes: [.font: font, .paragraphStyle: paragraphStyle]
                        ))
                case .paragraph(let runs):
                    for run in runs {
                        result.append(
                            NSAttributedString(
                                string: run.text,
                                attributes: [
                                    .font: font(for: run, base: bodyFont, size: size),
                                    .paragraphStyle: paragraphStyle,
                                ]
                            ))
                    }
                    result.append(NSAttributedString(string: "\n\n", attributes: [.font: bodyFont]))
                case .list(let ordered, let items):
                    for (index, item) in items.enumerated() {
                        let bullet = ordered ? "\(index + 1). " : "\u{2022} "
                        result.append(
                            NSAttributedString(
                                string: bullet + item + "\n",
                                attributes: [.font: bodyFont, .paragraphStyle: paragraphStyle]
                            ))
                    }
                    result.append(NSAttributedString(string: "\n", attributes: [.font: bodyFont]))
                case .image(_, let alt, let caption):
                    var label = "[image"
                    if let alt, !alt.isEmpty { label += ": \(alt)" }
                    label += "]"
                    if let caption, !caption.isEmpty { label += " \u{2014} \(caption)" }
                    let italicDescriptor = bodyFont.fontDescriptor.withSymbolicTraits(.italic)
                    let italicFont = NSFont(descriptor: italicDescriptor, size: size) ?? bodyFont
                    result.append(
                        NSAttributedString(
                            string: label + "\n\n",
                            attributes: [
                                .font: italicFont,
                                .foregroundColor: NSColor.secondaryLabelColor,
                                .paragraphStyle: paragraphStyle,
                            ]
                        ))
                }
            }
            sectionRanges[section.id] = NSRange(location: sectionStart, length: result.length - sectionStart)
        }
        return (result, sectionRanges)
    }

    private static func font(for run: FlowTextRunVM, base: NSFont, size: CGFloat) -> NSFont {
        if run.code {
            return NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
        }
        var traits: NSFontDescriptor.SymbolicTraits = []
        if run.bold { traits.insert(.bold) }
        if run.italic { traits.insert(.italic) }
        guard !traits.isEmpty else { return base }
        let descriptor = base.fontDescriptor.withSymbolicTraits(traits)
        return NSFont(descriptor: descriptor, size: size) ?? base
    }

    // MARK: - Search

    private func performSearch(_ textView: NSTextView, context: Context) {
        let coordinator = context.coordinator
        let text = textView.string
        guard !search.query.isEmpty else {
            coordinator.matches = []
            search.matchCount = 0
            search.currentMatchIndex = 0
            return
        }
        var ranges: [NSRange] = []
        var searchStart = text.startIndex
        while searchStart < text.endIndex,
            let found = text.range(of: search.query, options: .caseInsensitive, range: searchStart..<text.endIndex)
        {
            ranges.append(NSRange(found, in: text))
            searchStart = found.upperBound
        }
        coordinator.matches = ranges
        search.matchCount = ranges.count
        search.currentMatchIndex = 0
        coordinator.lastMatchIndex = 0
        scrollToCurrentMatch(textView, context: context)
    }

    private func scrollToCurrentMatch(_ textView: NSTextView, context: Context) {
        let coordinator = context.coordinator
        guard coordinator.matches.indices.contains(search.currentMatchIndex) else { return }
        let range = coordinator.matches[search.currentMatchIndex]
        textView.scrollRangeToVisible(range)
        textView.setSelectedRange(range)
        textView.showFindIndicator(for: range)
    }

    // MARK: - Coordinator

    final class Coordinator {
        var matches: [NSRange] = []
        var sectionRanges: [String: NSRange] = [:]
        var lastFontSize: Double = -1
        var lastQuery: String = ""
        var lastMatchIndex: Int = -1
        var lastNavigatedSectionId: String?
    }
}
