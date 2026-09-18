# Security Policy

GIST imports documents from local files and arbitrary URLs and stores a user's personal reading material locally, so we take security issues seriously even while the project is pre-1.0.

## Supported versions

GIST has not reached a 1.0 release. There is only one actively maintained line: the `main` branch. Security fixes are made there.

## Reporting a vulnerability

Please **do not** open a public GitHub issue for a suspected security vulnerability.

Instead, email **gerrygillies@gmail.com** with:

- A description of the issue and its potential impact.
- Steps to reproduce, or a proof-of-concept if you have one.
- The commit hash or version you tested against.

You should expect an acknowledgement within a few days. This is a small, independently-maintained project without a formal SLA, but reports are taken seriously and fixes for confirmed issues are prioritized.

## Scope

In scope:
- The Rust core (`crates/`), including all parsers, the FFI boundary, storage/encryption, and URL fetching.
- The native app shells (`apps/apple`, `apps/windows`) where they call into the core or handle sensitive data (imported documents, encryption keys).

Out of scope:
- CI/CD configuration issues with no user-facing impact (report these as a normal issue instead).
- Findings that require the attacker to already have arbitrary local code execution on the user's machine.

## Known findings and ongoing review

GIST tracks security findings from internal and independent reviews in an open register rather than hiding them: see the "Security register" section of [`CLAUDE.md`](./CLAUDE.md) and [`docs/security-review-v2.md`](./docs/security-review-v2.md) for what's been found, fixed, and what remains open. Please check there first — it's possible what you've found is already tracked.
