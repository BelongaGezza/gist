import SwiftUI
import Combine

// ── Session models ──────────────────────────────────────────────────────────
//
// Client-side mirror of `gist_rsvp::RsvpSession`'s JSON shape. Decoded with
// `.convertFromSnakeCase`, so these use the same field names as the Rust
// struct. `state`, `elapsed_at_pause`, and `session_words_shown` aren't
// needed by this client-driven playback loop and are intentionally left out
// — Swift's synthesized `Decodable` ignores unrecognised JSON keys.

struct RsvpSessionVM: Decodable {
    let config: RsvpConfigVM
    let tokens: [TokenVM]
    let cursor: Int
}

struct RsvpConfigVM: Decodable {
    let wpm: UInt32
    let pauseSentence: Double
    let pauseComma: Double
    let pauseParagraph: Double
    let pauseNumeral: Double
    let chunkSize: Int
}

enum TokenKindVM: String, Decodable {
    case word = "Word"
    case paragraphBreak = "ParagraphBreak"
    case sectionBreak = "SectionBreak"
}

struct TokenVM: Decodable {
    let text: String
    let kind: TokenKindVM
}

// ── Scrub/seek + back-words pure math ───────────────────────────────────────

/// Pure fraction↔index conversion for a scrub/seek slider over the token
/// stream. Kept separate from `RsvpPlayer` (which owns *time*, not
/// *position* math) so it's trivially unit-testable, matching this
/// codebase's `LibraryFiltering`/`ReadingProgress` convention.
enum RsvpScrubMath {
    /// Maps a slider fraction (clamped to 0...1) onto a token index in
    /// `0..<tokenCount`. `tokenCount == 0` always yields `0`.
    static func index(forFraction fraction: Double, tokenCount: Int) -> Int {
        guard tokenCount > 0 else { return 0 }
        guard tokenCount > 1 else { return 0 }
        let clampedFraction = min(max(fraction, 0), 1)
        let idx = Int((clampedFraction * Double(tokenCount - 1)).rounded())
        return min(max(idx, 0), tokenCount - 1)
    }

    /// Inverse of `index(forFraction:tokenCount:)`, for driving a slider's
    /// displayed position from the current token index. `tokenCount <= 1`
    /// always yields `0` (nothing to scrub across).
    static func fraction(forIndex index: Int, tokenCount: Int) -> Double {
        guard tokenCount > 1 else { return 0 }
        let clamped = min(max(index, 0), tokenCount - 1)
        return Double(clamped) / Double(tokenCount - 1)
    }
}


// ── Session stats ────────────────────────────────────────────────────────────

/// A point-in-time snapshot of session progress for a small stats readout:
/// words read so far, elapsed play time (excluding paused gaps, backed by
/// the engine's `statsAtElapsed`), and the reading pace actually
/// achieved (which can differ from the target `wpm` once punctuation pauses
/// and back-word rewinds are factored in). Pure data computed on demand from
/// `RsvpPlayer`'s live state — not its own `@Published` property, since
/// nothing needs it to redraw independently of the position tick that
/// already republishes `currentIndex` every token boundary.
struct RsvpSessionStats {
    let wordsShown: Int
    let elapsedMs: UInt64
    let achievedWpm: Double

    /// "3:42"-style mm:ss formatting for the stats readout.
    var formattedElapsed: String {
        let totalSeconds = Int(elapsedMs / 1000)
        return String(format: "%d:%02d", totalSeconds / 60, totalSeconds % 60)
    }
}

// ── Rotary dial math ─────────────────────────────────────────────────────────

/// Pure geometry/mapping math backing the retired rotary dial's drag gesture, kept
/// separate from the view so it's unit-testable without driving a live
/// `DragGesture`, matching this codebase's pure-logic convention.
enum RotaryDialMath {
    /// Angle in degrees (clockwise-positive, 0 = due north/up) of `point`
    /// relative to `center`, using standard SwiftUI/AppKit view coordinates
    /// (y increases downward) — so visually dragging clockwise from the top
    /// yields an increasing angle. Range: `(-180, 180]`.
    static func angleDegrees(from center: CGPoint, to point: CGPoint) -> Double {
        let dx = Double(point.x - center.x)
        let dy = Double(point.y - center.y)
        return atan2(dx, -dy) * 180 / .pi
    }

    /// Shortest signed angular delta (degrees) from `from` to `to`,
    /// handling the -180/180 wraparound so a drag crossing that seam
    /// doesn't jump to the opposite sign.
    static func angularDelta(from: Double, to: Double) -> Double {
        var delta = to - from
        while delta > 180 { delta -= 360 }
        while delta < -180 { delta += 360 }
        return delta
    }

    /// Maps a relative rotation (degrees, positive = clockwise) applied to
    /// a starting wpm into a new value clamped to the legal 100...1000
    /// range — mirrors `RsvpSession::set_wpm`'s Rust-side clamp.
    /// `wpmPerDegree` controls sensitivity (default covers the full
    /// 900-wpm range over roughly one 300-degree drag).
    static func wpm(startingFrom base: UInt32, rotatedByDegrees degrees: Double, wpmPerDegree: Double = 3.0) -> UInt32 {
        let raw = Double(base) + degrees * wpmPerDegree
        return UInt32(min(max(raw, 100), 1000).rounded())
    }
}

// ── Playback engine ──────────────────────────────────────────────────────────

/// Drives RSVP playback from a live `FfiRsvpSession` -- the Rust pacing
/// engine (`gist_rsvp::RsvpSession`) itself, not a Swift copy of it. All
/// pacing arithmetic (per-token durations, punctuation pauses, ORP split,
/// back-words, session stats) happens in Rust; this class only owns the
/// *clock* and the SwiftUI-facing published state.
///
/// **Wall-clock anchoring.** `resumeDate` is the instant playback last
/// resumed. Every tick asks the engine for `frameAtElapsed(now - resumeDate)`
/// and sleeps until the frame's `nextBoundaryMs`, so a late or coalesced
/// wake lands on the token actually due now instead of compounding drift.
///
/// **Mutate-while-playing.** `seek`/`backWords`/`setWpm`/the punctuation
/// toggle do not bank elapsed play time in the engine (only `pause` does),
/// so every mutation here goes through `mutate`: `pause(elapsed)` → mutate
/// at elapsed 0 → `resume()` → re-anchor `resumeDate`. Without that,
/// `statsAtElapsed` would under-count time before the mutation.
///
/// **On `CVDisplayLink` (investigated 2026-09-26, decision unchanged):**
/// `Task.sleep` is backed by a high-resolution timer far tighter than a
/// 16.6 ms frame; SwiftUI still has to composite whatever a display-link
/// callback hands it; and drift is solved at the model level, so a late wake
/// shows the correct word slightly late rather than a wrong word. A manual
/// `CVDisplayLink` would add a second timing lifecycle for an unmeasured,
/// likely imperceptible gain. Revisit only if hands-on testing at 800–1000
/// WPM shows visible per-word jitter.
@MainActor
final class RsvpPlayer: ObservableObject {
    @Published private(set) var isLoaded = false
    @Published private(set) var currentIndex = 0
    @Published private(set) var isPlaying = false
    @Published private(set) var wpm: UInt32 = 250
    /// Whether punctuation-based pacing pauses (sentence/comma/numeral) are
    /// applied. Paragraph/section-break pauses are structural and always
    /// apply -- enforced by the Rust engine (`Config::pause_on_punctuation`).
    @Published private(set) var punctuationPauseEnabled = true
    /// The frame for `currentIndex`: token text, kind and its pre-split ORP.
    @Published private(set) var currentFrame: FfiRsvpFrame?

    private var session: FfiRsvpSession?
    private(set) var tokenCount = 0
    /// Wall-clock instant playback last resumed from; `nil` while paused.
    private var resumeDate: Date?
    private var tickTask: Task<Void, Never>?
    private var core: CoreClient?
    private var itemId: String?

    var progressText: String? {
        guard tokenCount > 0 else { return nil }
        return "\(currentIndex + 1) / \(tokenCount)"
    }

    /// Fraction (0...1) of the document consumed so far, for a scrub
    /// slider's displayed position.
    var scrubFraction: Double {
        RsvpScrubMath.fraction(forIndex: currentIndex, tokenCount: tokenCount)
    }

    /// A snapshot of session progress for the stats readout. Not
    /// `@Published`: nothing needs it to redraw independently of the
    /// position tick that already republishes `currentIndex`.
    var stats: RsvpSessionStats {
        guard let session,
              let s = try? session.statsAtElapsed(elapsedMs: elapsedMs(at: Date()))
        else { return RsvpSessionStats(wordsShown: 0, elapsedMs: 0, achievedWpm: 0) }
        return RsvpSessionStats(
            wordsShown: Int(s.wordsShown), elapsedMs: s.durationMs, achievedWpm: Double(s.estimatedWpm)
        )
    }

    func load(
        core: CoreClient, itemId: String, initialWpm: UInt32 = 250,
        initialPunctuationPauseEnabled: Bool = true
    ) async {
        guard session == nil else { return }
        self.core = core
        self.itemId = itemId
        guard let opened = await core.openRsvpSession(itemId: itemId, wpm: initialWpm) else { return }
        do {
            try opened.setPauseOnPunctuation(enabled: initialPunctuationPauseEnabled, elapsedMs: 0)
            tokenCount = Int(try opened.tokenCount())
            wpm = try opened.wpm()
            punctuationPauseEnabled = initialPunctuationPauseEnabled
            session = opened
            refreshPosition()
        } catch {
            core.error = "\(error)"
            return
        }
        isLoaded = true
        // ADR-021: stamp "last read" once per open, fire-and-forget (a
        // failure must never interrupt reading; saveProgress is unaffected).
        Task { await core.markItemOpened(itemId: itemId) }
    }

    func play() {
        guard !isPlaying, let session, tokenCount > 0, currentIndex < tokenCount - 1 else { return }
        guard (try? session.resume()) != nil else { return }
        isPlaying = true
        resumeDate = Date()
        scheduleNextTick()
    }

    func pause() {
        guard isPlaying else { return }
        isPlaying = false
        tickTask?.cancel()
        tickTask = nil
        if let session {
            try? session.pause(elapsedMs: elapsedMs(at: Date()))
        }
        resumeDate = nil
        refreshPosition()
    }

    /// Change reading speed mid-session. The engine pins the position under
    /// the *old* speed before applying the new one, so a speed change never
    /// jumps the reader. Clamped to 100–1000 by the core as well.
    func setWpm(_ newWpm: UInt32) {
        let clamped = min(max(newWpm, 100), 1000)
        guard clamped != wpm else { return }
        guard let session else {
            wpm = clamped
            return
        }
        mutate { try session.setWpm(wpm: clamped, elapsedMs: 0) }
        wpm = (try? session.wpm()) ?? clamped
    }

    /// Toggles punctuation-based pacing pauses; pins the position under the
    /// old setting first, like `setWpm`.
    func setPunctuationPauseEnabled(_ enabled: Bool) {
        guard enabled != punctuationPauseEnabled else { return }
        guard let session else {
            punctuationPauseEnabled = enabled
            return
        }
        mutate { try session.setPauseOnPunctuation(enabled: enabled, elapsedMs: 0) }
        punctuationPauseEnabled = enabled
    }

    /// Seeks to a specific token index -- shared by scrub/seek.
    func seek(toIndex idx: Int) {
        guard let session, tokenCount > 0 else { return }
        let clamped = UInt64(min(max(idx, 0), tokenCount - 1))
        mutate { try session.seek(index: clamped) }
    }

    /// Seeks to a fraction (0...1) of the document -- backs a scrub slider.
    func seek(toFraction fraction: Double) {
        guard tokenCount > 0 else { return }
        seek(toIndex: RsvpScrubMath.index(forFraction: fraction, tokenCount: tokenCount))
    }

    /// Jumps back `n` word tokens (paragraph/section breaks skipped, not
    /// counted); done by the engine. Defaults to 5 ("back 5 words").
    func backWords(_ n: Int = 5) {
        guard let session else { return }
        mutate { try session.backWords(n: UInt64(max(n, 0)), elapsedMs: 0) }
    }

    func stopAndPersist() async {
        pause()
        guard let core, let itemId else { return }
        await core.saveProgress(itemId: itemId, tokenIndex: currentIndex)
    }

    // MARK: Internals

    /// Milliseconds of play since `resumeDate`; `0` while paused.
    private func elapsedMs(at now: Date) -> UInt64 {
        guard let resumeDate else { return 0 }
        // `.rounded()`: a 0.400 s interval can materialise as
        // 0.39999999999999997 and would otherwise truncate to 399 ms.
        return UInt64((max(0, now.timeIntervalSince(resumeDate)) * 1000).rounded())
    }

    /// Applies an engine mutation that takes its position at elapsed 0:
    /// while playing, bank the elapsed time with `pause`, mutate, `resume`,
    /// and re-anchor the clock; while paused, just mutate.
    private func mutate(_ body: () throws -> Void) {
        guard let session else { return }
        let wasPlaying = isPlaying
        if wasPlaying {
            try? session.pause(elapsedMs: elapsedMs(at: Date()))
            resumeDate = nil
        }
        do { try body() } catch { core?.error = "\(error)" }
        if wasPlaying {
            try? session.resume()
            resumeDate = Date()
        }
        refreshPosition()
        if wasPlaying { scheduleNextTick() }
    }

    /// Re-reads the frame at the engine's cursor (elapsed 0) -- used
    /// whenever playback is not mid-run.
    private func refreshPosition() {
        guard let session, let frame = try? session.frameAtElapsed(elapsedMs: 0) else { return }
        currentFrame = frame
        currentIndex = Int(frame.index)
    }

    /// Recomputes the frame from actual elapsed wall-clock time on every
    /// wake, then sleeps exactly until the frame's `nextBoundaryMs`.
    private func scheduleNextTick() {
        tickTask?.cancel()
        tickTask = Task { [weak self] in
            while let self, self.isPlaying, let session = self.session {
                let elapsed = self.elapsedMs(at: Date())
                guard let frame = try? session.frameAtElapsed(elapsedMs: elapsed) else { return }
                self.currentFrame = frame
                self.currentIndex = Int(frame.index)
                if frame.isLast {
                    self.pause()
                    return
                }
                let sleepMs = max(frame.nextBoundaryMs > elapsed ? frame.nextBoundaryMs - elapsed : 1, 1)
                try? await Task.sleep(nanoseconds: sleepMs * 1_000_000)
                if Task.isCancelled { return }
            }
        }
    }
}

// ── Views ────────────────────────────────────────────────────────────────────

/// Renders one word with its ORP pivot character emphasized (colored with
/// the current theme's accent) to anchor eye fixation — the standard RSVP
/// presentation technique. Uses a monospaced design specifically so the
/// prefix/suffix frame widths below keep the pivot roughly centered across
/// words of different lengths, which a proportional font can't guarantee.
struct OrpWordView: View {
    /// Pre-split by the Rust engine (`FfiOrpSplit`) -- the pivot is chosen in
    /// core, never recomputed in Swift.
    let split: FfiOrpSplit
    let theme: Theme
    let fontSize: CGFloat

    private var word: String { split.before + split.focus + split.after }

    var body: some View {
        HStack(spacing: 0) {
            Text(split.before)
                .frame(width: fontSize * 5, alignment: .trailing)
            Text(split.focus)
                .foregroundStyle(theme.accent)
            Text(split.after)
                .frame(width: fontSize * 5, alignment: .leading)
        }
        .font(.system(size: fontSize, weight: .regular, design: .monospaced))
        .foregroundStyle(theme.foreground)
        // Fixed height: an empty token (paragraph/section break) renders no
        // glyphs, which would otherwise collapse the row and make the page jump.
        .frame(height: fontSize * 1.5)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(word)
    }
}

/// The reading-speed range offered by the RSVP controls. The core accepts
/// 100–1000 wpm; the UI deliberately offers the practical 200–700 band.
enum RsvpSpeedRange {
    static let min: UInt32 = 200
    static let max: UInt32 = 700
}

/// A slider for reading speed (`RsvpSpeedRange`), in 5-wpm steps. Replaced
/// the rotary dial, whose drag lost direction when the pointer crossed the
/// dial. Adjustable by VoiceOver like any native slider.
struct WpmSliderView: View {
    let wpm: UInt32
    let theme: Theme
    let onChange: (UInt32) -> Void

    var body: some View {
        Slider(
            value: Binding(
                get: { Double(min(max(wpm, RsvpSpeedRange.min), RsvpSpeedRange.max)) },
                // Snap to 5 wpm here rather than via `Slider(step:)`, which draws a
                // row of tick marks under the track.
                set: { onChange(UInt32((($0 / 5).rounded() * 5))) }
            ),
            in: Double(RsvpSpeedRange.min)...Double(RsvpSpeedRange.max)
        )
        .controlSize(.small)
        .tint(theme.accent)
        .frame(width: 180)
        .accessibilityLabel("Reading speed")
        .accessibilityValue("\(wpm) words per minute")
    }
}

/// An accessible stepper alternative to the rotary dial: discrete +/-
/// buttons plus a numeric readout. A custom control (not SwiftUI's system
/// `Stepper`), so `accessibilityAdjustableAction` is mandatory here too, per
/// the product spec's accessibility requirement — VoiceOver users can
/// adjust it with a swipe-up/down gesture exactly like a native stepper,
/// while sighted/mouse users get two explicit, individually-clickable
/// buttons.
struct WpmStepperView: View {
    let wpm: UInt32
    let step: UInt32
    let onChange: (UInt32) -> Void

    var body: some View {
        HStack(spacing: 16) {
            Button {
                onChange(max(wpm - step, RsvpSpeedRange.min))
            } label: {
                Image(systemName: "minus.circle")
            }
            .accessibilityLabel("Decrease reading speed")

            Text("\(wpm) WPM")
                .monospacedDigit()
                .frame(minWidth: 90)

            Button {
                onChange(min(wpm + step, RsvpSpeedRange.max))
            } label: {
                Image(systemName: "plus.circle")
            }
            .accessibilityLabel("Increase reading speed")
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Reading speed stepper")
        .accessibilityValue("\(wpm) words per minute")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment:
                onChange(min(wpm + step, RsvpSpeedRange.max))
            case .decrement:
                onChange(max(wpm - step, RsvpSpeedRange.min))
            @unknown default:
                break
            }
        }
    }
}

struct RsvpView: View {
    let itemId: String
    /// Lets "Continue in Flow View" push `.flow(itemId:)` onto the same
    /// `NavigationStack` this view was itself pushed onto — see
    /// `ContentView`'s `navigationDestination(for: ReadingDestination.self)`
    /// closure, which is the only other place `ReadingDestination` values
    /// are appended.
    @Binding var navigationPath: [ReadingDestination]
    @EnvironmentObject var core: CoreClient
    @EnvironmentObject var themeManager: ThemeManager
    @StateObject private var player = RsvpPlayer()
    @State private var controlsHovered = false

    /// RSVP is the one place OLED "true black" matters most -- a word
    /// display lit against a black background during a reading session --
    /// so the theme's background is applied full-bleed here.
    private var theme: Theme { themeManager.resolvedTheme }

    var body: some View {
        Group {
            if player.isLoaded {
                playbackContent
            } else {
                ProgressView("Loading…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(theme.background.ignoresSafeArea())
        .navigationTitle("RSVP")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    exitToFlowView()
                } label: {
                    Label("Continue in Flow View", systemImage: "doc.text")
                }
                .disabled(!player.isLoaded)
            }
        }
        .task {
            // Settings scene's RSVP tab defaults -- see `RsvpDefaults
            // .defaultWpm`/`.pauseOnPunctuation`'s doc comments; fall back to
            // the same 250/true this used to hardcode when nothing has been
            // persisted yet.
            await player.load(
                core: core, itemId: itemId,
                initialWpm: UInt32(RsvpDefaults.shared.defaultWpm),
                initialPunctuationPauseEnabled: RsvpDefaults.shared.pauseOnPunctuation
            )
        }
        .onDisappear { Task { await player.stopAndPersist() } }
    }

    /// Persists progress, then replaces this RSVP screen with the flow view
    /// for the same item on the shared `NavigationStack` — "exit to flow
    /// view" reads as swapping reading modes, not stacking a second reader
    /// on top of the first.
    private func exitToFlowView() {
        Task { await player.stopAndPersist() }
        if !navigationPath.isEmpty {
            navigationPath.removeLast()
        }
        navigationPath.append(.flow(itemId: itemId))
    }

    /// The word owns the screen; every control lives in one compact, dimmed
    /// bar underneath that fades back while playing (full opacity on hover or
    /// when paused) so it doesn't compete with the words being read.
    private var playbackContent: some View {
        VStack(spacing: 0) {
            Spacer(minLength: 0)
            OrpWordView(
                split: player.currentFrame?.orp ?? FfiOrpSplit(before: "", focus: "", after: ""),
                theme: theme, fontSize: 56
            )
            .frame(maxWidth: .infinity)
            Spacer(minLength: 0)

            controlBar
                .opacity(player.isPlaying && !controlsHovered ? 0.3 : 1)
                .animation(.easeInOut(duration: 0.2), value: player.isPlaying)
                .animation(.easeInOut(duration: 0.2), value: controlsHovered)
                .onHover { controlsHovered = $0 }
                .padding(.bottom, 16)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.background)
    }

    private var controlBar: some View {
        VStack(spacing: 8) {
            Slider(
                value: Binding(
                    get: { player.scrubFraction },
                    set: { player.seek(toFraction: $0) }
                ),
                in: 0...1
            )
            .controlSize(.small)
            .tint(theme.accent)
            .frame(width: 320)
            .accessibilityLabel("Seek position in document")

            HStack(spacing: 20) {
                Button {
                    player.backWords(5)
                } label: {
                    Image(systemName: "gobackward.5")
                        .foregroundStyle(theme.foreground)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Back 5 words")

                Button {
                    if player.isPlaying { player.pause() } else { player.play() }
                } label: {
                    Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                        .font(.title3)
                        .foregroundStyle(theme.accent)
                }
                .buttonStyle(.plain)
                .keyboardShortcut(.space, modifiers: [])
                .accessibilityLabel(player.isPlaying ? "Pause" : "Play")

                WpmSliderView(wpm: player.wpm, theme: theme) { player.setWpm($0) }
                WpmStepperView(wpm: player.wpm, step: 25) { player.setWpm($0) }
                    .font(.caption)
            }

            Toggle(isOn: Binding(
                get: { player.punctuationPauseEnabled },
                set: { player.setPunctuationPauseEnabled($0) }
            )) {
                Text("Pause longer at punctuation")
                    .font(.caption)
            }
            .toggleStyle(.switch)
            .controlSize(.mini)
            .tint(theme.accent)

            Text([player.progressText, sessionStatsLine].compactMap { $0 }.joined(separator: "  •  "))
                .font(.caption2)
                .foregroundStyle(theme.foreground.opacity(0.6))
                .monospacedDigit()
        }
    }

    // (R5b localisation) Read via `Text(sessionStatsLine)`, which takes the
    // returned value, not a literal -- wrap so "Words:"/"Time:"/"Pace:"/
    // "wpm" reach the catalog. The numeric values themselves need no
    // translation.
    private var sessionStatsLine: String {
        let stats = player.stats
        return String(
            localized: "Words: \(stats.wordsShown) • Time: \(stats.formattedElapsed) • Pace: \(Int(stats.achievedWpm.rounded())) wpm"
        )
    }
}
