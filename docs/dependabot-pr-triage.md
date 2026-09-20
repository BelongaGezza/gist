# Dependabot PR triage

**Updated 2026-09-20** against `main` @ `07af835` (Rust pinned at 1.88.0). Supersedes the 2026-09-19 version of this file. 18 Dependabot PRs are open. Nothing here is required for any currently-green gate.

Evidence: `gh pr checks`, the PR diffs, upstream release notes, crates.io metadata (`rust_version`), and a grep of how this repo actually uses each crate.

## Freshness of CI results

PRs based on `0e9eb93` or `a46a2e8` (#2, #5, #8, #9, #10, #12, #18-#24) ran against a `main` that already has the current toolchain, Windows CI job and cargo-deny fix, so their results are trustworthy. PRs based on `59a5f21`, `129c079` or `ab14ee9` (#1, #3, #4, #7, #11) are **stale**: #3/#4/#11's `test`/`quality` failures are pre-fix breakage (old toolchain pin, cargo-deny CVSS-4.0 parse error), not real. They need `@dependabot rebase` before any verdict is trusted.

## Verdict table

| PR | Change | Verdict | Reason |
|---|---|---|---|
| #10 | `zip` 2.4.2 -> 8.6.0 | **MERGE-NOW** (merge on its own) | Both crates' Cargo.toml become `version = "8", default-features = false, features = ["deflate"]`; `deflate` still exists in 8.6.0, MSRV 1.88. All checks green incl. Windows and parser-corpus, so the entry-count (`max_zip_entries`), expanded-bytes bomb and DRM tests pass unchanged. Lockfile diff drops `arbitrary`/`crossbeam-utils`/`derive_arbitrary`, adds `typed-path`; no bzip2/lzma pulled in. No code change needed. |
| #24 | `sha2` 0.10.9 -> 0.11.0 | **MERGE-NOW** | Only use is `Sha256::new()/update/finalize` then `digest.iter().map(\|b\| format!("{b:02x}"))` in `Store::store_original_copy`; compiles unchanged on 0.11 (green on ubuntu/macos/windows). SHA-256 is a fixed standard function, so content-addressed filenames in `originals/` are identical; no on-disk migration. MSRV 1.85. |
| #5 | `uniffi` 0.32.0 -> 0.32.1 | **MERGE-NOW, with follow-up check** | Patch release; upstream notes list only a Kotlin checksum fix. Green on all OS. The Swift generator is built from `gist-ffi`'s own `uniffi/cli` feature, so it moves in lockstep. The pinned C# generator (`uniffi-bindgen-cs` rev `0fc022a`) is built `--locked` against `uniffi_bindgen 0.32.0` and its workspace requires `"0.32.0"` (caret), so semver-compatible with a 0.32.1 runtime, but **no CI job exercises C# generation** and I did not build the generator. After merge run `tools/gen-bindings-cs.sh` plus the .NET bindings spike and confirm `Generated/gist_ffi.cs` is unchanged; regenerate Swift with `tools/gen-bindings.sh`. `apple-build` did not run on this PR (lockfile only), so confirm it on the first main push. |
| #2 | `aes-gcm` 0.10.3 -> 0.11.1 | **HOLD (MSRV) / NEEDS-CODE-CHANGE** | Fails on every OS: `aes@0.9.3 requires rustc 1.89` (pin is 1.88.0). Beyond MSRV it is an API migration: `aead 0.6`/`cipher 0.5`/`hybrid-array` replace `generic-array`, so `Key::<Aes256Gcm>::from_slice`, `Nonce::from_slice` and `aead::OsRng` (`gist-store/src/lib.rs` lines 1-2, 107-135) must be rewritten and `generate_nonce` needs a rand_core 0.10 RNG. The construction (AES-256-GCM, 96-bit nonce, 16-byte tag) is standard so the on-disk format should not change, but that must be proved with a test decrypting a blob written by 0.10 before merging. Do it as a deliberate branch with a 1.89 toolchain bump (ADR-011/014 note). Not urgent: no advisory against 0.10.3. |
| #8 | `directories` 5.0.1 -> 6.0.0 | **MERGE-NOW** (better: remove the dep) | `gist-store` declares `directories = "5"` but no `.rs` file in the workspace references it (the storage dir is passed in by the caller). Green on all OS. Cheapest right fix is deleting the unused dep in a separate PR and closing this. |
| #1 | `roxmltree` 0.20.0 -> 0.21.1 | **MERGE after rebase** | Green but stale base (no Windows job). Changes: attribute accessors match local names, optional `entity_resolver`, fixes for a quadratic text merge and an entity panic (good for untrusted epub). Our call sites are only `Document::parse` (3). Rebase, confirm epub fixture tests, merge. |
| #12 | `encoding_rs` 0.8.35 -> 0.8.41 | **MERGE-NOW** | Green incl. Windows. MSRV 1.88 equals our pin. Declared in `gist-parse-txt` but not referenced in any `.rs` file. |
| #3 | `uuid` 1.26.0 -> 1.26.1 | **MERGE after rebase** | Patch. Red result is stale. MSRV 1.85. |
| #4 | `chardetng` 0.1.17 -> 1.0.0 | **MERGE after rebase** (or remove dep) | 1.0 API changes (enums instead of bools, `guess_assess` removed) are irrelevant: no source file uses `chardetng` (declared only). Red result is stale. |
| #7 | `infer` 0.15.0 -> 0.22.0 | **MERGE after rebase** | Only call: `infer::get(&bytes).map(\|t\| t.mime_type())` in `gist-core` (line ~389), a stable API. test/corpus green, `quality` red and base stale. Release notes include hardening (non-recursive LZ4/zstd frame detection). MSRV 1.74. Re-run the corpus after rebase to confirm mime gating is unchanged. |
| #9 | `actions/checkout` 4.2.2 -> 7.0.1 | **MERGE-NOW** | Pinned SHA `3d3c42e5...` verified to resolve to tag v7.0.1; `apple-build` and all tests green with it. The v6.1/v7 breaking change concerns `pull_request_target` unsafe checkouts; no workflow uses `pull_request_target`. Node runtime bump only. |
| #18 | `actions/cache` 4.2.0 -> 6.1.0 | **MERGE-NOW** | SHA `55cc8345...` verified as v6.1.0; all checks green incl. `apple-build`. Notes: read-only cache-token handling. |
| #11 | `actions/upload-artifact` 4.6.0 -> 7.0.1 | **MERGE after rebase** | SHA `043fb46d...` verified as v7.0.1. Red result is stale. Used only on failure/nightly in `fuzz.yml`/`parser-corpus.yml`. v6+ runs on Node 24 (runner >= 2.327.1; hosted runners fine). |
| #19 | `zip` 8.6.0 in /fuzz | **BATCH with #10** | Duplicate of #10 (same two crate manifests plus `fuzz/Cargo.lock`); conflicts once #10 merges. |
| #20 | `infer` 0.22 in /fuzz | **BATCH with #7** | Same duplicate pattern. |
| #21 | `chardetng` 1.0.0 in /fuzz | **BATCH with #4** | Same. |
| #22 | `encoding_rs` 0.8.41 in /fuzz | **BATCH with #12** | Same. |
| #23 | `sha2` 0.11.0 in /fuzz | **BATCH with #24** | Same. |

After a root PR merges, comment `@dependabot rebase` on its fuzz twin; it should shrink to a `fuzz/Cargo.lock` update, which is worth keeping so the fuzz lock does not drift.

## Recommended merge order

1. #9, #18 (Actions; SHA pins verified), then #11 after rebase.
2. #12, #3, #1 (rebase where stale).
3. #24 sha2, then #23.
4. #10 zip alone, then #19.
5. #5 uniffi, then run both bindings generators (see decisions).
6. #7, #4, #8 each after rebase (or remove the unused deps instead).
7. Remaining fuzz twins #20-#22 as their root PRs land.
8. #2 aes-gcm last, separately, after a toolchain decision.

## Decisions needed from a human

- **aes-gcm 0.11 / Rust 1.89:** bump the toolchain and migrate the AEAD code (with a 0.10-written-blob decrypt test), or add an `ignore` for `aes-gcm` in `dependabot.yml` until then. Recommend the latter until a deliberate crypto-migration pass.
- **uniffi 0.32.1 vs the C# generator:** no CI covers it; someone with the Windows toolchain should run `tools/gen-bindings-cs.sh` post-merge. Consider adding it to CI.
- **Unused dependencies:** `directories`, `chardetng`, `encoding_rs` are declared but unused in source. Remove them (and close #4, #8, #12 and their fuzz twins) or keep; removal reduces supply-chain surface.
- **zip 8:** a six-major jump merged on the strength of the existing bomb/entry-count tests (pass on all three OSes, no code change needed). Confirm you are comfortable.

## Open security alerts (carried over)

All four open Dependabot alerts were in `fuzz/Cargo.lock` and were fixed by the fuzz lock refresh; `.github/dependabot.yml` now covers `/fuzz` and `core-quality.yml` runs `cargo audit --file fuzz/Cargo.lock`. Remaining item: re-read the real GitHub alert list and confirm all 4 are closed.

## Notes

- If a PR needs real code changes, do them on a branch based on the PR's branch rather than force-pushing over Dependabot's commit.
- This triage was documentation-only; no PR was merged, closed or rebased.
