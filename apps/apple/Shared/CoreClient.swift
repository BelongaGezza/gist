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
            self.error = "Failed to initialise GIST core: \(error)"
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
            self.error = "Failed to initialise GIST core: \(error)"
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
            self.error = "Failed to initialise GIST core: \(error)"
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
                title: item.title ?? "Untitled",
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
    func removeItems(ids: [String], deleteSourceFiles: Bool) async {
        guard let core else { return }
        do {
            try core.removeItems(ids: ids, deleteSourceFiles: deleteSourceFiles)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        await reloadItems(clearErrorOnSuccess: false)
    }

    /// Imports a web page by URL via `Core::import_url`, following the same
    /// DRM/error-handling shape as `importFile` (DRM is unreachable in
    /// practice for a web-fetch import, but the shared `GistError` type
    /// still carries the case, so we still branch on it for consistency).
    func importUrl(urlString: String) async {
        guard let core else { return }
        drmProtectedFile = nil
        do {
            _ = try core.importUrl(url: urlString)
            error = nil
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
            _ = try core.importFile(path: url.path)
            error = nil
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
    var message: String {
        var parts: [String] = []
        if encryptedCount > 0 {
            parts.append("\(encryptedCount) item\(encryptedCount == 1 ? "" : "s") encrypted")
        }
        if alreadyEncryptedCount > 0 {
            parts.append("\(alreadyEncryptedCount) \(alreadyEncryptedCount == 1 ? "was" : "were") already encrypted")
        }
        if failedCount > 0 {
            parts.append("\(failedCount) failed")
        }
        return parts.isEmpty ? "No items were selected." : parts.joined(separator: ", ") + "."
    }
}

struct CollectionVM: Identifiable, Hashable {
    let id: String
    let name: String
    let createdAt: Int64
}
