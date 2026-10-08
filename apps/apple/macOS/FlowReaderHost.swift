import SwiftUI

/// Chooses between the scrolling flow layout and the paged layout (ADR-023)
/// and rebuilds the container when the reader switches. Both containers open
/// the document through `CoreClient.openFlowDocument`, so `markItemOpened`
/// (ADR-021) is stamped the same way in either mode. Neither container writes
/// `reading_progress`.
///
/// Switching re-creates the container (so the new layout starts clean) with a
/// `carryFraction` equal to the outgoing layout's current block, which lands
/// the reader at the same place. A switch re-stamps "last opened" -- harmless,
/// the reader is still in the same book.
struct FlowReaderHost: View {
    let itemId: String
    @ObservedObject private var layoutSettings = ReadingLayoutSettings.shared
    @State private var mode: ReadingLayoutMode
    @State private var carryFraction: Double?
    @State private var generation = 0

    init(itemId: String) {
        self.itemId = itemId
        _mode = State(initialValue: ReadingLayoutSettings.shared.mode)
    }

    var body: some View {
        Group {
            switch mode {
            case .scroll:
                FlowReaderContainer<FlowViewSwiftUINative>(
                    itemId: itemId,
                    carryFraction: carryFraction,
                    layoutMode: .scroll,
                    onSwitchLayout: switchLayout
                )
            case .pages:
                FlowReaderContainer<PagedReadingLayout>(
                    itemId: itemId,
                    carryFraction: carryFraction,
                    layoutMode: .pages,
                    onSwitchLayout: switchLayout
                )
            }
        }
        .id("\(mode.rawValue)-\(generation)")
    }

    private func switchLayout(_ newMode: ReadingLayoutMode, _ carry: Double) {
        carryFraction = carry
        mode = newMode
        generation += 1
        layoutSettings.setMode(newMode)
    }
}
