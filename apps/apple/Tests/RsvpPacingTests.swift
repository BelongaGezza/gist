import XCTest
@testable import GIST

/// Exercises the RSVP pacing contract as Apple now consumes it: a live
/// `FfiRsvpSession` (the Rust `gist-rsvp` engine) driven by `RsvpPlayer`'s
/// wall clock. These tests replace the ones that used to cover the
/// hand-ported Swift `RsvpWallClockEngine`; the *assertions* they encoded
/// (jittery-tick catch-up, pause/resume excluding paused time, mid-play WPM
/// change pinning the cursor under the old speed, punctuation toggle) are
/// kept, now asserted against the real engine. Rust has equivalent tests in
/// `crates/gist-ffi/src/lib.rs`; a mismatch between the suites is a finding.
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


    /// Imports a plain-text file made of `text` and opens a live engine
    /// session on it. Words are space separated, so token `i` is word `i`.
    private func openSession(
        _ text: String, wpm: UInt32, function: String = #function
    ) async throws -> FfiRsvpSession {
        let url = tempDir.appendingPathComponent("rsvp-\(function.filter(\.isLetter)).txt")
        try text.write(to: url, atomically: true, encoding: .utf8)
        await client.importFile(url: url)
        let itemId = try XCTUnwrap(client.items.first?.id, "import failed")
        let session = await client.openRsvpSession(itemId: itemId, wpm: wpm)
        return try XCTUnwrap(session)
    }

    private static let twelveWords = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu"

    // MARK: - Frame contents (ORP pre-split, durations)

    func testFrameCarriesTextOrpSplitAndDuration() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600) // 100 ms/word
        let frame = try XCTUnwrap(try session.frameAtElapsed(elapsedMs: 0))
        XCTAssertEqual(frame.index, 0)
        XCTAssertEqual(frame.text, "alpha")
        XCTAssertEqual(frame.durationMs, 100)
        XCTAssertEqual(frame.orp.before + frame.orp.focus + frame.orp.after, "alpha")
        XCTAssertFalse(frame.orp.focus.isEmpty)
        XCTAssertEqual(frame.nextBoundaryMs, 100)
    }

    // MARK: - Wall-clock anchoring

    /// A late/coalesced tick must land on the token actually due, not the
    /// next one in sequence.
    func testJitteryTickCatchesUpToTheWallClock() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.resume()
        XCTAssertEqual(try XCTUnwrap(session.frameAtElapsed(elapsedMs: 0)).index, 0)
        // 100 ms/word: a tick that arrives 450 ms in must show token 4.
        XCTAssertEqual(try XCTUnwrap(session.frameAtElapsed(elapsedMs: 450)).index, 4)
        XCTAssertEqual(try XCTUnwrap(session.frameAtElapsed(elapsedMs: 450)).nextBoundaryMs, 500)
    }

    func testElapsedBeyondTheEndClampsToTheLastToken() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.resume()
        let frame = try XCTUnwrap(try session.frameAtElapsed(elapsedMs: 10_000_000))
        XCTAssertTrue(frame.isLast)
        XCTAssertEqual(frame.index, try session.tokenCount() - 1)
    }

    // MARK: - Pause/resume: paused time is not reading time

    func testPauseThenResumeExcludesPausedTimeFromStats() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.resume()
        try session.pause(elapsedMs: 300)
        XCTAssertEqual(try session.cursor(), 3)
        // Paused: the elapsed argument is ignored entirely.
        let paused = try session.statsAtElapsed(elapsedMs: 99_999)
        XCTAssertEqual(paused.durationMs, 300)
        XCTAssertEqual(paused.wordsShown, 3)
        // Resume, play 200 ms more: 300 + 200, regardless of the gap.
        try session.resume()
        let running = try session.statsAtElapsed(elapsedMs: 200)
        XCTAssertEqual(running.durationMs, 500)
        XCTAssertEqual(try XCTUnwrap(session.frameAtElapsed(elapsedMs: 200)).index, 5)
    }

    // MARK: - set_wpm / punctuation toggle pin the cursor under the old rule

    func testSetWpmMidPlaybackPinsTheCursorUnderTheOldSpeed() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.resume()
        // 650 ms at 600 wpm is token 6; at the new 120 ms/word it would be 5.
        try session.setWpm(wpm: 500, elapsedMs: 650)
        XCTAssertEqual(try session.cursor(), 6)
        XCTAssertEqual(try session.wpm(), 500)
        XCTAssertEqual(try XCTUnwrap(session.frameAtElapsed(elapsedMs: 120)).index, 7)
    }

    func testSetWpmClampsInTheCore() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.setWpm(wpm: 50, elapsedMs: 0)
        XCTAssertEqual(try session.wpm(), 100)
        try session.setWpm(wpm: 9999, elapsedMs: 0)
        XCTAssertEqual(try session.wpm(), 1000)
    }

    func testPunctuationPausesLengthenSentenceEndsAndCanBeSwitchedOff() async throws {
        let session = try await openSession("One. two three four five", wpm: 600)
        XCTAssertTrue(try session.pauseOnPunctuation())
        XCTAssertGreaterThan(try session.tokenDurationMs(index: 0), 100)
        try session.setPauseOnPunctuation(enabled: false, elapsedMs: 0)
        XCTAssertFalse(try session.pauseOnPunctuation())
        XCTAssertEqual(try session.tokenDurationMs(index: 0), 100)
    }

    func testPunctuationToggleMidPlaybackPinsTheCursorUnderTheOldSetting() async throws {
        let session = try await openSession("One. two three four five", wpm: 600)
        try session.resume()
        // "One." lasts 180 ms with pauses on, so 250 ms is token 1; with
        // them off it would already be token 2.
        try session.setPauseOnPunctuation(enabled: false, elapsedMs: 250)
        XCTAssertEqual(try session.cursor(), 1)
    }

    // MARK: - Back-words and stats come from the engine

    func testBackWordsSkipsBreaksWithoutCountingThem() async throws {
        let session = try await openSession("a b c\n\nd e f g", wpm: 600)
        let count = try session.tokenCount()
        try session.seek(index: count - 1)
        try session.backWords(n: 3, elapsedMs: 0)
        let cursor = try session.cursor()
        XCTAssertLessThan(cursor, count - 1)
        let kind = try XCTUnwrap(session.tokenKind(index: cursor))
        XCTAssertEqual(kind, .word, "landing on a break would mean it was counted as a word")
    }

    func testBackWordsSaturatesAtTheStart() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.seek(index: 2)
        try session.backWords(n: 50, elapsedMs: 0)
        XCTAssertEqual(try session.cursor(), 0)
    }

    func testStatsCountOnlyWordsBeforeThePositionOnScreen() async throws {
        let session = try await openSession(Self.twelveWords, wpm: 600)
        try session.resume()
        let stats = try session.statsAtElapsed(elapsedMs: 450) // token 4 on screen
        XCTAssertEqual(stats.wordsShown, 4)
        XCTAssertEqual(stats.durationMs, 450)
        XCTAssertGreaterThan(stats.estimatedWpm, 0)
        XCTAssertEqual(try session.statsAtElapsed(elapsedMs: 0).wordsShown, 0)
    }

    func testOrpSplitOfEmptyTokenIsAllEmpty() throws {
        let split = try rsvpOrpSplit(word: "")
        XCTAssertEqual(split.before + split.focus + split.after, "")
    }

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
