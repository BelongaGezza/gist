# ADR-010: HTTP client — ureq + rustls

**Status:** Accepted
**Date:** 2026-09

## Decision

Use `ureq` with the `rustls` TLS backend for all outbound HTTP in `gist-web`.
No dependency on `tokio`, `reqwest`, or the system OpenSSL.

## Problem

URL import (`gist-web`) requires a synchronous HTTP client that:
- compiles to all target triples (macOS arm64/x86_64, iOS arm64, future Windows x86_64)
- does not pull `tokio` (the Rust core is synchronous by design; async adds complexity across the FFI boundary)
- avoids linking against the system OpenSSL (not present on macOS/iOS; version skew risk on Linux CI)
- supports rustls for a pure-Rust TLS stack

## Alternatives considered

- **`reqwest` (async):** depends on `tokio`; async executor management across the FFI boundary is error-prone and adds latency on short requests.
- **`hyper` (raw):** even lower level; would require hand-rolling redirect handling, response-size limiting, and a TLS backend selection.
- **`attohttpc`:** smaller community; less maintained; no streaming body support.
- **System `URLSession` on Apple targets:** was considered for iOS specifically (`Q5`), but keeping fetch in Rust means one implementation, one test suite, and consistent `robots.txt` enforcement. `URLSession` is a Swift-only option; the Rust core must work without it for macOS CLI / CI scenarios.

## Consequences

- Single synchronous HTTP implementation shared across macOS and iOS builds.
- `ureq` agent configured per ADR-005: rustls backend, max 5 redirects, 30 s connect / 60 s read timeout, no cookie jar, response body capped at `min(limits.max_bytes, 50 MB)`.
- `robots.txt` pre-fetch uses the same agent; adds one round-trip before each fetch.
- HTTPS-only: plain-HTTP URLs return `ParseError::InvalidInput` immediately.
- If `URLSession` integration is ever needed for iOS ATS / proxy / cellular-awareness, it slots in as an `OcrEngine`-style callback interface — the Rust layer calls a Swift-provided fetch closure rather than `ureq` directly. Decision deferred to iOS kickoff (`Q5` resolved: keep ureq for v1.0 macOS).
