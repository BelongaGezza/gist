import AppKit
import Foundation

/// Measures flow blocks with AppKit text layout and produces the
/// `PageBlockLayout` values the pure `Paginator` consumes (ADR-023). AppKit is
/// used purely as a measuring instrument; nothing here is drawn. Sizes mirror
/// the SwiftUI flow view's fonts (`FlowViewSwiftUINative.headingFont`, run
/// styling) so measured line breaks agree with rendered ones as closely as the
/// two text engines allow -- they can differ by a fraction of a line, which is
/// why the paged view subtracts a slack and clips each page (ADR-023).
enum PageBlockMeasurer {
    /// Matches `FlowViewSwiftUINative`'s `LazyVStack(spacing: 16)`.
    static let blockSpacing: CGFloat = 16

    static func layouts(for blocks: [FlowBlockVM], width: CGFloat, typography: TypographySettings) -> [PageBlockLayout] {
        let usable = max(width.isFinite ? width : 100, 40)
        return blocks.map { layout(for: $0, width: usable, typography: typography) }
    }

    static func layout(for block: FlowBlockVM, width: CGFloat, typography: TypographySettings) -> PageBlockLayout {
        let extra = CGFloat(typography.lineSpacing.extraPoints)
        switch block {
        case .heading(let level, let text):
            let size = typography.fontSize + (level <= 1 ? 11 : (level == 2 ? 6 : 3))
            let font = nsFont(size: size, design: typography.fontDesign, bold: true, italic: false)
            let measured = measureLines(NSAttributedString(string: text, attributes: [.font: font]), width: width, lineSpacing: extra)
            return PageBlockLayout(
                kind: .lines(lineStarts: measured.lineStarts, lineHeight: measured.lineHeight),
                charCount: text.count,
                keepWithNext: true
            )

        case .paragraph(let runs):
            let attributed = NSMutableAttributedString()
            for run in runs {
                let font: NSFont
                if run.code {
                    font = NSFont.monospacedSystemFont(ofSize: typography.fontSize, weight: .regular)
                } else {
                    font = nsFont(size: typography.fontSize, design: typography.fontDesign, bold: run.bold, italic: run.italic)
                }
                attributed.append(NSAttributedString(string: run.text, attributes: [.font: font]))
            }
            let measured = measureLines(attributed, width: width, lineSpacing: extra)
            return PageBlockLayout(
                kind: .lines(lineStarts: measured.lineStarts, lineHeight: measured.lineHeight),
                charCount: attributed.string.count
            )

        case .list(let ordered, let items):
            let font = nsFont(size: typography.fontSize, design: typography.fontDesign, bold: false, italic: false)
            let markerWidth: CGFloat = ordered ? 32 : 20
            var height: CGFloat = 0
            for item in items {
                let measured = measureLines(NSAttributedString(string: item, attributes: [.font: font]), width: max(width - markerWidth, 40), lineSpacing: 0)
                height += CGFloat(max(measured.lineStarts.count, 1)) * measured.lineHeight
            }
            height += CGFloat(max(items.count - 1, 0)) * 6
            return PageBlockLayout(kind: .atomic(height: height), charCount: block.plainText.count)

        case .table(let rows, _, _):
            return PageBlockLayout(
                kind: .atomic(height: estimatedTableHeight(rows: rows, width: width, typography: typography)),
                charCount: block.plainText.count
            )

        case .image(_, let alt, let caption):
            // Symbol (32pt font) plus optional caption-font lines, 4pt apart.
            let captionFont = NSFont.systemFont(ofSize: 12)
            var height: CGFloat = 40
            for text in [alt, caption] {
                guard let text, !text.isEmpty else { continue }
                let measured = measureLines(NSAttributedString(string: text, attributes: [.font: captionFont]), width: width, lineSpacing: 0)
                height += 4 + CGFloat(max(measured.lineStarts.count, 1)) * measured.lineHeight
            }
            return PageBlockLayout(kind: .atomic(height: height), charCount: block.plainText.count)
        }
    }

    // MARK: - Internals

    /// Line-break character offsets and a uniform line height for `text` laid
    /// out at `width`. Offsets are converted from NSLayoutManager's UTF-16
    /// units to Characters (the paginator's unit) with one forward walk.
    static func measureLines(_ text: NSAttributedString, width: CGFloat, lineSpacing: CGFloat) -> (lineStarts: [Int], lineHeight: CGFloat) {
        let fallbackHeight = (text.length > 0 ? (text.attribute(.font, at: 0, effectiveRange: nil) as? NSFont) : nil)
            .map { ceil($0.ascender - $0.descender + $0.leading) + lineSpacing } ?? (ceil(NSFont.systemFont(ofSize: 13).boundingRectForFont.height) + lineSpacing)
        guard text.length > 0 else { return ([0], max(fallbackHeight, 1)) }

        let mutable = NSMutableAttributedString(attributedString: text)
        let style = NSMutableParagraphStyle()
        style.lineSpacing = lineSpacing
        mutable.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: mutable.length))

        let storage = NSTextStorage(attributedString: mutable)
        let layoutManager = NSLayoutManager()
        let container = NSTextContainer(size: NSSize(width: max(width, 1), height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        layoutManager.addTextContainer(container)
        storage.addLayoutManager(layoutManager)
        layoutManager.ensureLayout(for: container)

        var utf16Starts: [Int] = []
        var totalHeight: CGFloat = 0
        let glyphCount = layoutManager.numberOfGlyphs
        var glyphIndex = 0
        while glyphIndex < glyphCount {
            var range = NSRange()
            let rect = layoutManager.lineFragmentRect(forGlyphAt: glyphIndex, effectiveRange: &range)
            let charRange = layoutManager.characterRange(forGlyphRange: range, actualGlyphRange: nil)
            utf16Starts.append(charRange.location)
            totalHeight += rect.height
            glyphIndex = NSMaxRange(range)
            if range.length == 0 { break }
        }
        guard !utf16Starts.isEmpty else { return ([0], max(fallbackHeight, 1)) }

        let string = text.string
        var starts: [Int] = []
        var characterOffset = 0
        var cursor = string.startIndex
        for utf16 in utf16Starts {
            let target = String.Index(utf16Offset: min(utf16, string.utf16.count), in: string)
            while cursor < target {
                cursor = string.index(after: cursor)
                characterOffset += 1
            }
            if starts.last != characterOffset { starts.append(characterOffset) }
        }
        if starts.first != 0 { starts.insert(0, at: 0) }
        let height = max(totalHeight / CGFloat(utf16Starts.count), 1)
        return (starts, height)
    }

    /// Row-by-row estimate: tallest wrapped cell per row plus the grid's cell
    /// padding (8pt horizontal, 6pt vertical, see `FlowTableView`), with a
    /// 10% allowance since the grid sizes columns itself. The estimate only
    /// has to be good enough to place the table on a page; an oversized table
    /// is scrolled inside its page.
    static func estimatedTableHeight(rows: [[String]], width: CGFloat, typography: TypographySettings) -> CGFloat {
        let columns = max(rows.map(\.count).max() ?? 1, 1)
        let columnWidth = max(width / CGFloat(columns) - 16, 30)
        let font = nsFont(size: typography.fontSize, design: typography.fontDesign, bold: false, italic: false)
        var total: CGFloat = 0
        for row in rows {
            var maxLines = 1
            var lineHeight = ceil(font.ascender - font.descender + font.leading)
            for cell in row where !cell.isEmpty {
                let measured = measureLines(NSAttributedString(string: cell, attributes: [.font: font]), width: columnWidth, lineSpacing: 0)
                maxLines = max(maxLines, measured.lineStarts.count)
                lineHeight = measured.lineHeight
            }
            total += CGFloat(maxLines) * lineHeight + 12
        }
        return ceil(total * 1.1)
    }

    static func nsFont(size: Double, design: ReadingFontDesign, bold: Bool, italic: Bool) -> NSFont {
        var font = NSFont.systemFont(ofSize: CGFloat(size), weight: bold ? .bold : .regular)
        let descriptorDesign: NSFontDescriptor.SystemDesign?
        switch design {
        case .system: descriptorDesign = nil
        case .serif: descriptorDesign = .serif
        case .rounded: descriptorDesign = .rounded
        }
        if let descriptorDesign, let descriptor = font.fontDescriptor.withDesign(descriptorDesign),
            let designed = NSFont(descriptor: descriptor, size: CGFloat(size))
        {
            font = designed
        }
        if italic {
            font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
        }
        return font
    }
}
