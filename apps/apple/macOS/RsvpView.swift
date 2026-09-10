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

// ── Playback engine ──────────────────────────────────────────────────────────

/// Drives RSVP playback client-side from a session fetched once via FFI.
/// `gist_rsvp::RsvpSession` is a pure, timer-free pacing engine by design
/// (see that crate's doc comment: "the SwiftUI shell drives it... by calling
/// token_at_elapsed on every frame") — this is that shell half. It ports
/// `RsvpSession::token_duration_ms` and its punctuation helpers from
/// crates/gist-rsvp/src/lib.rs; keep the two in sync if pacing rules change
/// there.
@MainActor
final class RsvpPlayer: ObservableObject {
    @Published private(set) var isLoaded = false
    @Published private(set) var currentIndex = 0
    @Published var isPlaying = false
    @Published var wpm: UInt32 = 250

    private var session: RsvpSessionVM?
    private var playTask: Task<Void, Never>?
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
        currentIndex = min(loaded.cursor, max(loaded.tokens.count - 1, 0))
        isLoaded = true
    }

    func play() {
        guard !isPlaying, let session, currentIndex < session.tokens.count - 1 else { return }
        isPlaying = true
        playTask = Task { [weak self] in
            while let self, self.isPlaying {
                guard let session = self.session, self.currentIndex < session.tokens.count - 1 else {
                    self.isPlaying = false
                    return
                }
                let durationMs = self.tokenDurationMs(session: session, index: self.currentIndex)
                try? await Task.sleep(nanoseconds: durationMs * 1_000_000)
                if Task.isCancelled { return }
                self.currentIndex += 1
            }
        }
    }

    func pause() {
        isPlaying = false
        playTask?.cancel()
        playTask = nil
    }

    func stopAndPersist() async {
        pause()
        guard let core, let itemId else { return }
        await core.saveProgress(itemId: itemId, tokenIndex: currentIndex)
    }

    // MARK: Pacing — ported from RsvpSession::token_duration_ms

    private func tokenDurationMs(session: RsvpSessionVM, index: Int) -> UInt64 {
        guard session.tokens.indices.contains(index) else { return 0 }
        let cfg = session.config
        let clampedWpm = min(max(wpm, 100), 1000)
        let baseMs = 60_000 / UInt64(clampedWpm)
        let token = session.tokens[index]

        let multiplier: Double
        switch token.kind {
        case .paragraphBreak, .sectionBreak:
            multiplier = cfg.pauseParagraph
        case .word:
            let text = token.text
            if Self.isNumeral(text) {
                let punct = Self.endsSentence(text) ? cfg.pauseSentence
                    : (Self.endsClause(text) ? cfg.pauseComma : 1.0)
                multiplier = max(cfg.pauseNumeral, punct)
            } else if Self.endsSentence(text) {
                multiplier = cfg.pauseSentence
            } else if Self.endsClause(text) {
                multiplier = cfg.pauseComma
            } else {
                multiplier = 1.0
            }
        }
        return UInt64((Double(baseMs) * multiplier).rounded())
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

// ── View ──────────────────────────────────────────────────────────────────────

struct RsvpView: View {
    let itemId: String
    @EnvironmentObject var core: CoreClient
    @StateObject private var player = RsvpPlayer()

    var body: some View {
        Group {
            if player.isLoaded {
                playbackContent
            } else {
                ProgressView("Loading…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle("RSVP")
        .task { await player.load(core: core, itemId: itemId) }
        .onDisappear { Task { await player.stopAndPersist() } }
    }

    private var playbackContent: some View {
        VStack(spacing: 32) {
            // Word display.
            Text(player.currentToken?.text ?? "")
                .font(.system(size: 48, weight: .regular, design: .serif))
                .frame(maxWidth: .infinity)
                .padding()

            if let progressText = player.progressText {
                Text(progressText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }

            // WPM control
            VStack {
                Text("\(player.wpm) WPM")
                    .font(.caption)
                    .monospacedDigit()
                Slider(
                    value: Binding(
                        get: { Double(player.wpm) },
                        set: { player.wpm = UInt32($0.rounded()) }
                    ),
                    in: 100...1000,
                    step: 10
                )
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
            }
            .keyboardShortcut(.space, modifiers: [])
            .accessibilityLabel(player.isPlaying ? "Pause" : "Play")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
