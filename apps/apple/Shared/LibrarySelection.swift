import Foundation

/// What the sidebar has selected, driving which content `ContentView`'s
/// detail pane shows -- the full library, or a single collection's
/// contents. This is additive to (and independent of) the existing
/// `navigationPath: [String]` that `ContentView` already owns: that array
/// only ever holds item ids pushed onto the detail `NavigationStack` to open
/// `RsvpView`, i.e. it drives *pushes on top of* whichever root this
/// selection picks. Equatable/Hashable are synthesized component-wise via
/// `CollectionVM`'s own conformance, so two `.collection` cases are equal
/// only when every field of the wrapped `CollectionVM` matches, not just
/// its `id` -- see `GISTTests.testLibrarySelectionEquality`.
enum LibrarySelection: Hashable {
    case all
    case collection(CollectionVM)

    /// Title for the detail pane's navigation bar.
    // (R5b localisation) `.collection`'s branch returns the user's own
    // collection name (data, not translatable); only the `.all` fallback is
    // a fixed UI string, so only that one is wrapped.
    var title: String {
        switch self {
        case .all:
            return String(localized: "Library")
        case .collection(let collection):
            return collection.name
        }
    }
}
