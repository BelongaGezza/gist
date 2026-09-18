# GIST

GIST is a cross-platform, RSVP-style ("rapid serial visual presentation") speed-reading app. A shared Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it over FFI.

Import a document — plain text, ePub, or DOCX today, with PDF and OCR planned — and read it either as a flowing document view or as a paced, one-word-at-a-time RSVP stream. Everything is stored locally: there's no server component and no account system.

## Status

GIST is pre-1.0 and under active development. The Rust core and macOS app are furthest along; Windows is a placeholder. See [`CLAUDE.md`](./CLAUDE.md) for a detailed, continuously-updated log of what's implemented, what's tested, and what's open, and [`docs/development-plan-v2.md`](./docs/development-plan-v2.md) for the milestone plan.

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
│   └── windows/                # Placeholder
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

Full build/toolchain notes, including known environment caveats, are in [`CLAUDE.md`](./CLAUDE.md#build--toolchain).

## Architecture

Key design decisions are recorded as ADRs in [`docs/adr/`](./docs/adr/). Start with [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) for an overview, and [`docs/product-spec-reader-app-v3.md`](./docs/product-spec-reader-app-v3.md) for the product spec.

## Security

GIST handles a user's personal reading material and imports from both local files and arbitrary URLs, so security review is an ongoing part of the project — see [`docs/security-review-v2.md`](./docs/security-review-v2.md) and the security register in `CLAUDE.md`. To report a vulnerability, see [`SECURITY.md`](./SECURITY.md).

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the development workflow, coding conventions, and CI expectations.

## License

MIT — see [`LICENSE`](./LICENSE).
