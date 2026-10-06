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
                if LibraryRowReadingState.hasReadingState(item) {
                    readingStateLine
                }
            }
            if item.contentEncrypted {
                Spacer()
                Image(systemName: "lock.fill")
                    .foregroundStyle(.secondary)
                    .help("Encrypted at rest (ADR-011/014)")
                    // Without this, VoiceOver falls back to an inferred
                    // label straight off the SF Symbol name ("lock fill"),
                    // which doesn't read as a sentence -- explicit label
                    // matching the `.help()` tooltip's meaning instead.
                    .accessibilityLabel("Encrypted")
            }
        }
    }

    /// Small progress bar + "42% read · Last read 3 days ago" caption
    /// (ADR-021). Progress is the *RSVP* position only, and the tooltip says
    /// so: an item read only in the flow view shows a last-read date but no
    /// percentage. The row's children are combined into one VoiceOver
    /// element carrying the full sentence.
    private var readingStateLine: some View {
        HStack(spacing: 6) {
            if item.progressFraction > 0 {
                ProgressView(value: LibraryRowReadingState.clampedFraction(item.progressFraction))
                    .progressViewStyle(.linear)
                    .frame(width: 56)
            }
            Text(LibraryRowReadingState.caption(for: item, now: Date()))
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .help(LibraryRowReadingState.limitationHelp)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(LibraryRowReadingState.accessibilityLabel(for: item, now: Date()))
    }
}

/// Pure presentation logic for the Library row's reading-state line
/// (ADR-021), factored out so it is unit-testable without SwiftUI state.
enum LibraryRowReadingState {
    /// Shown as the row's tooltip: the accepted D2 limitation, in the UI.
    static var limitationHelp: String {
        String(localized: "Progress counts RSVP reading only. Reading in the Flow view updates the last-read date but not the percentage.")
    }

    static func clampedFraction(_ f: Double) -> Double {
        guard f.isFinite else { return 0 }
        return min(max(f, 0), 1)
    }

    /// Whole-number percentage, 0...100.
    static func percent(_ item: LibraryItemVM) -> Int {
        Int((clampedFraction(item.progressFraction) * 100).rounded())
    }

    static func hasReadingState(_ item: LibraryItemVM) -> Bool {
        item.progressFraction > 0 || item.lastOpenedAt != nil
    }

    /// "3 days ago" style text, or nil when never opened.
    static func relativeLastRead(_ item: LibraryItemVM, now: Date) -> String? {
        guard let last = item.lastOpenedAt else { return nil }
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        return formatter.localizedString(for: last, relativeTo: now)
    }

    static func caption(for item: LibraryItemVM, now: Date) -> String {
        var parts: [String] = []
        if item.progressFraction > 0 {
            parts.append(String(localized: "\(percent(item))% read"))
        }
        if let rel = relativeLastRead(item, now: now) {
            parts.append(String(localized: "Last read \(rel)"))
        }
        return parts.joined(separator: " · ")
    }

    static func accessibilityLabel(for item: LibraryItemVM, now: Date) -> String {
        var parts: [String] = []
        if item.progressFraction > 0 {
            parts.append(String(localized: "\(percent(item)) percent read"))
        }
        if let rel = relativeLastRead(item, now: now) {
            parts.append(String(localized: "Last read \(rel)"))
        }
        return parts.joined(separator: ", ")
    }
}
