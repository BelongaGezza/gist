import Foundation
import Combine

/// Single touch-point for all FFI calls.
@MainActor
final class CoreClient: ObservableObject {
    static let shared = CoreClient()

    @Published var items: [LibraryItemVM] = []
    @Published var searchResults: [LibraryItemVM] = []
    @Published var isSearching = false
    @Published var collections: [CollectionVM] = []
    @Published var allTags: [String] = []
    @Published var isLoading = false
    @Published var error: String?
    /// Set (instead of `error`) when an import fails specifically because the
    /// source file is DRM-protected, so the view layer can present a
    /// dedicated DRM alert rather than a generic import-failure message.
    @Published var drmProtectedFile: URL?

    private let core: GistCore?

    /// **ADR-014 fix:** `.shared` is built with real, read-only decryption
    /// capability via `GistCore.newWithReadKey(dbPath:storageDir:keyProvider:)`
    /// and a production `KeychainKeyProvider()` — not plain `GistCore.init`
    /// (which this used to call) and not `newEncrypted` either. This is
    /// deliberately the middle option: new imports through `.shared` still
    /// land plaintext by default, exactly as before this change (nothing
    /// about the default import path changes), but an item a user encrypts
    /// via `encryptItems` (see below) is no longer permanently unreadable
    /// through this same running app afterward — before this fix, `.shared`
    /// had no key provider of any kind, so `loadDocument`/`startRsvp` on a
    /// freshly-encrypted item failed with `MissingKeyProvider`, a real
    /// data-access bug (a user could click "Encrypt" and permanently lock
    /// themselves out of a book). See `gist_store::Store::open_with_read_key`
    /// and `docs/adr/014-per-item-encryption.md` for the full rationale.
    private init() {
        do {
            let supportDir = try FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            ).appendingPathComponent("GIST", isDirectory: true)

            let dbPath = supportDir.appendingPathComponent("gist.sqlite3").path
            let storageDir = supportDir.appendingPathComponent("storage", isDirectory: true).path

            core = try GistCore.newWithReadKey(
                dbPath: dbPath,
                storageDir: storageDir,
                keyProvider: KeychainKeyProvider()
            )
        } catch {
            self.core = nil
            // (R5b localisation) Plain string interpolation, not passed
            // through Text() -- wrap so the fixed prefix reaches the
            // catalog. `error` itself is a system-provided description,
            // not further localisable here.
            // `String(describing:)` explicitly, per the compiler's own
            // suggestion: `error: Error` has no built-in
            // `String.LocalizationValue` interpolation support (it's an
            // existential, not `CustomLocalizedStringResourceConvertible`),
            // so interpolating it directly emits a deprecation warning
            // about producing an unlocalized debug description -- which is
            // exactly what's wanted here (this fixed prefix is the only
            // part meant to be localized; the underlying error's own text
            // is diagnostic, not further localizable by us).
            self.error = String(localized: "Failed to initialise GIST core: \(String(describing: error))")
        }
    }

    /// Test-only construction path: builds a `CoreClient` around a real
    /// `GistCore` pointed at caller-supplied db/storage locations, instead of
    /// `.shared`'s production Application Support directory. This exists so
    /// `GISTTests` can exercise real FFI + SQLite + filesystem behaviour
    /// against a scratch temp directory per test, without touching (or
    /// depending on) a real user's Application Support state.
    ///
    /// Additive only: `.shared` still goes through `private init()` above,
    /// unchanged in shape (still no test-Keychain dependency here), so no
    /// production call site is affected.
    init(dbPath: String, storageDir: String) {
        do {
            core = try GistCore(dbPath: dbPath, storageDir: storageDir)
        } catch {
            self.core = nil
            // (R5b localisation) Plain string interpolation, not passed
            // through Text() -- wrap so the fixed prefix reaches the
            // catalog. `error` itself is a system-provided description,
            // not further localisable here.
            // `String(describing:)` explicitly, per the compiler's own
            // suggestion: `error: Error` has no built-in
            // `String.LocalizationValue` interpolation support (it's an
            // existential, not `CustomLocalizedStringResourceConvertible`),
            // so interpolating it directly emits a deprecation warning
            // about producing an unlocalized debug description -- which is
            // exactly what's wanted here (this fixed prefix is the only
            // part meant to be localized; the underlying error's own text
            // is diagnostic, not further localizable by us).
            self.error = String(localized: "Failed to initialise GIST core: \(String(describing: error))")
        }
    }

    /// Test-only construction path mirroring `.shared`'s real production
    /// shape (ADR-014): a `GistCore` built via `newWithReadKey`, so tests can
    /// exercise the exact same "plaintext writes, read-capable-after-
    /// encrypt" configuration `.shared` actually uses in the running app,
    /// against a scratch temp directory and an injectable `KeyProvider`
    /// (normally a test-scoped `KeychainKeyProvider`, never the real
    /// production Keychain item).
    init(dbPath: String, storageDir: String, keyProvider: KeyProvider) {
        do {
            core = try GistCore.newWithReadKey(
                dbPath: dbPath,
                storageDir: storageDir,
                keyProvider: keyProvider
            )
        } catch {
            self.core = nil
            // (R5b localisation) Plain string interpolation, not passed
            // through Text() -- wrap so the fixed prefix reaches the
            // catalog. `error` itself is a system-provided description,
            // not further localisable here.
            // `String(describing:)` explicitly, per the compiler's own
            // suggestion: `error: Error` has no built-in
            // `String.LocalizationValue` interpolation support (it's an
            // existential, not `CustomLocalizedStringResourceConvertible`),
            // so interpolating it directly emits a deprecation warning
            // about producing an unlocalized debug description -- which is
            // exactly what's wanted here (this fixed prefix is the only
            // part meant to be localized; the underlying error's own text
            // is diagnostic, not further localizable by us).
            self.error = String(localized: "Failed to initialise GIST core: \(String(describing: error))")
        }
    }

    func refresh() async {
        await reloadItems(clearErrorOnSuccess: true)
    }

    /// Reloads `items`. `clearErrorOnSuccess` is false when this reload is the
    /// tail end of another operation (import, remove, encrypt) that has
    /// already published its own outcome -- a successful reload must not wipe
    /// a failure the user hasn't seen yet, or the DRM alert (and any other
    /// error) would never appear. Mirrors the Windows `CoreClient` split
    /// (`RefreshAsync`/`ReloadItemsAsync`) -- see PENDING_APPLE_CHANGES.md's
    /// 2026-09-21 entry for the bug this fixes.
    private func reloadItems(clearErrorOnSuccess: Bool) async {
        guard let core else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            let ffiItems = try core.listItems(offset: 0, limit: 500)
            items = mapItems(ffiItems)
            if clearErrorOnSuccess {
                error = nil
            }
        } catch {
            self.error = "\(error)"
        }
    }

    private func mapItems(_ ffiItems: [FfiLibraryItem]) -> [LibraryItemVM] {
        ffiItems.map { item in
            LibraryItemVM(
                id: item.id,
                // (R5b localisation) `?? "Untitled"` forces this to plain
                // String, losing Text()'s automatic literal handling at
                // every call site that renders `title` -- wrap here, once,
                // at the source.
                title: item.title ?? String(localized: "Untitled"),
                authors: item.authors,
                sourcePath: item.sourcePath,
                contentEncrypted: item.contentEncrypted
            )
        }
    }

    /// Runs an FTS5 search via `GistCore::search_items` and publishes into
    /// `searchResults`. An empty/whitespace-only query clears the results
    /// rather than round-tripping to FFI, since `LibraryView` treats a
    /// non-empty `searchResults` set as "showing search, not the full list."
    func search(query: String) async {
        guard let core else { return }
        guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            searchResults = []
            return
        }
        isSearching = true
        defer { isSearching = false }
        do {
            let ffiItems = try core.searchItems(query: query, limit: 200)
            searchResults = mapItems(ffiItems)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// Removes items from the library. `deleteSourceFiles` controls whether
    /// GIST's sandboxed ADR-006 copy is also deleted (the user's real,
    /// original file is never touched either way — see CLAUDE.md's
    /// copy-on-import note).
    ///
    /// Calls `GistCore.removeItemsDetailed` (PENDING_APPLE_CHANGES.md's
    /// 2026-09-21 W2/Q10 adoption entry) rather than the discarded-outcome
    /// `removeItems`, so a partial failure -- something else has a stored
    /// file open, most commonly on an SMB/AFP share or a `uchg`/`schg` flag
    /// -- can be told apart from a clean removal instead of silently
    /// reporting "removed" either way. Only `filesFailed` is ever surfaced
    /// as a warning; `filesMissing` (legacy items with no ADR-013 checksum
    /// sidecar) and `sharedCopiesKept` (ADR-006 dedup) are bookkeeping, not
    /// problems -- see `FfiRemoveOutcome`'s doc comment.
    @discardableResult
    func removeItems(ids: [String], deleteSourceFiles: Bool) async -> RemoveItemsSummary {
        guard let core else {
            return RemoveItemsSummary(removedCount: 0, filesFailedCount: 0)
        }
        var removedCount = 0
        var filesFailed = 0
        do {
            let outcome = try core.removeItemsDetailed(ids: ids, deleteSourceFiles: deleteSourceFiles)
            removedCount = outcome.removedIds.count
            filesFailed = Int(outcome.filesFailed)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        await reloadItems(clearErrorOnSuccess: false)
        return RemoveItemsSummary(removedCount: removedCount, filesFailedCount: filesFailed)
    }

    /// Reclaims any ADR-006 stored copy or `.json`/`.tokens.json` blob a
    /// previous removal's best-effort file delete could not clean up (see
    /// `removeItems`'s doc comment for why that can happen), plus anything
    /// else orphaned in the storage directory. Intended to run once per app
    /// launch (`GISTApp`'s `.task`), silently -- a failure here is logged to
    /// `error` like any other operation but is not something worth
    /// interrupting the user's session over. Never deletes a file a library
    /// row still references.
    @discardableResult
    func sweepOrphanedFiles() async -> Int {
        guard let core else { return 0 }
        do {
            let outcome = try core.sweepOrphanedFiles()
            return Int(outcome.filesDeleted)
        } catch {
            self.error = "\(error)"
            return 0
        }
    }

    /// Imports a web page by URL via `Core::import_url`, following the same
    /// DRM/error-handling shape as `importFile` (DRM is unreachable in
    /// practice for a web-fetch import, but the shared `GistError` type
    /// still carries the case, so we still branch on it for consistency).
    func importUrl(urlString: String) async {
        guard let core else { return }
        drmProtectedFile = nil
        do {
            let id = try core.importUrl(url: urlString)
            error = nil
            await autoEncryptIfEnabled(itemId: id)
        } catch let gistError as GistError {
            switch gistError {
            case .DrmProtected:
                drmProtectedFile = URL(string: urlString)
            case .Core, .InternalPanic:
                error = "\(gistError)"
            }
        } catch {
            self.error = "\(error)"
        }
        await reloadItems(clearErrorOnSuccess: false)
    }

    // MARK: - Collections & tags

    func createCollection(name: String) async {
        guard let core else { return }
        do {
            _ = try core.createCollection(name: name)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        await listCollections()
    }

    func listCollections() async {
        guard let core else { return }
        do {
            let ffiCollections = try core.listCollections()
            collections = ffiCollections.map {
                CollectionVM(id: $0.id, name: $0.name, createdAt: $0.createdAt)
            }
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func addItemToCollection(itemId: String, collectionId: String) async {
        guard let core else { return }
        do {
            try core.addItemToCollection(itemId: itemId, collectionId: collectionId)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func removeItemFromCollection(itemId: String, collectionId: String) async {
        guard let core else { return }
        do {
            try core.removeItemFromCollection(itemId: itemId, collectionId: collectionId)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func addTag(itemId: String, tagName: String) async {
        guard let core else { return }
        do {
            try core.addTag(itemId: itemId, tagName: tagName)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func removeTag(itemId: String, tagName: String) async {
        guard let core else { return }
        do {
            try core.removeTag(itemId: itemId, tagName: tagName)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    func listTagsForItem(itemId: String) async -> [String] {
        guard let core else { return [] }
        do {
            return try core.listTagsForItem(itemId: itemId)
        } catch {
            self.error = "\(error)"
            return []
        }
    }

    /// Publishes every tag name that exists anywhere in the library into
    /// `allTags`, for populating a filter menu -- unlike `listTagsForItem`,
    /// this isn't scoped to one item.
    func listAllTags() async {
        guard let core else { return }
        do {
            allTags = try core.listAllTags()
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// Returns the items tagged with `tagName` (newest first), via
    /// `GistCore::list_items_by_tag`. Like `listItemsInCollection`, this
    /// doesn't publish into a shared `@Published` property -- callers hold
    /// the result as local view state for whichever single tag filter is
    /// currently active.
    func listItemsByTag(tagName: String) async -> [LibraryItemVM] {
        guard let core else { return [] }
        do {
            let ffiItems = try core.listItemsByTag(tagName: tagName)
            error = nil
            return mapItems(ffiItems)
        } catch {
            self.error = "\(error)"
            return []
        }
    }

    /// Returns the items belonging to a collection (newest first), via
    /// `GistCore::list_items_in_collection`. Unlike `refresh()`/`search()`,
    /// this doesn't publish into a shared `@Published` property -- callers
    /// (currently just `CollectionDetailView`) hold the result as local view
    /// state, since only one collection is browsed at a time.
    func listItemsInCollection(collectionId: String) async -> [LibraryItemVM] {
        guard let core else { return [] }
        do {
            let ffiItems = try core.listItemsInCollection(collectionId: collectionId)
            error = nil
            return mapItems(ffiItems)
        } catch {
            self.error = "\(error)"
            return []
        }
    }

    /// Import any supported file (txt, epub, docx) via the generic FFI
    /// import path, which sniffs the format from magic bytes / extension.
    func importFile(url: URL) async {
        guard let core else { return }
        drmProtectedFile = nil
        do {
            let id = try core.importFile(path: url.path)
            error = nil
            await autoEncryptIfEnabled(itemId: id)
        } catch let gistError as GistError {
            switch gistError {
            case .DrmProtected:
                drmProtectedFile = url
            case .Core, .InternalPanic:
                error = "\(gistError)"
            }
        } catch {
            self.error = "\(error)"
        }
        await reloadItems(clearErrorOnSuccess: false)
    }

    /// Settings scene's Import-tab default (`ImportDefaults
    /// .autoEncryptOnImport`, see AppSettings.swift): when enabled, encrypts
    /// a just-imported item at rest immediately via the same `encryptItems`
    /// FFI path the Library's "Encrypt" action uses. Best-effort -- a
    /// failure here doesn't fail the import itself (the item is already in
    /// the library, plaintext, by the time this runs), matching the
    /// best-effort framing already used for e.g. `removeItems`'s leftover
    /// file cleanup.
    private func autoEncryptIfEnabled(itemId: String) async {
        guard let core, ImportDefaults.shared.autoEncryptOnImport else { return }
        _ = try? core.encryptItems(ids: [itemId], keyProvider: KeychainKeyProvider())
    }

    /// Fetches an RSVP session for `itemId` and decodes it. `startRsvp` is a
    /// one-shot FFI call — the returned token stream and pacing config are
    /// then driven entirely client-side by `RsvpPlayer` (see RsvpView.swift),
    /// per the "SwiftUI shell drives it" model documented on
    /// `gist_rsvp::RsvpSession`. Returns `nil` on failure and sets `error`.
    func startRsvp(itemId: String, wpm: UInt32) async -> RsvpSessionVM? {
        guard let core else { return nil }
        do {
            let json = try core.startRsvp(itemId: itemId, wpm: wpm)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(RsvpSessionVM.self, from: Data(json.utf8))
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    /// Fetches a document's full block structure (headings/paragraphs/
    /// images/lists) for the flow-view prototypes — as opposed to
    /// `startRsvp`'s flat token stream. Same one-shot-fetch-then-decode
    /// shape as `startRsvp`. Returns `nil` on failure and sets `error`.
    func loadDocument(itemId: String) async -> FlowDocumentVM? {
        guard let core else { return nil }
        do {
            let json = try core.getDocumentJson(itemId: itemId)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(FlowDocumentVM.self, from: Data(json.utf8))
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    // MARK: - Annotations (ADR-003)

    /// Creates a new annotation (highlight/note/bookmark), anchored per
    /// ADR-003 as `(block_id, start, len, prefix_hash, quote_hash)`.
    /// `prefixHash`/`quoteHash` must already be computed by the caller (see
    /// `AnnotationAnchoring` in `AnnotationModel.swift`) -- this call only
    /// persists them, exactly mirroring `GistCore.createAnnotation`'s own
    /// doc comment. Returns the new annotation's id, or `nil` on failure
    /// (with `error` set).
    @discardableResult
    func createAnnotation(
        itemId: String,
        kind: FfiAnnotationKind,
        blockId: String,
        start: Int,
        len: Int,
        prefixHash: UInt64,
        quoteHash: UInt64,
        noteText: String?
    ) async -> String? {
        guard let core else { return nil }
        do {
            let id = try core.createAnnotation(
                itemId: itemId,
                kind: kind,
                blockId: blockId,
                start: UInt64(start),
                len: UInt64(len),
                prefixHash: prefixHash,
                quoteHash: quoteHash,
                noteText: noteText
            )
            error = nil
            return id
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    /// Returns all annotations for one item, newest first, via
    /// `GistCore.listAnnotationsForItem`. Like `listItemsInCollection`/
    /// `listItemsByTag`, this doesn't publish into a shared `@Published`
    /// property -- callers (`FlowReaderContainer`'s `AnnotationState`) hold
    /// the result as their own state, since only one document's annotations
    /// are ever being browsed at a time.
    func listAnnotations(itemId: String) async -> [AnnotationVM] {
        guard let core else { return [] }
        do {
            let ffiAnnotations = try core.listAnnotationsForItem(itemId: itemId)
            error = nil
            return ffiAnnotations.map { AnnotationVM(ffi: $0) }
        } catch {
            self.error = "\(error)"
            return []
        }
    }

    /// Updates an annotation's note text (a `.note`'s real body, or a
    /// `.highlight`'s colour encoding -- see `HighlightColor`'s doc
    /// comment) via `GistCore.updateAnnotationNote`. Never touches the
    /// anchor fields (`blockId`/`start`/`len`/hashes) -- re-anchoring is out
    /// of scope here, matching the Rust method's own doc comment.
    func updateAnnotationNote(id: String, noteText: String?) async {
        guard let core else { return }
        do {
            try core.updateAnnotationNote(id: id, noteText: noteText)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// Deletes a single annotation via `GistCore.deleteAnnotation`.
    func deleteAnnotation(id: String) async {
        guard let core else { return }
        do {
            try core.deleteAnnotation(id: id)
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// Deletes one or more annotations; unknown ids are silently skipped
    /// (mirrors `removeItems`'s multi-id semantics). Returns the number
    /// actually deleted.
    @discardableResult
    func deleteAnnotations(ids: [String]) async -> Int {
        guard let core else { return 0 }
        do {
            let count = try core.deleteAnnotations(ids: ids)
            error = nil
            return Int(count)
        } catch {
            self.error = "\(error)"
            return 0
        }
    }

    /// Re-verifies/re-anchors every stored annotation for `itemId` against
    /// the document's *current* content (ADR-003) via
    /// `GistCore.reanchorAnnotations`. A `.reanchored` result has already
    /// been rewritten and persisted server-side by the time this returns; a
    /// `.orphaned` one is left completely untouched in the store. Matches
    /// the FFI doc comment's stated call pattern: call this when opening a
    /// reading view, before rendering highlights/notes/bookmarks --
    /// `AnnotationState.reload` (`FlowDocumentModel.swift`) does exactly
    /// that on every load. Returns `[]` (with `error` set) on failure,
    /// same convention as `listAnnotations`.
    func reanchorAnnotations(itemId: String) async -> [AnnotationAnchorResult] {
        guard let core else { return [] }
        do {
            let ffiResults = try core.reanchorAnnotations(itemId: itemId)
            error = nil
            return ffiResults.map(AnnotationAnchorResult.init(ffi:))
        } catch {
            self.error = "\(error)"
            return []
        }
    }

    // MARK: - Per-item encryption (ADR-014)

    /// Retroactively encrypts `ids` at rest, on demand -- the per-item,
    /// opt-in follow-up to ADR-011's whole-store encryption. Constructs a
    /// real, production `KeychainKeyProvider()` by default (the same key
    /// `.shared`'s `init()` itself uses for its own read-only decryption
    /// capability -- see that initializer's doc comment); a test can
    /// override `keyProvider` via `KeychainKeyProvider`'s existing
    /// `init(service:account:)` seam so it never touches the real
    /// production Keychain item. This call reaches the SAME `GistCore`
    /// instance `.shared` already has; only the specific items in `ids` are
    /// affected -- items not in `ids` are untouched, and new imports
    /// elsewhere in the app still land plaintext by default.
    ///
    /// **Fixed (ADR-014):** `.shared`'s `GistCore` now has genuine
    /// read-only decryption capability (`GistCore.newWithReadKey`, see
    /// `CoreClient.init()`), so an item this method encrypts *can* have its
    /// actual content read back through this same running app --
    /// `loadDocument`/`startRsvp` on that item's id succeed as long as the
    /// key `keyProvider` returns matches the one `.shared` was opened with
    /// (true by default, since both default to the same production
    /// `KeychainKeyProvider()`). Before this fix, `.shared` had no key
    /// provider of any kind and encrypting an item permanently locked it
    /// out of being reopened in the running app -- a real data-access bug,
    /// not just a documented limitation. See ADR-014 and this method's
    /// Rust-side counterparts (`gist_store::Store::encrypt_item`,
    /// `gist_store::Store::open_with_read_key`, `gist_core::Core::
    /// encrypt_items`) for the full explanation.
    ///
    /// Always calls `refresh()` afterward so `items`' `contentEncrypted`
    /// flags reflect the outcome, then returns a tallied summary for a
    /// result alert.
    @discardableResult
    func encryptItems(
        ids: [String],
        keyProvider: KeyProvider = KeychainKeyProvider()
    ) async -> EncryptItemsSummary {
        guard let core else {
            return EncryptItemsSummary(encryptedCount: 0, alreadyEncryptedCount: 0, failedCount: 0)
        }
        var encrypted = 0
        var alreadyEncrypted = 0
        var failed = 0
        do {
            let results = try core.encryptItems(ids: ids, keyProvider: keyProvider)
            for result in results {
                if let outcome = result.outcome {
                    switch outcome {
                    case .encrypted: encrypted += 1
                    case .alreadyEncrypted: alreadyEncrypted += 1
                    }
                } else {
                    failed += 1
                }
            }
            error = nil
        } catch {
            self.error = "\(error)"
            failed = ids.count
        }
        await reloadItems(clearErrorOnSuccess: false)
        return EncryptItemsSummary(
            encryptedCount: encrypted,
            alreadyEncryptedCount: alreadyEncrypted,
            failedCount: failed
        )
    }

    // MARK: - OCR import (role R8)

    /// Commits a multi-page OCR scan via `GistCore.importImageWithOcr`.
    /// `engine` is expected to be a `ReviewedOcrEngine` (see
    /// VisionOcrEngine.swift), built from the OCR review screen's
    /// already-recognized -- and possibly user-edited -- per-page text and
    /// confidence: unlike `importFile`/`importUrl`, there is no separate
    /// "commit, then find out something's wrong" step for content itself,
    /// since the review screen already resolved that client-side before
    /// this is ever called. There's also no DRM branch here (OCR imports
    /// have no DRM concept) -- any `GistError` case (most notably the
    /// ADR-009-addendum-2 byte-size cap surfacing as `.Core`) is returned as
    /// a plain, presentable failure message, matching this codebase's
    /// standing rule that a typed error from this path is real and must be
    /// shown, never silently swallowed.
    func importScannedDocument(pagePaths: [String], engine: OcrEngine) async -> OcrImportOutcome {
        guard let core else {
            return .failure(String(localized: "GIST core is not available."))
        }
        do {
            let result = try core.importImageWithOcr(paths: pagePaths, engine: engine)
            error = nil
            await reloadItems(clearErrorOnSuccess: false)
            return .success(itemId: result.itemId, pageConfidences: result.pageConfidences)
        } catch let gistError as GistError {
            let message = "\(gistError)"
            self.error = message
            return .failure(message)
        } catch {
            self.error = "\(error)"
            return .failure("\(error)")
        }
    }

    /// Persists the current token index so playback can resume later.
    func saveProgress(itemId: String, tokenIndex: Int) async {
        guard let core else { return }
        do {
            try core.saveProgress(itemId: itemId, tokenIndex: UInt64(tokenIndex))
        } catch {
            self.error = "\(error)"
        }
    }
}

struct LibraryItemVM: Identifiable {
    let id: String
    let title: String
    let authors: [String]
    let sourcePath: String?
    /// Mirrors `FfiLibraryItem.contentEncrypted` (ADR-011/014) -- whether
    /// this item's `.json`/`.tokens.json` blobs are encrypted at rest.
    /// Defaulted so every existing call site (production mapping and the
    /// many hand-built `LibraryItemVM(...)` literals in tests) keeps
    /// compiling unchanged.
    let contentEncrypted: Bool

    init(id: String, title: String, authors: [String], sourcePath: String?, contentEncrypted: Bool = false) {
        self.id = id
        self.title = title
        self.authors = authors
        self.sourcePath = sourcePath
        self.contentEncrypted = contentEncrypted
    }
}

/// Summary of a bulk `CoreClient.encryptItems` call, for a result alert
/// (e.g. "2 items encrypted, 1 was already encrypted") -- mirrors
/// `FfiEncryptItemResult`'s per-id outcomes, tallied.
struct EncryptItemsSummary {
    let encryptedCount: Int
    let alreadyEncryptedCount: Int
    let failedCount: Int

    var isEmpty: Bool { encryptedCount == 0 && alreadyEncryptedCount == 0 && failedCount == 0 }

    /// A short, human-readable summary line, e.g. "2 items encrypted, 1 was
    /// already encrypted, 1 failed." Only mentions the parts that happened.
    // (R5b localisation) Built via string interpolation/concatenation, not
    // `Text("literal")`, so none of this reaches the catalog automatically.
    // Each fragment is wrapped individually with `String(localized:)` --
    // scaffolding only (see this file's top note): true plural-aware
    // grammar (String Catalog's `%#@format@` plural variables) isn't set
    // up here, since English is the only shipping locale for v1.0 and each
    // singular/plural variant is already spelled out explicitly.
    var message: String {
        var parts: [String] = []
        if encryptedCount > 0 {
            parts.append(String(localized: "\(encryptedCount) item\(encryptedCount == 1 ? "" : "s") encrypted"))
        }
        if alreadyEncryptedCount > 0 {
            parts.append(
                String(
                    localized: "\(alreadyEncryptedCount) \(alreadyEncryptedCount == 1 ? "was" : "were") already encrypted"
                )
            )
        }
        if failedCount > 0 {
            parts.append(String(localized: "\(failedCount) failed"))
        }
        return parts.isEmpty
            ? String(localized: "No items were selected.")
            : parts.joined(separator: ", ") + "."
    }
}

/// Summary of a bulk `CoreClient.removeItems` call, for a warning alert.
/// Deliberately carries only `filesFailedCount` beyond the removed count --
/// `FfiRemoveOutcome.filesMissing`/`sharedCopiesKept` are bookkeeping, never
/// worth surfacing as a problem (see `FfiRemoveOutcome`'s doc comment).
struct RemoveItemsSummary {
    let removedCount: Int
    let filesFailedCount: Int

    /// Whether this summary is worth showing an alert for at all. A clean
    /// removal (the common case) should not interrupt the user with a
    /// dialog just to say "it worked."
    var hasFailures: Bool { filesFailedCount > 0 }

    /// e.g. "1 file for the removed item could not be deleted (something
    /// else may have it open). It has been reclaimed from your library --
    /// GIST will retry deleting the leftover file automatically."
    // (R5b localisation) See `EncryptItemsSummary.message`'s note above --
    // same reasoning applies here.
    var message: String {
        String(
            localized: "\(filesFailedCount) file\(filesFailedCount == 1 ? "" : "s") for the removed item\(removedCount == 1 ? "" : "s") could not be deleted (something else may have it open). It has been reclaimed from your library — GIST will retry deleting the leftover file\(filesFailedCount == 1 ? "" : "s") automatically."
        )
    }
}

/// Outcome of `CoreClient.importScannedDocument`. Returned as a value
/// (rather than only setting `core.error`, like most other `CoreClient`
/// methods) because `OcrImportState.commit` drives its own
/// `OcrImportPhase` state machine and needs to distinguish "committed
/// successfully" from "failed" without depending on `core.error` as a side
/// channel that some other concurrent operation could also be writing to.
enum OcrImportOutcome {
    case success(itemId: String, pageConfidences: [Float])
    case failure(String)
}

struct CollectionVM: Identifiable, Hashable {
    let id: String
    let name: String
    let createdAt: Int64
}
