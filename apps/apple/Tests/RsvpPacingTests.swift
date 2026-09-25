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
}
