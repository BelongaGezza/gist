# Rust 1.88.0 → 1.97.x toolchain upgrade — requirements analysis

**Written:** 2026-09-25, against `main`. Current pin: `rust-toolchain.toml` → `1.88.0` (bumped from `1.87.0` on 2026-09-19, see `N6` in `CLAUDE.md`'s security register).

## Scope and a caveat up front

This is a toolchain/dependency/CI exercise, not a source rewrite. Nothing in this repo currently sets `rust-version` in any `Cargo.toml`, no crate has opted into edition 2024 (all eleven are `edition = "2021"`), and edition is independent of the pinned rustc version — you can run rustc 1.97 against edition-2021 crates indefinitely. **Bumping the toolchain and adopting a newer edition are two separate decisions**; this document only covers the toolchain bump. Editions are noted at the end as an optional follow-on.

**Knowledge-boundary note:** I do not have reliable, verified changelog data for Rust releases between 1.89 and 1.97 — at six weeks per release that's roughly fifteen point releases, most of them past what I can cite with confidence. Rather than guess at specific stabilized features or lint additions, this document (a) lists every place *this repo* currently hardcodes a version number and needs a coordinated edit, (b) surfaces a **real, already-identified blocker** already sitting in this repo's own Dependabot triage notes, and (c) gives a verification-driven execution plan that catches whatever the actual 1.89–1.97 changes turn out to be, instead of trusting a stale prediction. Whoever executes this should read the real release notes (`https://github.com/rust-lang/rust/blob/master/RELEASES.md` or `rustup doc --releases` once installed) for the target version before starting.

## 1. Every place a Rust version is hardcoded

All of these currently say `1.87.0` or `1.88.0` and would need editing in the same PR (this repo's own convention — see `CLAUDE.md`'s "Handoff files" section: "Update them in the same commit as the change they describe"):

| File | What it says | Fix needed |
|---|---|---|
| `rust-toolchain.toml` | `channel = "1.88.0"` | The actual bump — `channel = "1.97.x"` (pick the exact patch version; this file requires an exact stable version per `docs/development-plan-v2.md`'s `[F9]` decision, not a bare `"stable"`) |
| `CLAUDE.md` (repo layout comment) | `rust-toolchain.toml # pinned to 1.87.0` | Already stale *today* (the file itself says 1.88.0) — fix regardless of this bump, and update again to 1.97.x |
| `CLAUDE.md` (Build & toolchain section) | `rustup show # should say 1.87.0` | Same pre-existing drift; update to 1.97.x |
| `.github/workflows/apple-build.yml:79` | comment: `"...pinned rust-toolchain.toml (1.87.0)..."` | Stale today (real pin is 1.88.0); update comment text, no behavior change since this step deliberately uses `+stable` not the pinned toolchain |
| `.github/workflows/fuzz.yml` (env comment block, lines ~15-27) | references `1.88.0`, an MSRV floor of `>= 1.91` for `cargo-platform`, and the dated nightly pin `nightly-2026-09-01` | Update the 1.88.0 mention; re-verify the `>= 1.91` floor is still cargo-fuzz's actual requirement by the time this runs, and roll the nightly date pin forward (see §4) |
| `docs/development-plan-v2.md` (×3: `[F9]` closure note, the "must specify exact stable version" rule, and the milestone table row) | all say `1.87.0` | These are historical closure records — either leave as a dated snapshot or add a superseding note; don't silently rewrite history that was true when written |
| `docs/security-review-v1.md` | `channel = "1.87.0"` | Historical record of what v1 verified — same "don't rewrite history" judgment call as above |
| `docs/security-review-v2.md` (×2) | references the pin as `1.87.0` while describing a since-fixed fuzz-tooling gap | Historical record, same judgment call |
| `docs/dependabot-pr-triage.md` | header: "Rust pinned at 1.88.0"; also an MSRV note for `aes-gcm` requiring `>= 1.89` | Update header; the `aes-gcm` row is the real blocker — see §2 |
| `docs/windows-development-plan.md` (×2) | "pinned Rust 1.88.0" | Update, and re-verify the Windows build claims still hold (see §5) |
| `PLATFORM_VERIFICATION.md` | "Pinned Rust 1.88.0" in the verification table | Update, and re-run/re-record verification per-platform after the bump (this file's whole purpose) |
| `.github/dependabot.yml` (comment) | references "cargo-deny 0.20+/rustc 1.88+" | Re-check whether this comment's reasoning still applies once the pin moves |

This list itself is evidence for the single biggest procedural risk here: **this repo has a documented history of exactly this kind of drift** (the `1.87.0`/`1.88.0` mismatch in `apple-build.yml` right now, predating this task). A version bump PR should grep for the old version string repo-wide as a final check, not just edit the files it remembers.

## 2. Known concrete blocker: `aes-gcm` needs a coordinated MSRV bump *and* an API migration

`docs/dependabot-pr-triage.md` already recorded this, independent of this task:

> `aes-gcm` 0.10.3 → 0.11.1: **HOLD (MSRV) / NEEDS-CODE-CHANGE** — Fails on every OS: `aes@0.9.3 requires rustc 1.89` (pin is 1.88.0). Beyond MSRV it is an API migration: `aead 0.6`/`cipher 0.5`/`hybrid-array` replace `generic-array`, so `Key::<Aes256Gcm>::from_slice`, `Nonce::from_slice` and `aead::OsRng` (`gist-store/src/lib.rs` lines 1-2, 107-135) must be rewritten and `generate_nonce` needs a rand_core 0.10 RNG.

This is `gist-store`'s ADR-011/ADR-014 encryption-at-rest code — a security-sensitive module, not incidental. A 1.97 bump clears the MSRV floor trivially, but **the code migration is the real work**:

- Rewrite `Key::<Aes256Gcm>::from_slice`/`Nonce::from_slice` call sites for the `hybrid-array`-based API.
- Re-source the nonce RNG for `rand_core` 0.10's `OsRng` shape.
- Per the triage note's own caution: **prove the on-disk ciphertext format is unchanged** with a test that decrypts a blob written by the old `aes-gcm` 0.10.3 before merging — this repo already has real encrypted-at-rest data paths (`Store::encrypt_item`, `open_with_read_key`) where silently changing the wire format would brick existing users' encrypted items.
- The triage doc explicitly recommends doing this "as a deliberate branch with a 1.89 toolchain bump" — i.e., this was already scoped as its own unit of work, separate from routine dependency bumps. A 1.97 bump is the forcing function that makes deferring it no longer viable if Dependabot's `aes-gcm` PR is ever merged, but the crypto migration itself should still land as its own reviewed change, not folded silently into the toolchain-bump PR.

Check whether that Dependabot PR has since been merged, closed, or is still open before starting — if it's still open, this migration is effectively the first real subtask of this bump.

## 3. Full dependency MSRV sweep (needs re-running at execution time, not trusted from this snapshot)

Current major pinned versions (from `Cargo.lock` as of this writing): `zip` 8.6.0, `quick-xml` 0.42.0, `ureq` 2.12.1 (constrained `>=2.10, <3`), `rusqlite` 0.40.2, `image` 0.25.10, `scraper` 0.27.0, `uniffi` 0.32.1, `rustls` 0.23.45, `blake3` 1.8.7, `base64` 0.22.1, `clap` 4.6.6, plus `serde`/`serde_json`/`thiserror` 2/`uuid`/`tracing`/`sha2` at workspace level.

None of these currently declare a `rust-version` above 1.88 in this lockfile (confirmed: no `rust-version` field appears in this repo's own `Cargo.toml`s, and `cargo build`/`test` are green under 1.88 today per `PLATFORM_VERIFICATION.md`). But between now and whenever 1.97.x actually ships, every one of these crates will have released newer versions, some of which may raise their own MSRV past what this snapshot shows — that's a moving target this document can't freeze correctly. At execution time:

1. `cargo update` (or accept whatever Dependabot has already merged by then) and re-check `cargo tree` for MSRV-relevant transitive bumps.
2. Run the existing `cargo-deny` gates (`cargo deny check bans licenses sources`, `cargo deny check advisories`) — this repo's own history (`N7`) shows dependency bumps here routinely surface real RUSTSEC findings and license-list gaps, not just MSRV noise.
3. `uniffi` in particular is worth special attention: this repo already hit one uniffi-version quirk (0.28+ dropped the standalone `uniffi-bindgen` binary, worked around via the `uniffi-bindgen-bin` feature in `crates/gist-ffi/Cargo.toml`). A multi-version jump increases the odds of another such breaking change in uniffi's proc-macro surface — regenerate `apps/apple/Generated/gist_ffi.swift` and the C# bindings and diff them, don't just trust that compilation succeeding means the generated bindings are unchanged.

## 4. CI/tooling surface

- **The five `dtolnay/rust-toolchain@<sha> # stable` steps** (`core-test`, `core-quality`, `apple-build`, `parser-corpus`, `windows-build`) install whatever "stable" resolves to at the pinned commit's own tracking — they do **not** need editing for the version bump itself, since `rust-toolchain.toml` in the repo overrides them for any `cargo` invocation run inside the checkout. Only their *comments* mention specific versions (see §1).
- **`fuzz.yml`'s `RUSTUP_TOOLCHAIN: nightly-2026-09-01`** is a separate, deliberately-pinned nightly (cargo-fuzz needs nightly regardless of the stable-channel bump) with its own MSRV floor comment (`cargo-platform` needs `rustc >= 1.91` as of when that comment was written). This pin is documented as needing quarterly review already — folding a review into this bump is natural timing, but it's an independent axis: confirm `cargo +nightly-<new-date> fuzz build` still succeeds, per the file's own documented bump procedure.
- **`windows-build.yml`** builds `gist-ffi` on `windows-latest` via the same `dtolnay/rust-toolchain@... # stable` + local `rust-toolchain.toml` pattern, so the MSVC host toolchain picks up whatever `channel` is set. Nothing here needs an edit beyond the version-string comment, but this is the one CI surface where the new rustc version's actual availability on the `windows-latest` runner image matters (a very fresh point release can lag `rustup`'s distribution by a day or two) — worth a `workflow_dispatch` dry run before merging.
- **`apple-build.yml`** builds `gist-ffi` for the xcframework the same way; also installs a separate `+stable` toolchain specifically for `uniffi-bindgen` (see the stale-comment fix in §1) — confirm that install step still works once "stable" has moved past 1.97 too.
- **Local `cargo-deny`:** `CLAUDE.md`'s own "Security policies" section notes this dev environment's `cargo-deny` binary is 0.18.3 and can't parse CVSS 4.0 advisories, unlike CI's `cargo-deny-action` v2.1.1 (bundling 0.20.2). Unrelated to the rustc bump, but doing `cargo install cargo-deny --locked` while already touching toolchain matters is a reasonable piggyback since a rustc bump could change what that fresh cargo-deny even needs to compile against.

## 5. Platform-specific verification needed after the bump

Per `PLATFORM_VERIFICATION.md`'s own structure, each platform needs independent re-verification — a green macOS run does not imply Windows or Linux are fine:

- **macOS:** `cargo test --workspace` / `clippy -D warnings` / `fmt --check` / `cargo deny check bans licenses sources`, then `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`) — this is also where any new clippy lint fallout is cheapest to find first, since this is the most-exercised platform in this repo's history.
- **Windows:** re-run the `windows-build.yml` checks locally or via `workflow_dispatch` — `cargo build -p gist-ffi` under the MSVC toolchain, plus the C# bindgen (`BINDGEN_REV`-pinned) and `.NET SDK 10.x` requirement documented in `docs/windows-development-plan.md`. This session's own environment cannot build or test this (`GIST ENV` reports `windows-shell=NO`) — any Windows-side findings from this bump belong in `PENDING_WINDOWS_CHANGES.md` per the standing convention, not silently assumed fine.
- **Fuzz targets:** re-run all four (`fuzz_parse_txt`/`fuzz_parse_epub`/`fuzz_parse_docx`/`fuzz_web_extract`) under the rolled-forward nightly pin — this repo's own history (`N1`) shows the fuzz harness has broken silently across toolchain-adjacent changes before (a `ParseLimits` field addition, not a version bump, but the failure mode — "nothing catches this until someone tries to actually run it" — is the same category of risk here).
- **`gist-model`'s `wasm32-unknown-unknown` target:** required by `CLAUDE.md`'s crate conventions but not currently exercised anywhere in CI (no workflow references `wasm32` — confirmed by grep). Not a new gap introduced by this bump, but a Rust-version jump is a reasonable moment to add a `cargo check -p gist-model --target wasm32-unknown-unknown` step somewhere, since a wasm-target regression is exactly the kind of thing that would otherwise go unnoticed until someone tries to build the (currently nonexistent) wasm consumer.

## 6. Code-level risk categories (verify, don't assume)

Since precise 1.89–1.97 changelog content is outside what I can responsibly assert from memory, treat these as categories to check for, using the real release notes at execution time:

- **New `clippy::` lints firing under `-D warnings`.** This has already happened once in this exact repo on a much smaller bump (1.87.0 → 1.88.0 surfaced 9 new `clippy::uninlined_format_args` sites, per `N6`). A nine-version jump should be expected to surface more, possibly across several lint groups. Budget real time for this; fix each site on its own merits rather than blanket `#[allow]`-ing, consistent with this repo's existing practice.
- **`rustfmt` output drift.** Formatting defaults occasionally change between rustc/rustfmt releases. Run a bare `cargo fmt` immediately after the bump (before touching any other code) and commit that as its own dedicated, reviewable diff — don't let formatting churn hide inside a functional change.
- **FFI panic-safety boundary (`gist-ffi`'s `ffi_catch!` macro, `crates/gist-ffi/src/lib.rs`).** This is a hard security requirement (`CLAUDE.md`: "every `#[uniffi::export]` function is wrapped in `catch_unwind`... panics map to `GistError::InternalPanic`, never propagate across the C ABI"). `catch_unwind`/unwind-safety semantics are exactly the kind of thing worth a manual re-read after any significant rustc jump, even without a specific known change to point at — the cost of silently regressing it is high (a panic reaching across the FFI boundary into the Swift/C# host).
- **`panic = "unwind"` in the release profile.** Root `Cargo.toml` pins this deliberately (comment: `ffi_catch!`'s `catch_unwind` is a no-op under `panic = "abort"`, verified by a `panic_containment` CI example). Confirm this profile setting and its verifying example still build and still behave as documented after the bump.
- **`unsafe` code review.** `gist-parse-epub`/`gist-parse-docx` zip handling and `gist-store`'s crypto code are the highest-value places to re-read for any tightened `unsafe`-related lint or changed UB-detection behavior a newer rustc might introduce — not because a specific issue is known, but because these are this codebase's actual `unsafe`-adjacent surfaces (FFI, crypto, parsing untrusted input).

## 7. Recommended execution plan

1. Confirm the exact target patch version (e.g. `1.97.0` vs a later `1.97.x`) and check whether the Dependabot `aes-gcm` PR (§2) is open, merged, or closed — resolve that as its own reviewed change first if still open, since it's already-identified, security-relevant work independent of this bump.
2. Branch, bump `rust-toolchain.toml`, `rustup toolchain install <version>`.
3. `cargo fmt` as its own commit (see §6).
4. Run the full local gate: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources`, `cargo deny check advisories` (upgrading the local `cargo-deny` binary first if needed, §4). Fix fallout as separate, reviewable commits grouped by cause (lint fixes vs. dependency bumps vs. actual behavior changes).
5. `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`) on macOS.
6. Trigger `windows-build.yml` via `workflow_dispatch` (or hand off to a Windows session per `PENDING_WINDOWS_CHANGES.md` convention) — this session cannot execute this step directly.
7. Roll forward and re-verify the `fuzz.yml` nightly pin; run all four fuzz targets locally for at least the ~100s smoke-test baseline this repo has used before (`N1`).
8. Regenerate and diff `apps/apple/Generated/gist_ffi.swift` and the C# bindings; confirm no unexpected shape changes from the `uniffi`/dependency sweep in §3.
9. Update every version-string reference from §1's table in the same PR; grep the whole repo for the old version string as a final check before opening the PR, given this project's documented history of exactly this drift.
10. Update `PLATFORM_VERIFICATION.md` with fresh per-platform verification results and dates, per that file's own stated purpose.
11. Push and confirm all six GitHub Actions workflows are genuinely green on the real PR (not just local runs) before merging — this repo's own history (`N6`) shows local "BUILD SUCCEEDED" claims have previously masked real CI-only failures (a Swift type-checker timeout, a zip-codec linker failure) that only appeared once actually run on GitHub's runners.

## 8. Out of scope, noted for completeness

- **Edition 2024 adoption.** Independent decision from the rustc bump; all crates would need `edition = "2024"` plus a `cargo fix --edition` pass and a manual review of edition-2024's stricter defaults (e.g. `unsafe_op_in_unsafe_fn` becoming a hard requirement inside `unsafe fn` bodies, RPIT lifetime-capture rule changes, `unsafe extern` blocks). Worth considering once the toolchain bump itself is stable, not bundled into it.
- **MSRV floor (`rust-version` field).** This repo doesn't currently declare one in any `Cargo.toml`. Adding `rust-version = "1.97"` (or similar) to enforce the new floor explicitly is a reasonable follow-up but is a policy decision, not a requirement of the bump itself.
