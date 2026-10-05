import AppKit
import SwiftUI

/// Applies `.searchFocused` only where it exists. `View.searchFocused(_:)` is
/// macOS 15+ (the SDK annotation says so; an older source comment claiming
/// "macOS/iOS 17+" was wrong). The app's minimum is macOS 14 (D1, 2026-10-04),
/// so on 14 the modifier is skipped and the Cmd+F shortcut falls back to
/// `SearchFieldLocator` below.
struct SearchFocusedIfAvailable: ViewModifier {
    var isFocused: FocusState<Bool>.Binding

    @ViewBuilder
    func body(content: Content) -> some View {
        if #available(macOS 15, *) {
            content.searchFocused(isFocused)
        } else {
            content
        }
    }
}

/// macOS 14 fallback for the Cmd+F "focus the library search field" shortcut:
/// finds the `NSSearchField` that `.searchable` installs in the window's
/// toolbar and makes it first responder. Best-effort and NOT verified on a
/// real macOS 14 machine (compile-verified only); if no field is found the
/// shortcut is a harmless no-op.
enum SearchFieldLocator {
    @MainActor
    static func focusSearchField() {
        guard let window = NSApp.keyWindow,
              let root = window.contentView?.superview,
              let field = firstSearchField(in: root) else { return }
        window.makeFirstResponder(field)
    }

    private static func firstSearchField(in view: NSView) -> NSSearchField? {
        if let field = view as? NSSearchField { return field }
        for sub in view.subviews {
            if let found = firstSearchField(in: sub) { return found }
        }
        return nil
    }
}
