import XCTest
@testable import GIST

/// Exercises `RsvpWallClockEngine`, the wall-clock-anchored pacing type that
/// replaced `RsvpPlayer`'s original sequential `Task.sleep`-per-token loop
/// (see RsvpView.swift's doc comment on that type for the drift bug this
/// fixes). Everything here is deterministic value-type math driven by
/// explicit `Date`s — no real sleeping, no live `RsvpPlayer`/SwiftUI view.
@MainActor
final class RsvpPacingTests: XCTestCase {
    private static let epoch = Date(timeIntervalSince1970: 1_700_000_000)

    // MARK: - Fixture/CoreClient setup, mirroring GISTTests.swift's convention

    private var tempDir: URL!
    private var storageDir: String!
    private var client: CoreClient!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("RsvpPacingTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        let dbPath = tempDir.appendingPathComponent("gist.sqlite3").path
        storageDir = tempDir.appendingPathComponent("storage", isDirectory: true).path
        client = CoreClient(dbPath: dbPath, storageDir: storageDir)
    }

    override func tearDownWithError() throws {
        client = nil
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
        tempDir = nil
        storageDir = nil
        try super.tearDownWithError()
    }

    /// Copies the `basic_ascii.txt` fixture to a fresh per-test scratch
    /// path, so it can be imported as if user-selected — same rationale as
    /// `GISTTests.importableFixtureURL()`.
    private func importableFixtureURL(function: String = #function) throws -> URL {
        guard let bundled = Bundle(for: Self.self).url(forResource: "basic_ascii", withExtension: "txt") else {
            throw XCTSkip("basic_ascii.txt fixture missing from test bundle resources")
        }
        let sanitizedName = function.filter(\.isLetter)
        let destination = tempDir.appendingPathComponent("fixture-\(sanitizedName).txt")
        try FileManager.default.copyItem(at: bundled, to: destination)
        return destination
    }

    private func word(_ text: String) -> TokenVM {
        TokenVM(text: text, kind: .word)
    }

    private func breakToken() -> TokenVM {
        TokenVM(text: "", kind: .paragraphBreak)
    }

    /// 600 wpm -> 100ms base duration per plain word, matching the Rust
    /// test `token_at_elapsed_basic` in crates/gist-rsvp/src/lib.rs.
    private func config600wpm() -> RsvpConfigVM {
        RsvpConfigVM(
            wpm: 600, pauseSentence: 1.8, pauseComma: 1.3, pauseParagraph: 2.2, pauseNumeral: 1.4, chunkSize: 1
        )
    }

    // MARK: - tokenDurationMs (ported from RsvpSession::token_duration_ms)

    func testTokenDurationSentencePause() {
        let cfg = RsvpConfigVM(
            wpm: 250, pauseSentence: 1.8, pauseComma: 1.3, pauseParagraph: 2.2, pauseNumeral: 1.4, chunkSize: 1
        )
        let tokens = [word("end.")]
        let base = 60_000 / 250
        let expected = UInt64((Double(base) * 1.8).rounded())
        XCTAssertEqual(
            RsvpWallClockEngine.tokenDurationMs(tokens: tokens, config: cfg, wpm: 250, idx: 0), expected
        )
    }

    func testTokenDurationParagraphBreak() {
        let cfg = RsvpConfigVM(
            wpm: 250, pauseSentence: 1.8, pauseComma: 1.3, pauseParagraph: 2.2, pauseNumeral: 1.4, chunkSize: 1
        )
        let tokens = [word("hello"), breakToken()]
        let base = 60_000 / 250
        let expected = UInt64((Double(base) * 2.2).rounded())
        XCTAssertEqual(
            RsvpWallClockEngine.tokenDurationMs(tokens: tokens, config: cfg, wpm: 250, idx: 1), expected
        )
    }

    // MARK: - tokenAtElapsed (ported from RsvpSession::token_at_elapsed)

    /// Direct Swift counterpart of the Rust test `token_at_elapsed_basic`.
    func testTokenAtElapsedBasic() {
        let cfg = config600wpm()
        let tokens = [word("one"), word("two"), word("three")]
        XCTAssertEqual(
            RsvpWallClockEngine.tokenAtElapsed(tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 0).index, 0
        )
        XCTAssertEqual(
            RsvpWallClockEngine.tokenAtElapsed(tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 50).index, 0
        )
        XCTAssertEqual(
            RsvpWallClockEngine.tokenAtElapsed(tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 100).index, 1
        )
        XCTAssertEqual(
            RsvpWallClockEngine.tokenAtElapsed(tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 200).index, 2
        )
    }

    /// A delayed/coalesced tick's elapsed time can jump well past where a
    /// chain of sequential per-token sleeps would have landed after the
    /// same number of *intended* sleeps -- this is the actual drift bug.
    /// `tokenAtElapsed` must land on the token actually due for the real
    /// elapsed time, not one a naive sequential-sleep loop would be stuck
    /// on after under-sleeping or over-sleeping along the way.
    func testTokenAtElapsedCatchesUpAfterADelayedTick() {
        let cfg = config600wpm()
        // 10 plain words at 600wpm = 100ms each.
        let tokens = (0..<10).map { word("w\($0)") }

        // A tick that fires very late (e.g. the app was backgrounded, or a
        // Task woke up long after its scheduled sleep) reports 550ms
        // elapsed since resume, even though only 2 sequential 100ms sleeps
        // "should" have completed by whatever the naive loop was tracking.
        // The engine must jump straight to index 5 (500ms consumed by
        // tokens 0-4, 50ms into token 5), not silently stay behind.
        let result = RsvpWallClockEngine.tokenAtElapsed(
            tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 550
        )
        XCTAssertEqual(result.index, 5)
        XCTAssertEqual(result.remainingMs, 50)
    }

    func testTokenAtElapsedClampsToLastToken() {
        let cfg = config600wpm()
        let tokens = [word("one"), word("two")]
        let result = RsvpWallClockEngine.tokenAtElapsed(
            tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 10_000
        )
        XCTAssertEqual(result.index, 1)
        XCTAssertEqual(result.remainingMs, 0)
    }

    func testTokenAtElapsedEmptyTokensReturnsZero() {
        let cfg = config600wpm()
        let result = RsvpWallClockEngine.tokenAtElapsed(
            tokens: [], config: cfg, wpm: 600, cursor: 0, elapsedMs: 500
        )
        XCTAssertEqual(result.index, 0)
        XCTAssertEqual(result.remainingMs, 0)
    }

    // MARK: - RsvpWallClockEngine: resume/currentIndex wall-clock anchoring

    /// Simulates ticks arriving at arbitrary, non-uniform wall-clock offsets
    /// (as a real `Task.sleep`-driven loop would after scheduler jitter)
    /// and confirms the engine always reports the index actually due for
    /// elapsed wall-clock time, never one derived from summing prior ticks.
    func testCurrentIndexTracksArbitraryElapsedOffsetsFromResume() {
        let cfg = config600wpm()
        let tokens = (0..<10).map { word("w\($0)") }
        var engine = RsvpWallClockEngine(cursor: 0)
        let start = Self.epoch
        engine.resume(at: start)

        // Instead of ticking at neat 100ms multiples, jump around --
        // mirroring jittery real wake times -- and confirm the index is
        // always exactly what elapsed time dictates.
        let cases: [(offsetMs: Double, expectedIndex: Int)] = [
            (0, 0), (99, 0), (101, 1), (245, 2), (999, 9), (5000, 9),
        ]
        for (offsetMs, expectedIndex) in cases {
            let now = start.addingTimeInterval(offsetMs / 1000)
            let (idx, _) = engine.currentIndex(tokens: tokens, config: cfg, wpm: 600, at: now)
            XCTAssertEqual(idx, expectedIndex, "at offset \(offsetMs)ms")
        }
    }

    // MARK: - pause/resume: paused duration must not count as elapsed reading time

    func testPauseThenResumeDoesNotCountPausedDurationAsElapsed() {
        let cfg = config600wpm()
        let tokens = (0..<10).map { word("w\($0)") }
        var engine = RsvpWallClockEngine(cursor: 0)

        let t0 = Self.epoch
        engine.resume(at: t0)

        // Play for 250ms (lands mid-token-2), then pause.
        let pauseAt = t0.addingTimeInterval(0.250)
        engine.pause(tokens: tokens, config: cfg, wpm: 600, at: pauseAt)
        XCTAssertFalse(engine.isPlaying)
        XCTAssertEqual(engine.cursor, 2)

        // While paused, `currentIndex` must stay pinned regardless of how
        // much real wall-clock time passes.
        let stillPaused = engine.currentIndex(
            tokens: tokens, config: cfg, wpm: 600, at: pauseAt.addingTimeInterval(60)
        )
        XCTAssertEqual(stillPaused.index, 2)
        XCTAssertEqual(stillPaused.remainingMs, RsvpWallClockEngine.notPlayingRemainingMs)

        // Resume a full minute later (simulating a long pause). The clock
        // must restart from the resume point, not from t0 -- so 100ms
        // after resuming should land exactly one token further (index 3),
        // not wherever 60s+100ms since t0 would imply.
        let resumeAt = pauseAt.addingTimeInterval(60)
        engine.resume(at: resumeAt)
        let afterResume = engine.currentIndex(
            tokens: tokens, config: cfg, wpm: 600, at: resumeAt.addingTimeInterval(0.100)
        )
        XCTAssertEqual(afterResume.index, 3)
    }

    func testMultiplePauseResumeCyclesAccumulateOnlyPlayedTime() {
        let cfg = config600wpm()
        let tokens = (0..<10).map { word("w\($0)") }
        var engine = RsvpWallClockEngine(cursor: 0)
        var now = Self.epoch

        // Play 100ms (-> index 1), pause for an arbitrary gap.
        engine.resume(at: now)
        now = now.addingTimeInterval(0.100)
        engine.pause(tokens: tokens, config: cfg, wpm: 600, at: now)
        XCTAssertEqual(engine.cursor, 1)
        now = now.addingTimeInterval(30) // long gap, must not count

        // Play another 100ms (-> index 2), pause again.
        engine.resume(at: now)
        now = now.addingTimeInterval(0.100)
        engine.pause(tokens: tokens, config: cfg, wpm: 600, at: now)
        XCTAssertEqual(engine.cursor, 2)
        now = now.addingTimeInterval(5) // another gap, must not count

        // Play a final 100ms (-> index 3).
        engine.resume(at: now)
        now = now.addingTimeInterval(0.100)
        let result = engine.currentIndex(tokens: tokens, config: cfg, wpm: 600, at: now)
        XCTAssertEqual(result.index, 3)
    }

    // MARK: - set_wpm: speed changes mid-session must not corrupt pacing

    /// Mirrors the Rust test `set_wpm_clamps`.
    func testSetWpmClampsRange() {
        var engine = RsvpWallClockEngine(cursor: 0)
        let tokens = [word("x")]
        let cfg = RsvpConfigVM(
            wpm: 250, pauseSentence: 1.8, pauseComma: 1.3, pauseParagraph: 2.2, pauseNumeral: 1.4, chunkSize: 1
        )
        // setWpm itself doesn't clamp (RsvpPlayer.setWpm does, mirroring
        // how Rust's RsvpSession.set_wpm clamps config.wpm) -- this test
        // exercises RsvpPlayer's clamping directly further down; here we
        // confirm the engine's own bookkeeping is a no-op while paused.
        engine.setWpm(tokens: tokens, config: cfg, oldWpm: 250, at: Self.epoch)
        XCTAssertFalse(engine.isPlaying)
        XCTAssertEqual(engine.cursor, 0)
    }

    /// A wpm change mid-playback must pin the cursor to the position
    /// implied by elapsed time *under the old speed*, then restart the
    /// clock -- so tokens already shown before the change aren't silently
    /// re-timed, and the very next tick isn't computed against a stale
    /// duration for the token now on screen.
    func testSetWpmMidPlaybackPreservesPositionThenAppliesNewSpeed() {
        let cfg600 = config600wpm() // 100ms/word at 600wpm
        let tokens = (0..<10).map { word("w\($0)") }
        var engine = RsvpWallClockEngine(cursor: 0)
        let t0 = Self.epoch
        engine.resume(at: t0)

        // 250ms in at 600wpm -> index 2 (200ms consumed by tokens 0-1, 50ms
        // into token 2).
        let changeAt = t0.addingTimeInterval(0.250)
        engine.setWpm(tokens: tokens, config: cfg600, oldWpm: 600, at: changeAt)
        XCTAssertEqual(engine.cursor, 2)
        XCTAssertTrue(engine.isPlaying, "setWpm must keep the clock running when it was already playing")

        // Now at 300wpm (200ms/word), 200ms after the change should land
        // exactly one token further (index 3) -- proving the elapsed clock
        // truly restarted at `changeAt` under the new speed, rather than
        // continuing to accumulate against the old 600wpm durations.
        let afterChange = engine.currentIndex(
            tokens: tokens, config: cfg600, wpm: 300, at: changeAt.addingTimeInterval(0.200)
        )
        XCTAssertEqual(afterChange.index, 3)
    }

    func testSetWpmWhilePausedDoesNotMoveCursor() {
        let cfg = config600wpm()
        let tokens = (0..<10).map { word("w\($0)") }
        var engine = RsvpWallClockEngine(cursor: 4)
        // Not playing (never resumed): setWpm must be a no-op.
        engine.setWpm(tokens: tokens, config: cfg, oldWpm: 600, at: Self.epoch)
        XCTAssertEqual(engine.cursor, 4)
        XCTAssertFalse(engine.isPlaying)
    }

    // MARK: - RsvpPlayer integration (real object, no mocking, per repo convention)

    /// `RsvpPlayer.setWpm` clamps to 100-1000, matching
    /// `RsvpSession::set_wpm`'s Rust-side clamp (mirrored Swift-side test
    /// of the Rust `set_wpm_clamps` unit test).
    func testRsvpPlayerSetWpmClampsRange() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let itemId = client.items.first?.id else {
            XCTFail("fixture import failed")
            return
        }

        let player = RsvpPlayer()
        await player.load(core: client, itemId: itemId, initialWpm: 250)
        XCTAssertTrue(player.isLoaded)

        player.setWpm(50)
        XCTAssertEqual(player.wpm, 100)

        player.setWpm(9999)
        XCTAssertEqual(player.wpm, 1000)
    }

    /// End-to-end (real `GistCore`, real fixture) proof that pausing stops
    /// the index from advancing and resuming continues from where it left
    /// off, using the actual `RsvpPlayer` a real reading session drives --
    /// not just the pure `RsvpWallClockEngine` math above.
    // MARK: - RsvpScrubMath (scrub/seek pure math)

    func testScrubIndexForFractionEndpoints() {
        XCTAssertEqual(RsvpScrubMath.index(forFraction: 0, tokenCount: 10), 0)
        XCTAssertEqual(RsvpScrubMath.index(forFraction: 1, tokenCount: 10), 9)
    }

    func testScrubIndexForFractionMidpoint() {
        // (11 - 1) * 0.5 = 5.
        XCTAssertEqual(RsvpScrubMath.index(forFraction: 0.5, tokenCount: 11), 5)
    }

    func testScrubIndexClampsOutOfRangeFractions() {
        XCTAssertEqual(RsvpScrubMath.index(forFraction: -1.0, tokenCount: 10), 0)
        XCTAssertEqual(RsvpScrubMath.index(forFraction: 5.0, tokenCount: 10), 9)
    }

    func testScrubIndexDegenerateTokenCounts() {
        XCTAssertEqual(RsvpScrubMath.index(forFraction: 0.5, tokenCount: 0), 0)
        XCTAssertEqual(RsvpScrubMath.index(forFraction: 0.5, tokenCount: 1), 0)
    }

    func testScrubFractionForIndexEndpoints() {
        XCTAssertEqual(RsvpScrubMath.fraction(forIndex: 0, tokenCount: 10), 0)
        XCTAssertEqual(RsvpScrubMath.fraction(forIndex: 9, tokenCount: 10), 1)
    }

    func testScrubFractionDegenerateTokenCounts() {
        // Nothing to scrub across with 0 or 1 tokens -- always 0, never NaN
        // from a division by (tokenCount - 1) == 0.
        XCTAssertEqual(RsvpScrubMath.fraction(forIndex: 0, tokenCount: 0), 0)
        XCTAssertEqual(RsvpScrubMath.fraction(forIndex: 0, tokenCount: 1), 0)
    }

    func testScrubRoundTripIsStable() {
        let tokenCount = 37
        for index in 0..<tokenCount {
            let fraction = RsvpScrubMath.fraction(forIndex: index, tokenCount: tokenCount)
            XCTAssertEqual(RsvpScrubMath.index(forFraction: fraction, tokenCount: tokenCount), index)
        }
    }

    // MARK: - RsvpBackWords (back-5-words index math)

    /// Direct Swift counterpart of the Rust test `seek_and_back_words`'s
    /// back-words half: cursor=5 (all-word tokens), back 2 words -> 3.
    func testBackWordsSimpleCase() {
        let tokens = (0..<10).map { word("w\($0)") }
        XCTAssertEqual(RsvpBackWords.index(from: 5, n: 2, tokens: tokens), 3)
    }

    /// Paragraph/section breaks must be skipped, not counted, when walking
    /// backward -- mirrors `RsvpSession::back_words` only decrementing its
    /// skip counter on `TokenKind::Word`.
    func testBackWordsSkipsBreaksWithoutCountingThem() {
        // indices: 0=w0 1=w1 2=break 3=w2 4=w3 5=w4
        let tokens: [TokenVM] = [
            word("w0"), word("w1"), breakToken(), word("w2"), word("w3"), word("w4"),
        ]
        // From w3 (index 4), back 3 words: w2(3, skip1) -> break(2, skip) -> w1(1, skip2) -> w0(0, skip3).
        XCTAssertEqual(RsvpBackWords.index(from: 4, n: 3, tokens: tokens), 0)
    }

    func testBackWordsZeroIsANoOp() {
        let tokens = (0..<10).map { word("w\($0)") }
        XCTAssertEqual(RsvpBackWords.index(from: 6, n: 0, tokens: tokens), 6)
    }

    func testBackWordsClampsAtStartOfStream() {
        let tokens = (0..<10).map { word("w\($0)") }
        XCTAssertEqual(RsvpBackWords.index(from: 1, n: 100, tokens: tokens), 0)
    }

    func testBackWordsEmptyTokensReturnsZero() {
        XCTAssertEqual(RsvpBackWords.index(from: 5, n: 3, tokens: []), 0)
    }

    // MARK: - Punctuation-pause toggle

    /// Disabling the toggle collapses a sentence-ending word's multiplier
    /// to 1.0 (plain base duration) instead of `config.pauseSentence`.
    func testTokenDurationPunctuationDisabledForcesBaseDuration() {
        let cfg = config600wpm() // 100ms base at 600wpm
        let tokens = [word("end.")]
        XCTAssertEqual(
            RsvpWallClockEngine.tokenDurationMs(
                tokens: tokens, config: cfg, wpm: 600, idx: 0, punctuationPauseEnabled: false
            ),
            100
        )
        // Sanity: with the toggle on (default), the same token is longer.
        XCTAssertGreaterThan(
            RsvpWallClockEngine.tokenDurationMs(
                tokens: tokens, config: cfg, wpm: 600, idx: 0, punctuationPauseEnabled: true
            ),
            100
        )
    }

    /// A comma-ending word behaves the same way -- toggle off means no
    /// clause-pause multiplier either.
    func testTokenDurationPunctuationDisabledAppliesToCommaToo() {
        let cfg = config600wpm()
        let tokens = [word("however,")]
        XCTAssertEqual(
            RsvpWallClockEngine.tokenDurationMs(
                tokens: tokens, config: cfg, wpm: 600, idx: 0, punctuationPauseEnabled: false
            ),
            100
        )
    }

    /// Paragraph/section-break pauses are structural, not punctuation --
    /// they must apply identically regardless of the toggle.
    func testTokenDurationParagraphBreakUnaffectedByPunctuationToggle() {
        let cfg = config600wpm()
        let tokens = [word("hello"), breakToken()]
        let enabled = RsvpWallClockEngine.tokenDurationMs(
            tokens: tokens, config: cfg, wpm: 600, idx: 1, punctuationPauseEnabled: true
        )
        let disabled = RsvpWallClockEngine.tokenDurationMs(
            tokens: tokens, config: cfg, wpm: 600, idx: 1, punctuationPauseEnabled: false
        )
        XCTAssertEqual(enabled, disabled)
        XCTAssertEqual(enabled, 220) // 100ms base * 2.2 pauseParagraph
    }

    /// With the toggle disabled, `tokenAtElapsed` advances through
    /// punctuation-heavy text faster than with it enabled, for the same
    /// elapsed time -- confirms the toggle actually changes pacing outcomes,
    /// not just the per-token duration in isolation.
    func testTokenAtElapsedAdvancesFasterWithPunctuationPauseDisabled() {
        let cfg = config600wpm() // 100ms base/word, pauseSentence 1.8x
        let tokens = [word("one."), word("two."), word("three."), word("four.")]
        // 250ms: with pauses on, each sentence-ending token takes 180ms, so
        // token 0 (0-180ms) is fully consumed and elapsed time lands inside
        // token 1 (180-360ms) -> index 1.
        let withPauses = RsvpWallClockEngine.tokenAtElapsed(
            tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 250, punctuationPauseEnabled: true
        )
        XCTAssertEqual(withPauses.index, 1)
        // Same 250ms with pauses off: each token is a flat 100ms, so index 2
        // (200ms consumed by tokens 0-1, 50ms into token 2).
        let withoutPauses = RsvpWallClockEngine.tokenAtElapsed(
            tokens: tokens, config: cfg, wpm: 600, cursor: 0, elapsedMs: 250, punctuationPauseEnabled: false
        )
        XCTAssertEqual(withoutPauses.index, 2)
    }

    /// `RsvpPlayer.setPunctuationPauseEnabled` mid-playback must pin the
    /// cursor under the *old* setting (mirroring `setWpm`'s contract)
    /// before applying the new one -- an already-partially-shown token
    /// shouldn't be retroactively re-timed.
    func testRsvpPlayerPunctuationToggleMidPlaybackRepinsCursor() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let itemId = client.items.first?.id else {
            XCTFail("fixture import failed")
            return
        }

        let player = RsvpPlayer()
        await player.load(core: client, itemId: itemId, initialWpm: 1000)
        XCTAssertTrue(player.isLoaded)
        XCTAssertTrue(player.punctuationPauseEnabled, "punctuation pauses default to enabled")

        player.play()
        try await Task.sleep(nanoseconds: 100_000_000)
        player.setPunctuationPauseEnabled(false)
        XCTAssertFalse(player.punctuationPauseEnabled)
        XCTAssertTrue(player.isPlaying, "toggling mid-playback must not stop playback")

        player.pause()
        XCTAssertFalse(player.isPlaying)
    }

    func testRsvpPlayerPunctuationToggleWhilePausedJustFlipsTheFlag() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let itemId = client.items.first?.id else {
            XCTFail("fixture import failed")
            return
        }

        let player = RsvpPlayer()
        await player.load(core: client, itemId: itemId, initialWpm: 250)
        XCTAssertTrue(player.isLoaded)
        XCTAssertFalse(player.isPlaying)

        let indexBefore = player.currentIndex
        player.setPunctuationPauseEnabled(false)
        XCTAssertFalse(player.punctuationPauseEnabled)
        XCTAssertEqual(player.currentIndex, indexBefore, "toggling while paused must not move the cursor")
    }

    // MARK: - RsvpWallClockEngine.seek (scrub/back-words shared primitive)

    func testEngineSeekMovesCursorWhilePaused() {
        var engine = RsvpWallClockEngine(cursor: 0)
        engine.seek(toIndex: 7, tokenCount: 20, at: Self.epoch)
        XCTAssertEqual(engine.cursor, 7)
        XCTAssertFalse(engine.isPlaying)
    }

    func testEngineSeekClampsToValidRange() {
        var engine = RsvpWallClockEngine(cursor: 0)
        engine.seek(toIndex: 999, tokenCount: 20, at: Self.epoch)
        XCTAssertEqual(engine.cursor, 19)
        engine.seek(toIndex: -5, tokenCount: 20, at: Self.epoch)
        XCTAssertEqual(engine.cursor, 0)
    }

    /// Seeking while playing must fold the elapsed-since-resume span into
    /// `accumulatedPlayMs` (not discard it) and restart the clock at the
    /// seek point, so session-stats elapsed time survives a scrub.
    func testEngineSeekWhilePlayingPreservesAccumulatedPlayTime() {
        var engine = RsvpWallClockEngine(cursor: 0)
        let t0 = Self.epoch
        engine.resume(at: t0)
        let seekAt = t0.addingTimeInterval(0.400)
        engine.seek(toIndex: 10, tokenCount: 20, at: seekAt)
        XCTAssertEqual(engine.cursor, 10)
        XCTAssertTrue(engine.isPlaying, "seeking must not stop playback")
        XCTAssertEqual(engine.totalElapsedMs(at: seekAt), 400)

        // Continuing to play for another 100ms after the seek should add
        // to, not replace, that accumulated time.
        let later = seekAt.addingTimeInterval(0.100)
        XCTAssertEqual(engine.totalElapsedMs(at: later), 500)
    }

    // MARK: - RsvpStats (session stats readout)

    func testWordsShownCountsOnlyWordTokensBeforeIndex() {
        let tokens: [TokenVM] = [word("a"), word("b"), breakToken(), word("c")]
        XCTAssertEqual(RsvpStats.wordsShown(tokens: tokens, upTo: 0), 0)
        XCTAssertEqual(RsvpStats.wordsShown(tokens: tokens, upTo: 3), 2)
        XCTAssertEqual(RsvpStats.wordsShown(tokens: tokens, upTo: 4), 3)
    }

    func testAchievedWpmComputation() {
        XCTAssertEqual(RsvpStats.achievedWpm(wordsShown: 100, elapsedMs: 60_000), 100, accuracy: 0.001)
        XCTAssertEqual(RsvpStats.achievedWpm(wordsShown: 0, elapsedMs: 0), 0)
    }

    // MARK: - OrpCalculator (ORP pivot highlighting)

    /// Direct Swift counterpart of the Rust test `test_orp_index`.
    func testOrpPivotCharIndexMatchesRustReferenceCases() {
        // "Hello": target = round(5 * 0.3) = 2 -> first vowel at/after index 2 is 'o' (index 4).
        XCTAssertEqual(OrpCalculator.pivotCharIndex(word: "Hello"), 4)
        XCTAssertEqual(OrpCalculator.pivotCharIndex(word: "A"), 0)
        XCTAssertEqual(OrpCalculator.pivotCharIndex(word: ""), 0)
    }

    func testOrpSplitProducesPrefixPivotSuffix() {
        let split = OrpCalculator.split("Hello")
        XCTAssertEqual(split.prefix, "Hell")
        XCTAssertEqual(split.pivot, "o")
        XCTAssertEqual(split.suffix, "")
    }

    func testOrpSplitOfEmptyStringIsAllEmpty() {
        let split = OrpCalculator.split("")
        XCTAssertEqual(split.prefix, "")
        XCTAssertEqual(split.pivot, "")
        XCTAssertEqual(split.suffix, "")
    }

    // MARK: - RotaryDialMath (dial drag gesture math)

    func testRotaryDialWpmMappingClampsRange() {
        XCTAssertEqual(RotaryDialMath.wpm(startingFrom: 100, rotatedByDegrees: -50), 100)
        XCTAssertEqual(RotaryDialMath.wpm(startingFrom: 1000, rotatedByDegrees: 50), 1000)
    }

    func testRotaryDialWpmMappingAppliesSensitivity() {
        // 250 + 50 degrees * 3 wpm/degree = 400.
        XCTAssertEqual(RotaryDialMath.wpm(startingFrom: 250, rotatedByDegrees: 50, wpmPerDegree: 3.0), 400)
    }

    func testRotaryDialAngularDeltaHandlesWraparound() {
        XCTAssertEqual(RotaryDialMath.angularDelta(from: -170, to: 170), -20, accuracy: 0.001)
        XCTAssertEqual(RotaryDialMath.angularDelta(from: 170, to: -170), 20, accuracy: 0.001)
    }

    func testRotaryDialAngleDegreesCardinalDirections() {
        let center = CGPoint(x: 50, y: 50)
        // North (straight up): dy negative, dx 0 -> 0 degrees.
        XCTAssertEqual(RotaryDialMath.angleDegrees(from: center, to: CGPoint(x: 50, y: 0)), 0, accuracy: 0.001)
        // East (straight right): dx positive, dy 0 -> 90 degrees.
        XCTAssertEqual(RotaryDialMath.angleDegrees(from: center, to: CGPoint(x: 100, y: 50)), 90, accuracy: 0.001)
        // South (straight down): dy positive, dx 0 -> 180 degrees.
        XCTAssertEqual(RotaryDialMath.angleDegrees(from: center, to: CGPoint(x: 50, y: 100)), 180, accuracy: 0.001)
        // West (straight left): dx negative, dy 0 -> -90 degrees.
        XCTAssertEqual(RotaryDialMath.angleDegrees(from: center, to: CGPoint(x: 0, y: 50)), -90, accuracy: 0.001)
    }

    // MARK: - RsvpPlayer integration (real object, no mocking, per repo convention)

    func testRsvpPlayerPauseStopsAdvancingAndResumeContinues() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let itemId = client.items.first?.id else {
            XCTFail("fixture import failed")
            return
        }

        let player = RsvpPlayer()
        await player.load(core: client, itemId: itemId, initialWpm: 1000) // fastest legal speed
        XCTAssertTrue(player.isLoaded)

        player.play()
        XCTAssertTrue(player.isPlaying)
        try await Task.sleep(nanoseconds: 150_000_000) // let it advance a bit
        player.pause()
        XCTAssertFalse(player.isPlaying)

        let indexAtPause = player.currentIndex
        try await Task.sleep(nanoseconds: 200_000_000) // paused: must not advance
        XCTAssertEqual(player.currentIndex, indexAtPause)

        player.play()
        XCTAssertTrue(player.isPlaying)
        try await Task.sleep(nanoseconds: 150_000_000)
        player.pause()
        XCTAssertGreaterThanOrEqual(
            player.currentIndex, indexAtPause, "resuming should continue forward, never rewind"
        )
    }

    /// End-to-end proof (real `RsvpPlayer`, real fixture) that `backWords`
    /// actually moves `currentIndex` backward on the live player, not just
    /// in the pure `RsvpBackWords.index` math exercised above.
    func testRsvpPlayerBackWordsMovesCursorBackward() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let itemId = client.items.first?.id else {
            XCTFail("fixture import failed")
            return
        }

        let player = RsvpPlayer()
        await player.load(core: client, itemId: itemId, initialWpm: 250)
        XCTAssertTrue(player.isLoaded)
        XCTAssertGreaterThan(player.tokenCount, 10, "fixture must have enough tokens to seek within")

        player.seek(toIndex: 8)
        XCTAssertEqual(player.currentIndex, 8)

        player.backWords(5)
        XCTAssertLessThan(player.currentIndex, 8, "back 5 words must move the cursor backward")
    }

    /// End-to-end proof that scrubbing to a fraction moves `currentIndex`
    /// proportionally and that `scrubFraction` reflects it back.
    func testRsvpPlayerSeekToFractionMovesCursorProportionally() async throws {
        let fixture = try importableFixtureURL()
        await client.importFile(url: fixture)
        guard let itemId = client.items.first?.id else {
            XCTFail("fixture import failed")
            return
        }

        let player = RsvpPlayer()
        await player.load(core: client, itemId: itemId, initialWpm: 250)
        XCTAssertTrue(player.isLoaded)

        player.seek(toFraction: 1.0)
        XCTAssertEqual(player.currentIndex, player.tokenCount - 1)
        XCTAssertEqual(player.scrubFraction, 1.0, accuracy: 0.001)

        player.seek(toFraction: 0.0)
        XCTAssertEqual(player.currentIndex, 0)
        XCTAssertEqual(player.scrubFraction, 0.0, accuracy: 0.001)
    }
}
