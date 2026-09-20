# Dependabot PR triage

**Purpose:** 11 Dependabot PRs are open as of 2026-09-19, none yet reviewed or merged. This file tracks them so triage can happen in a dedicated pass instead of being reconstructed from scratch. Not urgent — none of these are the RUSTSEC-driving PRs (those were `scraper` #6, already merged, and the direct fixes in PR #17).

**Before touching any of these:** `main` has moved substantially since these PRs were opened (2026-09-18) — the Rust toolchain bumped to 1.88.0, `quick-xml`/`ureq`/`scraper` were upgraded, and `.github/workflows/core-quality.yml`'s `cargo-deny-action` was bumped to v2.1.1. Every one of these PRs' existing CI results predates all of that and is stale. **Rebase (`@dependabot rebase` as a PR comment) before trusting any check result**, the same way PRs #6/#12 were handled earlier.

**How to use this file:** work top to bottom, lowest-risk first. For each: rebase, wait for CI, review the actual diff (not just the version bump — check for a linked changelog/release notes), merge or hold. Check the box and note anything that came up (a needed code change, a reason to hold) right under it.

## Low risk — patch/trivial bumps, merge after rebase + green CI

- [ ] **#1 `roxmltree` 0.20.0 → 0.21.1** — minor bump within 0.x. Used by `gist-parse-epub` (`container.xml`/OPF parsing). Check for API changes in the 0.21 changelog before assuming safe.
- [ ] **#3 `uuid` 1.26.0 → 1.26.1** — patch bump. Low risk.
- [ ] **#5 `uniffi` 0.32.0 → 0.32.1** — patch bump, but `uniffi` is ADR-001's mandated FFI layer underpinning `gist-ffi` and the Swift bindings. After merging, regenerate bindings (`./tools/gen-bindings.sh`) and confirm `apple-build` still passes on real CI — don't assume a patch bump is inert here given how much of today's `apple-build` work was "should be fine" turning out not to be.
- [ ] **#12 `encoding_rs` 0.8.35 → 0.8.41** — patch-level bump within 0.8.x. This is the PR that originally needed rustc 1.88 (now satisfied by the merged toolchain bump) — should just work now. Already independently reconfirmed green on `test`/`parser-corpus` once, but re-verify after rebase since `main` has moved further since.
- [ ] **#9 `actions/checkout` 4.2.2 → 7.0.1** — GitHub Action major-version jumps, but `checkout`'s core behavior is stable across majors (mostly Node.js runtime bumps). Low functional risk. Verify the new SHA resolves (same class of bug as `N6`) before merging.
- [ ] **#11 `actions/upload-artifact` 4.6.0 → 7.0.1** — same category as #9, and only used in `fuzz.yml`/`parser-corpus.yml` (nightly/manual workflows, not on the PR-blocking critical path). Low risk, low urgency.

## Medium risk — check for breaking API changes before merging

- [ ] **#2 `aes-gcm` 0.10.3 → 0.11.1** — minor bump, but this is the encryption crate behind ADR-011/013/014 (at-rest encryption/integrity). Security-sensitive: run the full `gist-store` encryption round-trip test suite after rebasing, not just a green CI glance. Check the 0.11 changelog for any semantic changes to nonce handling or key sizing before merging.
- [ ] **#4 `chardetng` 0.1.17 → 1.0.0** — a 0.x → 1.0 jump (semantically a major bump). Used for charset detection during text/DOCX import. 1.0 releases sometimes reflect real API stabilization changes, not just a version-number formality — check its changelog/migration notes, don't assume it's a no-op because "it's just hitting 1.0."
- [ ] **#7 `infer` 0.15.0 → 0.22.0** — a large jump across many 0.x minor versions (each technically allowed to break, per semver's pre-1.0 rules). Used for file-type detection during import. Check for API changes across that range, not just the diff between adjacent versions.
- [ ] **#8 `directories` 5.0.1 → 6.0.0** — major version bump. Used for platform storage-directory resolution, which `gist-store`/`gist-core`'s copy-on-import (ADR-006) and IR storage (ADR-007) paths depend on being correct. Review the 6.0 changelog for any path-resolution behavior changes before merging — a silent behavior change here could misdirect where user data gets written/read.

## High risk — needs real review time, don't rush

- [ ] **#10 `zip` 2.4.2 → 8.6.0** — a 6-major-version jump in the exact crate (`gist-parse-epub`/`gist-parse-docx`) that today's `N6`/`N7` work already touched twice (scoping to `deflate`-only features, then the `quick-xml`/`ureq` RUSTSEC fixes in the same files). Given how much breaking-API surface even `quick-xml`'s single major jump had, expect real work here: read the `zip` changelog across all 6 majors, check whether the `default-features = false, features = ["deflate"]` scoping from `N6`'s fix still applies the same way, rebuild, and re-run every fixture-parsing test — not just a CI glance. Budget real time for this one; don't merge it opportunistically alongside a "just rebase and merge" pass through the others.

## Open security alerts (added 2026-09-20)

GitHub reported **4 open Dependabot vulnerabilities on `main` (1 high, 1 moderate, 2 low)** when `b74726f` was pushed. `gh` was not authenticated on the Windows machine, so the alert list itself could not be read (https://github.com/BelongaGezza/gist/security/dependabot is the authoritative list; confirm the mapping below against it).

`cargo audit` (RustSec DB, 1251 advisories) run locally on both lockfiles: **root `Cargo.lock` is clean; `fuzz/Cargo.lock` has 6 findings.** PR #17's fixes only touched the root lockfile, and the fuzz workspace has its own stale lockfile (`quick-xml` 0.36.2, `rustls-webpki` 0.101.7, `time` 0.3.45, `fxhash` 0.2.1). Fuzz targets are dev tooling and are not shipped, but they still parse untrusted input in nightly CI.

| Crate (fuzz lock) | Advisory | Severity | Fix |
|---|---|---|---|
| `quick-xml` 0.36.2 | RUSTSEC-2026-0194 quadratic attribute check | 7.5 high | >= 0.41.0 |
| `quick-xml` 0.36.2 | RUSTSEC-2026-0195 unbounded namespace allocation | 7.5 high | >= 0.41.0 |
| `rustls-webpki` 0.101.7 | RUSTSEC-2026-0098 URI name constraints | n/a | >= 0.103.12 |
| `rustls-webpki` 0.101.7 | RUSTSEC-2026-0099 wildcard name constraints | n/a | >= 0.103.12 |
| `rustls-webpki` 0.101.7 | RUSTSEC-2026-0104 CRL parse panic | n/a | >= 0.103.13 |
| `time` 0.3.45 | RUSTSEC-2026-0009 stack exhaustion DoS | 6.8 medium | >= 0.3.47 |
| `fxhash` 0.2.1 | RUSTSEC-2025-0057 unmaintained (warning) | n/a | drop via `scraper` bump |

The 6 vs 4 count and the severity split do not match GitHub's report exactly (different advisory database and grouping), so this table is a best local reconstruction, not the alert list.

- [ ] Refresh `fuzz/Cargo.lock` so none of the above remain
- [ ] Check `.github/dependabot.yml` covers `/fuzz` (likely why its lockfile went stale unnoticed)
- [ ] Re-read the real GitHub alert list after the push and confirm all 4 are closed

## Notes

- None of these block anything currently green — `core-test`, `core-quality`, `parser-corpus`, and `apple-build` are all fully passing on `main` without any of these merged.
- If a PR turns out to need real code changes (likely for `zip`, possibly `chardetng`/`infer`/`directories`), do that work on the PR's own branch (or a fresh branch based on it) rather than force-pushing over Dependabot's commit, so Dependabot can still track and re-open it on the next release if needed.
