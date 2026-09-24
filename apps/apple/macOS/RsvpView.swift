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

// ── Wall-clock-anchored pacing engine ───────────────────────────────────────

/// Wall-clock-anchored RSVP pacing, mirroring `gist_rsvp::RsvpSession`'s
/// cursor/elapsed model (`token_duration_ms`, `token_at_elapsed`, `pause`,
/// `resume`, `set_wpm` in crates/gist-rsvp/src/lib.rs) as closely as
/// possible on the Swift side.
///
/// This fixes a known fidelity gap: the original port drove playback with a
/// sequential `Task.sleep`-per-token loop that trusted each sleep to be
/// exact and never re-checked against the wall clock, so any scheduling
/// jitter (or a delayed/coalesced `Task` wake) accumulated as drift over a
/// long session. This engine instead recomputes, on every tick, which token
/// index *should* be showing given actual elapsed wall-clock time since the
/// last `resume` — exactly what `token_at_elapsed` does on the Rust side —
/// so a late tick "catches up" instead of silently falling behind.
///
/// A plain value type (not `@State`/SwiftUI-view-coupled) so the pacing math
/// is unit-testable without driving a live view, matching this codebase's
/// existing convention for `LibraryFiltering`/`ReadingProgress`/
/// `LibrarySortOrder`. It takes wall-clock time as an explicit `at:`
/// parameter rather than reading `Date()` itself, so tests can simulate
/// delayed ticks and pause/resume gaps deterministically.
///
/// Keep `tokenDurationMs`/`tokenAtElapsed` and the punctuation/numeral
/// helpers below in sync with `crates/gist-rsvp/src/lib.rs` if pacing rules
/// change there — there is no per-tick FFI call, so this is a hand-ported
/// duplicate of that logic, not a live call-through.
struct RsvpWallClockEngine {
    /// Index into the token stream marking where the current play window
    /// starts — mirrors `RsvpSession::cursor`.
    private(set) var cursor: Int

    /// The wall-clock instant playback last resumed from, or `nil` while
    /// paused — mirrors `RsvpSession::state` (`Playing` iff non-nil).
    private(set) var resumeDate: Date?

    init(cursor: Int) {
        self.cursor = cursor
        self.resumeDate = nil
    }

    var isPlaying: Bool { resumeDate != nil }

    /// Sentinel `remainingMs` returned while paused, so a caller never
    /// mistakes "not currently running a clock" for "the next token is due
    /// in 0ms" and busy-loops.
    static let notPlayingRemainingMs = UInt64.max

    /// The index that should be displayed right now, and how many
    /// milliseconds remain before the next token boundary (`0` once the
    /// last token is reached; `notPlayingRemainingMs` while paused).
    ///
    /// Mirrors `RsvpSession::token_at_elapsed`, called with elapsed time
    /// measured from `resumeDate` to `now` rather than from a
    /// caller-tracked counter.
    func currentIndex(
        tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, at now: Date
    ) -> (index: Int, remainingMs: UInt64) {
        guard let resumeDate else {
            return (min(cursor, max(tokens.count - 1, 0)), Self.notPlayingRemainingMs)
        }
        let elapsedMs = Self.elapsedMs(from: resumeDate, to: now)
        return Self.tokenAtElapsed(tokens: tokens, config: config, wpm: wpm, cursor: cursor, elapsedMs: elapsedMs)
    }

    /// Pause playback at `now`, pinning `cursor` to wherever elapsed time
    /// actually put us — mirrors `RsvpSession::pause`.
    mutating func pause(tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, at now: Date) {
        guard let resumeDate else { return }
        let elapsedMs = Self.elapsedMs(from: resumeDate, to: now)
        cursor = Self.tokenAtElapsed(tokens: tokens, config: config, wpm: wpm, cursor: cursor, elapsedMs: elapsedMs).index
        self.resumeDate = nil
    }

    /// Resume playback, anchoring the elapsed clock to `now` — mirrors
    /// `RsvpSession::resume` (paired with the caller resetting its own
    /// elapsed counter, which here is just `resumeDate`).
    mutating func resume(at now: Date) {
        resumeDate = now
    }

    /// Change wpm mid-playback without breaking pacing continuity: pin
    /// `cursor` to the index computed under the *old* wpm as of `now`, then
    /// reset the elapsed clock to `now` — mirrors `RsvpSession::set_wpm`.
    /// A no-op while paused, since `cursor` already reflects the paused
    /// position and there is no running clock to reset.
    mutating func setWpm(tokens: [TokenVM], config: RsvpConfigVM, oldWpm: UInt32, at now: Date) {
        guard let resumeDate else { return }
        let elapsedMs = Self.elapsedMs(from: resumeDate, to: now)
        cursor = Self.tokenAtElapsed(tokens: tokens, config: config, wpm: oldWpm, cursor: cursor, elapsedMs: elapsedMs).index
        self.resumeDate = now
    }

    private static func elapsedMs(from start: Date, to now: Date) -> UInt64 {
        UInt64(max(0, now.timeIntervalSince(start)) * 1000)
    }

    // MARK: Pure pacing — ported from RsvpSession::token_duration_ms / token_at_elapsed

    /// Duration in milliseconds that token `idx` should be displayed —
    /// ported from `RsvpSession::token_duration_ms`
    /// (crates/gist-rsvp/src/lib.rs). Keep in sync with the Rust source if
    /// pacing rules change there.
    static func tokenDurationMs(tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, idx: Int) -> UInt64 {
        guard tokens.indices.contains(idx) else { return 0 }
        let clampedWpm = min(max(wpm, 100), 1000)
        let baseMs = 60_000 / UInt64(clampedWpm)
        let token = tokens[idx]

        let multiplier: Double
        switch token.kind {
        case .paragraphBreak, .sectionBreak:
            multiplier = config.pauseParagraph
        case .word:
            let text = token.text
            if isNumeral(text) {
                let punct = endsSentence(text) ? config.pauseSentence
                    : (endsClause(text) ? config.pauseComma : 1.0)
                multiplier = max(config.pauseNumeral, punct)
            } else if endsSentence(text) {
                multiplier = config.pauseSentence
            } else if endsClause(text) {
                multiplier = config.pauseComma
            } else {
                multiplier = 1.0
            }
        }
        return UInt64((Double(baseMs) * multiplier).rounded())
    }

    /// Walk forward from `cursor` summing token durations (recomputed at
    /// the *current* `wpm` every call, never cached) until the accumulated
    /// duration exceeds `elapsedMs` — ported from
    /// `RsvpSession::token_at_elapsed`. This is what makes the engine
    /// wall-clock-anchored: a late or coalesced tick still lands on the
    /// token actually due right now instead of drifting behind by however
    /// much a chain of sequential sleeps has overrun.
    static func tokenAtElapsed(
        tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, cursor: Int, elapsedMs: UInt64
    ) -> (index: Int, remainingMs: UInt64) {
        guard !tokens.isEmpty else { return (0, 0) }
        var accumulated: UInt64 = 0
        var idx = max(cursor, 0)
        while idx < tokens.count {
            let dur = tokenDurationMs(tokens: tokens, config: config, wpm: wpm, idx: idx)
            if accumulated + dur > elapsedMs {
                return (idx, accumulated + dur - elapsedMs)
            }
            accumulated += dur
            idx += 1
        }
        // Clamp to the last token.
        return (max(tokens.count - 1, 0), 0)
    }

    private static func endsSentence(_ text: String) -> Bool {
        let trimmed = text.reversed().drop {
            !($0.isLetter || $0.isNumber) && $0 != "." && $0 != "!" && $0 != "?"
        }
        guard let last = trimmed.first else { return false }
        return last == "." || last == "!" || last == "?"
    }

    private static func endsClause(_ text: String) -> Bool {
        guard let last = text.last else { return false }
        return last == "," || last == ";" || last == ":"
    }

    private static func isNumeral(_ text: String) -> Bool {
        !text.isEmpty
            && text.allSatisfy { ($0.isASCII && $0.isNumber) || $0 == "." || $0 == "," }
            && text.contains { $0.isASCII && $0.isNumber }
    }
}

// ── Playback engine ──────────────────────────────────────────────────────────

/// Drives RSVP playback client-side from a session fetched once via FFI.
/// `gist_rsvp::RsvpSession` is a pure, timer-free pacing engine by design
/// (see that crate's doc comment: "the SwiftUI shell drives it... by calling
/// token_at_elapsed on every frame") — this is that shell half, built on
/// `RsvpWallClockEngine` above so playback stays anchored to actual elapsed
/// wall-clock time rather than a chain of sequential sleeps.
@MainActor
final class RsvpPlayer: ObservableObject {
    @Published private(set) var isLoaded = false
    @Published private(set) var currentIndex = 0
    @Published private(set) var isPlaying = false
    @Published private(set) var wpm: UInt32 = 250

    private var session: RsvpSessionVM?
    private var engine = RsvpWallClockEngine(cursor: 0)
    private var tickTask: Task<Void, Never>?
    private var core: CoreClient?
    private var itemId: String?

    var currentToken: TokenVM? {
        guard let session, session.tokens.indices.contains(currentIndex) else { return nil }
        return session.tokens[currentIndex]
    }

    var progressText: String? {
        guard let session, !session.tokens.isEmpty else { return nil }
        return "\(currentIndex + 1) / \(session.tokens.count)"
    }

    func load(core: CoreClient, itemId: String, initialWpm: UInt32 = 250) async {
        guard session == nil else { return }
        self.core = core
        self.itemId = itemId
        guard let loaded = await core.startRsvp(itemId: itemId, wpm: initialWpm) else { return }
        session = loaded
        wpm = loaded.config.wpm
        let startIndex = min(loaded.cursor, max(loaded.tokens.count - 1, 0))
        currentIndex = startIndex
        engine = RsvpWallClockEngine(cursor: startIndex)
        isLoaded = true
    }

    func play() {
        guard !isPlaying, let session, currentIndex < session.tokens.count - 1 else { return }
        isPlaying = true
        engine.resume(at: Date())
        scheduleNextTick()
    }

    func pause() {
        guard isPlaying else { return }
        isPlaying = false
        tickTask?.cancel()
        tickTask = nil
        guard let session else { return }
        let now = Date()
        engine.pause(tokens: session.tokens, config: session.config, wpm: wpm, at: now)
        currentIndex = engine.currentIndex(tokens: session.tokens, config: session.config, wpm: wpm, at: now).index
    }

    /// Change reading speed mid-session. Mirrors `RsvpSession::set_wpm`:
    /// pins the pacing engine's cursor to wherever elapsed time (computed
    /// under the *old* wpm) actually put us, then resets the elapsed clock
    /// — so a token already partway through its display under the old
    /// speed doesn't silently get a new, different duration retroactively,
    /// and a speed change mid-tick doesn't cause the next tick to overshoot
    /// or stall.
    func setWpm(_ newWpm: UInt32) {
        let clamped = min(max(newWpm, 100), 1000)
        guard clamped != wpm else { return }
        guard let session else {
            wpm = clamped
            return
        }
        let now = Date()
        engine.setWpm(tokens: session.tokens, config: session.config, oldWpm: wpm, at: now)
        wpm = clamped
        currentIndex = engine.currentIndex(tokens: session.tokens, config: session.config, wpm: wpm, at: now).index
        if isPlaying {
            scheduleNextTick()
        }
    }

    func stopAndPersist() async {
        pause()
        guard let core, let itemId else { return }
        await core.saveProgress(itemId: itemId, tokenIndex: currentIndex)
    }

    /// Recomputes the display index from actual elapsed wall-clock time on
    /// every wake, then sleeps exactly until the next token boundary rather
    /// than a fixed tick interval. Because the index is always re-derived
    /// from `resumeDate` (not accumulated from prior sleeps), a wake that
    /// arrives late — a delayed/coalesced `Task`, a paused debugger, a
    /// slow simulator — self-corrects on the very next iteration instead of
    /// compounding drift over the session.
    private func scheduleNextTick() {
        tickTask?.cancel()
        tickTask = Task { [weak self] in
            while let self, self.isPlaying, let session = self.session {
                let now = Date()
                let (idx, remainingMs) = self.engine.currentIndex(
                    tokens: session.tokens, config: session.config, wpm: self.wpm, at: now
                )
                guard remainingMs != RsvpWallClockEngine.notPlayingRemainingMs else { return }
                self.currentIndex = idx
                if idx >= session.tokens.count - 1 {
                    self.pause()
                    return
                }
                let sleepMs = max(remainingMs, 1)
                try? await Task.sleep(nanoseconds: sleepMs * 1_000_000)
                if Task.isCancelled { return }
            }
        }
    }
}

// ── View ──────────────────────────────────────────────────────────────────────

struct RsvpView: View {
    let itemId: String
    @EnvironmentObject var core: CoreClient
    @EnvironmentObject var themeManager: ThemeManager
    @StateObject private var player = RsvpPlayer()

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
        .task { await player.load(core: core, itemId: itemId) }
        .onDisappear { Task { await player.stopAndPersist() } }
    }

    private var playbackContent: some View {
        VStack(spacing: 32) {
            // Word display.
            Text(player.currentToken?.text ?? "")
                .font(.system(size: 48, weight: .regular, design: .serif))
                .foregroundStyle(theme.foreground)
                .frame(maxWidth: .infinity)
                .padding()

            if let progressText = player.progressText {
                Text(progressText)
                    .font(.caption)
                    .foregroundStyle(theme.foreground.opacity(0.6))
                    .monospacedDigit()
            }

            // WPM control
            VStack {
                Text("\(player.wpm) WPM")
                    .font(.caption)
                    .foregroundStyle(theme.foreground.opacity(0.6))
                    .monospacedDigit()
                Slider(
                    value: Binding(
                        get: { Double(player.wpm) },
                        set: { player.setWpm(UInt32($0.rounded())) }
                    ),
                    in: 100...1000,
                    step: 10
                )
                .tint(theme.accent)
                .frame(width: 200)
                .accessibilityLabel("Reading speed")
            }

            // Play/pause
            Button {
                if player.isPlaying {
                    player.pause()
                } else {
                    player.play()
                }
            } label: {
                Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                    .font(.title)
                    .foregroundStyle(theme.accent)
            }
            .keyboardShortcut(.space, modifiers: [])
            .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.background)
    }
}
