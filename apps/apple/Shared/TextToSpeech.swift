import AVFoundation
import SwiftUI

// ── Read-aloud (role R6, development-plan-v2.md §3.9) ───────────────────────
//
// `AVSpeechSynthesizer`-backed read-aloud for the flow reading view. Mirrors
// `RsvpPlayer`'s shape (RsvpView.swift): a `@MainActor final class
// ObservableObject` wrapping the underlying engine, with as much of the
// actual logic factored into plain, synchronous, unit-testable pure
// functions/enums as possible (`TtsPlaybackPhase`/`TtsStateMachine`/
// `TtsRate`/`TtsTextExtraction`) rather than living inside the
// `AVSpeechSynthesizerDelegate` callbacks themselves -- exactly the same
// "keep the state machine testable without driving a live engine" judgment
// this codebase already applies to `RsvpWallClockEngine`.
//
// Scope: reads a document's blocks aloud one at a time (matching
// `FlowDocumentVM`'s block granularity, the same unit `FlowViewSwiftUINative`
// renders and searches by), with play/pause/stop and an adjustable rate.
// Deliberately does not attempt word-level highlighting sync with the flow
// view's rendered text -- that would need `AVSpeechSynthesizer`'s
// `willSpeakRangeOfSpeechString` delegate callback mapped back through the
// same byte/character-offset machinery `FlowViewSwiftUINative` already uses
// for search-match highlighting, which is a real feature in its own right,
// not a small addition; out of scope for this pass.

/// Playback speed for read-aloud, expressed as a small closed set of
/// human-meaningful steps (mirroring `LineSpacingOption`/`ReadingFontDesign`'s
/// existing "wrap a continuous engine parameter as a `CaseIterable` enum for
/// a `Picker`" idiom) rather than exposing `AVSpeechUtterance.rate`'s raw
/// `0.0...1.0` float directly to the UI.
enum TtsRate: String, CaseIterable, Identifiable, Equatable {
    case slower
    case slow
    case normal
    case fast
    case faster

    var id: String { rawValue }

    var label: String {
        switch self {
        case .slower: return "Slower"
        case .slow: return "Slow"
        case .normal: return "Normal"
        case .fast: return "Fast"
        case .faster: return "Faster"
        }
    }

    /// `AVSpeechUtterance.rate` value (`0.0...1.0`; `AVSpeechUtteranceDefaultSpeechRate`
    /// is `0.5`). Spread chosen to be clearly distinguishable per step without
    /// reaching either extreme (`AVSpeechUtteranceMinimumSpeechRate`/
    /// `MaximumSpeechRate`), which produce speech that's difficult to
    /// understand at all.
    var utteranceRate: Float {
        switch self {
        case .slower: return 0.35
        case .slow: return 0.42
        case .normal: return Float(AVSpeechUtteranceDefaultSpeechRate)
        case .fast: return 0.58
        case .faster: return 0.68
        }
    }
}

/// Pure playback-position state for read-aloud: either idle (nothing
/// queued/spoken), speaking block `index`, or paused at block `index`.
/// Deliberately mirrors the shape of a small state machine rather than a
/// loose bag of `isPlaying`/`currentIndex` booleans+ints, so the legal
/// transitions below (`TtsStateMachine`) can be expressed as total functions
/// over this type and unit-tested without any `AVSpeechSynthesizer` involved.
enum TtsPlaybackPhase: Equatable {
    case idle
    case speaking(index: Int)
    case paused(index: Int)

    var blockIndex: Int? {
        switch self {
        case .idle: return nil
        case .speaking(let index), .paused(let index): return index
        }
    }
}

/// Pure transition functions for `TtsPlaybackPhase`, factored out of
/// `TtsPlayer` so the actual state logic (not just the engine plumbing
/// around it) is unit-testable without a real `AVSpeechSynthesizer` --
/// which, like `KeychainKeyProviderIntegrationTests`, would risk being slow
/// or environment-dependent (real speech-audio playback) in a headless test
/// run rather than genuinely unavailable, so keeping the *logic* separate
/// from the *engine* is what actually makes this testable, not working
/// around a hang.
enum TtsStateMachine {
    /// Begins playback at `index` into `blocks`, or `.idle` if `index` is out
    /// of range (including an empty `blocks` array).
    static func start(blocks: [String], from index: Int) -> TtsPlaybackPhase {
        guard index >= 0, index < blocks.count else { return .idle }
        return .speaking(index: index)
    }

    /// `.speaking(i)` -> `.paused(i)`. Any other phase is unchanged --
    /// pausing while idle or already paused is a no-op, not an error.
    static func pause(_ phase: TtsPlaybackPhase) -> TtsPlaybackPhase {
        guard case .speaking(let index) = phase else { return phase }
        return .paused(index: index)
    }

    /// `.paused(i)` -> `.speaking(i)`. Any other phase is unchanged.
    static func resume(_ phase: TtsPlaybackPhase) -> TtsPlaybackPhase {
        guard case .paused(let index) = phase else { return phase }
        return .speaking(index: index)
    }

    /// Advances from `.speaking(i)` to `.speaking(i+1)`, or `.idle` once the
    /// last block has finished. Any other phase is unchanged -- advancing
    /// while paused/idle is meaningless and left as a no-op rather than an
    /// error, matching `pause`/`resume`'s permissiveness.
    static func advance(_ phase: TtsPlaybackPhase, blockCount: Int) -> TtsPlaybackPhase {
        guard case .speaking(let index) = phase else { return phase }
        let next = index + 1
        return next < blockCount ? .speaking(index: next) : .idle
    }

    /// Always `.idle`, regardless of the current phase -- stop is
    /// unconditional.
    static func stop() -> TtsPlaybackPhase { .idle }
}

/// Extracts the plain-text blocks read-aloud should speak, in the same
/// document order (and, deliberately, the same per-block granularity) as
/// `FlowViewSwiftUINative.flatBlocks` -- an empty block (e.g. an image with
/// no alt text) is kept as an empty string rather than filtered out, so a
/// block index computed against one array (e.g. a starting position derived
/// from `ReadingProgress.fraction`) stays meaningful against the other;
/// `TtsPlayer` skips empty blocks at speak time instead (see its `speak`
/// method), rather than this function silently renumbering everything.
enum TtsTextExtraction {
    static func speakableBlocks(for document: FlowDocumentVM) -> [String] {
        document.sections.flatMap { section in section.blocks.map(\.plainText) }
    }
}

/// Drives read-aloud playback of a document's blocks via
/// `AVSpeechSynthesizer`, one block/utterance at a time (queuing the next
/// block only once the previous one finishes, via the delegate's
/// `didFinish` callback) so `currentBlockIndex` is always exactly "the block
/// being spoken right now", not an approximation. `@MainActor final class
/// ... : ObservableObject` mirrors `RsvpPlayer`'s shape exactly, including
/// keeping the actual position/transition logic in a pure, testable type
/// (`TtsStateMachine`) rather than inline in delegate callbacks.
///
/// Must subclass `NSObject` to be usable as an `AVSpeechSynthesizerDelegate`.
/// Delegate callbacks arrive on an undocumented (and not necessarily main)
/// thread, so each one hops back to the main actor via `Task` before
/// touching any `@Published` state -- the same reason `ThemeManager`'s
/// `NSApp.observe` KVO callback in Theme.swift does the same `Task { @MainActor in ... }` hop.
@MainActor
final class TtsPlayer: NSObject, ObservableObject {
    @Published private(set) var phase: TtsPlaybackPhase = .idle
    /// Not persisted -- a read-aloud rate preference surviving across app
    /// launches wasn't asked for, and `AppSettings.swift`'s existing
    /// `.shared`/`UserDefaults`-backed settings objects are all reachable if
    /// that's wanted later; this stays session-local like `RsvpPlayer.wpm`'s
    /// in-session-only default before `RsvpDefaults` existed.
    @Published var rate: TtsRate = .normal

    private let synthesizer: AVSpeechSynthesizer
    private var blocks: [String] = []

    /// `synthesizer` is injectable so a future test could substitute a fake
    /// without touching real audio hardware; production call sites always
    /// use the default real `AVSpeechSynthesizer()`.
    init(synthesizer: AVSpeechSynthesizer = AVSpeechSynthesizer()) {
        self.synthesizer = synthesizer
        super.init()
        synthesizer.delegate = self
    }

    var currentBlockIndex: Int? { phase.blockIndex }
    var isSpeaking: Bool { if case .speaking = phase { return true } else { return false } }
    var isPaused: Bool { if case .paused = phase { return true } else { return false } }
    var isActive: Bool { phase != .idle }

    /// Begins reading `blocks` aloud starting at `index` (clamped by
    /// `TtsStateMachine.start`, not here) -- replaces whatever was
    /// previously playing, if anything.
    func start(blocks: [String], from index: Int = 0) {
        self.blocks = blocks
        synthesizer.stopSpeaking(at: .immediate)
        speak(atIndex: index)
    }

    func pause() {
        guard isSpeaking else { return }
        synthesizer.pauseSpeaking(at: .word)
        phase = TtsStateMachine.pause(phase)
    }

    func resume() {
        guard isPaused else { return }
        synthesizer.continueSpeaking()
        phase = TtsStateMachine.resume(phase)
    }

    func stop() {
        synthesizer.stopSpeaking(at: .immediate)
        phase = TtsStateMachine.stop()
        blocks = []
    }

    /// Changing rate mid-utterance has no effect on the utterance already in
    /// flight (`AVSpeechUtterance.rate` is read once when speech starts), so
    /// this restarts the *current* block under the new rate -- acceptable
    /// since a block is typically one paragraph, not a whole chapter.
    func setRate(_ newRate: TtsRate) {
        rate = newRate
        guard isSpeaking, let index = currentBlockIndex else { return }
        synthesizer.stopSpeaking(at: .word)
        speak(atIndex: index)
    }

    private func speak(atIndex index: Int) {
        let newPhase = TtsStateMachine.start(blocks: blocks, from: index)
        phase = newPhase
        guard case .speaking(let resolvedIndex) = newPhase else { return }
        let text = blocks[resolvedIndex]
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            advance()
            return
        }
        let utterance = AVSpeechUtterance(string: text)
        utterance.rate = rate.utteranceRate
        synthesizer.speak(utterance)
    }

    private func advance() {
        let next = TtsStateMachine.advance(phase, blockCount: blocks.count)
        if case .speaking(let index) = next {
            speak(atIndex: index)
        } else {
            phase = .idle
        }
    }
}

extension TtsPlayer: AVSpeechSynthesizerDelegate {
    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        Task { @MainActor [weak self] in
            self?.advance()
        }
    }
}
