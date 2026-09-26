import SwiftUI

// ── Word tokenizer ───────────────────────────────────────────────────────────

/// One selectable word inside a block's plain text, with its byte range
/// within that same text -- the addressing space `create_annotation`'s
/// `start`/`len` use once translated into the hosting section's
/// concatenated coordinate space (see `FlowSectionVM.concatenatedPlainText`
/// and `blockByteOffset(at:)`).
struct SelectableWord: Identifiable {
    let id: Int
    let text: String
    let byteRange: Range<Int>
}

/// Splits `text` on ASCII whitespace (space/tab/CR/LF) into words, each
/// carrying its own UTF-8 byte range within `text`. Splitting at the byte
/// level on these specific bytes is safe even for multi-byte UTF-8 text: none
/// of 0x20/0x09/0x0A/0x0D can appear as a continuation byte of a multi-byte
/// sequence, so a match can never occur "inside" a non-ASCII character.
/// Word-granularity selection (not exact character-range selection) is a
/// deliberate compromise -- see `FlowViewSwiftUINative`'s type doc comment
/// for why real character-range text selection isn't available in this
/// app's chosen SwiftUI-native reading-view architecture (CLAUDE.md's Q8).
func selectableWords(in text: String) -> [SelectableWord] {
    let bytes = Array(text.utf8)
    var words: [SelectableWord] = []
    var wordStart: Int?
    var wordIndex = 0

    func flush(endExclusive: Int) {
        guard let start = wordStart else { return }
        let wordBytes = bytes[start..<endExclusive]
        words.append(
            SelectableWord(id: wordIndex, text: String(decoding: wordBytes, as: UTF8.self), byteRange: start..<endExclusive)
        )
        wordIndex += 1
        wordStart = nil
    }

    for (offset, byte) in bytes.enumerated() {
        let isSpace = byte == 0x20 || byte == 0x09 || byte == 0x0A || byte == 0x0D
        if isSpace {
            flush(endExclusive: offset)
        } else if wordStart == nil {
            wordStart = offset
        }
    }
    flush(endExclusive: bytes.count)
    return words
}

// ── FlowLayout (wrapping row of chips) ──────────────────────────────────────

/// Minimal left-to-right, wrapping flow layout. SwiftUI has no built-in
/// wrapping `HStack` equivalent -- `HStack` never wraps, and `LazyVGrid`
/// isn't a wrap-flow either -- so `HighlightSelectionSheet` uses this small
/// custom `Layout` conformance (available since macOS 13) to arrange its
/// per-word selection chips.
struct FlowLayout: Layout {
    var spacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var origin = CGPoint.zero
        var lineHeight: CGFloat = 0
        var measuredWidth: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if origin.x > 0, origin.x + size.width > maxWidth {
                origin.x = 0
                origin.y += lineHeight + spacing
                lineHeight = 0
            }
            origin.x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
            measuredWidth = max(measuredWidth, origin.x - spacing)
        }
        return CGSize(width: min(measuredWidth, maxWidth), height: origin.y + lineHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var origin = CGPoint(x: bounds.minX, y: bounds.minY)
        var lineHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if origin.x > bounds.minX, origin.x + size.width > bounds.maxX {
                origin.x = bounds.minX
                origin.y += lineHeight + spacing
                lineHeight = 0
            }
            subview.place(at: origin, proposal: .unspecified)
            origin.x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
        }
    }
}

// ── Highlight creation (text selection → colour) ────────────────────────────

/// Sheet for creating a `.highlight` annotation by selecting a run of words
/// within one block, then picking a colour -- the "text selection in the
/// flow reading view → highlight in N distinct colours" feature. Selection
/// works by tapping a start word, then tapping an end word to extend the
/// range (tapping the same single word again clears the selection); real
/// drag-to-select character ranges aren't available in this app's
/// SwiftUI-native reading view (see CLAUDE.md's Q8), so word-granularity
/// tap-to-select is the deliberate, documented compromise.
///
/// If `noteText` is non-empty when a colour is picked, a second `.note`
/// annotation is created sharing the *exact same anchor* as the highlight --
/// this is how "a margin note attached to a highlight" is modelled, since
/// the backend has no explicit parent/child relationship (see
/// `AnnotationVM.anchor`'s doc comment).
struct HighlightSelectionSheet: View {
    @EnvironmentObject var core: CoreClient
    @ObservedObject var annotations: AnnotationState
    @Environment(\.dismiss) private var dismiss

    let itemId: String
    let section: FlowSectionVM
    let blockIndexInSection: Int

    @State private var anchorWordId: Int?
    @State private var focusWordId: Int?
    @State private var noteText = ""
    @State private var isSaving = false

    private let words: [SelectableWord]

    init(itemId: String, section: FlowSectionVM, blockIndexInSection: Int, blockPlainText: String, annotations: AnnotationState) {
        self.itemId = itemId
        self.section = section
        self.blockIndexInSection = blockIndexInSection
        self.annotations = annotations
        self.words = selectableWords(in: blockPlainText)
    }

    private var selectedRange: ClosedRange<Int>? {
        guard let anchorWordId, let focusWordId else { return nil }
        return min(anchorWordId, focusWordId)...max(anchorWordId, focusWordId)
    }

    private var selectedText: String {
        guard let range = selectedRange else { return "" }
        return words.filter { range.contains($0.id) }.map(\.text).joined(separator: " ")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Select Text to Highlight").font(.headline)
            Text("Tap a word to start a selection, then tap another word to extend it.")
                .font(.caption)
                .foregroundStyle(.secondary)

            ScrollView {
                FlowLayout(spacing: 4) {
                    ForEach(words) { word in
                        wordChip(word)
                    }
                }
                .padding(.vertical, 4)
            }
            .frame(minHeight: 80, maxHeight: 180)

            if selectedRange != nil {
                Divider()
                Text("Selected: \u{201C}\(selectedText)\u{201D}")
                    .font(.callout)
                    .lineLimit(2)

                TextField("Add a note to this highlight (optional)", text: $noteText, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...3)

                colorSwatches
            }

            HStack {
                Button("Clear Selection") {
                    anchorWordId = nil
                    focusWordId = nil
                }
                .disabled(selectedRange == nil)
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding()
        .frame(width: 380)
        .disabled(isSaving)
    }

    private var colorSwatches: some View {
        HStack(spacing: 10) {
            ForEach(HighlightColor.allCases) { color in
                Button {
                    Task { await commit(color: color) }
                } label: {
                    Circle().fill(color.color).frame(width: 24, height: 24)
                }
                .buttonStyle(.plain)
                .help(color.label)
                .accessibilityLabel("\(color.label) highlight")
            }
        }
    }

    @ViewBuilder
    private func wordChip(_ word: SelectableWord) -> some View {
        let isSelected = selectedRange?.contains(word.id) ?? false
        Button {
            selectWord(word)
        } label: {
            Text(word.text)
                .padding(.horizontal, 4)
                .padding(.vertical, 2)
                .background(isSelected ? Color.accentColor.opacity(0.35) : Color.clear)
                .cornerRadius(4)
        }
        .buttonStyle(.plain)
        // This is a fully custom word-tap-to-select control (see this
        // file's type doc comment on `HighlightSelectionSheet` for why real
        // text selection isn't available here) -- VoiceOver has no built-in
        // notion of "selected" for a plain `Button`, so the trait has to be
        // added explicitly for a selected word to be announced as such.
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    private func selectWord(_ word: SelectableWord) {
        if anchorWordId == nil {
            anchorWordId = word.id
            focusWordId = word.id
        } else if anchorWordId == word.id && focusWordId == word.id {
            anchorWordId = nil
            focusWordId = nil
        } else {
            focusWordId = word.id
        }
    }

    private func commit(color: HighlightColor) async {
        guard let range = selectedRange,
            let first = words.first(where: { $0.id == range.lowerBound }),
            let last = words.first(where: { $0.id == range.upperBound })
        else { return }

        isSaving = true
        defer { isSaving = false }

        let localStart = first.byteRange.lowerBound
        let localEnd = last.byteRange.upperBound
        let absoluteStart = section.blockByteOffset(at: blockIndexInSection) + localStart
        let absoluteLen = localEnd - localStart
        let fullText = section.concatenatedPlainText
        let (prefixHash, quoteHash) = AnnotationAnchoring.hashes(fullText: fullText, start: absoluteStart, len: absoluteLen)

        guard
            await core.createAnnotation(
                itemId: itemId,
                kind: .highlight,
                blockId: section.id,
                start: absoluteStart,
                len: absoluteLen,
                prefixHash: prefixHash,
                quoteHash: quoteHash,
                noteText: HighlightColor.encode(color)
            ) != nil
        else { return }

        let trimmedNote = noteText.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmedNote.isEmpty {
            await core.createAnnotation(
                itemId: itemId,
                kind: .note,
                blockId: section.id,
                start: absoluteStart,
                len: absoluteLen,
                prefixHash: prefixHash,
                quoteHash: quoteHash,
                noteText: trimmedNote
            )
        }

        await annotations.reload(itemId: itemId, core: core)
        dismiss()
    }
}

// ── Standalone note / bookmark creation ─────────────────────────────────────

/// Sheet for creating a standalone `.note` (not attached to any highlight),
/// anchored at a zero-length point at the start of the given section --
/// reachable via a block's context menu ("Add Note Here"), independent of
/// any text selection.
struct NoteComposerSheet: View {
    @EnvironmentObject var core: CoreClient
    @ObservedObject var annotations: AnnotationState
    @Environment(\.dismiss) private var dismiss

    let itemId: String
    let sectionId: String
    let contextLabel: String

    @State private var noteText = ""
    @State private var isSaving = false

    /// Explicit initializer (rather than relying on the synthesized
    /// memberwise one) so call sites can pass arguments in whatever order
    /// reads best, independent of this type's stored-property declaration
    /// order -- Swift's synthesized memberwise init requires call-site
    /// argument order to match declaration order exactly, which is an easy
    /// trap once a type also has excluded `@EnvironmentObject`/`@Environment`
    /// properties interleaved with its plain stored ones.
    init(itemId: String, sectionId: String, contextLabel: String, annotations: AnnotationState) {
        self.itemId = itemId
        self.sectionId = sectionId
        self.contextLabel = contextLabel
        self.annotations = annotations
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add Note").font(.headline)
            Text(contextLabel)
                .font(.caption)
                .foregroundStyle(.secondary)
            TextField("Note", text: $noteText, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(3...8)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save") { Task { await save() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(noteText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isSaving)
            }
        }
        .padding()
        .frame(width: 320)
    }

    private func save() async {
        let trimmed = noteText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        isSaving = true
        defer { isSaving = false }

        // Zero-length point anchor: FNV-1a of the empty byte sequence is a
        // well-defined constant (the FNV offset basis itself), same as any
        // other empty span.
        let (prefixHash, quoteHash) = AnnotationAnchoring.hashes(fullText: "", start: 0, len: 0)
        await core.createAnnotation(
            itemId: itemId,
            kind: .note,
            blockId: sectionId,
            start: 0,
            len: 0,
            prefixHash: prefixHash,
            quoteHash: quoteHash,
            noteText: trimmed
        )
        await annotations.reload(itemId: itemId, core: core)
        dismiss()
    }
}
