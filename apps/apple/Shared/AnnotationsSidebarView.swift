import SwiftUI

/// Browse-all-annotations view for the current document, presented as a
/// sheet from `FlowReaderContainer`'s "Annotations" toolbar button --
/// structurally similar to `TagEditorView` (a focused, single-purpose sheet
/// over one item) but larger, since a document can accumulate many
/// annotations across a long read.
///
/// Grouped into three sections (Highlights, Notes, Bookmarks) rather than
/// one flat list sorted only by date, since browsing "all my highlights" or
/// "all my notes" separately is the more common use case for a reader
/// revisiting a book, and a `.note` "attached to" a highlight is rendered
/// nested under that highlight (matching `AnnotationMarkdownExporter`'s
/// export grouping) rather than duplicated into the Notes section.
struct AnnotationsSidebarView: View {
    @EnvironmentObject var core: CoreClient
    @ObservedObject var annotations: AnnotationState
    @Environment(\.dismiss) private var dismiss

    let itemId: String
    let document: FlowDocumentVM

    @State private var editingNote: AnnotationVM?
    @State private var editedNoteText = ""
    @State private var showExporter = false
    @State private var exportDocument = AnnotationsMarkdownDocument(text: "")

    private var highlights: [AnnotationVM] {
        annotations.items.filter { $0.kind == .highlight }
    }

    private var notes: [AnnotationVM] {
        annotations.items.filter { $0.kind == .note }
    }

    private var standaloneNotes: [AnnotationVM] {
        notes.filter { note in !highlights.contains { $0.anchor == note.anchor } }
    }

    private var bookmarks: [AnnotationVM] {
        annotations.items.filter { $0.kind == .bookmark }
    }

    var body: some View {
        NavigationStack {
            listContent
                .navigationTitle("Annotations")
                .toolbar { toolbarContent }
                .sheet(item: $editingNote) { annotation in
                    editNoteSheet(for: annotation)
                }
                .fileExporter(
                    isPresented: $showExporter,
                    document: exportDocument,
                    contentType: .gistAnnotationsMarkdown,
                    defaultFilename: "\(document.metadata.title) — Annotations"
                ) { _ in }
        }
        .frame(minWidth: 380, minHeight: 460)
    }

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button("Done") { dismiss() }
        }
        ToolbarItem(placement: .primaryAction) {
            Button {
                exportDocument = AnnotationsMarkdownDocument(
                    text: AnnotationMarkdownExporter.markdown(for: annotations.items, in: document)
                )
                showExporter = true
            } label: {
                Label("Export", systemImage: "square.and.arrow.up")
            }
            .disabled(annotations.items.isEmpty)
        }
    }

    @ViewBuilder
    private var listContent: some View {
        if annotations.items.isEmpty {
            emptyState
        } else {
            List {
                if !highlights.isEmpty {
                    Section("Highlights") {
                        ForEach(highlights) { highlight in
                            highlightRow(highlight)
                        }
                    }
                }
                if !standaloneNotes.isEmpty {
                    Section("Notes") {
                        ForEach(standaloneNotes) { note in
                            noteRow(note)
                        }
                    }
                }
                if !bookmarks.isEmpty {
                    Section("Bookmarks") {
                        ForEach(bookmarks) { bookmark in
                            bookmarkRow(bookmark)
                        }
                    }
                }
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "highlighter")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text("No annotations yet")
                .font(.title3)
            Text("Select text in the flow view to add a highlight, note, or bookmark.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    // MARK: - Rows

    private func highlightRow(_ highlight: AnnotationVM) -> some View {
        let attachedNotes = notes.filter { $0.anchor == highlight.anchor }
        return VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .top, spacing: 8) {
                Circle()
                    .fill(highlight.highlightColor?.color ?? .yellow)
                    .frame(width: 12, height: 12)
                    .padding(.top, 4)
                    .accessibilityLabel("\(highlight.highlightColor?.label ?? "Yellow") highlight")
                VStack(alignment: .leading, spacing: 2) {
                    Text(document.sectionLabel(for: highlight))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(document.annotatedText(for: highlight) ?? "(text unavailable)")
                        .font(.body)
                        .lineLimit(3)
                }
                Spacer(minLength: 0)
                orphanedBadge(for: highlight)
            }
            ForEach(attachedNotes) { note in
                if let text = note.displayNoteText {
                    Text(text)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .padding(.leading, 20)
                }
            }
            rowActions(annotation: highlight, attachedNote: attachedNotes.first)
        }
        .padding(.vertical, 2)
    }

    private func noteRow(_ note: AnnotationVM) -> some View {
        HStack(alignment: .top, spacing: 8) {
            // Purely decorative -- this row's own text (below) already says
            // it's a note via the "Notes" section header VoiceOver reads on
            // the way in; hiding this avoids an extra, redundant "note text"
            // announcement per row.
            Image(systemName: "note.text")
                .foregroundStyle(.secondary)
                .padding(.top, 2)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(document.sectionLabel(for: note))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(note.displayNoteText ?? "")
                    .font(.body)
            }
            Spacer(minLength: 0)
            orphanedBadge(for: note)
            rowActions(annotation: note, attachedNote: note)
        }
        .padding(.vertical, 2)
    }

    private func bookmarkRow(_ bookmark: AnnotationVM) -> some View {
        HStack(spacing: 8) {
            // Decorative, same reasoning as noteRow's icon above -- the
            // "Bookmarks" section header already establishes what this row
            // is.
            Image(systemName: "bookmark.fill")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            Text(document.sectionLabel(for: bookmark))
            Spacer(minLength: 0)
            orphanedBadge(for: bookmark)
            rowActions(annotation: bookmark, attachedNote: nil)
        }
    }

    /// A distinct, accessible indicator for an annotation whose anchor
    /// couldn't be found in the document's current content (ADR-003's
    /// `.orphaned` status -- see `AnnotationAnchorStatus`). Empty for
    /// `.valid`/`.reanchored`/`nil` (the common case; `.reanchored` has
    /// already been silently corrected, so it needs no badge) -- only
    /// `.orphaned` renders anything here.
    @ViewBuilder
    private func orphanedBadge(for annotation: AnnotationVM) -> some View {
        if annotation.anchorStatus == .orphaned {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
                .help("Anchor not found — the surrounding text may have changed.")
                .accessibilityLabel("Anchor not found")
        }
    }

    private func rowActions(annotation: AnnotationVM, attachedNote: AnnotationVM?) -> some View {
        HStack(spacing: 12) {
            Button {
                annotations.pendingJumpAnnotationId = annotation.id
                dismiss()
            } label: {
                Image(systemName: "arrow.right.circle")
            }
            .buttonStyle(.borderless)
            .help("Jump to this annotation")
            // `.help()` alone is a hover tooltip, not a VoiceOver label --
            // every icon-only button in this row needs its own explicit
            // `.accessibilityLabel` too.
            .accessibilityLabel("Jump to this annotation")

            if annotation.kind == .note {
                Button {
                    editingNote = annotation
                    editedNoteText = annotation.displayNoteText ?? ""
                } label: {
                    Image(systemName: "pencil")
                }
                .buttonStyle(.borderless)
                .help("Edit note")
                .accessibilityLabel("Edit note")
            } else if annotation.kind == .highlight && attachedNote == nil {
                Button {
                    editingNote = AnnotationVM(
                        id: "new-note-for-\(annotation.id)",
                        itemId: annotation.itemId,
                        kind: .note,
                        blockId: annotation.blockId,
                        start: annotation.start,
                        len: annotation.len,
                        prefixHash: annotation.prefixHash,
                        quoteHash: annotation.quoteHash,
                        noteText: nil,
                        createdAt: annotation.createdAt,
                        updatedAt: annotation.updatedAt
                    )
                    editedNoteText = ""
                } label: {
                    Image(systemName: "plus.bubble")
                }
                .buttonStyle(.borderless)
                .help("Add a note to this highlight")
                .accessibilityLabel("Add a note to this highlight")
            }

            Button(role: .destructive) {
                Task {
                    await core.deleteAnnotation(id: annotation.id)
                    await annotations.reload(itemId: itemId, core: core)
                }
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .help("Delete")
            .accessibilityLabel("Delete annotation")
        }
    }

    // MARK: - Note editing

    /// Shared editor for both "edit an existing note's text" and "add a new
    /// note attached to a highlight that doesn't have one yet" -- the latter
    /// is signalled by `annotation.id` starting with the synthetic
    /// `"new-note-for-"` prefix set in `rowActions` above, which this sheet
    /// never persists directly; it always goes through
    /// `CoreClient.createAnnotation`/`updateAnnotationNote` as appropriate.
    @ViewBuilder
    private func editNoteSheet(for annotation: AnnotationVM) -> some View {
        let isNewAttachedNote = annotation.id.hasPrefix("new-note-for-")
        VStack(alignment: .leading, spacing: 12) {
            Text(isNewAttachedNote ? "Add Note" : "Edit Note").font(.headline)
            TextField("Note", text: $editedNoteText, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(3...8)
            HStack {
                Spacer()
                Button("Cancel") { editingNote = nil }
                Button("Save") {
                    Task {
                        if isNewAttachedNote {
                            await core.createAnnotation(
                                itemId: annotation.itemId,
                                kind: .note,
                                blockId: annotation.blockId,
                                start: annotation.start,
                                len: annotation.len,
                                prefixHash: annotation.prefixHash,
                                quoteHash: annotation.quoteHash,
                                noteText: editedNoteText.trimmingCharacters(in: .whitespacesAndNewlines)
                            )
                        } else {
                            await core.updateAnnotationNote(
                                id: annotation.id,
                                noteText: editedNoteText.trimmingCharacters(in: .whitespacesAndNewlines)
                            )
                        }
                        await annotations.reload(itemId: itemId, core: core)
                        editingNote = nil
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(editedNoteText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding()
        .frame(width: 320)
    }
}
