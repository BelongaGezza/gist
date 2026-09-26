import SwiftUI
import UniformTypeIdentifiers

/// Image types selectable for a "Scan/Import Images…" OCR import (role R8).
/// Falls back gracefully, like `importableContentTypes` in LibraryView.swift,
/// if a UTType isn't registered on the running system rather than crashing
/// the file picker.
private let ocrImportContentTypes: [UTType] = [
    .jpeg,
    .png,
    .heic,
    .tiff,
    .bmp,
    .image,
].compactMap { $0 }

/// Multi-page OCR import + review screen (role R8, `development-plan-v2.md`
/// §3.3). Presented as a sheet from `LibraryView`'s "Scan/Import Images…"
/// toolbar button.
///
/// Flow: pick page images -> Vision scans each page client-side
/// (cancellable, with progress) -> a review screen shows every page's
/// recognized text, editable, with low-confidence pages flagged -> "Import"
/// commits the whole document in one `GistCore.importImageWithOcr` call.
/// See `OcrImportModel.swift`'s and `VisionOcrEngine.swift`'s top notes for
/// why editing has to happen entirely client-side, before that one commit
/// call, rather than as a step of its own in the Rust pipeline.
struct OcrImportSheet: View {
    @EnvironmentObject var core: CoreClient
    @Environment(\.dismiss) private var dismiss
    @StateObject private var state = OcrImportState()
    @State private var showFileImporter = false

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Scan / Import Images")
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Close") { dismiss() }
                    }
                }
        }
        .frame(minWidth: 640, minHeight: 440)
        .fileImporter(
            isPresented: $showFileImporter,
            allowedContentTypes: ocrImportContentTypes,
            allowsMultipleSelection: true
        ) { result in
            if case .success(let urls) = result, !urls.isEmpty {
                state.beginScan(urls: urls)
            }
        }
        .onDisappear { state.cancelScan() }
    }

    @ViewBuilder
    private var content: some View {
        switch state.phase {
        case .idle:
            idleState
        case .scanning(let completed, let total):
            scanningState(completed: completed, total: total)
        case .reviewing:
            OcrReviewScreen(state: state) {
                Task { await state.commit(core: core) }
            }
        case .importing:
            importingState
        case .done(_, let pageConfidences):
            doneState(pageConfidences: pageConfidences)
        case .cancelled:
            terminalState(
                title: "Scan Cancelled",
                // (R5b localisation) `message`'s parameter type is plain
                // `String` (it must also accept the dynamic `.failed`
                // message below), so this literal needs an explicit wrap --
                // see `terminalState`'s doc comment.
                message: String(localized: "No pages were imported."),
                systemImage: "xmark.circle"
            )
        case .failed(let message):
            terminalState(
                title: "OCR Import Failed",
                message: message,
                systemImage: "exclamationmark.triangle"
            )
        }
    }

    private var idleState: some View {
        VStack(spacing: 16) {
            Image(systemName: "doc.text.viewfinder")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("Scan or Import Page Images")
                .font(.headline)
            Text(
                "Choose one or more page photos or scans (JPEG, PNG, HEIC, TIFF). "
                    + "GIST runs on-device text recognition on each page, then lets you "
                    + "review and correct the results before adding it to your library."
            )
            .font(.callout)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: 420)
            Button("Choose Page Images…") { showFileImporter = true }
                .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    private func scanningState(completed: Int, total: Int) -> some View {
        VStack(spacing: 16) {
            ProgressView(value: Double(completed), total: Double(max(total, 1)))
                .frame(maxWidth: 320)
            Text("Recognizing text — page \(min(completed + 1, total)) of \(total)")
                .foregroundStyle(.secondary)
            Button("Cancel") { state.cancelScan() }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    private var importingState: some View {
        VStack(spacing: 16) {
            ProgressView()
            Text("Importing…")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    private func doneState(pageConfidences: [Float]) -> some View {
        VStack(spacing: 16) {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 48))
                .foregroundStyle(.green)
            Text("Added to Your Library")
                .font(.headline)
            Text("\(pageConfidences.count) page\(pageConfidences.count == 1 ? "" : "s") imported.")
                .foregroundStyle(.secondary)
            Button("Done") { dismiss() }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    // (R5b localisation) `title` is `LocalizedStringKey` -- every call site
    // passes a fixed literal, so `Text(title)` below auto-localizes.
    // `message` stays plain `String` because the `.failed(let message)`
    // call site passes genuinely dynamic content (an underlying error's
    // description); the one call site that passes a fixed literal message
    // ("No pages were imported.") wraps it explicitly with
    // `String(localized:)` instead, above.
    private func terminalState(title: LocalizedStringKey, message: String, systemImage: String) -> some View {
        VStack(spacing: 16) {
            Image(systemName: systemImage)
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text(title)
                .font(.headline)
            Text(message)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            HStack {
                Button("Close") { dismiss() }
                Button("Try Again") { state.reset() }
                    .buttonStyle(.borderedProminent)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }
}

/// The review pane: a page list (flagging low-confidence pages, per
/// `OcrConfidence`) plus an editable text view for whichever page is
/// selected.
private struct OcrReviewScreen: View {
    @ObservedObject var state: OcrImportState
    let onImport: () -> Void

    @State private var selectedPageIndex: Int?

    private var lowConfidenceCount: Int {
        state.pages.filter(\.isLowConfidence).count
    }

    var body: some View {
        HStack(spacing: 0) {
            List(state.pages, selection: $selectedPageIndex) { page in
                pageRow(page)
            }
            .frame(minWidth: 200, maxWidth: 240)

            Divider()

            if let index = selectedPageIndex ?? state.pages.first?.pageIndex {
                editor(forPageIndex: index)
            } else {
                Text("Select a page to review its recognized text.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .safeAreaInset(edge: .bottom) {
            HStack {
                if lowConfidenceCount > 0 {
                    Label(
                        "\(lowConfidenceCount) page\(lowConfidenceCount == 1 ? "" : "s") may need correction",
                        systemImage: "exclamationmark.triangle.fill"
                    )
                    .foregroundStyle(.orange)
                    .font(.callout)
                }
                Spacer()
                Button("Import \(state.pages.count) Page\(state.pages.count == 1 ? "" : "s")", action: onImport)
                    .buttonStyle(.borderedProminent)
                    .disabled(state.pages.isEmpty)
            }
            .padding()
            .background(.bar)
        }
        .onAppear {
            if selectedPageIndex == nil {
                selectedPageIndex = state.pages.first?.pageIndex
            }
        }
    }

    private func pageRow(_ page: OcrReviewPage) -> some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Page \(page.pageIndex + 1)")
                Text("\(Int(page.confidence * 100))% confidence")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if page.isLowConfidence {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    // The row's orange tint (`.listRowBackground` below) and
                    // this icon are both purely visual signals of the same
                    // fact the confidence percentage above doesn't spell
                    // out in words -- give VoiceOver an explicit label
                    // rather than the SF-Symbol-name fallback.
                    .accessibilityLabel("Needs review")
            }
        }
        .padding(.vertical, 2)
        .listRowBackground(page.isLowConfidence ? Color.orange.opacity(0.15) : Color.clear)
    }

    private func editor(forPageIndex pageIndex: Int) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Page \(pageIndex + 1)")
                    .font(.headline)
                if let page = state.pages.first(where: { $0.pageIndex == pageIndex }), page.isLowConfidence {
                    Label("Low confidence — please check this page", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
                Spacer()
            }
            TextEditor(text: textBinding(forPageIndex: pageIndex))
                .font(.body.monospaced())
                .border(Color.secondary.opacity(0.3))
        }
        .padding()
    }

    private func textBinding(forPageIndex pageIndex: Int) -> Binding<String> {
        Binding(
            get: { state.pages.first(where: { $0.pageIndex == pageIndex })?.text ?? "" },
            set: { state.updateText(forPage: pageIndex, text: $0) }
        )
    }
}
