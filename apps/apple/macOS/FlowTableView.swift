import SwiftUI

// ── Pure layout/accessibility helpers (unit-tested, no SwiftUI state) ───────

/// Merged-cell geometry for a table (M7/R7, ADR-019 addendum 2).
///
/// `rows` is always the full grid (merged text in its top-left slot, `""` in
/// every covered slot); `spans` lists only merged cells. This type sanitises
/// the spans defensively (a hand-edited or corrupt blob must not crash or
/// mis-layout): out-of-grid origins are dropped, spans are clipped to the
/// grid, and a span overlapping an earlier one is dropped.
struct TableSpanMap {
    /// What a grid slot is for rendering and accessibility purposes.
    enum Slot: Equatable {
        /// An ordinary cell or the origin (top-left slot) of a merged cell.
        case cell
        /// Covered by a column span from a cell in the same row: not rendered.
        case coveredRight
        /// Below the origin row of a row span. A filler is drawn once, at
        /// `originCol`, `colspan` columns wide; the other covered columns of
        /// the row are skipped.
        case coveredBelow(originCol: Int, colspan: Int)
    }

    let rowCount: Int
    let columnCount: Int
    /// Sanitised spans keyed by their origin slot.
    private(set) var origins: [Int: TableSpanVM] = [:]
    private var owner: [Int?]

    init(rows: [[String]], spans: [TableSpanVM]) {
        rowCount = rows.count
        columnCount = rows.map(\.count).max() ?? 0
        owner = Array(repeating: nil, count: rowCount * columnCount)
        for span in spans {
            guard span.row >= 0, span.col >= 0, span.row < rowCount, span.col < columnCount,
                span.rowspan >= 1, span.colspan >= 1, span.rowspan > 1 || span.colspan > 1
            else { continue }
            let rowspan = min(span.rowspan, rowCount - span.row)
            let colspan = min(span.colspan, columnCount - span.col)
            let key = span.row * columnCount + span.col
            var free = true
            for r in span.row..<(span.row + rowspan) {
                for c in span.col..<(span.col + colspan) where owner[r * columnCount + c] != nil {
                    free = false
                }
            }
            guard free else { continue }
            for r in span.row..<(span.row + rowspan) {
                for c in span.col..<(span.col + colspan) { owner[r * columnCount + c] = key }
            }
            origins[key] = TableSpanVM(
                row: span.row, col: span.col, rowspan: rowspan, colspan: colspan)
        }
    }

    /// The (sanitised) merged cell whose origin is (`row`, `column`), if any.
    func span(row: Int, column: Int) -> TableSpanVM? {
        guard row >= 0, column >= 0, row < rowCount, column < columnCount else { return nil }
        return origins[row * columnCount + column]
    }

    func slot(row: Int, column: Int) -> Slot {
        guard row >= 0, column >= 0, row < rowCount, column < columnCount,
            let key = owner[row * columnCount + column], let origin = origins[key]
        else { return .cell }
        if origin.row == row && origin.col == column { return .cell }
        if origin.row == row { return .coveredRight }
        return .coveredBelow(originCol: origin.col, colspan: origin.colspan)
    }

    /// Whether VoiceOver should announce this slot as an element. Covered
    /// slots are part of a merged cell announced once at its origin, never as
    /// separate empty elements.
    func isAnnounced(row: Int, column: Int) -> Bool { slot(row: row, column: column) == .cell }

    /// Which horizontal borders to draw for a slot, so a row span reads as one
    /// tall cell: its origin has no bottom border, middle fillers have
    /// neither, the last filler has only a bottom border.
    func horizontalEdges(row: Int, column: Int) -> (top: Bool, bottom: Bool) {
        switch slot(row: row, column: column) {
        case .cell:
            if let s = span(row: row, column: column), s.rowspan > 1 { return (true, false) }
            return (true, true)
        case .coveredRight: return (false, false)
        case .coveredBelow(let originCol, _):
            guard let s = span(rowOwning: row, column: column), s.col == originCol else {
                return (false, true)
            }
            return (false, row == s.row + s.rowspan - 1)
        }
    }

    private func span(rowOwning row: Int, column: Int) -> TableSpanVM? {
        guard let key = owner[row * columnCount + column] else { return nil }
        return origins[key]
    }

    /// Text of the column header for `column` (row 0), falling back to the
    /// header cell that spans over it when the slot itself is covered/empty.
    func headerText(rows: [[String]], column: Int) -> String {
        guard rowCount > 0, column >= 0, column < columnCount else { return "" }
        let own = rows[0].indices.contains(column) ? rows[0][column] : ""
        if !own.isEmpty { return own }
        if let key = owner[column], let o = origins[key], rows[o.row].indices.contains(o.col) {
            return rows[o.row][o.col]
        }
        return ""
    }
}

/// Geometry and offset bookkeeping for rendering a `FlowBlockVM.table` as a
/// grid while search matches and annotation highlights -- which are computed
/// as *character offsets into the block's whole `plainText`* -- still land in
/// the right cell.
enum TableCellLayout {
    /// For each cell, its character-offset range within the table's
    /// `plainText` (cells joined by one separator character, rows joined by
    /// one separator character -- both single characters, see
    /// `FlowBlockVM.tableCellSeparator`/`tableRowSeparator`). Covered slots of
    /// a merged cell are empty strings and so get empty ranges.
    static func cellRanges(rows: [[String]]) -> [[Range<Int>]] {
        var running = 0
        return rows.map { row in
            row.map { cell in
                let range = running..<(running + cell.count)
                running += cell.count + 1  // the separator after this cell
                return range
            }
        }
    }

    /// `range` clipped to `cell` and shifted to be local to the cell's own
    /// text, or `nil` if they don't overlap.
    static func clip(_ range: Range<Int>, to cell: Range<Int>) -> Range<Int>? {
        let lower = max(range.lowerBound, cell.lowerBound)
        let upper = min(range.upperBound, cell.upperBound)
        guard lower < upper else { return nil }
        return (lower - cell.lowerBound)..<(upper - cell.lowerBound)
    }

    /// Number of columns = the widest row (rows may be ragged).
    static func columnCount(rows: [[String]]) -> Int {
        rows.map(\.count).max() ?? 0
    }

    /// Horizontal padding added around each cell's fixed-width text frame
    /// (8pt each side); a merged cell's inner width must add the padding of
    /// the columns it swallows so it ends flush with the last of them.
    static let cellHorizontalPadding: CGFloat = 16

    /// Approximate per-column pixel widths: proportional to the longest
    /// cell's character count, clamped so one huge cell cannot make a column
    /// unreadably wide (it wraps instead) and an empty column stays tappable.
    /// An estimate rather than a text measurement on purpose -- measuring
    /// every cell would defeat the lazy row layout. Column-merged cells are
    /// ignored for sizing (they wrap across the columns they span).
    static func columnWidths(
        rows: [[String]], fontSize: Double, spans: [TableSpanVM] = []
    ) -> [CGFloat] {
        let columns = columnCount(rows: rows)
        let map = TableSpanMap(rows: rows, spans: spans)
        return (0..<columns).map { column in
            let longest =
                rows.indices.map { r -> Int in
                    guard column < rows[r].count else { return 0 }
                    if let s = map.span(row: r, column: column), s.colspan > 1 { return 0 }
                    return rows[r][column].count
                }.max() ?? 0
            let raw = Double(longest) * fontSize * 0.55 + 24
            return CGFloat(min(max(raw, 72), 280))
        }
    }

    /// Inner (pre-padding) width of a cell spanning `colspan` columns from
    /// `column`: the columns' widths plus the padding of every swallowed
    /// column boundary.
    static func spannedWidth(widths: [CGFloat], column: Int, colspan: Int) -> CGFloat {
        guard column < widths.count else { return 0 }
        let end = min(column + max(colspan, 1), widths.count)
        let sum = widths[column..<end].reduce(0, +)
        return sum + cellHorizontalPadding * CGFloat(end - column - 1)
    }
}

/// VoiceOver text for table cells. SwiftUI exposes no native table
/// accessibility role for a custom grid, so each cell carries its own
/// header-qualified label and its position, rather than the table reading as
/// one flat blob.
enum TableAccessibility {
    static func tableLabel(rowCount: Int, columnCount: Int) -> String {
        String(localized: "Table, \(rowCount) rows, \(columnCount) columns")
    }

    /// Label for the cell at (`row`, `column`): the cell text, qualified with
    /// its column header for data rows when the table has a header row. A
    /// column covered by a spanning header is qualified by that header.
    static func cellLabel(
        rows: [[String]], headerRow: Bool, row: Int, column: Int, spans: [TableSpanVM] = []
    ) -> String {
        guard headerRow, row > 0 else {
            return cellLabel(rows: rows, headerRow: headerRow, row: row, column: column, map: nil)
        }
        return cellLabel(
            rows: rows, headerRow: headerRow, row: row, column: column,
            map: TableSpanMap(rows: rows, spans: spans))
    }

    /// Same label using an already-built span map, so a table view that
    /// renders many cells builds the map once instead of once per cell
    /// (review F73).
    static func cellLabel(
        rows: [[String]], headerRow: Bool, row: Int, column: Int, map: TableSpanMap?
    ) -> String {
        let text = cellText(rows: rows, row: row, column: column)
        let shown = text.isEmpty ? String(localized: "empty") : text
        guard headerRow, row > 0, let map else { return shown }
        let header = map.headerText(rows: rows, column: column)
        return header.isEmpty ? shown : "\(header): \(shown)"
    }

    /// Value read after the label: "row 2 of 4, column 1 of 3". A merged cell
    /// is announced once and states the whole region it covers: "rows 2 to 3
    /// of 4, column 3 of 3".
    static func cellPosition(
        rowCount: Int, columnCount: Int, row: Int, column: Int, rowspan: Int = 1, colspan: Int = 1
    ) -> String {
        let lastRow = row + max(rowspan, 1)
        let lastColumn = column + max(colspan, 1)
        switch (rowspan > 1, colspan > 1) {
        case (false, false):
            return String(
                localized: "Row \(row + 1) of \(rowCount), column \(column + 1) of \(columnCount)")
        case (false, true):
            return String(
                localized:
                    "Row \(row + 1) of \(rowCount), columns \(column + 1) to \(lastColumn) of \(columnCount)"
            )
        case (true, false):
            return String(
                localized:
                    "Rows \(row + 1) to \(lastRow) of \(rowCount), column \(column + 1) of \(columnCount)"
            )
        case (true, true):
            return String(
                localized:
                    "Rows \(row + 1) to \(lastRow) of \(rowCount), columns \(column + 1) to \(lastColumn) of \(columnCount)"
            )
        }
    }

    private static func cellText(rows: [[String]], row: Int, column: Int) -> String {
        guard rows.indices.contains(row), rows[row].indices.contains(column) else { return "" }
        return rows[row][column]
    }
}

// ── View ────────────────────────────────────────────────────────────────────

/// A `FlowBlockVM.table` rendered as a bordered grid inside a horizontal
/// scroll view (wide tables scroll sideways rather than squeezing). Rows are
/// laid out in a `LazyVStack` with estimated fixed column widths so big
/// tables stay lazy and columns line up; the whole table is still a single
/// block row in `FlowViewSwiftUINative`'s outer `LazyVStack`, so that
/// view's virtualisation, block indexing and annotation offsets are
/// untouched.
///
/// Merged cells (M7/R7): a column span draws one wide cell and skips the
/// covered slots; a row span draws the cell in its origin row (top-aligned)
/// and a blank, unannounced filler in each covered row with the inner
/// borders removed so the region reads as a single tall cell.
///
/// Presentational only: search/annotation highlighting is delegated to
/// `renderCell`, which `FlowViewSwiftUINative` supplies (so the highlight
/// colours and current-match logic stay in one place).
struct FlowTableView: View {
    let rows: [[String]]
    let headerRow: Bool
    var spans: [TableSpanVM] = []
    let fontSize: Double
    let fontDesign: Font.Design
    /// Builds a cell's `AttributedString` given its text and its
    /// character-offset range within the table's `plainText`.
    let renderCell: (_ text: String, _ cellRange: Range<Int>) -> AttributedString

    @EnvironmentObject private var themeManager: ThemeManager

    private var theme: Theme { themeManager.resolvedTheme }

    var body: some View {
        let ranges = TableCellLayout.cellRanges(rows: rows)
        let widths = TableCellLayout.columnWidths(rows: rows, fontSize: fontSize, spans: spans)
        let map = TableSpanMap(rows: rows, spans: spans)
        let columns = widths.count

        ScrollView(.horizontal, showsIndicators: true) {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(rows.indices, id: \.self) { r in
                    HStack(alignment: .top, spacing: 0) {
                        ForEach(0..<columns, id: \.self) { c in
                            slotView(r, c, ranges: ranges, widths: widths, map: map, columns: columns)
                        }
                    }
                    .fixedSize(horizontal: false, vertical: true)
                }
            }
            .overlay(Rectangle().stroke(borderColor, lineWidth: 1))
        }
        // The table as a whole: a container whose children are the cells.
        .accessibilityElement(children: .contain)
        .accessibilityLabel(TableAccessibility.tableLabel(rowCount: rows.count, columnCount: columns))
    }

    private var borderColor: Color { theme.foreground.opacity(0.28) }

    @ViewBuilder
    private func slotView(
        _ r: Int, _ c: Int, ranges: [[Range<Int>]], widths: [CGFloat], map: TableSpanMap, columns: Int
    ) -> some View {
        switch map.slot(row: r, column: c) {
        case .coveredRight:
            EmptyView()
        case .coveredBelow(let originCol, let colspan):
            if c == originCol {
                filler(r, c, width: TableCellLayout.spannedWidth(widths: widths, column: c, colspan: colspan), map: map)
            }
        case .cell:
            let span = map.span(row: r, column: c)
            cell(
                r, c, ranges: ranges, rowspan: span?.rowspan ?? 1, colspan: span?.colspan ?? 1,
                width: TableCellLayout.spannedWidth(widths: widths, column: c, colspan: span?.colspan ?? 1),
                rowCount: rows.count, columns: columns, map: map)
        }
    }

    /// Blank continuation of a row-spanned cell: no text, hidden from
    /// accessibility (the region is announced once at its origin).
    private func filler(_ r: Int, _ c: Int, width: CGFloat, map: TableSpanMap) -> some View {
        let edges = map.horizontalEdges(row: r, column: c)
        return Color.clear
            .frame(width: width)
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxHeight: .infinity)
            .overlay(edgeBorders(top: edges.top, bottom: edges.bottom))
            .accessibilityHidden(true)
    }

    @ViewBuilder
    private func cell(
        _ r: Int, _ c: Int, ranges: [[Range<Int>]], rowspan: Int, colspan: Int, width: CGFloat,
        rowCount: Int, columns: Int, map: TableSpanMap
    ) -> some View {
        let isHeader = headerRow && r == 0
        let text = c < rows[r].count ? rows[r][c] : ""
        let attributed: AttributedString =
            c < rows[r].count ? renderCell(text, ranges[r][c]) : AttributedString("")
        let edges = map.horizontalEdges(row: r, column: c)

        Text(attributed)
            .font(.system(size: fontSize - 1, weight: isHeader ? .semibold : .regular, design: fontDesign))
            .frame(width: width, alignment: .topLeading)
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxHeight: .infinity, alignment: .topLeading)
            .background(isHeader ? theme.accent.opacity(0.14) : Color.clear)
            .overlay(edgeBorders(top: edges.top, bottom: edges.bottom))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(
                TableAccessibility.cellLabel(
                    rows: rows, headerRow: headerRow, row: r, column: c, map: map)
            )
            .accessibilityValue(
                TableAccessibility.cellPosition(
                    rowCount: rowCount, columnCount: columns, row: r, column: c,
                    rowspan: rowspan, colspan: colspan)
            )
            .accessibilityAddTraits(isHeader ? .isHeader : [])
    }

    /// Cell borders with optional top/bottom edges (left/right always drawn),
    /// so a row-spanned region has no lines across its middle.
    private func edgeBorders(top: Bool, bottom: Bool) -> some View {
        ZStack {
            HStack {
                Rectangle().fill(borderColor).frame(width: 0.5)
                Spacer(minLength: 0)
                Rectangle().fill(borderColor).frame(width: 0.5)
            }
            VStack {
                if top { Rectangle().fill(borderColor).frame(height: 0.5) } else { Color.clear.frame(height: 0.5) }
                Spacer(minLength: 0)
                if bottom { Rectangle().fill(borderColor).frame(height: 0.5) } else { Color.clear.frame(height: 0.5) }
            }
        }
        .allowsHitTesting(false)
    }
}
