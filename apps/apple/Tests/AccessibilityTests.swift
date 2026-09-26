import AVFoundation
import XCTest
@testable import GIST

/// A real `AVSpeechSynthesizer` subclass whose speech/transport methods are
/// all no-ops -- used only so `TtsPlayer`'s integration tests below never
/// touch the real speech-audio engine (no actual sound, no dependency on
/// speech-voice assets being installed/downloaded, no risk of the same kind
/// of headless-environment hang this codebase already documents for
/// `KeychainKeyProviderIntegrationTests`). `TtsPlayer`'s own state-transition
/// logic under test lives entirely in `TtsStateMachine`, which these calls
/// exercise identically whether or not anything is actually spoken.
private final class NoOpSpeechSynthesizer: AVSpeechSynthesizer {
    override func speak(_ utterance: AVSpeechUtterance) {}
    override func stopSpeaking(at boundary: AVSpeechBoundary) -> Bool { true }
    override func pauseSpeaking(at boundary: AVSpeechBoundary) -> Bool { true }
    override func continueSpeaking() -> Bool { true }
}

/// Role R6 (Accessibility + TTS, development-plan-v2.md §3.9) test coverage.
/// Everything here is genuinely automated: pure math (`ContrastRatio`), a
/// pure state machine (`TtsStateMachine`), a pure text-extraction function
/// (`TtsTextExtraction`), and `Theme.swift`'s literal color values resolved
/// via real `NSColor` bridging -- no live VoiceOver run, no real speech
/// audio, no interactive UI. See this role's final report for the explicit
/// breakdown of what is and isn't covered by an automated test versus what
/// still needs a person with a real device and a real screen reader.
@MainActor
final class AccessibilityTests: XCTestCase {
    // MARK: - ContrastRatio: known-value sanity checks

    func testRelativeLuminanceOfPureBlackIsZeroAndPureWhiteIsOne() {
        XCTAssertEqual(ContrastRatio.relativeLuminance(red: 0, green: 0, blue: 0), 0, accuracy: 0.0001)
        XCTAssertEqual(ContrastRatio.relativeLuminance(red: 1, green: 1, blue: 1), 1, accuracy: 0.0001)
    }

    func testContrastRatioOfBlackOnWhiteIsTheMaximumTwentyOneToOne() {
        let ratio = ContrastRatio.ratio(r1: 0, g1: 0, b1: 0, r2: 1, g2: 1, b2: 1)
        XCTAssertEqual(ratio, 21, accuracy: 0.01)
    }

    func testContrastRatioOfIdenticalColorsIsExactlyOne() {
        let ratio = ContrastRatio.ratio(r1: 0.4, g1: 0.4, b1: 0.4, r2: 0.4, g2: 0.4, b2: 0.4)
        XCTAssertEqual(ratio, 1, accuracy: 0.0001)
    }

    func testContrastRatioIsSymmetricRegardlessOfArgumentOrder() {
        let ratioAB = ContrastRatio.ratio(r1: 0.1, g1: 0.2, b1: 0.3, r2: 0.9, g2: 0.8, b2: 0.7)
        let ratioBA = ContrastRatio.ratio(r1: 0.9, g1: 0.8, b1: 0.7, r2: 0.1, g2: 0.2, b2: 0.3)
        XCTAssertEqual(ratioAB, ratioBA, accuracy: 0.0001)
    }

    // MARK: - ContrastRatio against Theme.swift's real, live color values

    /// The actual pass/fail assertion the R6 brief asked for: every theme's
    /// foreground-on-background pair (the color pair used for actual body
    /// text everywhere in the app) must clear the WCAG AA normal-text bar.
    /// Resolved via real `NSColor` bridging of `Theme.swift`'s live `Color`
    /// values (see `ContrastRatio.srgbComponents`), not independently
    /// re-typed RGB numbers, so this test tracks `Theme.swift` automatically
    /// if its literals ever change.
    func testThemeForegroundOnBackgroundContrastMeetsWcagAaForEveryTheme() throws {
        for theme in Theme.allCases {
            let ratio = try XCTUnwrap(
                ContrastRatio.ratio(theme.foreground, theme.background),
                "could not resolve sRGB components for \(theme)"
            )
            XCTAssertGreaterThanOrEqual(
                ratio, ContrastRatio.minimumTextContrast,
                "\(theme): foreground/background contrast is \(ratio), below the \(ContrastRatio.minimumTextContrast):1 text bar"
            )
        }
    }

    /// `Theme.accent` is also used as text color (the ORP pivot character in
    /// `RsvpView.OrpWordView`), so it needs the same 4.5:1 bar as
    /// foreground/background, not just the looser 3:1 UI-component bar --
    /// see `Theme.accent`'s doc comment for the real, if modest, shortfall
    /// this test would have caught against the system `.blue` this codebase
    /// used before role R6's fix.
    func testThemeAccentTextContrastMeetsWcagAaForEveryTheme() throws {
        for theme in Theme.allCases {
            let ratio = try XCTUnwrap(
                ContrastRatio.ratio(theme.accent, theme.background),
                "could not resolve sRGB components for \(theme)"
            )
            XCTAssertGreaterThanOrEqual(
                ratio, ContrastRatio.minimumTextContrast,
                "\(theme): accent/background contrast is \(ratio), below the \(ContrastRatio.minimumTextContrast):1 text bar"
            )
        }
    }

    /// OLED's entire reason to exist is a pure-black background distinct
    /// from Dark's slightly-lifted one (already covered by
    /// `ThemeManagerTests`) -- confirm that difference doesn't accidentally
    /// tank contrast: OLED's true black should give *equal or better*
    /// contrast than Dark for the same foreground/accent colors, never
    /// worse.
    func testOledBackgroundNeverReducesContrastRelativeToDark() throws {
        let darkForegroundRatio = try XCTUnwrap(ContrastRatio.ratio(Theme.dark.foreground, Theme.dark.background))
        let oledForegroundRatio = try XCTUnwrap(ContrastRatio.ratio(Theme.oled.foreground, Theme.oled.background))
        XCTAssertGreaterThanOrEqual(oledForegroundRatio, darkForegroundRatio)
    }

    // MARK: - TtsRate

    func testTtsRateStepsAreStrictlyIncreasing() {
        let ordered: [TtsRate] = [.slower, .slow, .normal, .fast, .faster]
        for (a, b) in zip(ordered, ordered.dropFirst()) {
            XCTAssertLessThan(a.utteranceRate, b.utteranceRate, "\(a) should be slower than \(b)")
        }
    }

    func testTtsRateNormalMatchesAvSpeechUtteranceDefault() {
        XCTAssertEqual(TtsRate.normal.utteranceRate, Float(AVSpeechUtteranceDefaultSpeechRate))
    }

    func testTtsRateAllCasesStayWithinAvSpeechUtteranceLegalRange() {
        for rate in TtsRate.allCases {
            XCTAssertGreaterThanOrEqual(rate.utteranceRate, AVSpeechUtteranceMinimumSpeechRate)
            XCTAssertLessThanOrEqual(rate.utteranceRate, AVSpeechUtteranceMaximumSpeechRate)
        }
    }

    // MARK: - TtsStateMachine (pure playback-phase transitions)

    func testStartWithValidIndexEntersSpeakingAtThatIndex() {
        XCTAssertEqual(TtsStateMachine.start(blocks: ["a", "b", "c"], from: 1), .speaking(index: 1))
    }

    func testStartWithOutOfRangeIndexOrEmptyBlocksIsIdle() {
        XCTAssertEqual(TtsStateMachine.start(blocks: ["a"], from: 5), .idle)
        XCTAssertEqual(TtsStateMachine.start(blocks: ["a"], from: -1), .idle)
        XCTAssertEqual(TtsStateMachine.start(blocks: [], from: 0), .idle)
    }

    func testPauseFromSpeakingPinsTheCurrentIndex() {
        XCTAssertEqual(TtsStateMachine.pause(.speaking(index: 2)), .paused(index: 2))
    }

    func testPauseIsANoOpWhenAlreadyPausedOrIdle() {
        XCTAssertEqual(TtsStateMachine.pause(.paused(index: 2)), .paused(index: 2))
        XCTAssertEqual(TtsStateMachine.pause(.idle), .idle)
    }

    func testResumeFromPausedReturnsToSpeakingAtTheSameIndex() {
        XCTAssertEqual(TtsStateMachine.resume(.paused(index: 3)), .speaking(index: 3))
    }

    func testResumeIsANoOpWhenAlreadySpeakingOrIdle() {
        XCTAssertEqual(TtsStateMachine.resume(.speaking(index: 3)), .speaking(index: 3))
        XCTAssertEqual(TtsStateMachine.resume(.idle), .idle)
    }

    func testAdvanceMovesToTheNextBlockWhileMoreRemain() {
        XCTAssertEqual(TtsStateMachine.advance(.speaking(index: 0), blockCount: 3), .speaking(index: 1))
        XCTAssertEqual(TtsStateMachine.advance(.speaking(index: 1), blockCount: 3), .speaking(index: 2))
    }

    func testAdvancePastTheLastBlockGoesIdle() {
        XCTAssertEqual(TtsStateMachine.advance(.speaking(index: 2), blockCount: 3), .idle)
    }

    func testAdvanceIsANoOpWhenPausedOrIdle() {
        XCTAssertEqual(TtsStateMachine.advance(.paused(index: 0), blockCount: 3), .paused(index: 0))
        XCTAssertEqual(TtsStateMachine.advance(.idle, blockCount: 3), .idle)
    }

    func testStopIsAlwaysIdleRegardlessOfPriorPhase() {
        XCTAssertEqual(TtsStateMachine.stop(), .idle)
    }

    func testPlaybackPhaseBlockIndexReflectsSpeakingOrPausedAndNilWhenIdle() {
        XCTAssertNil(TtsPlaybackPhase.idle.blockIndex)
        XCTAssertEqual(TtsPlaybackPhase.speaking(index: 4).blockIndex, 4)
        XCTAssertEqual(TtsPlaybackPhase.paused(index: 5).blockIndex, 5)
    }

    // MARK: - TtsTextExtraction

    /// Mirrors `FlowViewTests`'s `paragraphDocumentJSON` builder -- kept as
    /// its own private copy rather than sharing one across test files/
    /// classes, since `XCTestCase` subclasses don't share private helpers
    /// and this is a small, self-contained fixture.
    private func documentJSON(sections: [(id: String, paragraphs: [String])]) -> Data {
        let sectionsJSON = sections.map { section -> String in
            let blocksJSON = section.paragraphs.map { text in
                """
                {"Paragraph": {"runs": [{"text": "\(text)", "bold": false, "italic": false, "code": false}]}}
                """
            }.joined(separator: ",")
            return """
            {"id": "\(section.id)", "heading": null, "blocks": [\(blocksJSON)]}
            """
        }.joined(separator: ",")
        let json = """
        {
            "id": "doc1",
            "metadata": {"title": "TTS Test Doc", "author": null},
            "sections": [\(sectionsJSON)]
        }
        """
        return Data(json.utf8)
    }

    func testSpeakableBlocksFlattensAllSectionsInDocumentOrder() throws {
        let json = documentJSON(sections: [
            (id: "s0", paragraphs: ["Hello", "World"]),
            (id: "s1", paragraphs: ["Goodbye"]),
        ])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        XCTAssertEqual(TtsTextExtraction.speakableBlocks(for: document), ["Hello", "World", "Goodbye"])
    }

    func testSpeakableBlocksOfDocumentWithNoBlocksIsEmpty() throws {
        let json = documentJSON(sections: [(id: "s0", paragraphs: [])])
        let document = try JSONDecoder().decode(FlowDocumentVM.self, from: json)
        XCTAssertTrue(TtsTextExtraction.speakableBlocks(for: document).isEmpty)
    }

    // MARK: - TtsPlayer (thin integration over the pure state machine above)

    func testTtsPlayerStartTracksCurrentBlockIndexAndIsSpeaking() {
        let player = TtsPlayer(synthesizer: NoOpSpeechSynthesizer())
        player.start(blocks: ["one", "two", "three"], from: 1)
        XCTAssertTrue(player.isSpeaking)
        XCTAssertEqual(player.currentBlockIndex, 1)
    }

    func testTtsPlayerPauseThenResumeRoundTripsThroughTheSameIndex() {
        let player = TtsPlayer(synthesizer: NoOpSpeechSynthesizer())
        player.start(blocks: ["one", "two"], from: 0)
        player.pause()
        XCTAssertTrue(player.isPaused)
        XCTAssertEqual(player.currentBlockIndex, 0)
        player.resume()
        XCTAssertTrue(player.isSpeaking)
        XCTAssertEqual(player.currentBlockIndex, 0)
    }

    func testTtsPlayerStopReturnsToIdleAndClearsCurrentBlockIndex() {
        let player = TtsPlayer(synthesizer: NoOpSpeechSynthesizer())
        player.start(blocks: ["one"], from: 0)
        player.stop()
        XCTAssertFalse(player.isSpeaking)
        XCTAssertFalse(player.isPaused)
        XCTAssertNil(player.currentBlockIndex)
    }

    func testTtsPlayerStartingPastTheEndOfBlocksLeavesItIdle() {
        let player = TtsPlayer(synthesizer: NoOpSpeechSynthesizer())
        player.start(blocks: ["one"], from: 5)
        XCTAssertFalse(player.isActive)
        XCTAssertNil(player.currentBlockIndex)
    }
}
