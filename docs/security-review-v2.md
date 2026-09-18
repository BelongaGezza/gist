# GIST Reader — Security Review v2

**Prepared for:** Systems Architect
**Date:** 2026-09-18
**Codebase state:** `main` @ `1649d8d` — M1 complete, M2 (Library & Reading UI) in progress
**Method:** Independent re-verification against the actual working tree — not a transcription of `CLAUDE.md`'s change log. Every claim below that says "confirmed" was checked by reading the current source, and the build/test/lint/deny commands were actually re-run in this session, not assumed from prior notes.

**Supersedes:** `security-review-v1.md` (2026-09-08) in full. That document itself flagged that it was partial as of 2026-09-12, pending this pass.

---

## Executive summary

`CLAUDE.md`'s running log for this project is unusually long and dense, with a large number of specific "closed same day" claims. That is a legitimate reason for a reviewer to be skeptical on principle — a self-reported changelog is a claim, not evidence — so this pass treated every closed-finding claim (`F14`–`F25`, `A5`) as a hypothesis and independently re-derived it from the current source, not from the log. **Every one of those closure claims checked out against the actual code**, and the project's own gates were re-run fresh in this session, genuinely green:

- `cargo test --workspace` — all tests pass (24 in `gist-web`, 18 in `gist-store`, 10 in `gist-parse-epub`, 9 in `gist-parse-docx`, corpus test in `gist-core`, etc.)
- `cargo clippy --workspace -- -D warnings` — clean
- `cargo fmt --check` — clean
- `cargo deny check bans licenses sources` — `bans ok, licenses ok, sources ok`
- `cargo deny check advisories` — genuinely fails in this environment with exactly the documented error (a CVSS 4.0 RUSTSEC entry the bundled parser can't read), confirming that gap is real and not just asserted

The Rust core's security posture is genuinely strong for its stage: SSRF-safe URL fetching with per-redirect-hop resolver validation, zip-bomb caps on both spine/content and container-metadata reads, DRM-ambiguity handling that fails closed, FTS5 query escaping, poison-tolerant mutex handling everywhere, panic safety at the FFI boundary with a redirected panic hook, and content-addressed copy-on-import that keeps deletion away from the user's real files. This is not a rubber stamp of the log — each of these was read and, where practical, exercised directly.

**This pass also found four new issues** that the existing register does not capture (detailed below): the fuzz-testing infrastructure does not currently compile at all (not just "cargo-fuzz isn't installed" — the harnesses have a real, independent build break plus a workspace-configuration bug); the DOCX parser's decompression budget is enforced per-part rather than cumulatively, unlike the ePub parser's equivalent path; the image pre-processing crate decodes untrusted bytes before checking their size, a live instance of the exact pattern `A7` warns about for OCR; and `Cargo.lock` is tracked in git despite being listed in `.gitignore`, a process inconsistency worth a decision rather than a security bug.

The two architectural gaps that most matter for a personal reading app remain open and unchanged from the existing register: **no encryption at rest** (`A6` — confirmed: `gist-store::insert_item` writes document JSON via a plain `std::fs::write`, no encryption anywhere in the storage path) and **no App Sandbox / entitlements at all** (`A3` — confirmed: `apps/apple/macOS/Info.plist` and `project.yml` contain zero sandboxing, ATS, or hardened-runtime keys; there is no `.entitlements` file in the repository). Neither is a regression — both were already flagged — but this review re-confirms they are still fully open, not partially mitigated.

---

## Verification of previously-claimed closures

All of the following were re-derived from the current source (file/line references below), not taken on the log's word.

| ID | Claim | Verified against |
|----|-------|-------------------|
| `F14` (SSRF) | `fetch_url` resolves through `safe_resolve`, rejecting non-globally-routable addresses on every connection attempt including redirects | `crates/gist-web/src/lib.rs:111-186`; `.https_only(true)` + `.resolver(safe_resolve)` on the `ureq::AgentBuilder`; 13 passing unit tests covering loopback/RFC1918/link-local/CGNAT/IPv6-mapped/unique-local |
| `F15` (epub zip bomb, metadata) | `container.xml`/OPF/`encryption.xml` capped at 4 MiB independent of `ParseLimits` | `crates/gist-parse-epub/src/lib.rs:10,242-290`; `test_container_xml_zip_bomb_is_capped`, `test_encryption_xml_zip_bomb_is_capped` pass |
| `F16` (zip entry-count cap) | `max_zip_entries` (default 10,000) checked immediately after opening the archive, before any entry is read | `crates/gist-model/src/lib.rs:21-27,37`; enforced identically in `gist-parse-epub/src/lib.rs:94-99` and `gist-parse-docx/src/lib.rs:48-53`; both have a passing 6-entries-capped-at-5 test |
| `F17` (HTML recursion depth) | `collect_text` threads `depth`/`max_depth` and errors past the cap | `crates/gist-web/src/lib.rs:422-473`; `test_deeply_nested_html_is_rejected` (300 deep, capped at 200) and `test_nesting_within_depth_cap_succeeds` both pass — **see the new finding below: this code path currently has no fuzz coverage**, only the unit test |
| `F18` (Actions permissions) | All 6 workflows declare `permissions: contents: read` | Confirmed via `grep -n "permissions:" .github/workflows/*.yml` — present in all 6 files |
| `F19` (FTS5 injection) | Whole query wrapped as one quoted phrase, embedded `"` doubled, trailing `*` for prefix match | `crates/gist-store/src/lib.rs:838-843`; adversarial-input, plain-match, and short-prefix tests all pass |
| `F20` (DRM ambiguity) | Missing/unrecognized `Algorithm` attribute on `EncryptionMethod` rejected as DRM, not passed through | `crates/gist-parse-epub/src/lib.rs:57-66`; three tests (ambiguous/IDPF-exempt/commercial) pass |
| `F21` (robots.txt port) | `robots_url_for` uses `Url::join`, preserving an explicit port | `crates/gist-web/src/lib.rs:221-232`; both port-preservation tests pass |
| `F22` (panic hook) | Panic messages routed through `tracing::debug!`, no-op under `cfg(test)` | `crates/gist-ffi/src/lib.rs:47-62`; idempotency test passes |
| `F23` (`sanitize_ext`) | Extension reduced to ASCII alphanumerics before use in a content-addressed filename | `crates/gist-store/src/lib.rs:850-852`; traversal-string test and a test confirming the resulting file stays inside `originals/` both pass |
| `A5` / ADR-006 (copy-on-import) | `store_original_copy` writes a SHA-256-addressed copy; `remove_items` deletes only `source_copy_path`, never `source_path` | `crates/gist-store/src/lib.rs:291-394`; `crates/gist-core/src/lib.rs:427-470`; the doc-comment-described ordering (DB commit, then best-effort file cleanup) matches the code exactly; round-trip test passes |
| `F13` (TOCTOU, accepted risk) | `fs::metadata` size check still precedes a separate `fs::read` | `crates/gist-core/src/lib.rs:154-168` (`import_txt`) and `:288-297` (`import_file`) — confirmed still present exactly as described; still a reasonable accepted risk given the OS-file-picker threat model, unchanged assessment |
| Mutex poison tolerance | No bare `.lock().unwrap()` in `gist-store` | Every one of the 15+ lock sites in `crates/gist-store/src/lib.rs` uses `.unwrap_or_else(\|p\| p.into_inner())` |
| Schema versioning | `SchemaTooNew` guard before any migration runs | `crates/gist-store/src/lib.rs:93-100` |
| `cargo-deny` advisories gap | Genuinely can't run here | Reproduced live: `cargo deny check advisories` fails with `unsupported CVSS version: 4.0` parsing a RUSTSEC entry — matches the documented cause exactly |
| `F10` (release pipeline stub) | Guard step exits non-zero | `.github/workflows/release-macos.yml:21-24` — confirmed, plus the `contents: write` deferral is called out in a comment rather than added speculatively |
| `F24` (no GitHub remote) | Dependabot config inert | `git remote -v` returns nothing; confirmed |
| `A3` (entitlements) | Still fully open | No `.entitlements` file anywhere in the repo; `apps/apple/macOS/Info.plist` has no sandbox/ATS/hardened-runtime keys at all |
| `A6` (encryption at rest) | Still fully open | `Store::insert_item` (`crates/gist-store/src/lib.rs:211-218`) and `store_original_copy` (`:311-313`) both write plaintext via `std::fs::write` with no encryption layer |

No discrepancies were found between the log's closure claims and the code. This is worth stating plainly: the verification effort was real, and the result is that the log held up.

---

## New findings (this pass)

### N1 — Medium: The fuzz-testing infrastructure does not currently compile

`CLAUDE.md` documents this as "cargo-fuzz isn't installed in this dev environment... unverified since 2026-09-10." That undersells the actual state. Two independent, code-level problems exist beyond tooling availability:

1. **All four fuzz targets fail to compile against the current `gist_model::ParseLimits`.** `ParseLimits` gained a fifth field, `max_zip_entries`, when `F16` was fixed (`crates/gist-model/src/lib.rs:10-28`). All four targets (`fuzz/fuzz_targets/fuzz_parse_txt.rs:6-13`, `fuzz_parse_epub.rs:9-16`, `fuzz_parse_docx.rs:9-16`, `fuzz_web_extract.rs:26-32`) construct `ParseLimits` with a bare struct literal naming only the original four fields and no `..Default::default()`. `ParseLimits` is not `#[non_exhaustive]`, so this is an unconditional `E0063` (missing field) compile error in every target — the harnesses have been broken since `F16` landed (2026-09-12), six days before this review, and nothing has compiled them since.
2. **`fuzz/Cargo.toml` has no `[workspace]` table**, and the root `Cargo.toml` doesn't list or exclude `fuzz/` either. Running `cargo check` from inside `fuzz/` fails immediately with "current package believes it's in a workspace when it's not" — the same error `cargo fuzz build`/`cargo fuzz run` would hit, since cargo-fuzz shells out to plain `cargo` under the hood. This is independent of problem 1 and would block the fuzz targets even if their `ParseLimits` literals were fixed.

Both were confirmed directly: adding a temporary `[workspace]` table to `fuzz/Cargo.toml` (reverted after, `git status` clean) let `cargo check --bins` proceed far enough to hit dependency resolution, which separately failed because several transitive crates now require `rustc 1.88` against this project's pinned `1.87.0` — a third, environment-level obstacle on top of the two code-level ones.

**Impact:** the fuzz corpus/nightly-CI story in `CLAUDE.md`'s "What's done" section is currently aspirational, not just "unverified." None of the parser hardening work from the 2026-09-12 audit (`F15`–`F17`) has ever actually been fuzzed since it landed, only unit-tested.

**Recommendation:** fix the four `ParseLimits` literals (`..gist_core::ParseLimits::default()` is the minimal change), add `[workspace]` to `fuzz/Cargo.toml`, and either pin fuzz's own lockfile or accept that fuzzing needs a newer toolchain than the pinned `1.87.0` (cargo-fuzz projects are conventionally exempt from the workspace's toolchain pin for exactly this reason). Re-run the corpus once fixed.

### N2 — Low: `fuzz_web_extract`'s harness doesn't exercise the code it's meant to cover

Independent of N1's compile break, `fuzz/fuzz_targets/fuzz_web_extract.rs` only fuzzes `gist_web::fetch_url` with fuzzer bytes reinterpreted as a URL string. Per its own comments (lines 4-15), it never calls `extract_content`/`collect_blocks`/`collect_text` — the HTML-parsing and DOM-recursion code that `F17`'s depth cap actually lives in. Since `fetch_url` requires a well-formed `https://` URL before it does any HTTP work, almost all fuzzer input will be rejected at the URL-parse step and never reach interesting code, and a fuzz environment has no network access besides. **`F17`'s fix currently has zero fuzz coverage, only the two unit tests already in `gist-web`'s own test module.**

**Recommendation:** expose `gist_web::extract_content` (or an equivalent) as `pub(crate)`-visible-to-fuzz / `pub` behind a `fuzzing` feature, and fuzz raw HTML bytes directly, as the target's own `TODO` comment already suggests.

### N3 — Low: DOCX's decompression budget is enforced per-part, not per-document

`gist-parse-epub`'s `parse_spine` shares one running accumulator (`total_expanded`) across every spine item, so the *cumulative* decompressed content of an ePub is bounded by `limits.max_expanded_bytes` (`crates/gist-parse-epub/src/lib.rs:211-234`). `gist-parse-docx`'s equivalent, `read_zip_entry_limited` (`crates/gist-parse-docx/src/lib.rs:634-667`), takes a fresh `total = 0usize` on every call. It's invoked separately for `word/styles.xml`, `word/numbering.xml`, and `word/document.xml` (and possibly footnotes/comments parts), each independently allowed up to the full `max_expanded_bytes` budget (512 MB by default). A DOCX crafted with three maximally-compressible parts could therefore expand to roughly 3× the intended per-document ceiling (~1.5 GB) before any check rejects it, rather than the single-budget model the ePub parser and the `ParseLimits` doc comment ("Maximum decompressed bytes for zip-based formats") both imply.

This is lower severity than the original `F15`/`F16` findings — there is still a hard cap on each individual part, so this is a 3× budget overrun, not an unbounded one — but it's an inconsistency between the two zip-based parsers that share the same limit type, and worth closing for the same reason `F15` was: the field is documented as a whole-document budget, not a per-part one.

**Recommendation:** thread a shared accumulator through `parse_styles`/`parse_numbering`/`parse_document` the same way `gist-parse-epub::parse_spine` does, or introduce a separate `max_expanded_bytes_per_part` if the per-part model is intentional (in which case document that explicitly and lower the epub side to match, so both parsers state the same policy).

### N4 — Medium (dormant): `gist-imageprep::prepare_image` decodes untrusted bytes before checking their size

`A7` (open, `M3` gate) currently reads as a forward-looking warning about OCR's *callback interface* needing a size cap before it's wired up. In fact, the concrete decode-then-check gap it warns about already exists in shipped code today, one layer down: `gist-imageprep::prepare_image` (`crates/gist-imageprep/src/lib.rs:26-40`) calls `image::load_from_memory(raw_bytes)` — which fully decodes the image into an in-memory buffer sized by whatever dimensions the file's header claims — and only *afterward* computes `pixel_count` and compares it against `limits.max_expanded_bytes / 4`. The crate's own test (`test_pixel_limit`, lines 133-147) confirms this ordering: it must construct a real, already-small PNG to get a rejection, because the check has nothing to act on until decoding has already happened. A crafted image with a small compressed size but enormous declared dimensions (a classic decompression bomb, e.g. a "PNG bomb") would cause `image::load_from_memory` to attempt the full allocation before `prepare_image`'s own guard ever runs — the same class of problem `F15`/`F16` fixed for zip-based formats, not yet fixed here.

**Severity is dormant, not live:** `Core::import_image_with_ocr` is still `todo!()` (`crates/gist-core/src/lib.rs:565-570`), so `prepare_image` has no reachable caller from any FFI-exposed import path today. This is exactly why it's easy to miss — but it means the gap will ship the moment `M3`'s OCR wiring calls this function, unless it's fixed first.

**Recommendation:** treat this as the concrete first item under `A7`'s addendum, not a separate future decision. The `image` crate supports pre-decode limits (`image::ImageReader::with_guessed_format` + `.limits(Limits::default().max_image_size(...))`, or reading header dimensions before calling the full decode); either approach lets `prepare_image` reject an oversized image before allocating its pixel buffer, mirroring the "check before allocating" principle every other parser in this codebase already follows.

### N5 — Informational: `Cargo.lock` is tracked in git despite `.gitignore` listing it

`.gitignore` (root) lists `Cargo.lock` under a "Rust" section with the comment "library crates; keep for app binaries if preferred" — but `git ls-files` shows it is in fact tracked (68 KB, present in the working tree's commit history). `.gitignore` has no effect on a file already tracked, so this isn't a bug in the ignore rule, just an inconsistency between the stated intent and the actual repo state. Given this project ships an application (`gist-ffi` → xcframework → app), the standard, `cargo-deny`-friendly guidance is to commit `Cargo.lock` for reproducible builds and supply-chain auditing — which is what's actually happening. **Recommendation:** remove `Cargo.lock` from `.gitignore` to match reality, rather than leaving a stale entry that suggests the opposite of current practice.

---

## Updated security register

This supersedes the register in `CLAUDE.md` only insofar as it adds the four new items above; it does not change the status of any existing `F`/`A` item — every one of those was independently reconfirmed, not merely copied.

| ID | Severity | Status | Notes |
|----|----------|--------|-------|
| `F14`–`F23` | High→Low | ✅ Closed, reconfirmed 2026-09-18 | See verification table above |
| `A5` | Architecture | ✅ Closed, reconfirmed | Copy-on-import; known residual (shared-hash dedup, no refcounting) unchanged and still accepted |
| `F13` | Informational | Accepted, reconfirmed | TOCTOU window unchanged; still low risk under the local single-user, OS-picker threat model |
| `F10` | Low | Open — `M4` | Release pipeline still a deliberate stub |
| `F24` | Low | Open — inert until GitHub-hosted | No remote exists; confirmed |
| `A3` | Architecture | **Open — `M3` gate, fully unaddressed** | Zero sandbox/entitlements/ATS/hardened-runtime configuration exists yet, not partially done |
| `A4`/`A6` | Architecture | Open — `M4` | No at-rest integrity or confidentiality; plaintext confirmed at every storage write site |
| `A7` | Architecture | Open — `M3` gate | **Sharpened by `N4`**: the exact decode-before-check pattern this item warns about already exists in `gist-imageprep::prepare_image`, dormant only because nothing calls it yet |
| **`N1`** | **Medium** | **Open — new** | Fuzz harnesses don't compile (missing `ParseLimits` field + missing `[workspace]` table); unrelated to cargo-fuzz's absence from this environment |
| **`N2`** | **Low** | **Open — new** | `fuzz_web_extract` never reaches the HTML-parsing code it's meant to fuzz |
| **`N3`** | **Low** | **Open — new** | DOCX's `max_expanded_bytes` is per-part (~3× effective budget), unlike ePub's cumulative enforcement |
| **`N4`** | **Medium (dormant)** | **Open — new, feeds into `A7`** | `prepare_image` decodes before size-checking; must be fixed before `M3` OCR wiring, not just documented as a future ADR addendum |
| **`N5`** | **Informational** | **Open — new** | `Cargo.lock` tracked despite `.gitignore` listing it; align the ignore file with actual practice |

---

## Recommended priority for the next work session

1. **`N1`** — fix the fuzz harnesses (small, mechanical change) so the parser-hardening work from the last audit is actually exercised by fuzzing again, not just unit tests.
2. **`N4`** — add a pre-decode size guard to `prepare_image` now, while it's cheap and isolated, rather than after `M3` wires OCR to a live import path.
3. **`A3`** — start the entitlements/App Sandbox pass; currently the shipped debug build has no sandboxing at all, which is a bigger gap than "review pending" suggests.
4. **`N3`** — align DOCX's decompression accounting with ePub's cumulative model.
5. **`N2`, `N5`** — low-effort cleanups, bundle with the above.

Everything else in the existing register (`F10`, `F24`, `A4`/`A6`) is correctly already scheduled at its documented milestone gate and doesn't need to move.
