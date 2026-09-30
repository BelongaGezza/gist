# GIST

GIST is a cross-platform, RSVP-style ("rapid serial visual presentation") speed-reading app. A shared Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it over FFI.

Import a document — plain text, ePub, or DOCX today, with PDF and OCR planned — and read it either as a flowing document view or as a paced, one-word-at-a-time RSVP stream. Everything is stored locally: there's no server component and no account system.

## Status

GIST is pre-1.0 and under active development — there is no tagged release and no public download
yet. The Rust core and macOS app are furthest along: import, RSVP and flow reading, search,
collections/tags, theming, annotations, an accessibility pass, and opt-in encryption-at-rest are
all implemented and covered by automated tests, though the release pipeline is still blocked on
real Apple signing credentials and a full manual click-through of the UI is still outstanding.
Windows is no longer a placeholder — a WinUI 3 app with library, collections, tags, and theming
is engineering-complete, with the RSVP reader, flow reader, and an accessibility/packaging pass
still in progress (see [`docs/windows-development-plan.md`](./docs/windows-development-plan.md)).

See [`CHANGELOG.md`](./CHANGELOG.md) for a running, feature-level log of what's shipped so far and
its known limitations, [`CLAUDE.md`](./CLAUDE.md) for a detailed, continuously-updated log of what's
implemented, what's tested, and what's open, and [`docs/development-plan-v2.md`](./docs/development-plan-v2.md)
for the milestone plan.

## Features

- **Import**: plain text, ePub (with DRM detection), and DOCX today; URL-paste import and
  on-device OCR from images also work. PDF import is not implemented yet.
- **Reading**: a paced, one-word-at-a-time RSVP view, and a continuous "flow" document view with
  typography controls, a table of contents, and in-document search.
- **Library**: full-text search, collections, tags, sort/filter, and removal that never touches a
  user's original imported file (copy-on-import).
- **Annotations**: highlights, notes, and bookmarks that survive a document being re-imported.
- **Theming**: system-follow, light, dark, sepia, and true-black OLED.
- **Privacy and security**: everything is local-first (see [`docs/PRIVACY.md`](./docs/PRIVACY.md)); optional
  per-item encryption at rest and at-rest integrity checksums are implemented, with an open
  security register tracked in `CLAUDE.md`.
- **Accessibility** (macOS): VoiceOver support, Dynamic Type, and on-device read-aloud.

Feature availability differs by platform — see the Status section above and `CHANGELOG.md` for
which of these are macOS-only today vs. also on Windows.

## Repository layout

```
gist/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── gist-model/             # Document model, IR types, token stream, errors
│   ├── gist-parse-txt/         # Plain text import
│   ├── gist-parse-epub/        # ePub import (DRM detection + resource limits)
│   ├── gist-parse-docx/        # DOCX import (style resolution + tracked changes)
│   ├── gist-parse-pdf/         # PDF — stub, not yet implemented
│   ├── gist-imageprep/         # OCR pre-processing
│   ├── gist-web/               # URL fetch + readability extraction
│   ├── gist-rsvp/              # Pure RSVP pacing engine (no I/O, no timers)
│   ├── gist-store/             # SQLite storage, FTS5 search, encryption at rest
│   ├── gist-core/              # Import pipeline facade
│   └── gist-ffi/               # uniffi FFI bindings → .xcframework
├── apps/
│   ├── apple/                  # SwiftUI macOS + iOS app (Xcode project is generated, not committed)
│   └── windows/                # WinUI 3 app — library/collections/tags/theming done; RSVP/flow
│                                #   reader and accessibility/packaging still in progress
├── fixtures/                   # Synthetic public-domain test fixtures
├── fuzz/                       # cargo-fuzz targets (separate Cargo workspace)
├── tools/                      # Build/bindings/notarization scripts
└── docs/                       # Architecture, ADRs, product spec, security reviews
```

## Building

Requirements: Rust (version pinned in `rust-toolchain.toml`), and for the macOS app, a full Xcode install plus [XcodeGen](https://github.com/yonaskolb/XcodeGen).

```bash
# Rust workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo deny check bans licenses sources

# macOS app
./tools/gen-bindings.sh                # regenerate Swift FFI bindings
cd apps/apple && xcodegen generate      # generate the Xcode project (never committed)
xcodebuild -scheme GISTmacOS build test
```

Full build/toolchain notes, including known environment caveats, are in [`CLAUDE.md`](./CLAUDE.md#build--toolchain). For building the Windows app (.NET SDK, WinUI 3, the C# FFI bindings), see [`docs/windows-development-plan.md`](./docs/windows-development-plan.md) §1–2 — it is not yet mirrored into this README.

## Architecture

Key design decisions are recorded as ADRs in [`docs/adr/`](./docs/adr/). Start with [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) for an overview, and [`docs/product-spec-reader-app-v3.md`](./docs/product-spec-reader-app-v3.md) for the product spec.

## Security

GIST handles a user's personal reading material and imports from both local files and arbitrary URLs, so security review is an ongoing part of the project — see [`docs/security-review-v2.md`](./docs/security-review-v2.md) and the security register in `CLAUDE.md`. To report a vulnerability, see [`SECURITY.md`](./SECURITY.md). For what GIST stores, what (if anything) leaves the device, and the current state of encryption at rest, see [`docs/PRIVACY.md`](./docs/PRIVACY.md).

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the development workflow, coding conventions, and CI expectations.

## License

MIT — see [`LICENSE`](./LICENSE).
