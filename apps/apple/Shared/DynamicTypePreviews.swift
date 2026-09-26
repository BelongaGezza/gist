import SwiftUI

// ── Dynamic Type verification previews (role R6, development-plan-v2.md §3.9 item 3) ──
//
// This dev environment has no way to drive a running app interactively (no
// Accessibility permission for `osascript`/System Events -- the same
// limitation CLAUDE.md already documents for every other manual-click-
// through gap in this project), so a real "does this actually look right at
// accessibility text sizes on screen" check isn't possible here. `#Preview`
// with a `.environment(\.dynamicTypeSize, ...)` override is the next best,
// genuinely-available thing: it's real SwiftUI environment injection (not a
// simulation of one), and it's real enough that Xcode's own Canvas renders
// it. It is **not** executed or asserted on by `xcodebuild build`/`test` --
// those only type-check/compile this file like any other Swift source, they
// do not render or screenshot the preview. So this file's actual, honest
// verification value is: (1) confirms these views compile cleanly against
// `.dynamicTypeSize`/`.sizeCategory` environment overrides at all (a
// genuinely broken layout expression could fail to compile at an unusual
// size in rare cases, though that's not the common failure mode), and (2)
// gives a person with Xcode's Canvas -- or, better, a real device with a
// real accessibility text size set -- one obvious place to actually look.
// Neither of those is a substitute for someone actually looking at a
// rendered accessibility-size layout; see this role's final report for the
// explicit "still needs a person" list.
//
// Deliberately placed in its own new file rather than added inline to
// `LibraryView.swift`/`SettingsView.swift`/`RsvpView.swift` themselves --
// those three files are also being edited concurrently by a parallel
// localisation pass (role R5b) in a separate worktree; a new file with no
// overlapping line ranges is much easier to reconcile at integration time
// than inline edits to their `body`s would have been.
//
// What this deliberately does *not* try to cover: `RsvpView`/
// `FlowViewSwiftUINative`'s own reading typography (`RotaryDialView`,
// `OrpWordView`, and Flow View's `TypographySettings.fontSize`-driven body
// text) is intentionally decoupled from the OS's Dynamic Type setting --
// both already expose their own dedicated, app-level font-size control
// (the RSVP WPM/size controls' sibling "Aa" typography menu in Flow View,
// and `TypographySettings.range`), the same "reading apps get their own
// font-size slider, separate from system Dynamic Type" convention Apple
// Books/Kindle use. That is a deliberate, pre-existing design choice, not a
// gap this pass introduces or needs to fix -- verified by inspection of
// `Theme.swift`/`RsvpView.swift`/`FlowDocumentModel.swift`'s typography
// types, which is why this file's coverage focuses on the app's *chrome*
// (Library/Settings/collections/tags/annotations), where semantic text
// styles (`.headline`, `.body`, `.caption`, ...) are used throughout and
// genuinely do participate in Dynamic Type scaling.

#Preview("Library — Default Type Size") {
    LibraryView(navigationPath: .constant([]))
        .environmentObject(CoreClient.shared)
        .environmentObject(ThemeManager.shared)
}

#Preview("Library — Accessibility XXXL") {
    LibraryView(navigationPath: .constant([]))
        .environmentObject(CoreClient.shared)
        .environmentObject(ThemeManager.shared)
        .environment(\.sizeCategory, .accessibilityExtraExtraExtraLarge)
}

#Preview("Settings — Default Type Size") {
    SettingsView()
}

#Preview("Settings — Accessibility XXXL") {
    SettingsView()
        .environment(\.sizeCategory, .accessibilityExtraExtraExtraLarge)
}

#Preview("Theme Settings — Accessibility XXXL") {
    ThemeSettingsView()
        .environmentObject(ThemeManager.shared)
        .environment(\.sizeCategory, .accessibilityExtraExtraExtraLarge)
}

#Preview("Tag Editor — Accessibility XXXL") {
    TagEditorView(itemId: "preview-item", itemTitle: "Preview Book Title")
        .environmentObject(CoreClient.shared)
        .environment(\.sizeCategory, .accessibilityExtraExtraExtraLarge)
}

/// RSVP's accessible transport controls specifically -- these are custom
/// controls (not system `Slider`/`Stepper`), so it's worth confirming their
/// *labels* (which are plain `Text`/`Label`, not the deliberately-decoupled
/// reading-typography pixel sizes) still read reasonably at a large type
/// size, independent of the rest of `RsvpView` (which needs a live
/// `CoreClient`/`itemId` round trip to render at all).
#Preview("RSVP WPM Stepper — Accessibility XXXL") {
    WpmStepperView(wpm: 250, step: 25) { _ in }
        .padding()
        .environment(\.sizeCategory, .accessibilityExtraExtraExtraLarge)
}
