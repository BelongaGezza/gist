# Contributing to GIST

Thanks for your interest in GIST. This document covers the practical mechanics of contributing; for the product vision and architecture, see `docs/product-spec-reader-app-v3.md` and `docs/ARCHITECTURE.md`.

## Getting set up

1. Install the Rust toolchain pinned in `rust-toolchain.toml` (`rustup show` should confirm the active version matches).
2. For the macOS app, install a full Xcode (not just Command Line Tools) and [XcodeGen](https://github.com/yonaskolb/XcodeGen).
3. Run `cargo test --workspace` to confirm the Rust workspace builds and passes.
4. Run `./tools/gen-bindings.sh` followed by `cd apps/apple && xcodegen generate` before opening the Xcode project — both the FFI bindings (`apps/apple/Generated/`) and the `.xcodeproj` itself are generated and are not committed.

## Before opening a PR

Run the same checks CI runs:

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo deny check bans licenses sources
```

If your change touches `apps/apple/**`, `crates/gist-ffi/**`, or the bindings/xcframework tooling, also verify the macOS app builds and tests:

```bash
./tools/gen-bindings.sh
cd apps/apple && xcodegen generate
xcodebuild -scheme GISTmacOS build test
```

## Coding conventions

- No `unwrap()` on parsed or externally-sourced data — use `?` or `unwrap_or_else`.
- No bare `.lock().unwrap()` in `gist-store` — always use poison-tolerant recovery (`.unwrap_or_else(|p| p.into_inner())`).
- Every `#[uniffi::export]` function body is wrapped in the `ffi_catch!` macro (`gist-ffi`) — panics must never cross the FFI boundary.
- Every parser accepts and enforces a `ParseLimits` struct (`max_bytes`, `max_pages`, `max_nesting_depth`, `max_expanded_bytes`, `max_zip_entries` where applicable) before allocating based on untrusted input.
- Document IDs are `Uuid::now_v7()`, never timestamps.
- Source file paths are logged at `debug!`, never `info!` or above.
- `gist-model` must stay free of I/O dependencies — it needs to compile to `wasm32-unknown-unknown`.

See `CLAUDE.md`'s "Crate conventions" and "Security policies" sections for the full, current list — that file is the living source of truth and is kept up to date as conventions evolve.

## Architectural decisions

Non-trivial design decisions are recorded as ADRs in `docs/adr/`, numbered sequentially. If your change makes or revisits an architectural call (a new FFI pattern, a storage format change, a security-relevant tradeoff), add or update an ADR rather than leaving the reasoning only in a commit message or PR description.

## Commit messages

This repo uses a lightweight conventional-commit style, e.g.:

```
feat(m2): nest TOC by heading level
fix(security): decouple read-decrypt from write-auto-encrypt for per-item encryption
docs: refresh M2 status
security: at-rest integrity checksums (A4 / ADR-013)
```

Common prefixes: `feat`, `fix`, `docs`, `security`, `refactor`, `test`, `chore`, `merge`. Reference the relevant finding ID (`F##`/`A##`/`N##`) or ADR number when the change closes or relates to one.

## Security-sensitive changes

GIST imports files and fetches URLs on the user's behalf, so changes to parsers (`crates/gist-parse-*`), `gist-web`, or `gist-store`'s encryption/storage code get extra scrutiny. If you're fixing or reporting a security issue, see `SECURITY.md` first rather than opening a public issue.
