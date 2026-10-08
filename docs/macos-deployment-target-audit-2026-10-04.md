# macOS deployment-target audit (M7 R5, D1 evidence) — 2026-10-04

Base: integration commit `1668bca`. Toolchain: Xcode 27.0 (27A266a), macOS SDK 27.0, Swift 6.4, host macOS 27.0 (Darwin 27.0.0), arm64.
Scope: evidence only. No product source, `project.yml` or `CLAUDE.md` was edited. The decision (ratify 26.5 vs restore 14/15) is the user's.

## 1. Where the deployment target is set or implied

| Location | Value | Notes |
|---|---|---|
| `apps/apple/project.yml` `options.deploymentTarget.macOS` | `26.5` | project-level default |
| `apps/apple/project.yml` target `GISTmacOS` `MACOSX_DEPLOYMENT_TARGET` | `26.5` | duplicate of the above |
| `apps/apple/macOS/Info.plist` `LSMinimumSystemVersion` | `26.5` | what makes Launch Services refuse older Macs |
| `project.yml` `options.xcodeVersion` | `16` | XcodeGen project-format hint only; not a toolchain gate |
| `project.yml` `SWIFT_VERSION` | `5.9` | language mode, independent of OS floor |
| `tools/build-core-xcframework.sh` | **nothing** | no `MACOSX_DEPLOYMENT_TARGET`, no `-mmacosx-version-min`, no `RUSTFLAGS` (see section 2: this is why C objects inherit the host OS) |
| `.cargo/config.toml` | Windows `crt-static` only | no macOS settings |
| `.github/workflows/apple-build.yml`, `release-macos.yml` | `macos-latest`, `setup-xcode latest-stable` | no deployment-target pin; Xcode and runner float |
| `docs/BUILDING-macos.md`, `README.md` | no stated minimum OS | (checked with grep) |
| `docs/development-plan-v2.md` Q9 | "macOS 14" | contradicted by the above |

Origin: commit `cc5d539` (2026-09-10) "fix: clean build on macOS 26 / Xcode 26.6" raised 14 to 26.5 in `project.yml` and `Info.plist` "to match the macOS 26.5 SDK the xcframework was compiled against, eliminating ~150 linker warnings about newer object files". So the bump was a workaround for the Rust static library carrying a high `minos`, not a response to a Swift API need (see section 2 and 3).

## 2. Minimum OS of the linked binaries (`vtool -show-build`)

| Binary | Slice | minos | Forces a floor above 14/15? |
|---|---|---|---|
| `artifacts/pdfium/lib/libpdfium.dylib` (pinned chromium/8076, universal) | arm64 and x86_64 | **13.0** (sdk 26.0) | No |
| `libgist_ffi.a`, Rust objects, as built by `build-core-xcframework.sh` | arm64 | 11.0 (868 of 894 members) | No |
| `libgist_ffi.a`, Rust objects | x86_64 | 10.12 (`LC_VERSION_MIN_MACOSX`; 777 members) | No |
| `libgist_ffi.a`, **C-compiled members** (ring, sqlite3, blake3, poly1305, ...), as built here | arm64: 26 members; x86_64: 35 members | **27.0** (= the host/SDK default) | Only as a linker-warning artifact, see below |
| Same `libgist_ffi.a` rebuilt with `MACOSX_DEPLOYMENT_TARGET=14.0` in the environment (aarch64 only, scratch target dir) | arm64 | 11.0 (391 members) and **14.0** (503 members); none above 14.0 | No |

Finding: the Rust static library does not need macOS 26/27. The C objects built by the `cc` crate take their minimum OS from `MACOSX_DEPLOYMENT_TARGET`, which `tools/build-core-xcframework.sh` never sets, so they default to whatever OS the building machine runs (27.0 here). Linking such a library into an app with a lower target produces one `ld: warning: object file ... was built for newer 'macOS' version (27.0) than being linked (15.0)` per object. That is the family of linker warnings which commit `cc5d539` (which cites "~150") removed by raising the app target to 26.5 instead of pinning the Rust build. Measured here: the Debug build at target 15.0 emitted 23 such warnings (27.0 vs 15.0, active arm64 slice); the 14.0 build failed at compile before linking, so it has no link count. The warnings are cosmetic (no symbol needs a newer OS), but a library built on a newer host could in principle contain code paths assuming newer instructions or syscalls; the C code in these crates targets baseline arm64/x86_64, and this was not exercised at runtime on an older OS (see section 6).
Implication for any lowering: also set `MACOSX_DEPLOYMENT_TARGET` (to the chosen floor) when running `cargo build` in `build-core-xcframework.sh` and in CI, otherwise the library's minos tracks the CI runner's OS and the warnings return. The build-time minos of the staticlib is unrelated to what libpdfium needs (13.0).

## 3. Swift compile audit (project.yml untouched; target overridden on the command line)

| Target | Command result | Errors | Availability warnings |
|---|---|---|---|
| 14.0 `build` | **BUILD FAILED** (exit 65) | 1 distinct (below) | 0 availability-related |
| 14.0 `build-for-testing` | **TEST BUILD FAILED** | same 1 error; tests add none (the app target fails first) | 0 |
| 15.0 `build` | **BUILD SUCCEEDED** | 0 | 0 availability-related; 23 linker "built for newer macOS (27.0)" warnings from the Rust static library |
| 15.0 `build-for-testing` | **TEST BUILD SUCCEEDED** | 0 | 0 (test files compile at 15) |

The two non-availability compiler warnings (Swift 6 mode: `OcrImportModel.swift:210` main-actor static referenced from a nonisolated context; `:232` implicit strong capture) appear at every target and are unrelated to the OS floor.

### Per-API findings (every error the compiler reported)

| API | File:line | Introduced in (SDK `.swiftinterface`) | Fallback feasibility |
|---|---|---|---|
| `View.searchFocused(_:)` (FocusState<Bool>.Binding overload) | `apps/apple/Shared/LibraryView.swift:185` | `@available(iOS 18.0, macOS 15.0, visionOS 2.0, *)` in `SwiftUI.swiftmodule/arm64e-apple-macos.swiftinterface` (line ~7494, found with grep) | Small. Wrap the one modifier in an `if #available(macOS 15, *)` (e.g. a tiny `ViewModifier`/`@ViewBuilder` helper, roughly 10-15 lines). The <kbd>Cmd</kbd>+<kbd>F</kbd> hidden button at ~line 195 sets `isSearchFieldFocused`; on 14 that flag would simply focus nothing, so the shortcut becomes a no-op unless an `NSApp`/responder-based fallback is written (additional ~20-40 lines of AppKit, not attempted). Search itself (`.searchable`, macOS 12) is unaffected. |

Note: the source comment at `LibraryView.swift:190` says "macOS/iOS 17+"; the SDK annotation says macOS 15 / iOS 18. The comment is wrong.

Caveat on completeness: this is the only error the compiler reported at 14.0. I attempted to also confirm there are no further errors hidden behind it by temporarily commenting out that line and rebuilding, but that edit was blocked by the sandbox classifier and, per the task brief (no product edits), I did not work around it. The Swift driver type-checks all files in a module and reports every error it finds, and the other compile batches in the 14.0 run completed without errors, so this is probably the only one, but "probably" is not "verified"; once the line is guarded, a rebuild at 14.0 would settle it.

### Item 4: APIs newer than 14 on paper that the compiler flagged

Only the one above. Things used in the code that are new-ish but did **not** fail at 14.0 (so they are available at 14): `View.onKeyPress(_:)` (`macOS/FlowViewSwiftUINative.swift:107-112`; macOS 14), String Catalog localisation (`SWIFT_EMIT_LOC_STRINGS`; Xcode 15 build feature, not an OS API). No `#available` / `@available` exists anywhere in `apps/apple/{Shared,macOS,Tests}` today.

## 4. Does this Xcode allow lower targets?

Yes. From the macOS 27.0 SDK's `SDKSettings.plist`: `MinimumDeploymentTarget` 12.0, `RecommendedDeploymentTarget` 14.0, `DefaultDeploymentTarget` 27.0, valid values 12.0 ... 15.6, 26.0 ... 26.6, 27.0 (there is no 16-25; macOS jumped from 15 to 26). Swift concurrency back-deploys to macOS 12.0 (`SwiftConcurrencyMinimumDeploymentTarget`). `xcodeVersion: "16"` in `project.yml` is only an XcodeGen project-format hint.

## 5. Neutral summary by floor

| Floor | Swift changes needed | Build/tooling changes | Rough effort | Excluded users |
|---|---|---|---|---|
| **macOS 14** | One `#available(macOS 15, *)` guard around `.searchFocused` (LibraryView:185); optional AppKit fallback to keep Cmd+F working on 14 | Edit `project.yml` (2 places) and `Info.plist` `LSMinimumSystemVersion` (a literal, not tied to the build setting); set `MACOSX_DEPLOYMENT_TARGET=14.0` in `build-core-xcframework.sh` (and release/CI) to avoid the 27.0-minos C objects; pdfium (13.0) is fine | Small: roughly an hour for the guard, plus a rebuild/test; a real older-OS runtime check is separate work and has not been done | Macs on macOS 13 or older (and anything that can only run those) |
| **macOS 15** | None: builds and `build-for-testing` succeed with zero source changes | Same plist/yml/build-script edits | Smallest | Macs on macOS 14 or older |
| **macOS 26.0** | None (superset of 15) | Same | Smallest | Macs on macOS 15 or older. Not tied to any API in this code base: nothing flagged needs 26 |
| **macOS 26.5 (current)** | None | Already set | None | Macs below 26.5. Per the code, no API needs 26.5; the bump in `cc5d539` was to silence linker warnings from the Rust static library (section 2), not an API requirement |

Evidence-backed conclusion: in the current code the only OS-version-gated API the compiler found is `searchFocused` (macOS 15). Nothing in the Swift sources, the Rust static library (once built with an explicit deployment target) or the embedded pdfium (minos 13.0) requires macOS 26 or 26.5. Market/user-share figures for macOS versions were not looked up and are **unverified**; I make no claim about how many users each floor excludes.

## 6. Commands run

```
git log --oneline -1                                  # 1668bca
./tools/fetch-pdfium.sh                               # artifacts/pdfium populated, hash verified
vtool -show-build artifacts/pdfium/lib/libpdfium.dylib
./tools/build-core-xcframework.sh                     # exit 0
ar x target/{aarch64,x86_64}-apple-darwin/release/libgist_ffi.a ; vtool -show-build / otool -l on each member
cd apps/apple && xcodegen generate
xcodebuild build            -project GIST.xcodeproj -scheme GISTmacOS -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO MACOSX_DEPLOYMENT_TARGET=14.0   # failed, 1 error
xcodebuild build            ... MACOSX_DEPLOYMENT_TARGET=15.0                                                                        # succeeded
xcodebuild build-for-testing ... MACOSX_DEPLOYMENT_TARGET=14.0                                                                       # failed, same error
xcodebuild build-for-testing ... MACOSX_DEPLOYMENT_TARGET=15.0                                                                       # succeeded
MACOSX_DEPLOYMENT_TARGET=14.0 CARGO_TARGET_DIR=<scratch> cargo build -p gist-ffi --release --target aarch64-apple-darwin           # then vtool on members
grep -n -B3 "func searchFocused" <SDK>/SwiftUI.swiftmodule/arm64e-apple-macos.swiftinterface
plutil -p <SDK>/SDKSettings.plist
```
All xcodebuild runs used a scratch `-derivedDataPath` outside the repo. Build products and `artifacts/` are gitignored; the only file added to the repo is this document.

## 7. What was NOT verified

- **Runtime on any old macOS.** This machine runs macOS 27.0.1 and only the macOS 27.0 SDK is installed. "Compiles at 14.0 / 15.0" means the compiler accepted the code against the 27.0 SDK with that deployment target (availability checking), not that the app was launched on macOS 14 or 15. Behavioural differences in SwiftUI/AppKit between OS versions, Keychain, App Sandbox, Vision OCR, AVSpeechSynthesizer, or loading the embedded libpdfium on 14/15 were not exercised.
- **Absence of further errors at 14.0** once `searchFocused` is guarded (section 3 caveat).
- **x86_64 app slice** at lower targets: the Xcode builds were the active arch (arm64) only. The Rust x86_64 staticlib minos was measured (section 2).
- **The Rust build at 14.0 for the full xcframework** (only aarch64 static lib rebuilt, not lipo/xcframework, and not linked into the app).
- **Release/Archive configuration and Hardened Runtime** behaviour at lower targets (`N8`).
- **User-share / market data** for macOS versions (not looked up).
- **Deprecation warnings** or behaviour changes in APIs that are available at 14 but changed later: the compiler flagged none, and none were speculated on here.
