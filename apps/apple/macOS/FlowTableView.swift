import SwiftUI

// ── Pure layout/accessibility helpers (unit-tested, no SwiftUI state) ───────

/// Geometry and offset bookkeeping for rendering a `FlowBlockVM.table` as a
/// grid while search matches and annotation highlights -- which are computed
/// as *character offsets into the block's whole `plainText`* -- still land in
/// the right cell.
enum TableCellLayout {
    /// For each cell, its character-offset range within the table's
    /// `plainText` (cells joined by one separator character, rows joined by
    /// one separator character -- both single characters, see
    /// `FlowBlockVM.tableCellSeparator`/`tableRowSeparator`).
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

    /// Approximate per-column pixel widths: proportional to the longest
    /// cell's character count, clamped so one huge cell cannot make a column
    /// unreadably wide (it wraps instead) and an empty column stays tappable.
    /// An estimate rather than a text measurement on purpose -- measuring
    /// every cell would defeat the lazy row layout.
    static func columnWidths(rows: [[String]], fontSize: Double) -> [CGFloat] {
        let columns = columnCount(rows: rows)
        return (0..<columns).map { column in
            let longest = rows.map { column < $0.count ? $0[column].count : 0 }.max() ?? 0
            let raw = Double(longest) * fontSize * 0.55 + 24
            return CGFloat(min(max(raw, 72), 280))
        }
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
    /// its column header for data rows when the table has a header row.
    static func cellLabel(rows: [[String]], headerRow: Bool, row: Int, column: Int) -> String {
        let text = cellText(rows: rows, row: row, column: column)
        let shown = text.isEmpty ? String(localized: "empty") : text
        guard headerRow, row > 0 else { return shown }
        let header = cellText(rows: rows, row: 0, column: column)
        return header.isEmpty ? shown : "\(header): \(shown)"
    }

    /// Value read after the label: "row 2 of 4, column 1 of 3".
    static func cellPosition(rowCount: Int, columnCount: Int, row: Int, column: Int) -> String {
        String(
            localized: "Row \(row + 1) of \(rowCount), column \(column + 1) of \(columnCount)")
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
/// Presentational only: search/annotation highlighting is delegated to
/// `renderCell`, which `FlowViewSwiftUINative` supplies (so the highlight
/// colours and current-match logic stay in one place).
struct FlowTableView: View {
    let rows: [[String]]
    let headerRow: Bool
    let fontSize: Double
    let fontDesign: Font.Design
    /// Builds a cell's `AttributedString` given its text and its
    /// character-offset range within the table's `plainText`.
    let renderCell: (_ text: String, _ cellRange: Range<Int>) -> AttributedString

    @EnvironmentObject private var themeManager: ThemeManager

    private var theme: Theme { themeManager.resolvedTheme }

    var body: some View {
        let ranges = TableCellLayout.cellRanges(rows: rows)
        let widths = TableCellLayout.columnWidths(rows: rows, fontSize: fontSize)
        let columns = widths.count

        ScrollView(.horizontal, showsIndicators: true) {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(rows.indices, id: \.self) { r in
                    HStack(alignment: .top, spacing: 0) {
                        ForEach(0..<columns, id: \.self) { c in
                            cell(r, c, ranges: ranges, width: widths[c], rowCount: rows.count, columns: columns)
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
    private func cell(
        _ r: Int, _ c: Int, ranges: [[Range<Int>]], width: CGFloat, rowCount: Int, columns: Int
    ) -> some View {
        let isHeader = headerRow && r == 0
        let text = c < rows[r].count ? rows[r][c] : ""
        let attributed: AttributedString =
            c < rows[r].count ? renderCell(text, ranges[r][c]) : AttributedString("")

        Text(attributed)
            .font(.system(size: fontSize - 1, weight: isHeader ? .semibold : .regular, design: fontDesign))
            .frame(width: width, alignment: .topLeading)
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxHeight: .infinity, alignment: .topLeading)
            .background(isHeader ? theme.accent.opacity(0.14) : Color.clear)
            .overlay(Rectangle().stroke(borderColor, lineWidth: 0.5))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(
                TableAccessibility.cellLabel(rows: rows, headerRow: headerRow, row: r, column: c)
            )
            .accessibilityValue(
                TableAccessibility.cellPosition(rowCount: rowCount, columnCount: columns, row: r, column: c)
            )
            .accessibilityAddTraits(isHeader ? .isHeader : [])
    }
}
