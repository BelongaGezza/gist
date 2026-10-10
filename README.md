# GIST

GIST is a cross-platform, RSVP-style ("rapid serial visual presentation") speed-reading app. A shared Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it over FFI.

Import a document — plain text, ePub, DOCX or PDF (text or scanned, via on-device OCR) — and read it either as a flowing document view or as a paced, one-word-at-a-time RSVP stream. Everything is stored locally: there's no server component and no account system.

## Status

GIST is pre-1.0 and under active development — there is no tagged release and no public download
yet. The Rust core and macOS app are furthest along: import, RSVP and flow reading, search,
collections/tags, theming, annotations, an accessibility pass, and opt-in encryption-at-rest are
all implemented and covered by automated tests, though the release pipeline is still blocked on
real Apple signing credentials and a full manual click-through of the UI is still outstanding.
Windows is no longer a placeholder: a WinUI 3 app with library, collections, tags, theming, an RSVP
reader and a flow reader is built and tested, while accessibility, hardening and packaging (phase
W6) have not started, and OCR, annotations, read-aloud, PDF and the paginated view are outside the
Windows plan (see [`docs/windows-development-plan.md`](./docs/windows-development-plan.md)).
Linux (a GTK4 spike) and Android (plan and ADRs only) are post-v1.0 and not committed.

The product spec ([`docs/product-spec-reader-app-v3.md`](./docs/product-spec-reader-app-v3.md))
promises more than is built today. Its §12 lists every requirement not yet implemented
(`SC-01`–`SC-24`), for example: TXT files that are not UTF-8 cannot be imported, there is no grid
view or smart collections, library sort is not remembered between launches, and typography
controls are limited to size, font style and line spacing. The schedule for them is in
[`docs/development-plan-v2.md`](./docs/development-plan-v2.md) §5 (M8).

See [`CHANGELOG.md`](./CHANGELOG.md) for a running, feature-level log of what's shipped so far and
its known limitations, [`CLAUDE.md`](./CLAUDE.md) for a detailed, continuously-updated log of what's
implemented, what's tested, and what's open, and [`docs/development-plan-v2.md`](./docs/development-plan-v2.md)
for the milestone plan.

## Features

- **Import**: plain text (UTF-8 only today), ePub (with DRM detection), and DOCX; URL-paste import
  and on-device OCR from images also work. PDF import (text and scanned) is macOS only and has only
  been tuned on synthetic PDFs. Embedded images are not extracted from any format.
- **Reading**: a paced, one-word-at-a-time RSVP view driven by the shared Rust pacing engine (speed
  slider, 200–700 WPM in the UI), a continuous "flow" document view with typography controls (size,
  font style, line spacing), a table of contents and in-document search, and an optional paginated
  view (macOS). Tables render as an accessible grid with merged cells.
- **Library**: full-text search, collections, tags, sort (title, author, type, date added, last read,
  progress) and tag filter, and removal that never touches a user's original imported file
  (copy-on-import). List view only; sort is not persisted yet.
- **Annotations** (macOS): highlights, notes, and bookmarks that survive a document being re-imported,
  with Markdown export.
- **Theming**: system-follow, light, dark, sepia, and true-black OLED.
- **Privacy and security**: everything is local-first (see [`docs/PRIVACY.md`](./docs/PRIVACY.md)); optional
  per-item encryption at rest and at-rest integrity checksums are implemented, with an open
  security register tracked in `CLAUDE.md`.
- **Accessibility** (macOS): VoiceOver labels, Dynamic Type and on-device read-aloud are implemented
  and unit-tested; no live VoiceOver or listening pass has been done yet.

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
│   ├── gist-parse-pdf/         # PDF — pdfium-based text extraction (macOS)
│   ├── gist-imageprep/         # OCR pre-processing
│   ├── gist-web/               # URL fetch + readability extraction
│   ├── gist-rsvp/              # Pure RSVP pacing engine (no I/O, no timers)
│   ├── gist-store/             # SQLite storage, FTS5 search, encryption at rest
│   ├── gist-core/              # Import pipeline facade
│   └── gist-ffi/               # uniffi FFI bindings → .xcframework
├── apps/
│   ├── apple/                  # SwiftUI macOS + iOS app (Xcode project is generated, not committed)
│   └── windows/                # WinUI 3 app — library, collections, tags, theming, RSVP and flow
│                                #   readers built; accessibility/packaging (W6) not started
├── spikes/                     # Throwaway experiments: linux-gtk (GTK4 prototype), pdf-isolation
├── assets/                     # App icon masters
├── fixtures/                   # Synthetic public-domain test fixtures
├── fuzz/                       # cargo-fuzz targets (separate Cargo workspace)
├── tools/                      # Build, bindings, PDFium fetch, notarization and CI-guard scripts
└── docs/                       # Architecture, ADRs, product spec, security reviews
```

## Building

Requirements: Rust (version pinned in `rust-toolchain.toml`), and for the macOS app, a full Xcode install plus [XcodeGen](https://github.com/yonaskolb/XcodeGen). The macOS app targets **macOS 14 or later** (decided 2026-10-04; compile-verified at that floor, not yet tested on a real macOS 14 machine).

```bash
# Rust workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo deny check bans licenses sources

# macOS app (PDF import also needs ./tools/fetch-pdfium.sh once)
./tools/gen-bindings.sh                # regenerate Swift FFI bindings
cd apps/apple && xcodegen generate      # generate the Xcode project (never committed)
xcodebuild -scheme GISTmacOS build test
```

Full build/toolchain notes, including known environment caveats, are in [`CLAUDE.md`](./CLAUDE.md#build--toolchain) and [`docs/BUILDING-macos.md`](./docs/BUILDING-macos.md). For the FFI boundary and binding generation, see [`docs/FFI.md`](./docs/FFI.md). For building the Windows app (.NET SDK, WinUI 3, the C# FFI bindings), see [`SETUP_NOTES.md`](./SETUP_NOTES.md) and [`docs/windows-development-plan.md`](./docs/windows-development-plan.md) §1–2. The Linux spike lives in [`spikes/linux-gtk`](./spikes/linux-gtk/README.md).

## Architecture

Key design decisions are recorded as ADRs in [`docs/adr/`](./docs/adr/). Start with [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) for an overview, and [`docs/product-spec-reader-app-v3.md`](./docs/product-spec-reader-app-v3.md) for the product spec.

## Security

GIST handles a user's personal reading material and imports from both local files and arbitrary URLs, so security review is an ongoing part of the project — see [`docs/security-review-v2.md`](./docs/security-review-v2.md) and the security register in `CLAUDE.md`. To report a vulnerability, see [`SECURITY.md`](./SECURITY.md). For what GIST stores, what (if anything) leaves the device, and the current state of encryption at rest, see [`docs/PRIVACY.md`](./docs/PRIVACY.md).

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the development workflow, coding conventions, and CI expectations.

## License

MIT — see [`LICENSE`](./LICENSE).
