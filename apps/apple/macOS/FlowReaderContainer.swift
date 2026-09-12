import SwiftUI

/// Hosts one `ReadingLayout` implementation plus the toolbar chrome both
/// prototypes share (font-size stepper, search field with find-next/
/// previous, and a TOC menu) — generic over `Layout` so the same container
/// serves both `FlowViewSwiftUINative` and `FlowViewTextKit2` from
/// `ContentView`'s navigation destinations. Fetches the document once via
/// `CoreClient.loadDocument`, the same one-shot-fetch-then-decode shape
/// `RsvpPlayer.load` uses for RSVP sessions (see RsvpView.swift).
struct FlowReaderContainer<Layout: ReadingLayout>: View {
    let itemId: String
    @EnvironmentObject var core: CoreClient
    @State private var document: FlowDocumentVM?
    @State private var typography = TypographySettings()
    @StateObject private var search = SearchState()
    @StateObject private var navigation = SectionNavigator()

    var body: some View {
        Group {
            if let document {
                Layout(document: document, typography: $typography, search: search, navigation: navigation)
                    .navigationTitle(document.metadata.title)
                    .toolbar { toolbarContent(document: document) }
            } else {
                ProgressView("Loading…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            document = await core.loadDocument(itemId: itemId)
        }
    }

    @ToolbarContentBuilder
    private func toolbarContent(document: FlowDocumentVM) -> some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            if !document.tableOfContents.isEmpty {
                Menu {
                    ForEach(document.tableOfContents) { entry in
                        Button(String(repeating: "    ", count: max(entry.level - 1, 0)) + entry.title) {
                            navigation.pendingSectionId = entry.sectionId
                        }
                    }
                } label: {
                    Label("Contents", systemImage: "list.bullet")
                }
            }

            HStack(spacing: 2) {
                Button {
                    typography.fontSize = max(TypographySettings.range.lowerBound, typography.fontSize - 1)
                } label: {
                    Image(systemName: "textformat.size.smaller")
                }
                Button {
                    typography.fontSize = min(TypographySettings.range.upperBound, typography.fontSize + 1)
                } label: {
                    Image(systemName: "textformat.size.larger")
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Text size")

            HStack(spacing: 4) {
                TextField("Find in document", text: $search.query)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 160)
                    .onSubmit { search.findNext() }
                Text(matchCountLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .frame(minWidth: 36)
                Button { search.findPrevious() } label: {
                    Image(systemName: "chevron.up")
                }
                .disabled(search.matchCount == 0)
                Button { search.findNext() } label: {
                    Image(systemName: "chevron.down")
                }
                .disabled(search.matchCount == 0)
                .keyboardShortcut("g", modifiers: .command)
            }
        }
    }

    private var matchCountLabel: String {
        guard !search.query.isEmpty else { return "" }
        return search.matchCount > 0 ? "\(search.currentMatchIndex + 1)/\(search.matchCount)" : "0/0"
    }
}
