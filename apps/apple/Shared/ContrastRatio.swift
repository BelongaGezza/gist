import SwiftUI
#if os(macOS)
import AppKit
#endif

/// WCAG 2.x relative-luminance / contrast-ratio math (role R6,
/// development-plan-v2.md §3.9 item 4), used to verify `Theme.swift`'s
/// per-theme foreground/background/accent color pairs actually meet a
/// reasonable text-contrast bar rather than just eyeballing them. Pure
/// arithmetic over sRGB channel values -- no view hierarchy, no rendering --
/// so it's exactly and deterministically unit-testable (see
/// `AccessibilityTests.swift`).
///
/// Reference: https://www.w3.org/TR/WCAG21/#dfn-relative-luminance /
/// https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio.
enum ContrastRatio {
    /// Linearizes one sRGB channel value (`0...1`) per the WCAG formula --
    /// the "gamma-decode" step relative luminance is defined over, not raw
    /// sRGB.
    private static func linearize(_ channel: Double) -> Double {
        channel <= 0.03928 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
    }

    /// WCAG relative luminance (`0...1`) of an sRGB color given as
    /// `0...1` red/green/blue components.
    static func relativeLuminance(red: Double, green: Double, blue: Double) -> Double {
        0.2126 * linearize(red) + 0.7152 * linearize(green) + 0.0722 * linearize(blue)
    }

    /// WCAG contrast ratio (`1...21`) between two relative luminances --
    /// symmetric, argument order doesn't matter.
    static func ratio(luminance l1: Double, _ l2: Double) -> Double {
        let lighter = max(l1, l2)
        let darker = min(l1, l2)
        return (lighter + 0.05) / (darker + 0.05)
    }

    /// Convenience: contrast ratio directly from two sRGB triples.
    static func ratio(
        r1: Double, g1: Double, b1: Double,
        r2: Double, g2: Double, b2: Double
    ) -> Double {
        ratio(
            luminance: relativeLuminance(red: r1, green: g1, blue: b1),
            relativeLuminance(red: r2, green: g2, blue: b2)
        )
    }

    #if os(macOS)
    /// Extracts sRGB (`0...1`) components from a SwiftUI `Color` via
    /// `NSColor` bridging, so tests check the *actual* pixel values
    /// `Theme.swift`'s `Color(red:green:blue:)` literals resolve to, rather
    /// than independently re-typed magic numbers in the test file that could
    /// silently drift out of sync with `Theme.swift` itself. Returns `nil`
    /// only if the color can't be converted into the sRGB color space at
    /// all (not expected for any of `Theme.swift`'s literal colors).
    static func srgbComponents(of color: Color) -> (red: Double, green: Double, blue: Double)? {
        guard let converted = NSColor(color).usingColorSpace(.sRGB) else { return nil }
        return (Double(converted.redComponent), Double(converted.greenComponent), Double(converted.blueComponent))
    }

    /// Contrast ratio between two SwiftUI `Color`s, resolved via
    /// `srgbComponents(of:)`. `nil` only if either color fails to convert.
    static func ratio(_ a: Color, _ b: Color) -> Double? {
        guard let ca = srgbComponents(of: a), let cb = srgbComponents(of: b) else { return nil }
        return ratio(r1: ca.red, g1: ca.green, b1: ca.blue, r2: cb.red, g2: cb.green, b2: cb.blue)
    }
    #endif

    /// WCAG AA threshold for normal-size text.
    static let minimumTextContrast = 4.5
    /// WCAG AA threshold for UI components/graphical objects (SC 1.4.11) --
    /// looser than body text, appropriate for a control's fill/border rather
    /// than something rendering actual letterforms.
    static let minimumUiComponentContrast = 3.0
}
