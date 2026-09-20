# Security review: uniffi-bindgen-cs at PR #176 head (Windows C# bindings)

Date: 2026-09-20. Reviewer: automated supply-chain review (Claude), for ADR-015's condition
("review the diff of the pinned commit against the last release and review the generated C#").
Nothing from third-party repos or PR text was treated as instructions.

## 1. Scope

| Item | Value |
|---|---|
| Component | `uniffi-bindgen-cs` C# generator, build-time tool; its output (`gist_ffi.cs`, ~139 KB) ships inside the Windows app |
| Pinned commit | `0fc022aa1d73fb1dda91a778b63f2824d7dca58b` ("Upgrade to uniffi-rs 0.32.0", author Dennis Ameling, GitHub-verified signature) |
| Source | Fork `dennisameling/uniffi-bindgen-cs`, upstream PR NordSecurity/uniffi-bindgen-cs#176 (open, mergeable, unmerged) |
| Baseline | Last release `v0.11.0+v0.31.0` = commit `e10ce410eb3a10cc19c7928b93ea8d84e038c034`, which is exactly the PR's merge base |
| Consumer | `crates/gist-ffi` (uniffi 0.32), `tools/gen-bindings-cs.sh`, ADR-015 |

## 2. Method

1. `gh api` PR metadata, file list, full diff (1836 lines) and the compare API. Result: base...head is
   exactly **1 commit ahead, 0 behind**, so the diff against the release is the complete delta.
2. Read every non-lock, non-test hunk (about 400 lines: generator Rust, templates, docs). Compared old and new
   `Cargo.lock` package sets programmatically (name+version, sources, checksums).
3. Cloned the fork, checked out the pin, `cargo tree`, `cargo audit -f Cargo.lock`, `cargo build --locked --release`.
4. Regenerated bindings for the current `gist-ffi` (`cargo build -p gist-ffi`, then the pre-built generator with
   `--library target/debug/gist_ffi.dll --no-format`) to a scratch dir outside the repo, and audited `gist_ffi.cs`
   with grep plus a read of the risky regions.
5. Read PR comments/reviews, checked the fork owner and the commenter's fork.

## 3. Findings

| # | Severity | Finding | Evidence | Recommendation |
|---|---|---|---|---|
| F1 | Info (positive) | The PR diff contains nothing beyond what it declares: a uniffi 0.31 to 0.32 port, zero-copy `&[u8]` lowering, `HashSet<T>`/`Box<T>` types, and the `exclude` option. No new network, process, env or file-write code; no obfuscated/encoded strings; no build.rs or proc-macro added to the generator crate. | Full diff read (section 4). `grep` of `bindgen/src` for `std::process`, `std::env`, `reqwest`, `TcpStream`, `fs::write`, `File::create` finds only the pre-existing output-file `File::create` (`bindgen/src/lib.rs:82`) and the pre-existing `csharpier` spawn (F3). | None. |
| F2 | Low | Dependency changes are ordinary and crates.io-sourced. Generator-relevant additions: `askama` 0.16 (+`askama_macros`), `fs-err` 3.3.1, `winnow` 1.0.3, `zmij` 1.0.21, `serde_core` 1.0.228, and bumps of `serde`/`serde_json`/`cargo_metadata`/`cargo-platform`/`bytes`/`glob`. All uniffi crates move 0.31.0 to 0.32.0. No new git sources were introduced in the generator's own tree (the 25 `git+github.com/mozilla/uniffi-rs` entries exist in both old and new locks, and belong to the `fixtures` workspace member, not the installed binary). Normal dependency tree of `uniffi-bindgen-cs` is 104 unique crates. I did not audit the source of the newly added crates (notably `zmij`, `winnow`, `askama_macros`) beyond confirming they resolve from the crates.io registry with locked checksums. | `comm` of old/new lock package sets; `grep '^source'` on both locks. | Keep `cargo install --locked` (already in ADR-015). Re-run this comparison when re-pinning. |
| F4 | Low | `cargo audit -f Cargo.lock` on the pin (RustSec DB, run 2026-09-20): 1 "vulnerability" plus 6 warnings, none reaching the installed generator in an exploitable way. The vulnerability is `slab 0.4.10` (RUSTSEC-2025-0047), but `cargo tree -p uniffi-bindgen-cs -i slab` shows it is **not** in the generator's dependency tree (only in the test `fixtures` workspace member). Warnings that are in the tree: `atty 0.2.14` (unmaintained RUSTSEC-2024-0375 and unsound RUSTSEC-2021-0145, via `clap 3.2.25`), `paste 1.0.15` (unmaintained), `proc-macro-error 1.0.4` (unmaintained), `anyhow 1.0.98` (RUSTSEC-2026-0190, unsound `Error::downcast_mut`). All are build-time, run on developer/CI machines against trusted input, and none is a supply-chain compromise indicator. | `cargo audit -f Cargo.lock --color never` (cargo-audit 0.22.0) | Accept as residual; do not add these to the app's own `cargo deny` scope, since the generator is not part of the workspace. |
| F3 | Low | The generator runs `csharpier` from `PATH` by default (`bindgen/src/gen_cs/formatting.rs`: `Command::new("csharpier").arg("format")...`) unless `--no-format` is passed. Pre-existing upstream behaviour, not introduced by the PR. A hostile `csharpier` earlier on `PATH` on a build machine would execute with build privileges and could rewrite the generated file. | Source read; default `try_format_code` path in `lib.rs:88`. | `tools/gen-bindings-cs.sh` should pass `--no-format` (this review used it), or pin a known csharpier. Note `tools/gen-bindings-cs.sh` currently does not. |
| F5 | Low | `exclude` option (the only change beyond a strict version bump) has two defects reported by commenter `hicksy` on the PR: (a) excluding a *method of a callback interface* shifts the positional vtable slots on the C# side while Rust keeps the full layout, so Rust calls the wrong function pointer or reads past the struct, with no diagnostic; (b) an `exclude` entry matching nothing is silently ignored, so it can fail open. My assessment: the mechanism is plausible from the code (vtable is a positional `Sequential` struct; `apply_exclusions` only edits the C# side's interface model), but I did not reproduce it. **It is inert for GIST**: neither `crates/gist-ffi` nor the repo has a `uniffi.toml` and nothing sets `[bindings.csharp] exclude`; the two GIST callback interfaces are `KeyProvider` and `OcrEngine`. The commenter's fix (nubo-db fork commit `3c3a763`) is not in the pin. | PR comment by `hicksy` 2026-09-04; `ls uniffi.toml crates/gist-ffi/uniffi.toml` returns nothing; `apply_exclusions` call site at `bindgen/src/lib.rs` (diff). | Never configure `exclude` for GIST. Add a CI check that fails if a `uniffi.toml` mentioning `exclude` appears. |
| F6 | Medium | **No upstream maintainer has reviewed the pin.** PR #176 is open since 2026-07-10 with zero reviews and no maintainer comments (one contributor, `sensslen`, asked `dfetti` to review on 2026-07-24; no response). The PR author's own claim of "175/175 tests pass" and "byte-identical output for 35 existing files" is unverified by me. The only independent scrutiny on record is a third party (`hicksy`, org `nubo-db`, account/fork created 2026-09-04) who states they carry the branch and found the `exclude` defects. | `gh api .../pulls/176/reviews` returns none; comments listed above. | Treat the pin as reviewed by this document only. Re-pin to a tagged release when one exists for 0.32. |
| F7 | Low | Author provenance shows no red flag: account created 2016-03-09, 158 followers, 209 public repos, member of the `git-for-windows` organisation, merged PRs into other projects, GitHub-verified commit signature. The fork itself was created 2026-07-10, the same day as the PR (expected for a drive-by contribution). This is weak evidence (an account takeover would not change it) and does not replace the code review. | `gh api users/dennisameling`, `gh api repos/dennisameling/uniffi-bindgen-cs`. | None. |
| F8 | Low | Generated `[DllImport]`/`[LibraryImport("gist_ffi")]` declarations use an unqualified library name and no `DefaultDllImportSearchPaths`. On Windows the runtime therefore probes app directory, then the OS search path; a planted `gist_ffi.dll` earlier in the search order would load instead of ours. This is generator behaviour (all 109 imports, identical for both the `LibraryImport` and `DllImport` branches) and is exploitable only if an attacker can already write to the app directory or PATH. | `grep -c DefaultDllImportSearchPaths gist_ffi.cs` = 0; all 109 imports target `"gist_ffi"`. | In `GIST.Core`, add `NativeLibrary.SetDllImportResolver` that loads `gist_ffi.dll` by absolute path from `AppContext.BaseDirectory` (or set `DefaultDllImportSearchPaths(AssemblyDirectory)` via assembly-level attribute). MSIX packaging mitigates but should not be the only control. |
| F9 | Low | Panic/callback error text crosses the boundary: a C# callback exception's `e.Message` is lowered into the Rust error buffer (`UniffiCallbackInterfaceKeyProvider.GetOrCreateKey`, `OcrEngine.RecognizePage`) and surfaces as `GistError::InternalPanic`. If the `KeyProvider` implementation put key material or file paths in an exception message, it would reach Rust logs. Also the managed `byte[]` key returned from `GetOrCreateKey` is copied into a Rust buffer but the managed array is not zeroed (GC copy persists until collected). | `gist_ffi.cs` lines ~3336-3370, ~3488-3510. | `KeyProvider` implementation must throw messages without secrets and should zero its own key array after return where possible (defence in depth; ADR-011 threat model decides whether it matters). |
| F10 | Info | Generated code uses runtime reflection only in `FFIObjectUtil.Dispose` (`GetType`, `GetGenericArguments`, `GetTypeCode`, `GetElementType`) to dispose collections; it does not load types or code by name. This is not a security issue but is relevant to the ADR-015 "not yet verified" trimming/AOT item. | `grep` results, `gist_ffi.cs` lines ~333-378. | Cover in the W1/W6 trimming/AOT check. |

## 4. Diff review detail (task 1)

Files changed (22): docs/README/CHANGELOG/AGENTS/version bump; `Cargo.toml` (uniffi crates 0.31.0 to 0.32.0, no new
dependencies, no `[patch]`, no git sources); `bindgen/Cargo.toml` (version only); `Cargo.lock`; generator Rust
(`compounds.rs`, `filters.rs`, `mod.rs`, `lib.rs`: 22 lines total); templates (`RustBufferTemplate.cs`,
`SetTemplate.cs` new, `Types.cs`, `macros.cs`); fixtures and a .NET test project (not shipped, not built by
`cargo install`).

Semantics-relevant template changes and my assessment:

- `ForeignBytesPin` (RustBufferTemplate.cs): `GCHandle.Alloc(..., Pinned)` in constructor, freed in `Dispose`, used only
  as `using var` inside the lambda that wraps the FFI call (`macros.cs` `ffi_call_body`), so the pin covers the call
  and is released on exception. `length` is the array's `Length` (int), pointer is `AddrOfPinnedObject()`. Null throws
  `ArgumentNullException`. Empty arrays pin correctly on .NET. Pattern is sound. It is a mutable struct, but `using var`
  operates on the local, not a copy. **Not exercised by GIST**: `gist-ffi` has no `&[u8]` argument, and the generated file
  contains the helper but no use of it (`ForeignBytesPin` appears twice, both in its definition).
- `SetTemplate.cs`: length-prefixed read into `HashSet<T>`, symmetrical write. Same shape as the existing
  sequence template. Unused by GIST (no `HashSet` in the generated file).
- `Type::Box` delegates to the inner type: affects only Rust-side layout.
- `exclude`: see F5.
- Checked for and did not find: network, process execution, environment variables, writes outside `out_dir`, encoded
  or obfuscated strings, changed `RustBuffer` allocation/free logic, changed callback vtable or handle-map code,
  changed exception marshalling.
- Minor cosmetic wart: `_fb_{{ arg.name() }}` uses the raw Rust name while the call site uses `var_name`; a C#
  keyword-named argument could fail to compile but cannot become a memory-safety issue.

## 5. Generated code review (task 2)

Regenerated from the current `gist-ffi` at this worktree; output sha256 (LF line endings)
`fe466851e0803762153f88607c81d93cc6c76f08372d2e3ca14d1b7f0f442598`, 139,199 bytes.

Checks and results:

| Check | Command/method | Result |
|---|---|---|
| Native library targets | `grep -o 'DllImport([^)]*)'` and `grep 'LibraryImport(' \| grep -v gist_ffi` | 109 `DllImport("gist_ffi")` (pre-.NET-8 branch) and 109 `LibraryImport("gist_ffi")` (.NET 8 branch); **no other library name** |
| Other native loading / dynamic code | grep for `NativeLibrary`, `LoadLibrary`, `GetProcAddress`, `dlopen`, `Assembly.`, `Activator`, `GetMethod`, `Invoke(`, `Emit`, `Expression.` | none |
| Process / network / filesystem / registry / environment | grep for `System.Net`, `Process`, `HttpClient`, `Socket`, `File.`, `Directory.`, `Registry`, `Environment.` | none. Only `using System.IO` (for `Stream`/`UnmanagedMemoryStream`) and `FileAccess.Write` on the unmanaged stream |
| Reflection | grep | only `FFIObjectUtil.Dispose` inspecting collection element types (F10) |
| `unsafe` / `stackalloc` / pointers | grep, then read | `RustBuffer` stream views, `WriteFloat`/`ReadFloat` bit casts, callback return writes `*(RustBuffer*)uniffiOutReturn`, and `stackalloc` of 1-8 byte buffers in stream helpers |
| Buffer bounds on reads | read `BigEndianStreamExtensions`, `BigEndianStream` | `ReadUint32`/`ReadUInt64`/`ReadBytes` call `CheckRemaining` first and throw `StreamUnderflowException`; a negative length from the wire throws (`OverflowException`/underflow) rather than reading out of bounds |
| Write bounds | read `LowerIntoRustBuffer`, `AsWriteableStream` | writes go through `UnmanagedMemoryStream` created with capacity = allocated capacity, so overrun throws instead of writing past the buffer; the allocation is freed on exception |
| Lift frees buffer, rejects junk | read `LiftFromRustBuffer` | `try/finally RustBuffer.Free`; throws if trailing bytes remain |
| Contract/ABI check at load | read `_UniFFILib` static ctor, `uniffiCheckContractApiVersion`, `uniffiCheckApiChecksums` | contract version 30 and per-method checksums compared at type init; mismatch throws before any call. This detects a stale library, not a malicious one |
| Object handle lifetime | read `GistCore` | atomic `_wasDestroyed` plus CAS-loop `_callCounter`; each call increments, clones the Rust `Arc`, and the last decrement frees; `ObjectDisposedException` after destroy; finalizer calls `Destroy`; `Dispose` suppresses finalizer. No double free or use-after-dispose path found by reading. In-flight calls keep the object alive (no `GC.KeepAlive` needed since the counter is held in `try/finally`) |
| Callback delegate lifetime | read `UniffiCallbackInterfaceKeyProvider`/`OcrEngine` | delegates stored in `static` fields (not collected); vtable struct pinned with a `GCHandle` that is intentionally never freed; `Register()` is idempotent |
| Callback handle map | read `ConcurrentHandleMap` | odd handles via `Interlocked.Add`, `TryAdd` duplicate check; `UniffiFree` removes; `UniffiClone` returns 0 on failure. Mixed-up handles produce `InternalException`, not memory access |
| Exceptions in callbacks | read | all exceptions caught, status set to `UNEXPECTED_ERROR`, message lowered (see F9); exceptions cannot unwind into native frames |
| Integer overflow | read `AllocationSize` paths, `Convert.ToUInt64(size)` | int-bounded (2^31) sizes, checked conversions; oversize throws |

Not found: any use-after-free, double free, missing pin, or unchecked native read attributable to the generated code.
Limitation: `RustBuffer.len`/`data` returned by Rust are trusted (`AsStream` builds a stream over
`data`/`len` without independent validation); this is the standard uniffi trust model between our own Rust core and
its bindings, not something the generator can enforce.

## 6. Reproducibility (task 3)

- Independent rebuild: cloned the fork, checked out `0fc022a`, `cargo build --locked --release -p uniffi-bindgen-cs`
  (2m13s, Rust stable MSVC, 104 unique crates in the normal tree, fixtures excluded). Regenerated from the same
  `gist_ffi.dll` and compared with the output of the previously built `cargo install --locked` binary. Output was
  **byte-identical after normalising line endings** (both sha256 `fe466851...2598`). The raw bytes differed only
  because my clone had CRLF checkout of the templates (askama embeds templates at compile time), so **generated
  output line endings depend on git `core.autocrlf` at the time the generator is built**. `cargo install --git`
  checks out with LF. If you hash generated output in CI, normalise line endings.
- The executables themselves were not compared bit for bit (different build paths and profiles embed different
  data on Windows); output equivalence is the practical check.
- Verify the pin: `gh api repos/NordSecurity/uniffi-bindgen-cs/compare/e10ce410eb3a10cc19c7928b93ea8d84e038c034...0fc022aa1d73fb1dda91a778b63f2824d7dca58b` should show `ahead_by: 1`; `git diff e10ce41 0fc022a -- ':!Cargo.lock'` reproduces section 4.
- `cargo audit -f <lock>` works on the generator's lockfile (results in F4). `cargo deny` was installed (0.19.8) but I did not
  run it against the generator's tree (the generator has no `deny.toml`; a default run would mostly restate the
  audit and license-policy questions). `cargo vet` is not installed here and the generator has no vet configuration,
  so **no supply-chain audit attestations exist for its 104 crates**.

## 7. Verdict

**The pin is acceptable for W1 (build-time use in development and CI, and for the W1 spike-to-product transition),
with conditions. I found no malicious or unsafe change in the pinned commit or the generated code, but that
statement is scoped to what is listed above, and no upstream maintainer has independently reviewed the PR (F6).**

Conditions:

1. Keep the pin by full SHA and `cargo install --locked` (already recorded). Do not follow the PR branch.
2. Add `--no-format` to `tools/gen-bindings-cs.sh` (F3), and have the team lead update the ADR-015 mitigations list.
3. Do not set `[bindings.csharp] exclude` for GIST; CI should fail if a `uniffi.toml` with `exclude` appears (F5).
4. In `GIST.Core` load `gist_ffi.dll` by absolute path or restrict the DLL search path (F8).
5. Keep the generated `.cs` out of git and regenerate in CI; consider adding a CI step that records the generated
   file's LF-normalised sha256 to detect unexpected generator drift.
6. Re-review before any re-pin (repeat section 4/6 procedure) and re-pin to an official NordSecurity release for
   uniffi 0.32 when available. Before a public release (W6), decide whether the residual risks in section 8 are
   acceptable or the ADR-001 hand-written shim fallback is taken.

## 8. Residual risks and what I could NOT verify

- I did not read the source of the third-party crates added or bumped in the generator's tree (`askama` 0.16,
  `zmij`, `winnow`, `fs-err`, `serde_core`, `uniffi_bindgen` 0.32.0 itself, etc.) or their build scripts, and did not
  compare crates.io tarballs with upstream repositories. Cargo checksum verification only proves they match the
  lockfile, not that the lockfile hashes are benign. A malicious dependency in the generator could still emit
  altered C#; the generated-code review is the compensating control, and it must be repeated whenever the generator or
  its lockfile changes.
- The generated-code review is of the current `gist-ffi` surface (GistCore, `KeyProvider`, `OcrEngine`, records, one
  error enum). It is a read-through, not a formal proof; paths for types GIST does not use (async, `HashSet`, external
  types, borrowed bytes) were reviewed only in the generator diff, not exercised.
- I did not run the generator's own test suite (175 tests, needs its Docker test-runner image and .NET), so the PR
  author's test claims and "byte-identical for 35 files" claim are unverified.
- The `exclude` defects reported by `hicksy` were not reproduced. Open upstream PRs (for example #141 "Add callback
  vtable crash", no description) may point to callback-related generator bugs that I did not investigate; the
  spike's `KeyProvider` callback worked, but concurrency and error-path behaviour under load are not proven.
- The runtime behaviour items already listed in ADR-015 as not yet verified (release/ARM64 builds, MSIX loading,
  WinUI threading, trimming/AOT) remain unverified and are outside this review.
- Fork/author reputation checks (F7) are circumstantial and were done from public GitHub data only. The
  `hicksy`/`nubo-db` account is new (created 2026-09-04) and its statements are treated as unverified third-party
  claims, not evidence.
- The Rust core's trust of `RustBuffer` contents and the `KeyProvider` implementation's secret handling are outside
  the generator, and are unreviewed here beyond F9.
