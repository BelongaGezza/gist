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
  - a wrong-length key from the callback surfaces as `GistException.InternalPanic` and the process survives (the `ffi_catch!` guarantee holds across the C# boundary)

## Decision
Use uniffi-generated C# bindings over the unchanged `gist-ffi` crate. No Windows-specific C ABI shim is written. The generator is pinned to the exact commit above and built with `--locked`.

## Consequences
- One FFI surface for both platforms; no second API to keep in sync. `tools/gen-bindings-cs.sh` records the pin and install command.
- **Supply-chain condition.** The generator is unreviewed third-party code that runs at build time and whose output ships in the app, and it is currently an unmerged PR from an outside fork. Mitigations: pin by full commit SHA (never a branch); review the generated C# and the diff of the pinned commit against the last release before W1 closes; re-pin to an official release as soon as upstream publishes one for 0.32 (watch #176/#183); keep the generated file out of git so it is always rebuilt from the pinned generator. **Fallback if this becomes untenable:** the hand-written `extern "C"` shim from ADR-001 (~25 exported methods; est. 2-3 weeks); nothing in the spike depends on the generator's internals beyond generated signatures.
- **Version coupling.** The generator must match `uniffi` in `Cargo.lock` (0.32.x). A uniffi bump on the Apple side (e.g. Dependabot `uniffi` 0.32.1, PR #5) needs the generator to be compatible before merge, so those PRs must run the Windows bindings job once it exists (W1 CI).
- Generated types are `internal`; `GIST.Core` owns them and `GIST.Core.Tests` needs `InternalsVisibleTo`. The app layer sees only `CoreClient` and view models.
- serde's externally-tagged enums (e.g. `{"Paragraph":{...}}` in `get_document_json`) arrive as JSON strings and still need a custom `JsonConverter` (spike confirmed the tag shape).
- **Not yet verified:** release builds and ARM64; loading `gist_ffi.dll` from an MSIX-packaged app; UI-thread behaviour under WinUI; the generated code under trimming/AOT; the callback interface `OcrEngine` (unused until M3). Each is a W1/W6 exit item.
