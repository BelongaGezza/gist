import SwiftUI

/// SC-01: shown when a plain-text file's detected encoding is uncertain or
/// would decode lossily. Cancelling imports nothing and loses nothing: the
/// user's file is untouched and can simply be picked again.
struct EncodingPromptSheet: View {
    let pending: PendingEncodingImport
    let onImport: (String) -> Void
    let onCancel: () -> Void
    /// Reports whether decoding with a label would be lossy (nil = unknown).
    let isLossy: (String) -> Bool?

    @State private var selection: String

    init(
        pending: PendingEncodingImport,
        isLossy: @escaping (String) -> Bool?,
        onImport: @escaping (String) -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.pending = pending
        self.isLossy = isLossy
        self.onImport = onImport
        self.onCancel = onCancel
        _selection = State(initialValue: TextEncodingChoices.selection(for: pending.detectedName))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Choose Text Encoding")
                .font(.headline)
            Text(
                "GIST guessed \(pending.detectedName) for \(pending.url.lastPathComponent). Choose the encoding if the text looks wrong."
            )
            .fixedSize(horizontal: false, vertical: true)
            Picker("Encoding", selection: $selection) {
                ForEach(TextEncodingChoices.options(including: pending.detectedName)) { option in
                    Text(option.name).tag(option.label)
                }
            }
            if isLossy(selection) == true {
                Label(
                    "Some characters can't be read with this encoding and will appear as \u{FFFD}.",
                    systemImage: "exclamationmark.triangle"
                )
                .font(.callout)
                .foregroundStyle(.orange)
                .accessibilityLabel(
                    "Warning: some characters can't be read with this encoding and will appear as replacement characters."
                )
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Import") { onImport(selection) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 380)
    }
}

/// SC-03 / SC-06: a read-only info sheet for one library item. A canonical
/// URL is shown as text; it becomes a link only when it is http(s), and is
/// never opened except by the user clicking it.
struct ItemInfoView: View {
    let itemTitle: String
    let info: ItemInfoVM?
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(itemTitle).font(.headline)
            if let info {
                Form { rows(info) }
                    .formStyle(.grouped)
            } else {
                Text("No details are available for this item.")
                    .foregroundStyle(.secondary)
            }
            HStack {
                Spacer()
                Button("Done", action: onClose).keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 420, minHeight: 240)
    }

    @ViewBuilder
    private func rows(_ info: ItemInfoVM) -> some View {
        if let author = info.author, !author.isEmpty {
            LabeledContent("Byline", value: author)
        }
        if !info.sourceType.isEmpty {
            LabeledContent("Source type", value: info.sourceType)
        }
        if let d = ItemInfoPresentation.displayDate(info.publishedDate) {
            LabeledContent("Published", value: d)
        }
        if let d = ItemInfoPresentation.displayDate(info.retrievedDate) {
            LabeledContent("Retrieved", value: d)
        }
        if let d = ItemInfoPresentation.displayDate(info.importDate) {
            LabeledContent("Imported", value: d)
        }
        if let lang = info.language, !lang.isEmpty {
            LabeledContent("Language", value: lang)
        }
        if let url = info.canonicalUrl, !url.isEmpty {
            LabeledContent("Canonical URL") {
                if let link = ItemInfoPresentation.safeLink(url) {
                    Link(url, destination: link)
                        .accessibilityHint("Opens in your browser")
                } else {
                    Text(url).textSelection(.enabled)
                }
            }
        }
        if info.trackedChanges {
            Label(
                "This document had unresolved tracked changes when it was imported.",
                systemImage: "pencil"
            )
        }
    }
}

/// Bundles the encoding-prompt sheet, item-info sheet and import notice so
/// `LibraryView.body` / `CollectionDetailView.body` stay small for the
/// Swift type checker (see N6).
struct ImportMetadataPresentations: ViewModifier {
    @ObservedObject var core: CoreClient
    @Binding var infoTarget: TagEditorTarget?

    func body(content: Content) -> some View {
        content
            .safeAreaInset(edge: .top, spacing: 0) {
                if let notice = core.importNotice {
                    ImportNoticeBanner(message: notice) { core.importNotice = nil }
                }
            }
            .sheet(item: $core.pendingEncodingImport) { pending in
                EncodingPromptSheet(
                    pending: pending,
                    isLossy: { core.detectEncoding(url: pending.url, encoding: $0)?.lossy },
                    onImport: { label in
                        Task { await core.importTextFile(url: pending.url, encoding: label) }
                    },
                    onCancel: { core.pendingEncodingImport = nil }
                )
            }
            .sheet(item: $infoTarget) { target in
                ItemInfoView(
                    itemTitle: target.title,
                    info: core.itemInfo(itemId: target.id),
                    onClose: { infoTarget = nil }
                )
            }
    }
}

/// Dismissible, non-blocking banner for `CoreClient.importNotice` (SC-03).
struct ImportNoticeBanner: View {
    let message: String
    let onDismiss: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "info.circle")
                .accessibilityHidden(true)
            Text(message)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            Button(action: onDismiss) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("Dismiss notice")
        }
        .font(.callout)
        .padding(10)
        .background(.regularMaterial)
        .accessibilityElement(children: .contain)
    }
}
