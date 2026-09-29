# Building GIST for macOS

This file covers building and testing the macOS app end-to-end. For the Rust core alone (no
Xcode/macOS-specific steps needed), `cargo test --workspace` from the repo root is sufficient. For
Windows, see [`windows-development-plan.md`](./windows-development-plan.md) instead — it is not
covered here.

## Prerequisites

- **Rust**, pinned by `rust-toolchain.toml` at the repo root — `rustup show` from the repo root
  should confirm the active toolchain matches. The file is the source of truth; do not hardcode a
  version number here, since it's bumped in place via PR as the project's MSRV moves.
- **A full Xcode install, not just Command Line Tools.** `xcode-select -p` should print a path
  inside an `Xcode.app` bundle (e.g. `/Applications/Xcode.app/Contents/Developer`), not
  `CommandLineTools`. `xcodebuild` will fail confusingly otherwise.
- **[XcodeGen](https://github.com/yonaskolb/XcodeGen)** — the Xcode project
  (`apps/apple/GIST.xcodeproj`) is generated from `apps/apple/project.yml` and is not committed to
  git.
- The `aarch64-apple-darwin` and `x86_64-apple-darwin` Rust targets, for a universal
  (Apple Silicon + Intel) build — `tools/build-core-xcframework.sh` installs these automatically if
  missing, so you don't need to add them by hand first.

## Build steps

### 1. Rust workspace

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo deny check bans licenses sources
```

`cargo deny check advisories` may fail locally with a CVSS 4.0 parse error depending on how old
your installed `cargo-deny` binary is (`cargo install cargo-deny --locked` to upgrade it) — CI's
`core-quality` workflow bundles a newer version via `cargo-deny-action` and is the source of truth
for this check if your local binary can't run it.

### 2. Generate the Swift FFI bindings

For local/day-to-day development (debug, host architecture only):

```bash
./tools/gen-bindings.sh
```

This needs a `uniffi-bindgen` binary on `PATH`. uniffi 0.28+ no longer ships one on crates.io;
`gist-ffi` builds its own via a `uniffi-bindgen-bin` feature — see the comment block at the top of
`tools/gen-bindings.sh` for the exact install command. If no `uniffi-bindgen` is found, the script
leaves any already-generated bindings under `apps/apple/Generated/` alone rather than failing.

For a release-quality universal xcframework (what `apple-build` CI and the release pipeline use)
instead of a debug/host-arch-only build:

```bash
./tools/build-core-xcframework.sh
```

This builds `gist-ffi` for both `aarch64-apple-darwin` and `x86_64-apple-darwin`, `lipo`s them into
a universal static library, and generates bindings from that — the output lands under
`artifacts/GistCore.xcframework`.

`apps/apple/Generated/` (the Swift bindings) is `.gitignore`d and rebuilt fresh, same as the
`.xcodeproj` itself.

### 3. Generate the Xcode project

```bash
cd apps/apple && xcodegen generate
```

Do not commit the resulting `.xcodeproj` — it's regenerated from `apps/apple/project.yml` every
time. If you change `project.yml` (new target, new file group, new build setting), re-run this
before building in Xcode or from the command line.

### 4. Build and test

```bash
xcodebuild -scheme GISTmacOS build test
```

CI (`apple-build.yml`) runs this with `CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO`, since it
has no Apple Developer signing identity configured. Do the same locally unless you have a real
signing identity set up and specifically want to test a signed build:

```bash
xcodebuild -scheme GISTmacOS build test CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO
```

## Known environment caveats

- **A signed, notarized Release build needs a real Apple Developer ID certificate and App Store
  Connect app-specific-password credentials.** Most development environments — including this
  project's own CI, as of this writing — do not have these configured. `.github/workflows/
  release-macos.yml` and `tools/build-dmg.sh` implement the full pipeline and have been exercised
  end-to-end unsigned; the signed/notarized half has never been run for real. See `CLAUDE.md`'s
  `F10` security-register row for the exact current state.
- **No XCUITest / fully-automated UI click-through suite exists yet.** `xcodebuild test` runs real
  unit and logic tests (against a real `GistCore`/SQLite/filesystem, not mocks) but does not drive
  the UI itself. A manual click-through pass by a person is still outstanding for several features
  — see `CLAUDE.md`'s milestone register for exactly what's open.
- **iOS is not built yet.** `apps/apple/project.yml` currently declares macOS targets only; there
  is no `apps/apple/iOS/` directory or iOS scheme, despite iOS being part of the long-term product
  vision (see `docs/development-plan-v2.md`).
