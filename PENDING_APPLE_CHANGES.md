# PENDING_APPLE_CHANGES.md

Apple-platform changes identified during a non-macOS session that must be applied in the
next macOS session. On macOS, the SessionStart hook (`tools/detect-platform.sh`) flags
this file when it contains entries.

**Delete each entry's block after the change is applied and committed.**

<!-- Template — copy to add an entry (real headings start at column 0 with "## Pending Apple Change"; this example is indented so the detector ignores it):

    ## Pending Apple Change — [YYYY-MM-DD]
**File:** [path]
**Change required:** [what]
**Reason:** [why]
**Related commit/PR:** [hash or PR]
**Action:** [steps to apply]

-->

<!-- No pending items. -->

## Pending Apple Change — 2026-09-23 — new finding, not a regression
**File:** none — environment/tooling observation only
**Change required:** none. `xcodebuild test` (scheme `GISTmacOS`) reliably hangs at `KeychainKeyProviderIntegrationTests` (specifically observed at `testConcurrentGetOrCreateKeyCallsConvergeOnOneKey`, 20 concurrent `SecItemAdd`/`SecItemCopyMatching` calls) when run from an unattended/headless session on this machine — reproduced twice in a row. Almost certainly a macOS Keychain access-authorization prompt that needs a human to dismiss; it isn't a code bug (`FlowViewTests` and `GISTTests` — 28 tests total, including both new tests above — pass cleanly *before* the hang every time).
**Reason:** blocks `LibraryFiltering`/`LibrarySelection`/sort and `ThemeManager` suites from ever running in this kind of session, since XCTest runs suites in sequence and never reaches them. Also leaves a few orphaned, uniquely-UUID-suffixed test Keychain items behind when a hung run is killed instead of completing tearDown (harmless — isolated from the real production key by the existing test seam — but `security dump-keychain | grep 'com.gist.macos.encryption-at-rest.TEST-'` found 4 after this session's two interrupted runs).
**Action:** when running the full suite unattended, expect to need a human present to click through a Keychain prompt the first time (or run interactively in Xcode once to pre-authorize). Periodically clean up stray `...TEST-<uuid>` Keychain items. No code action needed.


## Pending Apple Change — 2026-10-04
**File:** `apps/apple/macOS/RsvpView.swift` (`RsvpPlayer`, `RsvpWallClockEngine`, `RsvpStats`, `RsvpSessionVM`/`TokenVM`/`RsvpConfigVM`), `apps/apple/Shared/CoreClient.swift` (`startRsvp`), `apps/apple/Tests/RsvpPacingTests.swift`
**Change required:** adopt the new FFI pacing API in place of the hand-ported pacing/ORP/stats logic. Specifically:
- `GistCore.openRsvpSession(itemId:wpm:) -> FfiRsvpSession` (new; `gist-ffi`) replaces `startRsvp(itemId:wpm:) -> String` + client-side JSON decoding. `startRsvp` is **unchanged and still works** — this is a migration, not a break.
- `FfiRsvpSession.frameAtElapsed(elapsedMs:) -> FfiRsvpFrame?` replaces `RsvpWallClockEngine.index(at:)`/`tokenDurationMs(_:)` and the whole hand-ported pacing table. The frame carries `index`, `text`, `kind`, `orp` (pre-split), `durationMs`, `nextBoundaryMs`, `isLast`, `tokenCount` — so one call per tick replaces the current per-token arithmetic, and `nextBoundaryMs` is the value the wall-clock tick should sleep until.
- `pause(elapsedMs:)`, `resume()`, `seek(index:)`, `backWords(n:elapsedMs:)`, `setWpm(_:elapsedMs:)`, `cursor()`, `playState()`, `wpm()`, `tokenCount()`, `tokenText(index:)`, `tokenKind(index:)`, `isLastToken(index:)`, `tokenDurationMs(index:)`, `tokenAtElapsed(elapsedMs:)` mirror `RsvpSession`'s surface one-for-one, with identical semantics (`setWpm` still pins the cursor under the *old* speed; `pause` still excludes paused time).
- `orpSplit(index:)` / the free `rsvpOrpSplit(word:)` return `FfiOrpSplit { before, focus, after }` and replace Swift's own ORP index math. Pre-split deliberately: `gist_rsvp::orp_index` returns a **byte** offset into UTF-8, which does not transfer safely to Swift `String` indices.
- `statsAtElapsed(elapsedMs:) -> FfiSessionStats` replaces `RsvpStats.wordsShown(tokens:upTo:)` and `RsvpStats.achievedWpm(wordsShown:elapsedMs:)`. Same definitions (word tokens strictly before the current index; achieved pace; paused gaps excluded) — one behavioural difference worth knowing: while paused, `statsAtElapsed` ignores its `elapsedMs` argument and reports from the pinned cursor, which is the correct reading and what the Rust test pins.
**Reason:** `docs/windows-development-plan.md` §4.3 — "Apple's `RsvpPlayer` hand-ports `token_duration_ms` and its punctuation helpers from `crates/gist-rsvp` … **Windows must not become a third copy.**" W4 R1 did step 1 (expose the pacing over FFI) so Windows consumes the engine instead of copying it. Adopting it on Apple too deletes the **second** copy of the pacing table (`tokenDurationMs` and its `endsSentence`/`endsClause`/`isNumeral` helpers), the second copy of the ORP rule, and the second copy of the stats arithmetic — the drift risk those three copies carry is exactly what §4.3 item 3 schedules this for. There is also a free performance win: `token_at_elapsed` is now memoised in `gist-rsvp` (flat ~22 ns instead of 78.8 µs at the 10-minute-soak point), which the Swift copy does not have.
**Related commit/PR:** this branch's `perf(rsvp): memoise token_at_elapsed, add live stats and a pacing bench` and `feat(ffi): expose the RSVP pacing engine as a live FFI object` (W4 role R1, `docs/w4-agent-roles.md` §2 R1)
**Action:**
1. On macOS, regenerate the Swift bindings — `./tools/build-core-xcframework.sh` (or `./tools/gen-bindings.sh`). `apps/apple/Generated/gist_ffi.swift` is **gitignored**, so it is stale in every working copy until regenerated; the new API is invisible to Swift until then. The C# bindings for the same surface were regenerated and verified on the Windows session (`apps/windows/GIST.Core/Generated/` is likewise gitignored).
2. Rewrite `RsvpPlayer` to hold an `FfiRsvpSession` and drive it from the existing wall-clock tick, calling `frameAtElapsed` per tick and sleeping until `nextBoundaryMs`. Delete `RsvpWallClockEngine`'s duplicated duration/punctuation logic and `RsvpStats` once the call sites are moved.
3. Keep `RsvpPacingTests.swift`'s *assertions* — they encode the semantics (jittery-tick catch-up, pause/resume excluding paused time, mid-playback WPM change) and should now be asserted against the FFI session. The Rust side has equivalent tests (`crates/gist-ffi/src/lib.rs`: `jittery_ticks_never_accumulate_drift`, `pause_then_resume_excludes_paused_time`, `set_wpm_pins_the_cursor_under_the_old_speed`), so a mismatch between the two suites is a real finding, not a porting detail.
4. **Mutate-while-playing pattern (found by W4 R2, Windows).** `RsvpSession::seek`/`back_words`/`set_wpm` do **not** bank the elapsed play time into `elapsed_at_pause` when called while `Playing` (only `pause` does), so a client that calls them mid-play and merely restarts its elapsed counter makes `statsAtElapsed().durationMs`/`estimatedWpm` under-count the time before the mutation. The Windows `RsvpPlaybackController` therefore always does `pause(elapsed)` -> mutate at elapsed 0 -> `resume()` -> re-anchor. Apple's adoption of `FfiRsvpSession` should use the same sequence for seek/back-5/WPM changes (or the Rust engine should bank elapsed in those mutators — a shared-engine decision, deliberately not made from a Windows session).
5. **Cannot be done or verified from a Windows session.** `xcodebuild`/`xcodegen` are not installed on this machine (`[GIST ENV]` 2026-10-04: `apple(macOS/iOS)=NO`), and Apple code is off-limits from a non-macOS session per `CLAUDE.md`. The Rust/FFI change itself is shared code that Apple's Swift consumes, so `apple-build` CI should be watched on the PR that lands it even before this adoption happens — `start_rsvp`'s JSON is unchanged and pinned by test, but nothing Swift-side has been compiled against the new surface here.
