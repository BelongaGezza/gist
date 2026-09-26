import SwiftUI
import UniformTypeIdentifiers

// ── Export format choice ─────────────────────────────────────────────────────
//
// Markdown, not JSON, and not a custom binary format. Rationale: annotations
// are a *user-facing* artifact -- someone exports them to actually read
// their own highlights/notes later, paste them into another note-taking
// app, or share a summary with someone else. Markdown renders as readable
// prose in virtually every text editor, note app (Obsidian, Notion, Apple
// Notes via paste), and GitHub/GitLab issue or PR description without any
// tooling, and still degrades gracefully to plain text if opened in
// something that doesn't render Markdown at all. JSON would be strictly
// better for a *machine* consumer (e.g. a future re-import feature), but
// nothing in this app's scope re-imports annotations today, so optimizing
// for round-trip fidelity over human readability isn't justified yet -- if
// that need appears later, a `.json` export can be added alongside this one
// without displacing it.

extension UTType {
    /// A small custom type conforming to `.plainText` so `.fileExporter`
    /// defaults to a `.md` extension, rather than reusing the generic
    /// `.plainText` type (which maps to `.txt`).
    static var gistAnnotationsMarkdown: UTType {
        UTType(exportedAs: "com.gist.annotations-markdown", conformingTo: .plainText)
    }
}

/// `FileDocument` wrapper around the exported Markdown string, for
/// `.fileExporter(document:)` (the standard SwiftUI mechanism for
/// user-initiated file export -- see CLAUDE.md's working conventions).
struct AnnotationsMarkdownDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.gistAnnotationsMarkdown] }
    static var writableContentTypes: [UTType] { [.gistAnnotationsMarkdown] }

    var text: String

    init(text: String) {
        self.text = text
    }

    init(configuration: ReadConfiguration) throws {
        guard let data = configuration.file.regularFileContents,
            let string = String(data: data, encoding: .utf8)
        else {
            throw CocoaError(.fileReadCorruptFile)
        }
        text = string
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

/// Builds the exported Markdown for one document's annotations. Pure
/// function (no FFI, no I/O) so it's unit-testable directly -- see
/// `FlowViewTests`.
enum AnnotationMarkdownExporter {
    static func markdown(for annotations: [AnnotationVM], in document: FlowDocumentVM) -> String {
        let highlights = annotations.filter { $0.kind == .highlight }.sorted { $0.createdAt < $1.createdAt }
        let notes = annotations.filter { $0.kind == .note }.sorted { $0.createdAt < $1.createdAt }
        let bookmarks = annotations.filter { $0.kind == .bookmark }.sorted { $0.createdAt < $1.createdAt }

        // (R5b localisation) This builds a Markdown file a person reads
        // directly (see this file's own top note on why Markdown was
        // chosen) via plain string interpolation/concatenation, never
        // through `Text()` -- none of it reaches the catalog automatically,
        // so every fixed English fragment below is wrapped explicitly.
        // `document.metadata.title`/`sectionLabel`/`displayNoteText` are
        // real document/user content, not translatable, and are left as-is.
        var lines = [String(localized: "# Annotations — \(document.metadata.title)"), ""]

        if !highlights.isEmpty {
            lines.append(String(localized: "## Highlights"))
            lines.append("")
            for highlight in highlights {
                lines.append(contentsOf: highlightLines(highlight, notes: notes, document: document))
            }
            lines.append("")
        }

        let standaloneNotes = notes.filter { note in
            !highlights.contains { $0.anchor == note.anchor }
        }
        if !standaloneNotes.isEmpty {
            lines.append(String(localized: "## Notes"))
            lines.append("")
            for note in standaloneNotes {
                lines.append("- \(document.sectionLabel(for: note)): \(note.displayNoteText ?? "")")
            }
            lines.append("")
        }

        if !bookmarks.isEmpty {
            lines.append(String(localized: "## Bookmarks"))
            lines.append("")
            for bookmark in bookmarks {
                lines.append("- \(document.sectionLabel(for: bookmark))")
            }
            lines.append("")
        }

        if highlights.isEmpty && notes.isEmpty && bookmarks.isEmpty {
            lines.append(String(localized: "_No annotations yet._"))
        }

        return lines.joined(separator: "\n")
    }

    /// One highlight's bullet, plus (indented underneath, if present) the
    /// text of every `.note` "attached to" it -- sharing its exact anchor
    /// (see `AnnotationVM.anchor`'s doc comment for what that means; the
    /// backend has no explicit parent/child relationship, this is inferred
    /// purely from anchor equality).
    private static func highlightLines(
        _ highlight: AnnotationVM,
        notes: [AnnotationVM],
        document: FlowDocumentVM
    ) -> [String] {
        let quote = document.annotatedText(for: highlight) ?? String(localized: "(text unavailable)")
        // `highlightColor?.label` is already localized at its own
        // definition (see `HighlightColor.label`); only this fallback
        // (an annotation somehow missing a valid encoded colour) needs its
        // own wrap.
        let colorLabel = highlight.highlightColor?.label ?? String(localized: "Highlight")
        var result = ["- **[\(colorLabel)]** \(document.sectionLabel(for: highlight)): \u{201C}\(quote)\u{201D}"]
        for note in notes where note.anchor == highlight.anchor {
            if let text = note.displayNoteText, !text.isEmpty {
                result.append("  > \(text)")
            }
        }
        return result
    }
}
