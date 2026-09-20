# GIST for Windows — Development Plan

**Status:** Draft v1, 2026-09-20. Companion to `docs/windows-ui-spec.md` (what to build); this file is how and in what order.
**Supersedes:** the one-line "Windows is a C-ABI follow-on" in `docs/development-plan-v2.md` (§ layout, R11) and the "C ABI (C#/WinRT)" row in the product spec §8. Where they disagree with this file on the Windows binding approach, this file wins once ADR-015 is accepted (W0).

---

## 1. Verified starting point (measured on the Windows dev machine, 2026-09-20)

| Question | Result |
|---|---|
| Does the Rust core build on Windows? | **Yes.** `cargo build -p gist-ffi` succeeded (14.6 s incremental, pinned Rust 1.88.0, MSVC toolchain), producing `target/debug/gist_ffi.dll` (11 MB) and `gist_ffi.lib`. `gist-ffi` already declares `crate-type = ["staticlib","cdylib"]`, so a DLL comes for free. `rusqlite` is `bundled`, so no system SQLite is needed. |
| Rust tests/clippy on Windows? | **Yes (W0, 2026-09-20).** `cargo test --workspace` 117 passed / 0 failed; `clippy -D warnings` and `fmt --check` clean. |
| .NET SDK? | **Installed in W0:** .NET SDK 10.0.401 (via winget). Was runtimes only. |
| Visual Studio? | Community 2026 (18.10) with **only the Native Desktop C++ workload**, Build Tools 2022 (17.14). **No WinUI/Windows App SDK/MSIX/.NET-desktop workload — and none is needed to build**: a hand-made WinUI 3 project builds and runs with plain `dotnet build` (W0, `apps/windows/spikes/winui-hello`, Windows App SDK **2.5.1** from NuGet, net10.0-windows10.0.19041.0). The workload is only for the XAML designer / Hot Reload / debugger. Windows SDK 10.0.26100.0; Windows App Runtime 2.5.1 installed machine-wide. Developer Mode is off. |
| MSBuild / cl on PATH | No (normal for VS installs; use a Developer shell or `vswhere`). Note: `link.exe` on the Git Bash PATH is Git's coreutils `link`, not MSVC's — cargo finds MSVC via `vswhere` so builds work, but ad-hoc `link` calls from Git Bash will hit the wrong one. |
| C# bindings generator for uniffi? | **Resolved in W0 (see ADR-015).** No release supports uniffi 0.32 (latest, v0.11.0, is 0.31), but upstream PR #176 does; pinned commit `0fc022a` built and drove the real core through a 20-check .NET spike, all passing. Condition: unreviewed unmerged third-party generator, pinned by SHA. |

Everything below that depends on an unverified row is marked as a gate.

---

## 2. Architecture

```
apps/windows/
├── GIST.sln
├── GIST.Core/              net10.0 (no UI). Generated uniffi bindings, CoreClient, view models,
│   │                       JSON models, theme logic, persistence stores. Headless-testable.
│   └── Generated/          gitignored, produced by tools/gen-bindings-cs.sh (mirrors apps/apple/Generated)
├── GIST.Core.Tests/        xunit. Real GistCore against a temp dir per test (no mocks) — mirrors GISTTests.swift
├── GIST.App/               WinUI 3, packaged (MSIX), net10.0-windows10.0.19041.0, Windows App SDK 2.x. Views, XAML, dialogs, navigation
├── GIST.App.UITests/       FlaUI (UI Automation) smoke + click-through tests
└── native/                 build output staging: gist_ffi.dll (x64, arm64)
tools/
├── build-core-windows.sh   cargo build -p gist-ffi --release --target x86_64-pc-windows-msvc (+ aarch64); runs under Git Bash
└── gen-bindings-cs.sh      uniffi bindgen → apps/windows/GIST.Core/Generated
```

**Why the split:** Apple's 49 tests are valuable precisely because `CoreClient` and the filtering/theme logic are testable without driving SwiftUI. `GIST.Core` keeps that property: everything except XAML lives in a plain library that `dotnet test` runs on any CI runner, including without a desktop session.

**Layering (mirrors Apple):**

| Apple | Windows |
|---|---|
| `GistCore` (uniffi Swift) | `GistCore` (generated C#, P/Invoke into `gist_ffi.dll`) |
| `CoreClient` (`@MainActor ObservableObject`, single touch point to FFI) | `CoreClient` : `ObservableObject` (CommunityToolkit.Mvvm). Every FFI call runs on `Task.Run`; results marshalled back via `DispatcherQueue`. **No FFI call on the UI thread.** |
| `LibraryItemVM`, `CollectionVM`, `LibrarySelection`, `LibraryFiltering` | same names, C# `record`s + static `LibraryFiltering` |
| `ThemeManager`/`Theme`/`ThemeSelection` | same, `LocalSettings` instead of `UserDefaults`, `UISettings` instead of `NSApp.effectiveAppearance`; same `initialSystemIsDark` test seam |
| `FlowDocumentModel.swift` VMs + `TypographySettings`/`SearchState`/`SectionNavigator` | ported 1:1 into `GIST.Core`, `System.Text.Json` with `SnakeCaseLower` and a custom converter for serde's externally-tagged `Block` enum (`{"Heading":{"level":..,"text":..}}`) |
| `KeychainKeyProvider` | `DpapiKeyProvider` (§4.5) |

Platform guards: `apps/windows/**` is Windows-only by definition and needs no per-block guards. Any shared file that gains Windows-specific behaviour (Rust, `docs/`, CI) follows the parent guard convention, and the `[PLATFORM: Windows]` comment style is used in Rust with `#[cfg(windows)]`.

---

## 3. Key decisions (each becomes an ADR in W0)

| ADR | Decision | Rationale / alternatives |
|---|---|---|
| **015 Windows FFI binding** | Prefer **uniffi C# bindings** over the *same* proc-macro `gist-ffi` used by Apple, contingent on the W0 spike. Fallback, in order: (a) pin/upgrade a compatible `uniffi-bindgen-cs` + matching uniffi, if the Apple side can move in lockstep; (b) hand-written `extern "C"` shim (what ADR-001 originally reserved for Windows) — the surface is ~25 exported methods and constructors with mostly `String`/`Vec`/record returns, so ~2–3 extra weeks; (c) C++/WinRT wrapper (rejected: adds a third language). | One binding source keeps Apple and Windows behaviourally identical and avoids a second API to keep in sync. The reason ADR-001 avoided a dylib ("avoid a second signed dylib") was macOS-specific; on Windows the DLL simply ships in the MSIX. |
| **016 Windows key custody** | 32-byte data key generated once, protected with **DPAPI `ProtectedData` (scope CurrentUser)**, stored under `LocalState`. Implements the `KeyProvider` callback (`get_or_create_key() -> Vec<u8>`, exactly 32 bytes). Race-safe create (file created with `CreateNew`, loser re-reads) — the same race Apple fixed with `SecItemAdd`. | DPAPI ≈ Keychain for this threat model (protects against other users and offline disk access, not against malware running as the user). Credential Locker rejected (size/roaming semantics, no advantage). TPM-backed keys (CNG/Platform Crypto Provider) are a possible v1.1 hardening. |
| **017 Packaging & sandbox** | **MSIX**, full-trust WinUI 3 desktop app with package identity. Capabilities: `internetClient` only (URL import), no `broadFileSystemAccess`; all user files via `FileOpenPicker`. This is the Windows analogue of ADR-012's macOS sandbox. Storage in package `LocalState` (`gist.sqlite3` + `storage/`, same layout as Apple's Application Support/GIST). | Package identity gives clean install/uninstall, per-app data isolation, and required APIs (`ApplicationData`). Unpackaged dev builds are supported for the inner loop. Distribution channel (Microsoft Store vs signed sideload MSIX) decided at W6. |
| **018 UI stack** | WinUI 3 / Windows App SDK (latest stable at W0), **C# / .NET 8 LTS** (decision 2026-09-20: **.NET 10 LTS**, SDK 10.0.401 installed; .NET 8 support ends Nov 2026. The spike runs on net10.0), MVVM via CommunityToolkit.Mvvm, no other UI framework. | Matches product spec §8. Rust is the only non-C# code. |

Also recorded at W0: the R11 exception. The v2 plan says "do not begin Windows until the iOS shell has validated the core API is platform-neutral." Windows is being started deliberately; the mitigation is that W0/W1 *are* the neutrality test — anything the C# client needs that the FFI can't express cleanly is fixed in Rust once, for both platforms (§4.3).

---

## 4. Cross-cutting work

### 4.1 Environment prerequisites (W0, one-time, recorded in `SETUP_NOTES.md`)
1. Install a **.NET 8 SDK** (currently runtimes only).
2. (Optional) Add the VS "WinUI application development" workload for the designer/Hot Reload/debugger — not required to build (proved in W0). Enable **Developer Mode** (Settings > System > For developers, needs UAC) to install/run the MSIX locally.
3. `cargo install` the chosen bindgen (per ADR-015) — note this machine's cargo works, `rustup` pinned 1.88.0 is installed.
4. Add targets as needed: `rustup target add aarch64-pc-windows-msvc`.
5. Enable Windows Developer Mode (needed to deploy unsigned/dev MSIX).

### 4.2 Repo, CI and supply chain
- New workflow `.github/workflows/windows-build.yml` on `windows-latest`, triggered by PRs touching `apps/windows/**`, `crates/gist-ffi/**`, `crates/gist-core/**`, the two tools scripts: build core (x64), generate bindings, `dotnet build`, `dotnet test GIST.Core.Tests`. **All actions pinned to full commit SHAs, `permissions: contents: read`** (F3/F18 policy). Verify each pinned SHA resolves *before* relying on it (N6 lesson: unresolvable pins silently broke every workflow).
- Treat "green locally" as unproven until the workflow has passed on real GitHub Actions (the standing lesson from N6).
- **Dependabot:** add a `nuget` ecosystem entry for `/apps/windows` (the `/fuzz` gap that produced alerts #5–#8 is the cautionary tale — new manifests must be added to `dependabot.yml` in the same PR that introduces them).
- **Vulnerability gate:** `dotnet list package --vulnerable --include-transitive` in CI, failing on High/Critical. Add NuGet package licences to `docs/THIRD-PARTY.md`. `cargo deny` continues to cover Rust; CI must not regress `core-quality`.
- Cross-platform safety: the Windows workflow must not be required for PRs that don't touch its paths, and the Apple workflow must remain unaffected by Windows-only changes.

### 4.3 The pacing-engine problem (do it once, in Rust)
Apple's `RsvpPlayer` hand-ports `token_duration_ms` and its punctuation helpers from `crates/gist-rsvp`, and its `Task.sleep`-per-token loop drifts over long sessions (known gap, per CLAUDE.md). **Windows must not become a third copy.** Plan:
1. W4 starts with a small additive Rust change: expose the pacing to the UI over FFI (e.g. an `RsvpEngine` object: `duration_ms(index, wpm)` and/or `token_at_elapsed(ms)` using the existing wall-clock-anchored engine) plus tests in `gist-rsvp`/`gist-ffi`.
2. Windows consumes it via a `DispatcherQueueTimer` that re-anchors to a monotonic clock, so it does not drift.
3. The Apple app adopting the same call is **not** done from a Windows session; log it in `PENDING_APPLE_CHANGES.md` (the guard hook is only a safety net; CODEOWNERS/PR review is the real enforcement) with a pointer to the Rust API.
If the FFI addition is judged too large, the fallback is a faithful C# port with a golden-file test that runs the same token/config fixtures through Rust and C# and asserts identical durations, so drift between the copies is caught mechanically.

### 4.4 Testing strategy
| Layer | Tooling | What |
|---|---|---|
| Core logic | xunit in `GIST.Core.Tests`, real `GistCore` on a temp dir | Port every Apple test: import→list, search (incl. partial word), empty-query no-FFI, removal never touching originals (recompute the `sha256(bytes).ext` path in `originals/`), collections round-trip, tags/`listAllTags`/`listItemsByTag`, encrypt idempotency + read-after-encrypt through a read-capable client, sort (5 cases), `LibrarySelection` equality (component-wise), theme resolution/persistence/OLED-vs-Dark, Flow: TOC indent/clamp, scroll-position store clamping, `RangesOfSubstring` multi-byte, JSON decoding of all four block kinds |
| Key provider | xunit, real DPAPI | create-once, second call returns identical 32 bytes, **20 concurrent calls converge on one key** (parity with `KeychainKeyProviderIntegrationTests`), tamper/corrupt key file → clear error, not a silent new key |
| UI | FlaUI (UIA3) in `GIST.App.UITests` | Launch, import fixture, search, multi-select + remove, sort/filter, theme switch, open RSVP, play/pause, open Flow, TOC/find. UI Automation works on this machine (Apple's environment can't script its UI), so **the click-through checklist can be automated on Windows**, closing the gap the Apple side still has |
| Accessibility | Accessibility Insights for Windows + Narrator pass | W6 |
| Fixtures | reuse `fixtures/` corpus; add `apps/windows/GIST.Core.Tests/Fixtures/basic_ascii.txt` copy like Apple | |

`PLATFORM_VERIFICATION.md` is updated by every phase that verifies something, with date and machine.

### 4.5 Security requirements (carry over, do not relax)
- Every FFI failure handled: `GistError` → typed exception → user-facing dialog; `InternalPanic` must never crash the process and must never expose panic text.
- `DrmProtected` is matched by exception type, never by message string.
- Source paths are never logged above debug level; Windows logs must not contain document titles/paths in release builds.
- Encrypted-at-rest: `new_with_read_key` from first launch (ADR-014), otherwise a user-encrypted item becomes unreadable — the exact Apple bug fixed 2026-09-18. There is a test for it (§4.4).
- Only `internetClient` capability; no file-system broad access; no telemetry.
- Signing: release MSIX signed; dev builds use a self-signed test certificate never committed. Signing secrets only in GitHub encrypted secrets. Track alongside F10 (release pipeline).

---

## 5. Phases

Effort figures are rough single-engineer estimates assuming ~4-6 productive days/week and WinUI familiarity; they are planning aids, not commitments. Total ≈ **12–17 weeks** to feature parity with the current Apple app, plus buffer if the ADR-015 fallback is needed.

### W0 — Foundations and the binding spike (1–1.5 weeks) — **GATE** — status 2026-09-20: DONE except enabling Developer Mode (needs UAC; MSIX install/run unverified)
Goals: answer R1, make the environment reproducible.
- Install .NET SDK + VS workload (§4.1); run `cargo test --workspace`, `clippy -D warnings`, `fmt --check` on Windows; record results.
- **Spike:** generate C# bindings for the current `gist-ffi`; a console app that calls `new`, `health`, `import_file` on a fixture, `list_items`, `search_items`, and implements the `KeyProvider` callback; confirm records, `Vec<String>`, `Option`, errors→exceptions and callback interfaces all work against uniffi 0.32.
- Write ADR-015..018; add `SETUP_NOTES.md` Windows section (already stubbed); create `PENDING_WINDOWS_CHANGES.md` mirror of the Apple one for Windows-needed changes discovered in macOS sessions.
- **Exit:** spike passes (or ADR-015 fallback chosen with a measured cost); Windows results recorded in `PLATFORM_VERIFICATION.md`.

### W1 — Skeleton, CoreClient, CI (1.5–2 weeks)
- Solution structure (§2), `tools/build-core-windows.sh`, `tools/gen-bindings-cs.sh`, `.gitignore` for `Generated/` and build output.
- `GIST.Core`: `CoreClient` (init with `new_with_read_key` + DPAPI provider, storage paths, `Refresh`, import file/URL, error/DRM surfacing), VMs; ADR-016 `DpapiKeyProvider` + its tests.
- **From the W0 ADRs (requirements):** (1) `CoreClient` calls `DpapiKeyProvider.GetOrCreateKey()` eagerly in managed code *before* `NewWithReadKey`, because the uniffi callback has no error channel (a throw inside it becomes an opaque `InternalPanic`); a `KeyStoreCorruptException` must show a blocking "encrypted items unrecoverable" state and never generate a new key (ADR-016). (2) Promote `apps/windows/spikes/keyprovider` into `GIST.Core` with its 13 tests. (3) **Pin every NuGet version** (no `10.*`/`17.*` ranges), commit `packages.lock.json`, and add the Dependabot `nuget` entry in the same PR. (4) Decide clean-machine runtime: require Windows App Runtime 2.x or ship self-contained (ADR-017; neither verified). (5) CI: confirm the `windows-latest` image has .NET 10 and a Windows SDK before relying on it. (6) Generator conditions from `docs/security-review-windows-bindgen.md`: `--no-format` (done), a CI guard that `exclude` is never set, load `gist_ffi.dll` by absolute path / `DefaultDllImportSearchPaths`, regenerate in CI, redo the review before any re-pin. (7) Pre-W1 review gate items Q1–Q4 are fixed on `fix/w1-gates` (release `panic = "unwind"` + release-mode probe in CI, static CRT + DLL-imports check, `windows-latest` in `core-test`, CODEOWNERS/branch protection); Q5–Q14 of `docs/review-pre-w1-quality-security.md` are scheduled below and in W2/W5/W6 (triaged 2026-09-20).
- **Review items scheduled into W1 (Q5–Q9, Q12, Q13):**
  - **Q5** apple-edit guard path normalisation — delegated, PR `fix/q5-guard-normalise`; W1 must not start feature work on a branch that predates it. Hook is a safety net; CODEOWNERS is the enforcement.
  - **Q6** mirror the pinned generator commit into a repo the project controls, re-pin `tools/gen-bindings-cs.sh` to the mirror (same SHA), cache the built generator in CI keyed on the SHA, and add the normalised-line-ending generated-output hash check. Needs the maintainer to create the mirror (outward-facing) — first W1 task.
  - **Q7 — DECIDED 2026-09-20: ARM64 is in scope for v1.0** (maintainer decision). W1 must therefore: (a) install the VS "MSVC v143 ARM64 build tools" component (needs UAC) and get `cargo build -p gist-ffi --release --target aarch64-pc-windows-msvc` passing locally (currently fails: `libsqlite3-sys`/`blake3`/`ring` cannot find an ARM64 `cl.exe`); (b) add an ARM64 leg to CI (cross-compile on `windows-latest`, plus the DLL-imports check on the ARM64 DLL); (c) confirm the generated C# bindings and `+crt-static` behave on ARM64; (d) find runtime-test capacity now — a `windows-11-arm` hosted runner if available to this repo, otherwise real hardware — because W6 gates on it; (e) decide MSIX shape (per-arch packages vs a bundle) in ADR-017 before W6.
  - **Q8** `GIST.Core` states comparers up front: title/author sort uses `StringComparer.CurrentCultureIgnoreCase` via a stable `OrderBy`; find offsets are computed over `StringInfo` text elements (one choice, documented). Ported `LibraryFiltering`/find tests add emoji, surrogate-pair and combining-mark fixtures. Spec §4.3/§7.2 wording changed from "exactly" to the chosen semantics.
  - **Q9** `CoreClient`'s corrupt-key state offers Retry and never deletes or recreates anything; only a proven-corrupt blob (tamper/truncate) is "unrecoverable", other `CryptographicException`s are "temporarily unavailable, retry". Add a test that the key directory derives from the same root as the store (packaged vs unpackaged). Reap stale `.tmp` files, restrict the key directory ACL to the current user. Decide the recovery-key export before Encrypt is exposed in W2 (ADR-016 addendum).
  - **Q12** spike bad-callback check made profile-aware, spikes deleted after promotion, NuGet exact pins + lock files + Dependabot `nuget` (already item 3 above).
  - **Q13** reconcile ADR-017/018 statuses before W1 exit (018 -> Accepted once the shell lands; 017 stays Proposed until the W6 channel decision, with plan wording adjusted to say so); mark plan estimates/third-party claims verified or unverified.
- **Delegated 2026-09-20 (not W1 work):** Q14 fuzz nightly pin (`fix/q14-pin-nightly`), and Dependabot PR triage (`docs/dependabot-triage-2026-09-20`). Merge order for the dependency PRs comes from that triage; `aes-gcm`/`sha2`/`zip`/`uniffi` bumps must land (or be explicitly held) before W1's first C# PR because they change what the generator and DLL are built against.
- `GIST.App`: window, title bar, Mica, `NavigationView` shell with empty Library page, app icon per iconspecification, packaged-and-unpackaged run profiles.
- `windows-build.yml` green on real GitHub Actions; Dependabot `nuget` entry.
- **Exit:** app launches, lists imported items from the real store; CoreClient tests pass locally **and** in CI.

### W2 — Library (2–3 weeks)
- §4 of the spec in full: list, row template, multi-select, search (debounce + Ctrl+F), Sort, Filter (tags), Import File/URL, all dialogs, empty states, context menu, Remove and Encrypt flows with result summary, DRM dialog.
- Port `LibraryFiltering`/sort and their tests; FTS partial-word regression test.
- **Q10 (review):** Windows path and file-locking tests for remove/import — long (>260) paths, UNC, Unicode and space-containing paths, reserved names, trailing dots, a file held open by another process, OneDrive placeholders — in Rust (`gist-core`) and `GIST.Core.Tests`; enable long-path support in the app manifest; surface partial-delete failures over FFI or sweep orphaned copies on next launch, so the UI never reports "removed" while a stored copy remains (this is a Rust change shared with Apple: log it in `PENDING_APPLE_CHANGES.md`).
- **Q11 (review, start of W2):** FlaUI smoke test against the hello/skeleton app on this machine, then on a real `windows-latest` runner; enable Developer Mode (needs UAC) and verify MSIX install/run. Until it is green on a runner, CI UI automation is treated as unproven and W6 planning must not assume it.
- **Exit:** everything in spec §4 works; corresponding tests green; manual pass through spec §10 items 1–3, 8.

### W3 — Sidebar, collections, tags, themes (1.5–2 weeks)
- Sidebar/nav model, Collection screen (§5), New Collection flow, Add-to-Collection, Tag editor (§6), `ThemeManager` + 5 themes + theme dialog + high-contrast handling + Mica rules (§3.1).
- **Exit:** spec §5/§6/§7.3 done; theme tests (incl. OLED `#000000`, persistence, OS-follow seam) green; screenshots of all 5 themes on the library screen attached to the PR.

### W4 — RSVP reader (1–2 weeks)
- §4.3 pacing decision executed first (Rust change or golden-test port).
- RSVP view per spec §7.1: word, progress, WPM slider **+ NumberBox**, Play/Pause + Space, load/persist progress via `start_rsvp`/`save_progress`.
- Stretch: ←/→ and Ctrl+←/→ seeking, scrubber (from product spec, beyond Apple).
- **Exit:** a 10-minute soak at 600 WPM shows no cumulative drift vs. wall clock (measured, recorded); progress restores after restart.

### W5 — Flow reader (3 weeks) — largest phase
- `get_document_json` → models (custom serde-enum converter tested against real output from fixtures); virtualised block list; `RichTextBlock` runs; images; lists.
- Typography menu (size/font/line spacing; resolve the "Rounded" question), TOC flyout with indentation, in-document find with highlighting and F3, progress bar + persisted position with restore-before-first-render, keyboard navigation (native paging).
- Decide **Q4 (Windows OCR: `Windows.Media.Ocr` vs Tesseract)** here as a design note only; implementation stays M3 scope on all platforms (`import_image_with_ocr` is still `todo!()`).
- **Exit:** spec §7.2 complete; tests for TOC/find/store/decoder green; large-document (≥ 100k words fixture) scroll stays smooth with bounded memory (measure and record; target UI thread never blocked > 50 ms).

### W6 — Hardening, accessibility, packaging (2–3 weeks)
- FlaUI automation of the click-through checklist (mirror of `docs/qa-manual-clickthrough-m2.md`, as `docs/qa-manual-clickthrough-windows.md`, then automate what's automatable).
- Narrator + Accessibility Insights pass, keyboard-only pass, text-scale 150 %/200 %, high-contrast pass, contrast audit of all themes.
- MSIX build in CI (unsigned artifact), signing plan, SmartScreen/Store decision (ADR-017), ARM64 build and MSIX per-arch/bundle output (in scope, Q7).
- **ARM64 runtime gate (Q7):** the release DLL and app run and pass the smoke suite (DPAPI key create/read, import, search, RSVP, flow open) on real ARM64 Windows (hardware or a `windows-11-arm` runner); cross-compile alone does not satisfy this gate.
- **Clean-machine gate (Q2/Q11):** run the release build on a clean VM with no VC++ redistributable and no Windows App Runtime (whichever ADR-017 chose), confirm `gist_ffi.dll` loads and the core works; static-CRT was only import-checked, never loaded on a clean box.
- **Q14 watch items:** `ubuntu-latest` moves to Ubuntu 26 on 2026-10-19; pinned nightly for fuzz needs a periodic bump.
- Security review of the Windows shell (FFI error paths, key file permissions, package capabilities); add `docs/security-review-windows.md`.
- **Exit:** manual click-through complete with no open High issues; MSIX installs/uninstalls cleanly on a clean machine (or VM); all CI green on real Actions.

---

## 6. Parity and status matrix (to be maintained in `PLATFORM_VERIFICATION.md` as work lands)

| Feature | Apple status (2026-09-20) | Windows phase |
|---|---|---|
| Import txt/epub/docx, URL, DRM error | done | W2 |
| Library list, multi-select, search, sort, tag filter | done | W2 |
| Remove (copy-on-import semantics), Encrypt, lock badge | done | W2 |
| Sidebar, collections, tag editor | done | W3 |
| Themes (system/light/dark/sepia/OLED) | done | W3 |
| RSVP reader | done (drifting pacing) | W4 |
| Flow reader (typography, TOC, find, progress) | done | W5 |
| Accessibility, packaging, CI | Apple CI green | W6 |
| OCR, annotations, TTS, paginated, PDF, export | not built | out of scope |

---

## 7. Risks

| ID | Risk | L | I | Mitigation |
|---|---|---|---|---|
| **WR1** | No release of a C# generator supports uniffi 0.32 (only unmerged PR #176) | H (realised) | M | Pinned by SHA, built `--locked`; review generated code and the pinned diff before W1 closes; re-pin to an official release when one exists; hand C ABI fallback (~2–3 wk). See ADR-015 |
| WR2 | WinUI 3 virtualised `RichTextBlock` list performance/selection quirks in the flow view | M | M | Prototype the block list in W5 day 1 with the ≥100k-word fixture; fall back to per-section paragraphs or `WebView2` only if measured unacceptable (would need an ADR) |
| WR3 | Pacing copies diverge (Rust/Swift/C#) | H | M | §4.3: single Rust source, or golden-file test |
| WR4 | Team lacks WinUI/MSIX experience | M | M | W1 is deliberately thin; keep XAML simple, standard Fluent controls only |
| WR5 | Windows work changes shared Rust and breaks the Apple build | L | H | All `crates/**` changes need `cargo test --workspace` + Apple CI green before merge; Apple adoption logged in `PENDING_APPLE_CHANGES.md`, never edited from Windows |
| WR6 | A CI/Actions pin that doesn't resolve (recurrence of N6) | M | M | Verify SHAs against the GitHub API before merging; treat first real Actions run as the test |
| WR7 | DPAPI key loss on Windows profile reset makes encrypted items unrecoverable | L | H | Same data-loss profile as Apple Keychain reset; document it, and surface a clear "cannot decrypt this item" state rather than a crash; consider an optional recovery-key export in v1.1 |
| WR8 | Dependency drift (NuGet/Windows App SDK) creating alerts | M | L | Dependabot nuget + CI vulnerability gate (§4.2) |

---

## 8. Open questions

| Q | Decide by |
|---|---|
| Distribution channel: Microsoft Store vs signed sideload MSIX (affects signing cost and update story) | W6 |
| "Rounded" font option on Windows (drop vs Trebuchet MS mapping) | W5 |
| Windows OCR engine (`Windows.Media.Ocr` vs Tesseract) — spec Q4/Q8 | W5 (design note), implement M3 |
| ~~.NET 8 vs .NET 10 LTS~~ resolved: .NET 10 | done W0 |
| ~~ARM64 as a v1.0 requirement or fast-follow~~ resolved: **v1.0 requirement** (maintainer, 2026-09-20). Open sub-items: runtime-test hardware/runner by end of W1; MSIX per-arch vs bundle (ADR-017) | decided; sub-items W1 |
| Mirror the pinned bindgen fork under a project-controlled repo (Q6) | Day 1 of W1 |
| Recovery-key export for encrypted items (Q9) | Before Encrypt ships in W2 |
| Adopt the pacing FFI (§4.3) on Apple in the same release | Next macOS session |

---

## 9. Definition of done for "Windows v1 matches Apple M2"

1. Every **[P]** item in the UI spec works and has a passing automated test or a checked line in the click-through checklist.
2. `dotnet test` (Core) and the FlaUI suite are green in CI on real GitHub Actions.
3. `cargo test --workspace`, `clippy -D warnings`, `fmt --check`, `cargo deny check` still pass; Apple `apple-build` unaffected.
4. `PLATFORM_VERIFICATION.md` shows every Windows row `verified <date>`, and `PENDING_APPLE_CHANGES.md` lists every **[+]** back-port.
5. Security review recorded; no open High findings.
