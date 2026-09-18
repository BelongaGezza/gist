import SwiftUI

/// Row visual content shared by `LibraryView` and `CollectionDetailView`'s
/// item lists. Deliberately just the row's *content* -- no gesture
/// recognizers, no `NavigationLink` wrapper. Both of those are the macOS
/// `List(selection:)` trap documented in detail on the doc comment above
/// `LibraryView.itemList`: either one silently breaks native click-to-select.
/// Callers attach `.contextMenu` themselves since the menu items differ
/// between the two call sites (library removal vs. collection removal).
struct LibraryRowContent: View {
    let item: LibraryItemVM

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading) {
                Text(item.title)
                    .font(.headline)
                if !item.authors.isEmpty {
                    Text(item.authors.joined(separator: ", "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if item.contentEncrypted {
                Spacer()
                Image(systemName: "lock.fill")
                    .foregroundStyle(.secondary)
                    .help("Encrypted at rest (ADR-011/014)")
            }
        }
    }
}
