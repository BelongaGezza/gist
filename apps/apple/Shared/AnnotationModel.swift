import SwiftUI

// ── FNV-1a hashing (ADR-003) ─────────────────────────────────────────────────
//
// `gist_store::create_annotation`/`gist_core::Core::create_annotation`
// persist whatever `prefix_hash`/`quote_hash` they're given -- they never
// compute or verify a hash themselves (see both methods' doc comments, and
// `FfiAnnotation`'s in `crates/gist-ffi/src/lib.rs`). Per ADR-003, the
// *caller* (the reading view) is responsible for computing them, using
// FNV-1a. This is that implementation on the Swift side. There is currently
// no Rust-side FNV-1a implementation to cross-check against (the
// re-anchoring/verification logic that would consume these hashes is being
// built concurrently by a separate engineer per ADR-003 -- see CLAUDE.md's
// M3 role register) -- this hashes the standard FNV-1a 64-bit constants
// (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) over UTF-8
// bytes, which is the conventional definition and the one most likely to
// match whatever the Rust side eventually implements.

/// Standard 64-bit FNV-1a, hashing over UTF-8 bytes.
enum FNV1a {
    private static let offsetBasis: UInt64 = 0xcbf2_9ce4_8422_2325
    private static let prime: UInt64 = 0x0100_0000_01b3

    static func hash(_ bytes: [UInt8]) -> UInt64 {
        var hash = offsetBasis
        for byte in bytes {
            hash ^= UInt64(byte)
            hash = hash &* prime
        }
        return hash
    }

    static func hash(_ string: String) -> UInt64 {
        hash(Array(string.utf8))
    }
}

/// Computes ADR-003's `prefix_hash`/`quote_hash` for a `start`/`len` (byte
/// offsets) anchor within `fullText` -- the section's
/// `concatenatedPlainText` (see that property's doc comment for why a
/// *section's* concatenated text, not one block's text in isolation, is the
/// coordinate space this reading view anchors against).
enum AnnotationAnchoring {
    /// ADR-003's own wording: "FNV-1a of the 30 characters preceding
    /// `start`" -- characters (grapheme clusters), not bytes.
    static let prefixContextLength = 30

    /// - `quoteHash`: FNV-1a of the anchored span's own bytes
    ///   (`fullText`'s UTF-8 bytes `start..<start+len`, clamped to
    ///   `fullText`'s bounds).
    /// - `prefixHash`: FNV-1a of the up-to-`prefixContextLength` characters
    ///   immediately preceding `start` (clamped at the start of `fullText`
    ///   if there isn't that much context).
    static func hashes(fullText: String, start: Int, len: Int) -> (prefixHash: UInt64, quoteHash: UInt64) {
        let utf8 = Array(fullText.utf8)
        let clampedStart = min(max(start, 0), utf8.count)
        let clampedEnd = min(max(clampedStart + len, clampedStart), utf8.count)
        let quoteBytes = Array(utf8[clampedStart..<clampedEnd])

        let beforeBytes = Array(utf8[0..<clampedStart])
        let beforeText = String(decoding: beforeBytes, as: UTF8.self)
        let prefixText = String(beforeText.suffix(prefixContextLength))

        return (
            prefixHash: FNV1a.hash(prefixText),
            quoteHash: FNV1a.hash(quoteBytes)
        )
    }
}

// ── Highlight colour (client-side convention, no backend column) ───────────

/// The 6 highlight colours the flow view offers -- a conventional reader-app
/// palette (yellow/green/blue/pink/orange/purple), not anything the backend
/// dictates.
///
/// **Important limitation:** ADR-003's backend scaffold (commit `39c0942`)
/// defines `Annotation`/`FfiAnnotation` as `(id, item_id, kind, block_id,
/// start, len, prefix_hash, quote_hash, note_text, created_at, updated_at)`
/// -- there is no `color` column anywhere in `gist-model`/`gist-store`/
/// `gist-ffi`. Rather than block this whole feature on a Rust-side schema
/// change (out of scope for this "CRUD only" Apple-engineer role, and risky
/// to land concurrently with a separate engineer's own in-flight ADR-003
/// Rust work on re-anchoring), a highlight's colour is persisted as a small,
/// documented client-side convention inside `note_text`, for
/// `AnnotationKind.highlight` annotations *only*: `"color:<rawValue>"`
/// (`encode`/`decode` below). `.note`/`.bookmark` annotations never use this
/// encoding -- a `.note`'s `note_text` is always the user's real note body
/// (see `AnnotationVM.displayNoteText`). If a real `color` column is ever
/// added to the `annotations` table, this convention should be retired in
/// favour of it; nothing here depends on that not happening.
enum HighlightColor: String, CaseIterable, Identifiable {
    case yellow, green, blue, pink, orange, purple

    var id: String { rawValue }

    // (R5b localisation) These are read via `Text(color.label)`/`.help(color.label)`
    // -- both take `color.label`'s *value* at the call site, not a literal, so
    // they don't get SwiftUI's automatic Text("literal") -> LocalizedStringKey
    // treatment. Wrapping the literal here with `String(localized:)` is what
    // actually gets each case's name into the String Catalog.
    var label: String {
        switch self {
        case .yellow: return String(localized: "Yellow")
        case .green: return String(localized: "Green")
        case .blue: return String(localized: "Blue")
        case .pink: return String(localized: "Pink")
        case .orange: return String(localized: "Orange")
        case .purple: return String(localized: "Purple")
        }
    }

    var color: Color {
        switch self {
        case .yellow: return .yellow
        case .green: return .green
        case .blue: return .blue
        case .pink: return .pink
        case .orange: return .orange
        case .purple: return .purple
        }
    }

    private static let notePrefix = "color:"

    static func encode(_ color: HighlightColor) -> String { notePrefix + color.rawValue }

    /// Decodes a highlight's colour from its `note_text`, defaulting to
    /// `.yellow` if `noteText` is `nil`, doesn't match the `"color:"`
    /// convention, or names an unknown colour (e.g. written by a future app
    /// version with a wider palette this build doesn't know about).
    static func decode(from noteText: String?) -> HighlightColor {
        guard let noteText, noteText.hasPrefix(notePrefix),
            let color = HighlightColor(rawValue: String(noteText.dropFirst(notePrefix.count)))
        else { return .yellow }
        return color
    }
}

// ── AnnotationVM ─────────────────────────────────────────────────────────────

/// Client-side identity for an annotation's anchor -- two annotations with
/// an identical anchor are considered "the same location," used to group a
/// `.note` that's "attached to" a `.highlight` (i.e. shares its anchor
/// exactly) apart from a standalone note anchored elsewhere. Nothing in the
/// Rust backend models this relationship explicitly (there is no parent/
/// child id) -- it's inferred client-side purely from anchor equality.
struct AnnotationAnchor: Hashable {
    let blockId: String
    let start: Int
    let len: Int
}

/// Swift-friendly wrapper around `FfiAnnotation`, matching how
/// `LibraryItemVM`/`CollectionVM` wrap their own Ffi counterparts elsewhere
/// in this app.
struct AnnotationVM: Identifiable, Equatable {
    let id: String
    let itemId: String
    let kind: FfiAnnotationKind
    let blockId: String
    let start: Int
    let len: Int
    let prefixHash: UInt64
    let quoteHash: UInt64
    /// Raw FFI value -- for `.highlight` this is the `HighlightColor`
    /// encoding (see that type's doc comment), never a real note body; use
    /// `displayNoteText` to get the user-authored text a `.note` actually
    /// carries.
    let noteText: String?
    let createdAt: Int64
    let updatedAt: Int64
    /// This annotation's most recently known re-anchoring status (ADR-003),
    /// stamped on by `CoreClient.reanchorAnnotations`'s Swift wrapper --
    /// `nil` until that step has run at least once for this annotation
    /// (e.g. a value fetched via the plain `listAnnotations`, which never
    /// re-anchors). See `AnnotationAnchorStatus` below.
    let anchorStatus: AnnotationAnchorStatus?

    init(ffi: FfiAnnotation, anchorStatus: AnnotationAnchorStatus? = nil) {
        id = ffi.id
        itemId = ffi.itemId
        kind = ffi.kind
        blockId = ffi.blockId
        start = Int(ffi.start)
        len = Int(ffi.len)
        prefixHash = ffi.prefixHash
        quoteHash = ffi.quoteHash
        noteText = ffi.noteText
        createdAt = ffi.createdAt
        updatedAt = ffi.updatedAt
        self.anchorStatus = anchorStatus
    }

    /// Test/preview-friendly direct initializer, mirroring `LibraryItemVM`'s
    /// own plain-value `init` -- production code always goes through
    /// `init(ffi:)`.
    init(
        id: String,
        itemId: String,
        kind: FfiAnnotationKind,
        blockId: String,
        start: Int,
        len: Int,
        prefixHash: UInt64,
        quoteHash: UInt64,
        noteText: String?,
        createdAt: Int64,
        updatedAt: Int64,
        anchorStatus: AnnotationAnchorStatus? = nil
    ) {
        self.id = id
        self.itemId = itemId
        self.kind = kind
        self.blockId = blockId
        self.start = start
        self.len = len
        self.prefixHash = prefixHash
        self.quoteHash = quoteHash
        self.noteText = noteText
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.anchorStatus = anchorStatus
    }

    var anchor: AnnotationAnchor { AnnotationAnchor(blockId: blockId, start: start, len: len) }

    /// The colour to render this annotation's highlight in -- only
    /// meaningful for `.highlight`; `nil` for `.note`/`.bookmark`.
    var highlightColor: HighlightColor? {
        kind == .highlight ? HighlightColor.decode(from: noteText) : nil
    }

    /// The user-authored note body. `nil` for `.highlight` (whose
    /// `noteText`, if present, is a colour encoding, never prose) and for
    /// `.bookmark` (which never carries text), and for a `.note` with no
    /// text of its own.
    var displayNoteText: String? {
        guard kind == .note, let noteText, !noteText.isEmpty else { return nil }
        return noteText
    }
}

/// Client-side mirror of `FfiAnchorStatus` (ADR-003 re-anchoring --
/// `crates/gist-ffi/src/lib.rs`), stamped onto `AnnotationVM.anchorStatus` by
/// `CoreClient.reanchorAnnotations`/`AnnotationAnchorResult` below.
///
/// `.valid` and `.reanchored` both mean "this annotation currently resolves
/// to real text in the document" -- `.reanchored` has already been silently
/// corrected and persisted (its `start` rewritten) by the time this status
/// is observed, so from the UI's perspective it's `.valid`-equivalent going
/// forward. Only `.orphaned` (the quoted text could not be found anywhere in
/// its block) needs a visible indicator -- see `AnnotationsSidebarView`.
enum AnnotationAnchorStatus: Equatable {
    case valid
    case reanchored
    case orphaned

    init(ffi: FfiAnchorStatus) {
        switch ffi {
        case .valid: self = .valid
        case .reanchored: self = .reanchored
        case .orphaned: self = .orphaned
        }
    }
}

/// Swift-friendly wrapper around `FfiAnnotationAnchorResult`, returned by
/// `CoreClient.reanchorAnnotations` -- one annotation's re-anchoring outcome,
/// pairing the (possibly just-corrected) `AnnotationVM` with what happened
/// to it. `oldStart` is non-`nil` only when `status == .reanchored`, mirroring
/// the FFI record's own doc comment.
struct AnnotationAnchorResult {
    let annotation: AnnotationVM
    let status: AnnotationAnchorStatus
    let oldStart: Int?

    init(ffi: FfiAnnotationAnchorResult) {
        let status = AnnotationAnchorStatus(ffi: ffi.status)
        self.status = status
        self.oldStart = ffi.oldStart.map(Int.init)
        self.annotation = AnnotationVM(ffi: ffi.annotation, anchorStatus: status)
    }
}

// ── Document-relative helpers (snippets, section labels) ───────────────────

extension FlowDocumentVM {
    /// Best-effort plain-text snippet for one annotation's anchored span,
    /// resolved against this document's *current* sections -- i.e. always
    /// "what the text says right now," not a stored copy from anchoring
    /// time. Returns `nil` if the anchor no longer resolves (unknown
    /// `block_id`, or an out-of-range offset) -- detecting and surfacing
    /// that as a "this annotation may have moved" status is the concurrent
    /// re-anchoring work's job (see `AnnotationVM.anchorStatus`), not this
    /// helper's; it just declines to guess.
    func annotatedText(for annotation: AnnotationVM) -> String? {
        guard let section = sections.first(where: { $0.id == annotation.blockId }) else { return nil }
        let full = Array(section.concatenatedPlainText.utf8)
        guard annotation.start >= 0, annotation.start <= full.count else { return nil }
        let end = min(annotation.start + annotation.len, full.count)
        guard end >= annotation.start else { return nil }
        return String(decoding: full[annotation.start..<end], as: UTF8.self)
    }

    /// A human-readable label for the section an annotation anchors to --
    /// its heading text if it has one, else "Section N" (1-based).
    func sectionLabel(for annotation: AnnotationVM) -> String {
        guard let index = sections.firstIndex(where: { $0.id == annotation.blockId }) else {
            return String(localized: "Unknown section")
        }
        // `heading?.text` is real document content (untranslatable data) when
        // present; only the "Section N" fallback used in its absence is a
        // fixed UI string, so only that branch is wrapped for localisation.
        return sections[index].heading?.text ?? String(localized: "Section \(index + 1)")
    }
}
