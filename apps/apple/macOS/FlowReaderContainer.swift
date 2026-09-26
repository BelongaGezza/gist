import SwiftUI

/// Hosts a `ReadingLayout` implementation plus its shared toolbar chrome
/// (typography menu, search field with find-next/previous, and a nested TOC
/// popover indented by heading level — see `TocListView` below) — kept
/// generic over `Layout` even though `FlowViewSwiftUINative` is
/// currently the only conformer (CLAUDE.md's Q8 decided 2026-09-12; the
/// TextKit 2 alternative this was prototyped against was removed), since Q3
/// (a paginated view, still open for v1.1) is a plausible second
/// `ReadingLayout` this container would host unchanged. Fetches the document
/// once via `CoreClient.loadDocument`, the same one-shot-fetch-then-decode
/// shape `RsvpPlayer.load` uses for RSVP sessions (see RsvpView.swift).
struct FlowReaderContainer<Layout: ReadingLayout>: View {
    let itemId: String
    @EnvironmentObject var core: CoreClient
    @EnvironmentObject var themeManager: ThemeManager
    @State private var document: FlowDocumentVM?
    @State private var typography: TypographySettings
    @State private var showingToc = false
    @StateObject private var search = SearchState()
    @StateObject private var navigation = SectionNavigator()
    @StateObject private var progress: ReadingProgress

    init(itemId: String) {
        self.itemId = itemId
        // Seeded synchronously from the Settings scene's Typography-tab
        // defaults (`TypographyDefaults`, see AppSettings.swift) at init
        // time, same as `progress` below -- a fresh install still gets
        // `TypographySettings()`'s own defaults, since `TypographyDefaults`
        // falls back to those exact values when nothing is persisted yet.
        _typography = State(wrappedValue: TypographyDefaults.shared.current)
        // Seeded synchronously from UserDefaults at init time (not `.task`)
        // so the very first `Layout` instance already has the real
        // `initialFraction` to restore to, rather than a placeholder 0 that
        // would need a second, jarring scroll once the real value loads.
        _progress = StateObject(wrappedValue: ReadingProgress(initialFraction: FlowScrollPositionStore.load(itemId: itemId)))
    }

    var body: some View {
        Group {
            if let document {
                Layout(document: document, typography: $typography, search: search, navigation: navigation, progress: progress)
                    .navigationTitle(document.metadata.title)
                    .toolbar { toolbarContent(document: document) }
                    .safeAreaInset(edge: .bottom) { progressBar }
            } else {
                ProgressView("Loading…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(themeManager.resolvedTheme.background)
        .foregroundStyle(themeManager.resolvedTheme.foreground)
        .task {
            document = await core.loadDocument(itemId: itemId)
        }
        .onChange(of: progress.fraction) { _, newValue in
            FlowScrollPositionStore.save(itemId: itemId, fraction: newValue)
        }
    }

    private var progressBar: some View {
        HStack(spacing: 8) {
            ProgressView(value: progress.fraction)
                .tint(themeManager.resolvedTheme.accent)
            Text("\(Int((progress.fraction * 100).rounded()))%")
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
                .frame(minWidth: 32, alignment: .trailing)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
        .background(.thinMaterial)
    }

    @ToolbarContentBuilder
    private func toolbarContent(document: FlowDocumentVM) -> some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            if !document.tableOfContents.isEmpty {
                Button {
                    showingToc = true
                } label: {
                    Label("Contents", systemImage: "list.bullet")
                }
                .popover(isPresented: $showingToc) {
                    TocListView(entries: document.tableOfContents) { sectionId in
                        navigation.pendingSectionId = sectionId
                        showingToc = false
                    }
                }
            }

            typographyMenu

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

    /// The "Aa" typography panel: size, font design, and line spacing, the
    /// three controls the flow view's spec calls "full typography controls"
    /// (see CLAUDE.md's M2 item 5) -- one menu rather than three separate
    /// toolbar items, since none of these are reached often enough to
    /// justify permanent toolbar real estate the way search is.
    private var typographyMenu: some View {
        Menu {
            Section("Size") {
                Button {
                    typography.fontSize = max(TypographySettings.range.lowerBound, typography.fontSize - 1)
                } label: {
                    Label("Smaller", systemImage: "textformat.size.smaller")
                }
                Button {
                    typography.fontSize = min(TypographySettings.range.upperBound, typography.fontSize + 1)
                } label: {
                    Label("Larger", systemImage: "textformat.size.larger")
                }
            }
            Section("Font") {
                Picker("Font", selection: $typography.fontDesign) {
                    ForEach(ReadingFontDesign.allCases) { design in
                        Text(design.label).tag(design)
                    }
                }
            }
            Section("Line Spacing") {
                Picker("Line Spacing", selection: $typography.lineSpacing) {
                    ForEach(LineSpacingOption.allCases) { option in
                        Text(option.label).tag(option)
                    }
                }
            }
        } label: {
            Label("Typography", systemImage: "textformat.size")
        }
    }

    private var matchCountLabel: String {
        guard !search.query.isEmpty else { return "" }
        return search.matchCount > 0 ? "\(search.currentMatchIndex + 1)/\(search.matchCount)" : "0/0"
    }
}

/// Nested table-of-contents list, indented by heading level (`h1` flush,
/// `h2` indented once, `h3` indented twice, ...) via `TocEntry.indentLevel`.
/// Presented in a `.popover` off the same "Contents" toolbar button the
/// previous flat implementation used -- the entry point is unchanged, only
/// what appears once it's open. A `List` rather than a `Menu`: macOS menu
/// items don't reliably support per-row indentation (the prior
/// implementation faked it with literal leading spaces in the button title),
/// whereas ordinary `List` rows can just carry leading padding.
private struct TocListView: View {
    let entries: [TocEntry]
    let onSelect: (String) -> Void

    /// Points of leading padding per nesting level.
    private static let indentUnit: CGFloat = 16

    var body: some View {
        List(entries) { entry in
            Button {
                onSelect(entry.sectionId)
            } label: {
                Text(entry.title)
                    .padding(.leading, CGFloat(entry.indentLevel) * Self.indentUnit)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .buttonStyle(.plain)
        }
        .frame(minWidth: 260, idealHeight: 320, maxHeight: 420)
    }
}
