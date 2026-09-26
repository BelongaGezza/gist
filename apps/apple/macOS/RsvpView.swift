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

// ── ORP (Optimal Recognition Point) ─────────────────────────────────────────

/// Splits a word into the text before, at, and after its ORP pivot
/// character, so the pivot can be rendered distinctly (typically colored)
/// to anchor eye fixation — the standard RSVP presentation technique.
///
/// Ported from `gist_rsvp::orp_index` (crates/gist-rsvp/src/lib.rs): target
/// position ≈ 30% into the word; prefer a vowel at or after that position,
/// otherwise use the character at that index. Keep in sync with the Rust
/// source if that rule changes.
///
/// Deliberately operates in `Character` (grapheme-cluster) space rather than
/// porting Rust's byte-offset return value verbatim — Swift's `Text` views
/// split on `Character` boundaries anyway, so there is no equivalent "byte
/// index" this code needs to produce; what matters is landing on the same
/// pivot *character*, which this does identically to the Rust rule for all
/// the cases the Rust unit tests cover.
enum OrpCalculator {
    private static let vowels: Set<Character> = ["a", "e", "i", "o", "u", "A", "E", "I", "O", "U"]

    /// Character index of the ORP pivot within `word`. Returns `0` for an
    /// empty string (mirroring the Rust function's early return).
    static func pivotCharIndex(word: String) -> Int {
        let chars = Array(word)
        guard !chars.isEmpty else { return 0 }
        let target = min(Int((Double(chars.count) * 0.30).rounded()), chars.count - 1)
        for i in target..<chars.count where vowels.contains(chars[i]) {
            return i
        }
        return target
    }

    struct Split {
        let prefix: String
        let pivot: String
        let suffix: String
    }

    /// Splits `word` into (prefix, pivot character, suffix). All three are
    /// empty for an empty `word`.
    static func split(_ word: String) -> Split {
        let chars = Array(word)
        guard !chars.isEmpty else { return Split(prefix: "", pivot: "", suffix: "") }
        let idx = pivotCharIndex(word: word)
        let prefix = String(chars[0..<idx])
        let pivot = String(chars[idx])
        let suffix = idx + 1 < chars.count ? String(chars[(idx + 1)...]) : ""
        return Split(prefix: prefix, pivot: pivot, suffix: suffix)
    }
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
///
/// **On `CVDisplayLink` vs. this state-driven approach (investigated as
/// part of the RSVP-view polish pass, 2026-09-26):** at 1000 WPM a plain
/// word displays for ~60ms, only a handful of frames at 60Hz, and a naive
/// `Timer`-driven redraw loop can visibly jitter at that rate. That is a
/// **different** problem from the drift this engine already fixed: drift is
/// "is the model showing the *correct* word for elapsed time," which is
/// fully solved above by recomputing from the wall clock on every tick;
/// jitter is "does the *frame* carrying that correct word hit the screen on
/// a predictable cadence." Decision: **keep SwiftUI's `@Published`-driven
/// redraw off this engine; do not add a manual `CVDisplayLink` integration.**
/// Reasoning:
///   1. `Task.sleep(nanoseconds:)` is backed by a high-resolution dispatch
///      timer, accurate to well under a millisecond — far tighter than the
///      ~16.6ms/frame budget that would be the actual limiting factor
///      either way, whether driven by `Task.sleep` or a `CVDisplayLink`
///      callback.
///   2. AppKit/SwiftUI already composites and presents frames on the
///      system's own vsync-driven pipeline internally; a manual
///      `CVDisplayLink` callback would still have to hand its result to
///      SwiftUI's state system (there is no lower-level "skip SwiftUI and
///      paint directly" path available to a `Text`-based view), so it
///      would not shorten the state-change → composited-frame path that
///      actually determines when pixels change.
///   3. Because drift is fixed at the model level, a late/coalesced wake
///      still shows the *correct* word — the residual risk from staying
///      with `Task.sleep` is at most "this frame's content is right but
///      arrived a fraction of a frame later than ideal," not "the wrong
///      word is shown" or "durations compound-drift over a session." That
///      residual is not perceptible at normal reading distances/rates.
///   4. A manual `CVDisplayLink` integration is AppKit-only (this file is
///      already macOS-only, so that's not disqualifying by itself, but it
///      is real added complexity: a second timing lifecycle to start/stop
///      in lockstep with play/pause/backgrounding, bridged back into
///      SwiftUI's state system) for a gain that is unmeasured and, per (1)
///      and (2), not expected to be perceptible.
/// Revisit only if real hands-on testing at 800–1000 WPM surfaces visible
/// per-word jitter that the above reasoning doesn't predict.
struct RsvpWallClockEngine {
    /// Index into the token stream marking where the current play window
    /// starts — mirrors `RsvpSession::cursor`.
    private(set) var cursor: Int

    /// The wall-clock instant playback last resumed from, or `nil` while
    /// paused — mirrors `RsvpSession::state` (`Playing` iff non-nil).
    private(set) var resumeDate: Date?

    /// Total milliseconds spent actually playing (elapsed time while
    /// `resumeDate` was non-nil), accumulated across every past
    /// pause/resume/seek/wpm-change cycle but excluding paused gaps —
    /// mirrors `RsvpSession::elapsed_at_pause`, which this codebase's
    /// session-stats readout (`RsvpSessionStats`) is built on. Unlike
    /// discarding and rebuilding a fresh `RsvpWallClockEngine`, mutating
    /// methods below fold each cycle's elapsed time into this rather than
    /// losing it, so seeking/back-words/wpm/punctuation-toggle changes
    /// don't silently reset session stats.
    private(set) var accumulatedPlayMs: UInt64 = 0

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
    /// caller-tracked counter. `punctuationPauseEnabled` defaults to `true`
    /// (the original, only) behavior, so every pre-existing call site
    /// keeps compiling and behaving identically.
    func currentIndex(
        tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, at now: Date,
        punctuationPauseEnabled: Bool = true
    ) -> (index: Int, remainingMs: UInt64) {
        guard let resumeDate else {
            return (min(cursor, max(tokens.count - 1, 0)), Self.notPlayingRemainingMs)
        }
        let elapsedMs = Self.elapsedMs(from: resumeDate, to: now)
        return Self.tokenAtElapsed(
            tokens: tokens, config: config, wpm: wpm, cursor: cursor, elapsedMs: elapsedMs,
            punctuationPauseEnabled: punctuationPauseEnabled
        )
    }

    /// Pause playback at `now`, pinning `cursor` to wherever elapsed time
    /// actually put us — mirrors `RsvpSession::pause`. Also folds the
    /// elapsed time since the last resume into `accumulatedPlayMs`.
    mutating func pause(
        tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, at now: Date,
        punctuationPauseEnabled: Bool = true
    ) {
        guard let resumeDate else { return }
        let elapsedMs = Self.elapsedMs(from: resumeDate, to: now)
        cursor = Self.tokenAtElapsed(
            tokens: tokens, config: config, wpm: wpm, cursor: cursor, elapsedMs: elapsedMs,
            punctuationPauseEnabled: punctuationPauseEnabled
        ).index
        accumulatedPlayMs += elapsedMs
        self.resumeDate = nil
    }

    /// Resume playback, anchoring the elapsed clock to `now` — mirrors
    /// `RsvpSession::resume` (paired with the caller resetting its own
    /// elapsed counter, which here is just `resumeDate`).
    mutating func resume(at now: Date) {
        resumeDate = now
    }

    /// Change wpm mid-playback without breaking pacing continuity: pin
    /// `cursor` to the index computed under the *old* wpm/punctuation
    /// settings as of `now`, fold that elapsed span into
    /// `accumulatedPlayMs`, then reset the elapsed clock to `now` — mirrors
    /// `RsvpSession::set_wpm`. A no-op while paused, since `cursor` already
    /// reflects the paused position and there is no running clock to reset.
    ///
    /// Also doubles as the repin primitive for a mid-playback
    /// punctuation-pause toggle (`RsvpPlayer.setPunctuationPauseEnabled`):
    /// "wpm changed" and "punctuation-pause setting changed" are both just
    /// "a pacing rule changed, repin under the old rule then restart the
    /// clock," so passing the *old* `punctuationPauseEnabled` value here
    /// (default `true`, matching every pre-existing call site) does the
    /// right thing for either kind of change.
    mutating func setWpm(
        tokens: [TokenVM], config: RsvpConfigVM, oldWpm: UInt32, at now: Date,
        punctuationPauseEnabled: Bool = true
    ) {
        guard let resumeDate else { return }
        let elapsedMs = Self.elapsedMs(from: resumeDate, to: now)
        cursor = Self.tokenAtElapsed(
            tokens: tokens, config: config, wpm: oldWpm, cursor: cursor, elapsedMs: elapsedMs,
            punctuationPauseEnabled: punctuationPauseEnabled
        ).index
        accumulatedPlayMs += elapsedMs
        self.resumeDate = now
    }

    /// Seek playback to `idx` — shared by scrub/seek and "back N words"
    /// (both are just a jump to a computed token index, mirroring how
    /// `RsvpSession::seek`/`back_words` both just reposition `cursor` on
    /// the Rust side). Unlike discarding and rebuilding a fresh
    /// `RsvpWallClockEngine(cursor:)`, this preserves `accumulatedPlayMs`
    /// bookkeeping across the jump. If currently playing, the elapsed clock
    /// restarts at `now` so the next tick paces correctly from the new
    /// position; if paused, only the cursor moves.
    mutating func seek(toIndex idx: Int, tokenCount: Int, at now: Date) {
        let clamped = min(max(idx, 0), max(tokenCount - 1, 0))
        if let resumeDate {
            accumulatedPlayMs += Self.elapsedMs(from: resumeDate, to: now)
            self.resumeDate = now
        }
        cursor = clamped
    }

    /// Total time spent actually playing so far (excluding paused gaps),
    /// as of `now` — `accumulatedPlayMs` plus any still-running span since
    /// the last resume. Backs `RsvpSessionStats`'s elapsed-time readout.
    func totalElapsedMs(at now: Date) -> UInt64 {
        guard let resumeDate else { return accumulatedPlayMs }
        return accumulatedPlayMs + Self.elapsedMs(from: resumeDate, to: now)
    }

    private static func elapsedMs(from start: Date, to now: Date) -> UInt64 {
        // `.rounded()` before truncating to `UInt64` -- floating-point
        // representation error (e.g. a 0.400s interval materializing as
        // 0.39999999999999997) would otherwise truncate to 399ms instead of
        // 400ms. Rounding is also the semantically correct choice for "how
        // many milliseconds elapsed," not just a rounding-error workaround.
        UInt64((max(0, now.timeIntervalSince(start)) * 1000).rounded())
    }

    // MARK: Pure pacing — ported from RsvpSession::token_duration_ms / token_at_elapsed

    /// Duration in milliseconds that token `idx` should be displayed —
    /// ported from `RsvpSession::token_duration_ms`
    /// (crates/gist-rsvp/src/lib.rs). Keep in sync with the Rust source if
    /// pacing rules change there.
    ///
    /// `punctuationPauseEnabled` (default `true`, matching every
    /// pre-existing call site's behavior) gates only the *punctuation*
    /// pauses on word tokens (sentence/comma/numeral) — a paragraph/section
    /// break's pause is a structural pause, not punctuation, so it always
    /// applies regardless of this toggle.
    static func tokenDurationMs(
        tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, idx: Int,
        punctuationPauseEnabled: Bool = true
    ) -> UInt64 {
        guard tokens.indices.contains(idx) else { return 0 }
        let clampedWpm = min(max(wpm, 100), 1000)
        let baseMs = 60_000 / UInt64(clampedWpm)
        let token = tokens[idx]

        let multiplier: Double
        switch token.kind {
        case .paragraphBreak, .sectionBreak:
            multiplier = config.pauseParagraph
        case .word:
            if !punctuationPauseEnabled {
                multiplier = 1.0
            } else {
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
        tokens: [TokenVM], config: RsvpConfigVM, wpm: UInt32, cursor: Int, elapsedMs: UInt64,
        punctuationPauseEnabled: Bool = true
    ) -> (index: Int, remainingMs: UInt64) {
        guard !tokens.isEmpty else { return (0, 0) }
        var accumulated: UInt64 = 0
        var idx = max(cursor, 0)
        while idx < tokens.count {
            let dur = tokenDurationMs(
                tokens: tokens, config: config, wpm: wpm, idx: idx,
                punctuationPauseEnabled: punctuationPauseEnabled
            )
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

// ── Scrub/seek + back-words pure math ───────────────────────────────────────

/// Pure fraction↔index conversion for a scrub/seek slider over the token
/// stream. Kept separate from `RsvpWallClockEngine` (which owns *time*, not
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

/// Pure "jump back N words" index math, ported from
/// `RsvpSession::back_words` (crates/gist-rsvp/src/lib.rs): walk backward
/// from `from`, counting only `.word` tokens (paragraph/section breaks are
/// skipped, not counted), until `n` words have been skipped or the start of
/// the stream is reached.
enum RsvpBackWords {
    static func index(from: Int, n: Int, tokens: [TokenVM]) -> Int {
        guard !tokens.isEmpty else { return 0 }
        var target = min(max(from, 0), tokens.count - 1)
        var skipped = 0
        while target > 0 && skipped < n {
            target -= 1
            if tokens[target].kind == .word {
                skipped += 1
            }
        }
        return target
    }
}

// ── Session stats ────────────────────────────────────────────────────────────

/// A point-in-time snapshot of session progress for a small stats readout:
/// words read so far, elapsed play time (excluding paused gaps, backed by
/// `RsvpWallClockEngine.totalElapsedMs`), and the reading pace actually
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

/// Pure helpers backing `RsvpSessionStats`.
enum RsvpStats {
    /// Number of `.word` tokens strictly before `index` (i.e. already
    /// displayed) — paragraph/section breaks never count as a "word read."
    static func wordsShown(tokens: [TokenVM], upTo index: Int) -> Int {
        guard index > 0 else { return 0 }
        let end = min(index, tokens.count)
        return tokens[0..<end].filter { $0.kind == .word }.count
    }

    /// Words-per-minute actually achieved given `wordsShown` words in
    /// `elapsedMs` milliseconds of actual play time. `0` while no time has
    /// elapsed, rather than dividing by zero.
    static func achievedWpm(wordsShown: Int, elapsedMs: UInt64) -> Double {
        guard elapsedMs > 0 else { return 0 }
        return Double(wordsShown) / (Double(elapsedMs) / 60_000.0)
    }
}

// ── Rotary dial math ─────────────────────────────────────────────────────────

/// Pure geometry/mapping math backing `RotaryDialView`'s drag gesture, kept
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
    /// Whether punctuation-based pacing pauses (sentence/comma/numeral) are
    /// applied. Paragraph/section-break pauses are structural, not
    /// punctuation, and always apply regardless of this toggle — see
    /// `RsvpWallClockEngine.tokenDurationMs`.
    @Published private(set) var punctuationPauseEnabled = true

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

    var tokenCount: Int { session?.tokens.count ?? 0 }

    /// Fraction (0...1) of the document consumed so far, for a scrub
    /// slider's displayed position.
    var scrubFraction: Double {
        RsvpScrubMath.fraction(forIndex: currentIndex, tokenCount: tokenCount)
    }

    /// A snapshot of session progress for the stats readout. See
    /// `RsvpSessionStats`'s doc comment for why this isn't `@Published`.
    var stats: RsvpSessionStats {
        guard let session else { return RsvpSessionStats(wordsShown: 0, elapsedMs: 0, achievedWpm: 0) }
        let wordsShown = RsvpStats.wordsShown(tokens: session.tokens, upTo: currentIndex)
        let elapsedMs = engine.totalElapsedMs(at: Date())
        let achieved = RsvpStats.achievedWpm(wordsShown: wordsShown, elapsedMs: elapsedMs)
        return RsvpSessionStats(wordsShown: wordsShown, elapsedMs: elapsedMs, achievedWpm: achieved)
    }

    func load(
        core: CoreClient, itemId: String, initialWpm: UInt32 = 250,
        initialPunctuationPauseEnabled: Bool = true
    ) async {
        guard session == nil else { return }
        self.core = core
        self.itemId = itemId
        guard let loaded = await core.startRsvp(itemId: itemId, wpm: initialWpm) else { return }
        session = loaded
        wpm = loaded.config.wpm
        punctuationPauseEnabled = initialPunctuationPauseEnabled
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
        engine.pause(
            tokens: session.tokens, config: session.config, wpm: wpm, at: now,
            punctuationPauseEnabled: punctuationPauseEnabled
        )
        currentIndex = engine.currentIndex(
            tokens: session.tokens, config: session.config, wpm: wpm, at: now,
            punctuationPauseEnabled: punctuationPauseEnabled
        ).index
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
        engine.setWpm(
            tokens: session.tokens, config: session.config, oldWpm: wpm, at: now,
            punctuationPauseEnabled: punctuationPauseEnabled
        )
        wpm = clamped
        currentIndex = engine.currentIndex(
            tokens: session.tokens, config: session.config, wpm: wpm, at: now,
            punctuationPauseEnabled: punctuationPauseEnabled
        ).index
        if isPlaying {
            scheduleNextTick()
        }
    }

    /// Toggles punctuation-based pacing pauses. Mid-playback, this pins the
    /// cursor under the *old* setting before resetting the clock — the same
    /// repin-then-restart shape as `setWpm`, since both are just "a pacing
    /// rule changed" (see `RsvpWallClockEngine.setWpm`'s doc comment, which
    /// this reuses directly rather than duplicating the logic).
    func setPunctuationPauseEnabled(_ enabled: Bool) {
        guard enabled != punctuationPauseEnabled else { return }
        if let session, isPlaying {
            let now = Date()
            engine.setWpm(
                tokens: session.tokens, config: session.config, oldWpm: wpm, at: now,
                punctuationPauseEnabled: punctuationPauseEnabled
            )
            punctuationPauseEnabled = enabled
            currentIndex = engine.currentIndex(
                tokens: session.tokens, config: session.config, wpm: wpm, at: now,
                punctuationPauseEnabled: punctuationPauseEnabled
            ).index
            scheduleNextTick()
        } else {
            punctuationPauseEnabled = enabled
        }
    }

    /// Seeks to a specific token index — shared by scrub/seek and "back N
    /// words" (both are just a jump to a computed index). Preserves session
    /// stats bookkeeping via `RsvpWallClockEngine.seek`.
    func seek(toIndex idx: Int) {
        guard let session, !session.tokens.isEmpty else { return }
        let now = Date()
        engine.seek(toIndex: idx, tokenCount: session.tokens.count, at: now)
        currentIndex = engine.currentIndex(
            tokens: session.tokens, config: session.config, wpm: wpm, at: now,
            punctuationPauseEnabled: punctuationPauseEnabled
        ).index
        if isPlaying {
            scheduleNextTick()
        }
    }

    /// Seeks to a fraction (0...1) of the document — backs a scrub slider.
    func seek(toFraction fraction: Double) {
        guard let session, !session.tokens.isEmpty else { return }
        let idx = RsvpScrubMath.index(forFraction: fraction, tokenCount: session.tokens.count)
        seek(toIndex: idx)
    }

    /// Jumps back `n` word tokens (skipping paragraph/section breaks),
    /// mirroring `RsvpSession::back_words`. Defaults to 5, matching the
    /// "back 5 words" transport control.
    func backWords(_ n: Int = 5) {
        guard let session else { return }
        let target = RsvpBackWords.index(from: currentIndex, n: n, tokens: session.tokens)
        seek(toIndex: target)
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
    /// compounding drift over the session. See `RsvpWallClockEngine`'s doc
    /// comment for why this stays `Task.sleep`/`@Published`-state-driven
    /// rather than moving to a manual `CVDisplayLink` callback.
    private func scheduleNextTick() {
        tickTask?.cancel()
        tickTask = Task { [weak self] in
            while let self, self.isPlaying, let session = self.session {
                let now = Date()
                let (idx, remainingMs) = self.engine.currentIndex(
                    tokens: session.tokens, config: session.config, wpm: self.wpm, at: now,
                    punctuationPauseEnabled: self.punctuationPauseEnabled
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

// ── Views ────────────────────────────────────────────────────────────────────

/// Renders one word with its ORP pivot character emphasized (colored with
/// the current theme's accent) to anchor eye fixation — the standard RSVP
/// presentation technique. Uses a monospaced design specifically so the
/// prefix/suffix frame widths below keep the pivot roughly centered across
/// words of different lengths, which a proportional font can't guarantee.
struct OrpWordView: View {
    let word: String
    let theme: Theme
    let fontSize: CGFloat

    private var split: OrpCalculator.Split { OrpCalculator.split(word) }

    var body: some View {
        HStack(spacing: 0) {
            Text(split.prefix)
                .frame(width: fontSize * 5, alignment: .trailing)
            Text(split.pivot)
                .foregroundStyle(theme.accent)
            Text(split.suffix)
                .frame(width: fontSize * 5, alignment: .leading)
        }
        .font(.system(size: fontSize, weight: .regular, design: .monospaced))
        .foregroundStyle(theme.foreground)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(word)
    }
}

/// A rotary speed-dial control for WPM. Drag angle (relative to where the
/// drag started, via `RotaryDialMath`) maps to a wpm delta; the visual
/// indicator's rest position is a simple proportional sweep across the
/// legal 100...1000 range, purely cosmetic. `accessibilityAdjustableAction`
/// is mandatory here (this is a custom control, not a system one) per the
/// product spec's accessibility requirement.
struct RotaryDialView: View {
    let wpm: UInt32
    let theme: Theme
    let onChange: (UInt32) -> Void

    @State private var dragBaseWpm: UInt32?
    @State private var dragStartAngle: Double?

    private let diameter: CGFloat = 88
    private let step: UInt32 = 25

    var body: some View {
        ZStack {
            Circle()
                .strokeBorder(theme.foreground.opacity(0.25), lineWidth: 4)
            Circle()
                .fill(theme.accent)
                .frame(width: 10, height: 10)
                .offset(y: -(diameter / 2 - 10))
                .rotationEffect(.degrees(indicatorAngleDegrees))
            Image(systemName: "gauge.medium")
                .font(.title2)
                .foregroundStyle(theme.foreground.opacity(0.5))
        }
        .frame(width: diameter, height: diameter)
        .contentShape(Circle())
        .gesture(
            DragGesture(minimumDistance: 1)
                .onChanged { value in
                    let center = CGPoint(x: diameter / 2, y: diameter / 2)
                    if dragBaseWpm == nil {
                        dragBaseWpm = wpm
                        dragStartAngle = RotaryDialMath.angleDegrees(from: center, to: value.startLocation)
                    }
                    guard let base = dragBaseWpm, let startAngle = dragStartAngle else { return }
                    let currentAngle = RotaryDialMath.angleDegrees(from: center, to: value.location)
                    let delta = RotaryDialMath.angularDelta(from: startAngle, to: currentAngle)
                    onChange(RotaryDialMath.wpm(startingFrom: base, rotatedByDegrees: delta))
                }
                .onEnded { _ in
                    dragBaseWpm = nil
                    dragStartAngle = nil
                }
        )
        .accessibilityElement()
        .accessibilityLabel("Reading speed dial")
        .accessibilityValue("\(wpm) words per minute")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment:
                onChange(min(wpm + step, 1000))
            case .decrement:
                onChange(max(wpm - step, 100))
            @unknown default:
                break
            }
        }
    }

    /// Purely cosmetic: proportionally sweeps 270 degrees across the
    /// 100...1000 wpm range, starting at -135 degrees (bottom-left), the
    /// typical rotary-knob look. Not the drag math itself (see
    /// `RotaryDialMath`), just where the indicator dot rests for a given
    /// `wpm`.
    private var indicatorAngleDegrees: Double {
        let fraction = (Double(wpm) - 100) / 900
        return -135 + fraction * 270
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
                onChange(max(wpm - step, 100))
            } label: {
                Image(systemName: "minus.circle")
            }
            .accessibilityLabel("Decrease reading speed")

            Text("\(wpm) WPM")
                .monospacedDigit()
                .frame(minWidth: 90)

            Button {
                onChange(min(wpm + step, 1000))
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
                onChange(min(wpm + step, 1000))
            case .decrement:
                onChange(max(wpm - step, 100))
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

    private var playbackContent: some View {
        VStack(spacing: 20) {
            OrpWordView(word: player.currentToken?.text ?? "", theme: theme, fontSize: 48)
                .frame(maxWidth: .infinity)
                .padding(.top)

            if let progressText = player.progressText {
                Text(progressText)
                    .font(.caption)
                    .foregroundStyle(theme.foreground.opacity(0.6))
                    .monospacedDigit()
            }

            // Scrub/seek within the document.
            VStack(spacing: 4) {
                Slider(
                    value: Binding(
                        get: { player.scrubFraction },
                        set: { player.seek(toFraction: $0) }
                    ),
                    in: 0...1
                )
                .tint(theme.accent)
                .frame(width: 260)
                .accessibilityLabel("Seek position in document")

                Text(sessionStatsLine)
                    .font(.caption2)
                    .foregroundStyle(theme.foreground.opacity(0.5))
                    .monospacedDigit()
            }

            // WPM controls: numeric readout, rotary dial, accessible stepper.
            VStack(spacing: 12) {
                Text("\(player.wpm) WPM")
                    .font(.caption)
                    .foregroundStyle(theme.foreground.opacity(0.6))
                    .monospacedDigit()
                RotaryDialView(wpm: player.wpm, theme: theme) { player.setWpm($0) }
                WpmStepperView(wpm: player.wpm, step: 25) { player.setWpm($0) }
            }

            Toggle(isOn: Binding(
                get: { player.punctuationPauseEnabled },
                set: { player.setPunctuationPauseEnabled($0) }
            )) {
                Text("Pause longer at punctuation")
            }
            .toggleStyle(.switch)
            .tint(theme.accent)
            .frame(width: 260)

            // Transport: back 5 words + play/pause.
            HStack(spacing: 32) {
                Button {
                    player.backWords(5)
                } label: {
                    Image(systemName: "gobackward.5")
                        .font(.title2)
                        .foregroundStyle(theme.foreground)
                }
                .accessibilityLabel("Back 5 words")

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
            .padding(.bottom)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.background)
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
