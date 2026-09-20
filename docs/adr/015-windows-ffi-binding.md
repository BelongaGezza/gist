# ADR 015 — Windows FFI binding: uniffi C# bindings

**Date:** 2026-09-20
**Status:** Accepted, with a supply-chain condition (see Consequences)

## Context
ADR-001 chose uniffi (proc-macro mode) for the Swift bindings and reserved a hand-written C ABI shim "for Windows, at Windows kickoff." `docs/windows-development-plan.md` §3 preferred generating C# bindings from the *same* `gist-ffi`, contingent on a W0 spike, because one binding source keeps Apple and Windows behaviourally identical. Risk WR1 was that no C# generator supports uniffi 0.32, the version this repo uses.

Findings, 2026-09-20:
- The only maintained C# generator is NordSecurity's `uniffi-bindgen-cs`. Its latest release, `v0.11.0+v0.31.0` (2026-06-23), targets uniffi **0.31**, so it cannot read a 0.32 library.
- Upstream PR #176 ("Upgrade to uniffi-rs 0.32.0", from an outside fork, `dennisameling/uniffi-bindgen-cs`, one commit, open and mergeable) adds 0.32 support. Issue #183 confirms 0.32 has breaking changes. A commenter reports carrying the branch in a fork for a shipping desktop app.
- **Spike:** built the generator at PR #176 head `0fc022aa1d73fb1dda91a778b63f2824d7dca58b` (`cargo install --locked`; without `--locked` cargo pulls a second `toml` major and it fails to compile), generated bindings from `target/debug/gist_ffi.dll` in library mode, and drove them from a .NET 10 console app (`apps/windows/spikes/bindings`). All 20 checks passed against the real Rust core:
  - construction (`NewWithReadKey`) and `Health`; import, list, FTS search including partial-word prefix and hostile input; RSVP and document JSON endpoints; collections; tags; save progress
  - typed errors: `GistException.DrmProtected` for the DRM fixture (not string matching), a generic `GistException` for a missing file
  - the `KeyProvider` callback interface invoked from Rust, `EncryptItems`, then read-after-encrypt (the ADR-014 requirement) and `ContentEncrypted` in listings
  - 64 concurrent FFI calls from a thread pool, no errors
  - `RemoveItems(deleteSourceFiles: true)` leaves the user's original file in place
  - a wrong-length key from the callback surfaces as `GistException.InternalPanic` and the process survives (the `ffi_catch!` guarantee holds across the C# boundary **in debug builds only; see the amendment below**)

## Decision
Use uniffi-generated C# bindings over the unchanged `gist-ffi` crate. No Windows-specific C ABI shim is written. The generator is pinned to the exact commit above and built with `--locked`.

## Consequences
- One FFI surface for both platforms; no second API to keep in sync. `tools/gen-bindings-cs.sh` records the pin and install command.
- **Supply-chain condition.** The generator is unreviewed third-party code that runs at build time and whose output ships in the app, and it is currently an unmerged PR from an outside fork. Mitigations: pin by full commit SHA (never a branch); review the generated C# and the diff of the pinned commit against the last release before W1 closes; re-pin to an official release as soon as upstream publishes one for 0.32 (watch #176/#183); keep the generated file out of git so it is always rebuilt from the pinned generator. **Fallback if this becomes untenable:** the hand-written `extern "C"` shim from ADR-001 (~25 exported methods; est. 2-3 weeks); nothing in the spike depends on the generator's internals beyond generated signatures.
- **Version coupling.** The generator must match `uniffi` in `Cargo.lock` (0.32.x). A uniffi bump on the Apple side (e.g. Dependabot `uniffi` 0.32.1, PR #5) needs the generator to be compatible before merge, so those PRs must run the Windows bindings job once it exists (W1 CI).
- Generated types are `internal`; `GIST.Core` owns them and `GIST.Core.Tests` needs `InternalsVisibleTo`. The app layer sees only `CoreClient` and view models.
- serde's externally-tagged enums (e.g. `{"Paragraph":{...}}` in `get_document_json`) arrive as JSON strings and still need a custom `JsonConverter` (spike confirmed the tag shape).
- **Not yet verified:** release builds and ARM64; loading `gist_ffi.dll` from an MSIX-packaged app; UI-thread behaviour under WinUI; the generated code under trimming/AOT; the callback interface `OcrEngine` (unused until M3). Each is a W1/W6 exit item.

## Security review (2026-09-20)
`docs/security-review-windows-bindgen.md` reviews the pinned commit's full diff against v0.11.0 (one commit, 22 files) and the generated C#. **Verdict: acceptable for W1 with conditions; nothing malicious or unsafe found.** Limits: no upstream maintainer has reviewed PR #176, and the source of the newly added dependency crates was not audited. `cargo audit` on the generator's lockfile shows one vulnerability that lives only in its test fixtures (slab, RUSTSEC-2025-0047) plus unmaintained-crate warnings in a build-time tool. Independent rebuild from a fresh clone produced byte-identical output once line endings are normalised.

Conditions (each a W1 exit item): keep the pin by full SHA and `--locked`; `tools/gen-bindings-cs.sh` passes `--no-format` (done); never set the generator's `exclude` option and add a CI guard for it; load `gist_ffi.dll` by absolute path or set `DefaultDllImportSearchPaths` (the generated `DllImport("gist_ffi")` is an unqualified name); regenerate in CI and keep the generated file out of git; repeat the review before any re-pin and re-pin to an official release as soon as one exists for uniffi 0.32.

## Amendment 2026-09-20 — panic containment and the release build
The spike's "bad callback -> `InternalPanic`, process survives" result was first obtained on a **debug** DLL. Re-running the spike against a **release** DLL showed the workspace release profile (`panic = "abort"`) killed the process instead (exit `0xC0000409`), so the statement above was not true of shipped builds (`docs/review-pre-w1-quality-security.md`, Q1). Fixed on `fix/w1-gates`: the release profile now keeps `panic = "unwind"`, CI runs a release-mode `panic_containment` probe on every OS, and the spike passes 20/20 against the release DLL (process survives, `InternalPanic` returned). The same review (Q2) also found the DLL imported the dynamic VC++ runtime; Windows targets now link the CRT statically and CI checks the DLL's imports.
