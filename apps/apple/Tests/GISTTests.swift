import XCTest
@testable import GIST

final class GISTTests: XCTestCase {
    func testPlaceholder() throws {
        XCTAssertTrue(true)
    }

    func testLibrarySelectionTitles() {
        XCTAssertEqual(LibrarySelection.all.title, "Library")

        let collection = CollectionVM(id: "c1", name: "Sci-Fi", createdAt: 0)
        XCTAssertEqual(LibrarySelection.collection(collection).title, "Sci-Fi")
    }

    /// `LibrarySelection`'s `Hashable`/`Equatable` conformance is synthesized
    /// via `CollectionVM`'s own conformance, which compares *every* stored
    /// field -- not just `id`. That matters because `SidebarView`'s
    /// `List(selection:)` matches selection state against row `.tag(...)`
    /// values by equality: if this ever silently degraded to an id-only
    /// comparison (e.g. by hand-rolling `Equatable` on `CollectionVM` later),
    /// a renamed collection could compare equal to its stale pre-rename
    /// value and the sidebar could show wrong content. Two collections
    /// sharing an id but differing in name should NOT compare equal today.
    func testLibrarySelectionEqualityIsComponentWise() {
        let a = CollectionVM(id: "same-id", name: "A", createdAt: 1)
        let b = CollectionVM(id: "same-id", name: "B", createdAt: 1)

        XCTAssertNotEqual(LibrarySelection.collection(a), LibrarySelection.collection(b))
        XCTAssertEqual(LibrarySelection.collection(a), LibrarySelection.collection(a))
        XCTAssertNotEqual(LibrarySelection.all, LibrarySelection.collection(a))
    }

    func testLibrarySelectionUsableAsSetElement() {
        // Sanity check that Hashable actually works end-to-end (not just
        // Equatable), since SidebarView relies on hashing for List(selection:).
        let collection = CollectionVM(id: "c1", name: "Sci-Fi", createdAt: 0)
        let selections: Set<LibrarySelection> = [.all, .collection(collection), .all]
        XCTAssertEqual(selections.count, 2)
    }

    func testTagEditorTargetIdentity() {
        // .sheet(item:) identifies the presented sheet by `id` alone, so two
        // targets sharing an id must be treated as "the same sheet" even if
        // other fields differ.
        let t1 = TagEditorTarget(id: "item-1", title: "Foo")
        let t2 = TagEditorTarget(id: "item-1", title: "Bar")
        XCTAssertEqual(t1.id, t2.id)
        XCTAssertNotEqual(t1, t2)
    }
}
