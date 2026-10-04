# W4 Execution Plan — Agent Roles & Team Structure

**Status:** ADOPTED 2026-10-04 (Team Leader session, Windows 11 / ProArt13).
**Scope authority:** `docs/windows-development-plan.md` §5 W4 and §4.3; `docs/windows-ui-spec.md` §7.1, §9, §10.
This file refines them into assignable work, modelled on `docs/m6-agent-roles.md`.

**Platform scope:** Windows session (`[GIST ENV]` 2026-10-04: `os=windows`, `rust-core=yes`,
`windows-shell=yes`, `apple(macOS/iOS)=NO`; tools present `cargo rustup dotnet`, missing
`xcodebuild xcodegen msbuild`). **No Apple-side edits may be made from this session** — not
`apps/apple/`, `ios/`, `macos/`, Xcode projects, entitlements or `Info.plist`. Apple-side adoption
of anything added here is logged in `PENDING_APPLE_CHANGES.md` instead.

## Why W4 and not M6

`docs/m6-agent-roles.md` §2 reports every role either done (R1/R2/R3/R7) or deliberately not run
this milestone (R4 paginated view per D3, R5/R6 iOS per D5). Its own header scopes it to a macOS
session and says Windows W4–W6 "is not part of this plan." The next *planned, approved,
buildable-on-this-machine* phase is therefore **W4 — RSVP reader** from
`docs/windows-development-plan.md` §5. Nothing here is invented scope: W4's goals, exit criterion
and the §4.3 pacing mandate were written on 2026-09-20 and are unchanged.

Picked up alongside it, because they are Windows- or Rust-only and already open in the register:

- **`F27`** — no Windows security/quality review since the pre-W1 gate (2026-09-20), despite W1–W3
  landing since. M6 R7 explicitly deferred it to "a Windows session"; this is one.
- **`F34`** — `tools/fetch-pdfium.sh` hardening (bash, not Apple code).
- **`F29`** — `deny.toml` hygiene (prune the unused `BSD-2-Clause` entry, note duplicates).

## Baseline measured before any role was spawned (2026-10-04, this machine)

| Gate | Result |
|---|---|
| `git fetch` + `git pull --ff-only` | `main` was 36 commits behind; fast-forwarded to `16d5cbf` |
| `cargo fmt --check` | clean |
| `cargo test --workspace` | **280 passed**, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | **2 errors on `main`** — Rust 1.99.0 added `clippy::cloned_ref_to_slice_refs` and `gist-core` test code hit it twice. Fixed as the first commit on the integration branch (test-only, no behaviour change). Clean after. |
| `cargo deny check bans licenses sources` | `bans ok, licenses ok, sources ok` |
| `tools/build-core-windows.sh x64 debug` | staged `apps/windows/native/x64/gist_ffi.dll` |
| `dotnet build GIST.sln -warnaserror` | succeeded, **0 warnings** |
| `dotnet test GIST.Core.Tests` | **224 passed**, 0 failed |

Note: `rustup` auto-installed `1.99.0-x86_64-pc-windows-msvc` on first use this session. The
`[GIST ENV]` banner's "pinned Rust 1.88.0" line is stale — `rust-toolchain.toml` has said 1.99.0
since PR #89.

## 0. Team Leader role

Same protocol as `docs/m6-agent-roles.md` §0, with the Apple verification step replaced by the
Windows one, because this session cannot run `xcodebuild`:

1. **Preflight.** `PENDING_APPLE_CHANGES.md` / `PENDING_WINDOWS_CHANGES.md`, `git status` clean,
   `main` current. **Done** — see the baseline table above. `PENDING_WINDOWS_CHANGES.md`'s single
   entry (2026-09-30, icon assets) is assigned to R3 below.
2. **Compute the ready batch** from §2.
3. **Spawn the batch in parallel**, one `Agent` call per ready role, `isolation: "worktree"`,
   self-contained prompts.
4. **Integrate one at a time** onto `integration/w4-2026-10-04` off `main`, resolving conflicts by
   hand, re-running the gate between merges.
5. **Advance** when dependencies are satisfied.
6. **Full verification on the integrated tree:** `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`,
   `cargo deny check bans licenses sources`, `tools/build-core-windows.sh x64 debug`,
   `tools/gen-bindings-cs.sh`, `dotnet build GIST.sln -warnaserror`,
   `dotnet test GIST.Core.Tests`, and the FlaUI suite with `GIST_RUN_UI_TESTS=1` on an unlocked
   interactive desktop. **`xcodebuild` cannot run here** — any shared-Rust change must be flagged
   for Apple CI (`apple-build`) rather than claimed verified.
7. **Independent security/quality review** of the integrated change set by a fresh agent that built
   none of it (`F30`, adopted 2026-10-04 — permanent step).
8. **Update the record:** `CLAUDE.md`, `docs/windows-development-plan.md` (W4 status + closeout),
   `PLATFORM_VERIFICATION.md`, `CHANGELOG.md`, this file's §2 status column, and
   `PENDING_APPLE_CHANGES.md` for the pacing back-port.
9. **Report, then stop before publishing.** No push, no PR — that is the user's separate decision.

## 1. Already done — do not re-assign

- W0–W3 in full (see each phase's closeout in `docs/windows-development-plan.md` §5): bindings
  spike + ADR-015..018; shell / `CoreClient` / DPAPI key custody / CI; the whole Library screen
  with its dialogs, removal and encrypt semantics; sidebar + Collection screen + tag editor + 5
  themes + Appearance dialog; 224 `GIST.Core.Tests` and 28 FlaUI tests.
- `gist_rsvp::RsvpSession` itself — the pacing engine exists and is tested in Rust. W4 exposes it,
  it does not rewrite it.
- `GistCore::start_rsvp` / `save_progress` already exist over FFI and in the C# bindings.

## 2. Backlog → roles

| # | Role | Scope | Depends on | Status |
|---|---|---|---|---|
| R1 | Rust+FFI — expose RSVP pacing over FFI (plan §4.3 item 1) | `crates/gist-rsvp`, `crates/gist-ffi`, `PENDING_APPLE_CHANGES.md` | — | Spawned 2026-10-04 |
| R2 | C# — RSVP reader view + playback timer (spec §7.1) | `apps/windows/GIST.Core`, `apps/windows/GIST.App`, `GIST.Core.Tests`, `GIST.App.UITests` | R1 | Blocked on R1 |
| R3 | Windows asset verification + `F34` / `F29` hygiene | `apps/windows/GIST.App/Assets`, `PENDING_WINDOWS_CHANGES.md`, `tools/fetch-pdfium.sh`, `deny.toml` | — | Spawned 2026-10-04 |
| R4 | Review — `F27` Windows shell security review | `docs/security-review-windows.md` | R1–R3 as landed | Blocked |
| R5 | Review — `F30` standing security/quality pass over the W4 diff | `docs/security-quality-review-<date>.md` | R1–R3 as landed | Blocked |

Batches: **Batch 1:** R1, R3. **Batch 2:** R2. **Batch 3:** R4, R5 (both fresh agents; R5 must not
be an agent that wrote any W4 code).

### R1 — Rust+FFI: pacing over FFI

Plan §4.3 is explicit that Windows **must not** become a third hand-port of `token_duration_ms`
and its punctuation helpers. Apple's `RsvpPlayer` / `RsvpWallClockEngine` is the second, and its
drift bug was only fixed on 2026-09-24. So W4 starts in Rust:

- A new `#[derive(uniffi::Object)]` type in `gist-ffi` wrapping `gist_rsvp::RsvpSession`, built from
  a document's token stream, exposing the session's whole surface — `token_at_elapsed`,
  `token_duration_ms`, `seek`, `pause`, `resume`, `back_words`, `set_wpm`, `stats` — plus the
  read-only accessors a UI needs (`cursor`, token count, token text at an index, play state).
- Interior mutability with poison-tolerant locking (`gist-store`'s convention:
  `.unwrap_or_else(|p| p.into_inner())`, never a bare `.lock().unwrap()`), every export wrapped in
  `ffi_catch!`.
- **Performance:** `RsvpSession::token_at_elapsed` is an O(n-from-cursor) scan that recomputes
  `token_duration_ms` — including Unicode string analysis — for every token it passes. W4's exit
  criterion is a 10-minute soak at 600 WPM, i.e. roughly 6 000 tokens scanned per tick with no
  intervening pause. Measure it, and memoise if warranted; any memoisation must be invalidated by
  `set_wpm` / `seek` / `back_words` and proven equivalent to the naive path by test.
- Apple adoption is **not** done here (platform rule): log it in `PENDING_APPLE_CHANGES.md`.

### R2 — C#: RSVP reader view

Spec §7.1 in full, plus the §9 shortcuts and §10 contracts that touch it. Playback is a
`DispatcherQueueTimer` re-anchored to a monotonic clock (`Stopwatch`) calling R1's engine — never
a C# copy of the pacing table. UI-free logic lands in `GIST.Core` so xunit can test it without
WinUI, matching the `LibraryFiltering` / `ThemeManager` split already established there.

### R3 — Asset verification + hygiene

`PENDING_WINDOWS_CHANGES.md`'s 2026-09-30 entry (icon assets regenerated during a macOS session
without `System.Drawing` or ImageMagick available to preview them), plus two small open register
rows, `F34` and `F29`.

### R4 / R5 — Reviews

`F27` (Windows shell: FFI error paths, key-file permissions, package capabilities, bindings trust
boundary) and the standing `F30` pass over everything W4 landed.

## 3. Explicitly out of scope for W4 agents

- W5 (flow reader) and W6 (hardening / accessibility / packaging / MSIX / ARM64 runtime gate).
- Anything under `apps/apple/`, `ios/`, `macos/`, Xcode projects, entitlements, `Info.plist`.
- Signing / notarisation, MSIX install and run (Developer Mode is off), the ARM64 runtime gate and
  the clean-machine gate — all still W6-, credential- or hardware-gated.
- Human visual passes (`docs/qa-manual-clickthrough-windows.md`), Narrator, Accessibility Insights.
