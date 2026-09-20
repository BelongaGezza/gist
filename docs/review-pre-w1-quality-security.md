# Pre-W1 quality and security review

**Date:** 2026-09-20 · **Repo state:** `main` @ `0bc737b` · **Reviewer role:** independent software-quality and security consultant
**Scope:** the Windows work created so far (W0 spikes, ADR-015…018, DPAPI key provider, WinUI hello app, generator pin, hooks/tooling, docs) and the architecture defined for the rest of the Windows development (`docs/windows-ui-spec.md`, `docs/windows-development-plan.md`), including the shared Rust core and CI it depends on.
**Status of this document:** findings and recommendations. **Update 2026-09-20 (branch `fix/w1-gates`): gate items Q1–Q4 are fixed and verified locally; see the "Resolution of gate items" section at the end.** Q5–Q14 remain open.

## 1. Executive summary

The W0 work is solid where it was tested: the bindings spike, the DPAPI provider logic, the generator review and the hand-built WinUI app all check out on re-execution. **Four items should be resolved before W1 starts**, because W1 builds on top of them:

| # | Finding | Severity |
|---|---|---|
| **Q1** | In **release** builds the FFI panic guard does not work: `panic = "abort"` turns any panic into a process kill instead of `GistError::InternalPanic`. Reproduced. Affects Apple builds too. | **High** |
| **Q2** | `gist_ffi.dll` needs `VCRUNTIME140.dll`; a clean machine or MSIX without the VC++ runtime will fail to load the core. Not in the plan. | **High** (ship blocker) |
| **Q3** | The Rust core is not tested on Windows in CI (matrix is ubuntu + macos only). | Medium |
| **Q4** | `main` is unprotected, there is no CODEOWNERS, and the repo now ships hooks/scripts that execute on contributors' machines. | Medium |

One correction to earlier work in this project: **ADR-015 and the W0 spike report state that a bad callback "surfaces as `InternalPanic` and the process survives" (`ffi_catch!` holds across the C# boundary). That was verified on a debug build only and is false for release builds (Q1).** The ADR wording should be amended.

## 2. Method and limits

Method: read the code and configuration directly; re-ran builds, tests and scripts; probed behaviour empirically where a claim mattered; checked the GitHub repository settings via the API. Agent-produced work was treated as untrusted until reproduced.

Not covered: the Rust core beyond the FFI/profile/dependency surface (it has its own audits, `docs/security-review-v2.md`); the Apple app except where the Windows spec claims parity with it; a clean-machine VM run; MSIX install (Developer Mode is off here); FlaUI/UI Automation; ARM64 runtime; performance. "Not verified" is stated wherever it applies.

## 3. Findings

### Q1 — Release builds abort on panic; the documented panic containment is not true in production. **High**
- **Evidence:** `Cargo.toml` sets `[profile.release] panic = "abort"`. `ffi_catch!` (`crates/gist-ffi/src/lib.rs`) relies on `catch_unwind`, which cannot catch a panic under `panic = "abort"`. `docs/development-plan-v2.md` lists both as "belt-and-braces" (§ lines 31–32, 393); they are mutually exclusive. **Reproduction:** I built `gist-ffi --release` and ran the W0 spike against the release DLL. All functional checks passed, then the deliberate bad-callback check killed the process with exit code `0xC0000409` (Windows fail-fast); no `InternalPanic` was produced and no "SPIKE RESULT" line printed. The debug DLL returns `InternalPanic` as documented.
- **Why it was missed:** all Rust tests and the spike run in debug, where unwinding is enabled. The shipped Apple libraries are built with `--release` (`tools/build-core-xcframework.sh`), so the shipped behaviour differs from the tested behaviour.
- **Impact:** any parser or core panic on a hostile or malformed file crashes the whole app (denial of service, loss of unsaved UI state) instead of a handled error. Not memory-unsafe (abort is safe), but it contradicts security policy F1 ("panics map to `InternalPanic`, never propagate") and `docs/product-spec-reader-app-v3.md` (line 262: "every exported function wraps its body in `catch_unwind`, panics map to a typed error").
- **Recommendation:** decide explicitly. (a) **Preferred:** use `panic = "unwind"` for the release profile so `ffi_catch!` works, delete the "belt-and-braces" claim, and add a release-mode test that forces a panic through an exported function (e.g. a test-only feature flag) and asserts `InternalPanic`, run in CI on release. (b) If abort is kept for size/hardening reasons, drop the catch guarantee from all docs and treat panics as crashes, moving risky parsing behind a process boundary. Apply the same decision to Apple. Amend ADR-015 either way.

### Q2 — The core DLL depends on the dynamic VC++ runtime. **High (ship blocker)**
- **Evidence:** both `target/debug/gist_ffi.dll` and `target/release/gist_ffi.dll` import `VCRUNTIME140.dll` (plus UCRT `api-ms-win-crt-*`, which ships with Windows). No `crt-static` setting exists (no `.cargo/config.toml`). `VCRUNTIME140.dll` is not part of Windows; it comes from the Visual C++ Redistributable.
- **Impact:** on a machine without the redistributable the app fails with `DllNotFoundException` at first FFI use. Unpackaged builds and MSIX packages without a VCLibs framework dependency are both exposed. Not mentioned in the plan or ADR-017.
- **Recommendation:** either link the CRT statically for the MSVC target (`-C target-feature=+crt-static` scoped to `[target.x86_64-pc-windows-msvc]`; then confirm the imports no longer include `VCRUNTIME140`) or declare the VC runtime dependency in the MSIX. Add a clean-VM smoke test to W6 and a W1 CI check that inspects the DLL's imports.

### Q3 — No Windows job for the Rust core in CI. **Medium**
- **Evidence:** `.github/workflows/core-test.yml` matrix is `[ubuntu-latest, macos-latest]`. The planned `windows-build.yml` triggers only on Windows/FFI paths and is not yet written. Today's Windows result (117 tests, clippy, fmt clean) exists only as a local run.
- **Impact:** path, file-locking and line-ending regressions in the core would go unnoticed until a Windows developer runs the tests.
- **Recommendation:** add `windows-latest` to the `core-test` matrix now (the suite passes locally). Verify the runner has MSVC (needed for the bundled SQLite and `blake3` C builds). Keep the pinned-SHA and `permissions: contents: read` rules.

### Q4 — Repository governance: unprotected `main`, no CODEOWNERS, executable repo config. **Medium**
- **Evidence:** `gh api …/branches/main/protection` returns "Branch not protected"; no `CODEOWNERS` file. `.claude/settings.json` registers hooks that run `tools/*.sh` at session start and before every edit. (During this session `main` was pushed to directly on two occasions; the absence of protection is why that was possible.)
- **Impact:** a malicious or careless PR that alters `tools/*.sh`, `.claude/settings.json`, `.github/`, `deny.toml` or a `.csproj` becomes code execution on maintainers' machines or a CI supply-chain change, with no required review or required checks. Positives: secret scanning and push protection are enabled; Dependabot alerts and security updates are on; all 8 alerts are closed.
- **Recommendation:** enable branch protection on `main` (required PR, required checks `quality` and both `test` jobs, no force-push); add `CODEOWNERS` covering `.claude/`, `tools/`, `.github/`, `deny.toml`, `Cargo.toml`, `apps/windows/**/*.csproj`, `docs/adr/`; add `windows-build` to required checks once it exists.

### Q5 — The Apple-edit guard is bypassable and documented as "enforced". **Medium**
- **Evidence (tested):** `tools/guard-apple-edits.sh` matches case-sensitively on `*/apps/apple/*`. On Windows the filesystem is case-insensitive, and these inputs exit 0 (allowed): `C:\x\apps\APPLE\a.swift`, `C:\x\Apps\Apple\a.swift`, `C:\x\APPS\apple\a.swift`, and the relative `apps/apple/a.swift`. Only a literal-case absolute path is blocked. Edits made through the Bash tool are not covered by the matcher at all.
- **Impact:** the platform-safety rule (parent CLAUDE.md §7) is not reliably enforced on the machine where it matters most (non-macOS). Low blast radius (an unintended Apple-file edit is visible in review), but "enforced by hook" overstates it.
- **Recommendation:** lower-case and normalise the path before matching, handle relative paths, and describe the hook as a safety net, not enforcement. Real enforcement: CODEOWNERS on `apps/apple/` and `PENDING_APPLE_CHANGES.md` review.

### Q6 — Generator supply chain: fork dependency without a mirror. **Medium**
- **Evidence:** `docs/security-review-windows-bindgen.md` (acceptable with conditions; I independently confirmed one commit ahead of v0.11.0 touching 22 files, all 109 `DllImport`s target only `gist_ffi`, and no process/network/registry/dynamic-code APIs in the generated file). The pin is a commit on an outside fork (`dennisameling/uniffi-bindgen-cs`) of an unmerged PR.
- **Impact:** if the fork or branch is deleted or rewritten, CI and new developer machines cannot reproduce the build; the SHA is only fetchable while the fork exists. Every build compiles third-party code (`cargo install --git`), and `cargo audit` of its lockfile shows unmaintained-crate warnings and one test-fixture-only vulnerability (`slab`).
- **Recommendation:** mirror the pinned commit into a repository the project controls (fork under the org) and pin to that; cache the built generator in CI keyed on the SHA; add the generated-output hash check (normalised line endings) the review describes; keep the "never set `exclude`" CI guard; re-review at any re-pin.

### Q7 — ARM64 cannot be built on this machine, and the plan treats it as routine. **Medium**
- **Evidence:** `cargo check -p gist-ffi --target aarch64-pc-windows-msvc` fails: the build scripts of `libsqlite3-sys` and `blake3` cannot find `cl.exe` for aarch64 (no ARM64 MSVC C++ build tools installed). `ring` (TLS) is also in the tree and compiles C/assembly.
- **Impact:** the plan lists ARM64 as a v1.0 question and a W6 build; without the ARM64 toolchain component and an ARM64 runner it cannot even be compile-tested.
- **Recommendation:** decide ARM64 scope at W1 start. If in scope, add the "MSVC ARM64 build tools" VS component to `SETUP_NOTES.md`, cross-compile in CI, and plan real ARM64 hardware or a `windows-11-arm` runner for the runtime test. Otherwise mark it as a post-1.0 fast follow.

### Q8 — Some parity claims in the UI spec are not automatically true in C#. **Medium (functional)**
- **Evidence:** Apple sorts with `localizedCaseInsensitiveCompare` (locale-aware); C# `List<T>.Sort` is unstable and culture/ordinal comparison must be chosen deliberately. Apple's find offsets come from `distance(from: startIndex, …)` over Swift `Character`s (grapheme clusters); C# strings index UTF-16 code units, so offsets diverge for emoji, surrogate pairs and combining marks. The spec (§4.3, §7.2) says "reproduce exactly / same semantics".
- **Impact:** a 1:1 port of the Apple tests can pass while user-visible behaviour differs (item order, highlight positions).
- **Recommendation:** specify the comparer (`StringComparer.CurrentCultureIgnoreCase` or `CompareInfo` with `IgnoreCase`) and use a stable sort (`OrderBy`); implement find offsets over `StringInfo` text elements (or `Rune`s, choosing one and testing it with multi-byte/combining fixtures); add those fixtures to the ported tests.

### Q9 — DPAPI key provider: sound core, several design points to settle in W1. **Medium**
Reviewed `DpapiKeyProvider.cs` and its 13 tests (re-run: 13/13 on repeated runs). The create race, no-overwrite publish, tamper/truncate/entropy cases and "never generate a new key over a bad file" are correct.
- **Every `CryptographicException` is classed "corrupt".** Transient causes (profile not loaded, credential state) would route users to the "encrypted items unrecoverable" state. The UX must offer retry and must never delete or recreate anything; distinguish cases where the API allows.
- **Packaged vs unpackaged key locations differ** (LocalState vs `%LOCALAPPDATA%\GIST`). The key and the encrypted store must always move together; a dev build must not point at a production store with a different key. Add a test that the key directory is derived from the same root as the store.
- **No ACL hardening** on the key directory; a crashed create leaves a `.tmp` file that is never reaped; the returned key array is not zeroed (accepted in the ADR).
- **Recovery:** DPAPI blobs are not portable across users/machines. Profile reset or migration makes encrypted items unrecoverable (WR7). A recovery-key export should be decided before encryption is offered broadly, since new imports are plaintext by default (ADR-014) and encryption is opt-in per item.
- **In release builds a wrong-length key aborts the process** (see Q1), not `InternalPanic` as ADR-016 implies; the eager managed-side validation the ADR requires in `CoreClient` becomes the only reliable check.

### Q10 — File deletion and Windows path semantics are untested. **Medium**
- **Evidence:** `Core::remove_items` deletes blobs and the stored copy best-effort and logs failures at debug (per CLAUDE.md; not exercised on Windows). No test uses long paths (> 260 chars), UNC paths, reserved names, trailing dots, or OneDrive placeholder files.
- **Impact:** on Windows an antivirus scan, the indexer or an open reader commonly holds a file open; the UI would report "removed" while a stored copy remains, which matters for the "remove deletes my data" promise. Long package-family paths under LocalState shrink the path budget.
- **Recommendation:** surface partial-delete failures over FFI (or sweep orphaned copies on next launch); add Rust and `GIST.Core.Tests` cases with long, Unicode, space-containing and locked-file paths; enable long-path support in the app manifest.

### Q11 — Test strategy assumptions not yet proven. **Low–Medium**
The plan states UI Automation (FlaUI) "works on this machine, closing the gap Apple has". Not verified: no FlaUI run has been attempted, hosted CI runners' interactive-session behaviour for a packaged WinUI app is untested, and MSIX install needs Developer Mode (off). The DPAPI tests only run on Windows and there is no Windows job yet (Q3). **Recommendation:** make a FlaUI smoke against `winui-hello` a W1 task, and treat CI UI automation as unproven until it runs green on a real runner.

### Q12 — Spike code in `main` and floating dependency versions. **Low**
`apps/windows/spikes/**` is throwaway (hard-coded relative-path arithmetic, temp-dir usage, manual DLL copy). The key-provider spike uses floating NuGet ranges (`10.*`, `17.*`, `2.*`), contrary to the pinning policy, and there is no `packages.lock.json`. **Recommendation:** promote into `GIST.Core` in W1 with exact versions and lock files, keep spikes out of the solution and delete them after promotion; add the Dependabot `nuget` entry in the same PR (the `/fuzz` alert episode is the precedent). The spike's bad-callback check must be made profile-aware after Q1.

### Q13 — Documentation and decision hygiene. **Low**
ADR-015 is "Accepted" while ADR-016…018 are "Proposed" but the plan already treats their decisions as resolved (.NET 10, DPAPI, MSIX). `CLAUDE.md` still carries long macOS-era status text (marked historical, but hard to trust). Several plan estimates and the ".NET 10 is LTS" statement are unverified. **Recommendation:** reconcile ADR statuses before W1 exit, keep `CLAUDE.md` short by moving history into `docs/`, and label each plan estimate and third-party claim as verified or not.

### Q14 — CI hygiene. **Low**
The fuzz workflow now runs on floating `nightly` (`RUSTUP_TOOLCHAIN: nightly`), so results are not reproducible; pin a dated nightly. `ubuntu-latest` migrates to Ubuntu 26 on 2026-10-19 and Node 20 actions are being force-upgraded; both need a watch. `cargo audit` steps install the tool from crates.io on every run (acceptable but unpinned).

## 4. What was verified as sound

- Bindings spike reproduces: 20/20 checks (debug build); typed `DrmProtected`, callback invocation from Rust, read-after-encrypt, 64 concurrent calls, removal leaving the user's original.
- `cargo test --workspace` 117 passed, `clippy -D warnings` and `fmt --check` clean on Windows; `cargo audit` clean on both lockfiles; Dependabot alerts all closed; secret scanning and push protection on.
- WinUI 3 (Windows App SDK 2.5.1, net10.0) builds with plain `dotnet build`, launches, responds, loads `gist_ffi.dll`; the "VS workload not needed to build" claim holds.
- Generator: pinned by full SHA, built `--locked`, output reproducible modulo line endings; `--no-format` applied.
- Workflows declare `permissions: contents: read` and pin actions to SHAs; the fuzz workflow is fixed and green.

## 5. Recommended actions

**Before W1 starts (gate):**
1. Q1 — choose unwind vs abort for release, test it in release mode, fix docs and ADR-015; apply to Apple via `PENDING_APPLE_CHANGES.md`.
2. Q2 — link the CRT statically (or declare the runtime dependency) and add a DLL-imports check.
3. Q3 — add `windows-latest` to the `core-test` matrix.
4. Q4 — branch protection and CODEOWNERS.

**In W1:** Q5 (guard normalisation), Q6 (mirror the generator), Q7 (ARM64 decision), Q8 (specify comparers and text-element offsets), Q9 design points, Q12 (pinning, lock files, Dependabot `nuget`).

**Before W2/W6:** Q10 (path and locked-file tests), Q11 (FlaUI proof, Developer Mode, clean-VM run), Q13–Q14.

**Residual risks accepted for now:** unreviewed third-party generator source (mitigated by pin and review); single-engineer schedule estimates; full-trust MSIX being weaker than the macOS sandbox (stated honestly in ADR-017).

## 6. Resolution of gate items (2026-09-20, branch `fix/w1-gates`)

| Item | Fix | Verification |
|---|---|---|
| **Q1** | Release profile now `panic = "unwind"` (with an explanatory comment). New `test-panic` feature, `gist_ffi::test_support::ffi_panic_probe` and `examples/panic_containment.rs` (crate type gains `rlib` so the example can link). CI runs the probe in release mode on every OS. `docs/development-plan-v2.md` (lines 32, 393, 519) and ADR-015 corrected; Apple verification logged in `PENDING_APPLE_CHANGES.md`. | Baseline reproduced the failure (probe dies `0xc0000409` with `panic = "abort"`); after the fix it prints `ok: panic contained as InternalPanic`, exit 0. The full C# spike against the **release** DLL passes 20/20, including bad callback -> `InternalPanic` with the process alive. |
| **Q2** | `.cargo/config.toml`: `+crt-static` for the two Windows MSVC targets. `tools/check-dll-imports.sh` fails on any `VCRUNTIME140`/`MSVCP140`/`concrt140` import; CI runs it on the Windows leg. | The check fails on the old release DLL (imports `VCRUNTIME140.dll`) and passes on the rebuilt one (no VC++ or UCRT imports; DLL grew 5.06 -> 6.07 MB). Clean-machine load is **not** verified (no clean VM). |
| **Q3** | `windows-latest` added to the `core-test` matrix. | **Verified on GitHub:** PR #31 ran `test (windows-latest)` green (5m42s): `cargo test --workspace`, clippy, fmt, the release panic probe and the DLL-imports check all succeeded. |
| **Q4** | `.github/CODEOWNERS` added. Branch protection applied to `main` (2026-09-20): PR required (0 approvals, solo maintainer), required checks `quality`, `test (ubuntu-latest)`, `test (macos-latest)`, `test (windows-latest)`, no force-push, no deletion, **enforced for admins**. The path-filtered Apple `build` job is deliberately not required (it would block unrelated PRs). | Read back via `gh api .../branches/main/protection`. To relax for an emergency: repo Settings > Branches, or set `enforce_admins` false. Code-owner review is **not** required (a sole owner cannot approve their own PR). |

Residual notes: `rlib` was added to `gist-ffi`'s crate types (a tooling-only change; the Apple script consumes only the static library). Static CRT is untested on ARM64 (Q7) and on a clean machine (W6).
