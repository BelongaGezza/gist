# ADR 012 — macOS App Sandbox entitlements

**Date:** 2026-09-18
**Status:** Accepted

## Context

Security register item `A3` flagged that GIST's macOS app shipped with **no App Sandbox configuration at all** — no `.entitlements` file existed anywhere in the repository, and `apps/apple/macOS/Info.plist` carried no sandbox, ATS, or hardened-runtime keys. This was previously tracked as "needs review," but a from-scratch inspection found there was nothing partially done: the app ran fully unsandboxed. For an application that imports arbitrary user documents (txt/epub/docx) and fetches arbitrary user-supplied URLs (`gist-web::fetch_url`), running unsandboxed means a bug or supply-chain compromise in any dependency (the zip/XML/HTML parsers in particular, given they process untrusted input directly) has unrestricted access to the user's entire filesystem and network, not just what the app actually needs.

`ENABLE_HARDENED_RUNTIME: YES` was already set in `apps/apple/project.yml`, but hardened runtime and App Sandbox are separate, independent protections — the former restricts code-injection/library-validation at the process level, the latter restricts filesystem/network/IPC access via entitlements. Having one said nothing about the other.

## Decision

Enable App Sandbox (`com.apple.security.app-sandbox`) for `GISTmacOS`, with exactly two additional entitlements, each tied to a concrete existing feature rather than requested speculatively:

- **`com.apple.security.files.user-selected.read-write`** — the import flow (`LibraryView`'s "Import File" toolbar action) drives an `NSOpenPanel`, then `gist-core` reads the chosen file once and immediately copies it into GIST's own sandboxed storage via ADR-006's copy-on-import. Nothing in the app re-reads the original path in a later session — every subsequent read goes through the sandboxed copy or the serialized IR blobs under GIST's own container. The standard per-launch user-selected-file grant this entitlement provides is therefore sufficient; **no security-scoped bookmark persistence was added**, since nothing needs standing access to a location outside the container beyond the single read-then-copy that happens at import time.
- **`com.apple.security.network.client`** — URL-paste import (`CoreClient.importUrl` → `gist-web::fetch_url`) makes outbound HTTPS requests. This is client-only: GIST runs no server and accepts no inbound connections, so the broader `com.apple.security.network.server` entitlement was deliberately not requested.

No other entitlements were added. In particular, no bookmark/persistent-resource-access entitlements, no hardware entitlements (camera/microphone — OCR image import in M3 will use an image picker, not live capture, so this can be revisited then if that assumption changes), and no App Group or keychain-sharing entitlements (ADR-011's key-provider work, if it lands, will need to add `keychain-access-groups` at that point — not added here since it isn't needed yet).

Implementation: `apps/apple/macOS/GISTmacOS.entitlements` is a new file, wired into `apps/apple/project.yml` via `CODE_SIGN_ENTITLEMENTS: macOS/GISTmacOS.entitlements` on the `GISTmacOS` target. This is independent of CI's `CODE_SIGNING_ALLOWED=NO`/`CODE_SIGNING_REQUIRED=NO` invocation flags: those flags mean CI's build product is never actually signed, so the entitlements are never embedded in that build (verified — an unsigned build produces no `.xcent` file at all). The sandbox therefore only takes effect for an actually-signed build (Automatic signing with a local development identity, or a real Developer ID release build later) — this is expected and correct, not a gap: CI's job is to prove the app still builds and its logic tests still pass, not to exercise sandbox enforcement, which requires a signed, launched process.

## Verification

- `xcodegen generate` regenerated `GIST.xcodeproj` with `CODE_SIGN_ENTITLEMENTS` present on both build configurations.
- `xcodebuild build -scheme GISTmacOS CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO` — **BUILD SUCCEEDED**, matching CI's existing invocation exactly.
- `xcodebuild build -scheme GISTmacOS CODE_SIGN_IDENTITY="-"` (ad-hoc signed, so entitlements actually get embedded) — **BUILD SUCCEEDED**; `codesign -d --entitlements -` on the resulting `GIST.app` confirms all three entitlements (`app-sandbox`, `files.user-selected.read-write`, `network.client`) plus Xcode's own automatically-added `get-task-allow` (debug-only, stripped from release/notarized builds) are genuinely present in the signed binary, not just declared in the project file.
- `xcodebuild test -scheme GISTmacOS CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO` — **43/43 tests pass**, unaffected. (`GISTTests`' file-based tests operate against `FileManager.default.temporaryDirectory` subdirectories, which remain freely accessible to a sandboxed app's own container regardless of the user-selected-file entitlement, so no test needed to change.)

**Not verified in this environment:** an actual runtime click-through of a signed, sandbox-enforced app instance (launching it, exercising the file-import `NSOpenPanel` flow, and confirming no sandbox-violation denials appear in the system log). This environment has no Accessibility permission for `osascript`/System Events to drive that automatically, consistent with the same limitation already noted elsewhere for this project's manual-QA passes. A person should do one pass of: import a file, import a URL, confirm no `sandboxd` denial messages appear in Console.app, before this is considered fully closed rather than "correctly configured and building."

## Consequences

**Easier:** a compromised or buggy dependency (a malicious epub/docx triggering a parser bug, for instance) is now contained to what the sandbox entitlements allow — it cannot read arbitrary files elsewhere on the user's disk, cannot open listening sockets, and cannot make outbound connections to anything but what `com.apple.security.network.client` permits (which `gist-web`'s own SSRF-safe resolver, `F14`, already restricts to public addresses at the application layer — this is now a second, OS-enforced layer behind that one, not a replacement for it). This directly reduces the blast radius the audit's SSRF and zip-bomb findings would have had if a mitigation had ever been incompletely applied.

**Harder / to watch:** any future feature that needs standing access to a location outside the sandbox container (e.g., "watch this folder for new books" or a persistent bookmark to a synced cloud-drive folder) will need a security-scoped bookmark and possibly a new entitlement, not just a file path. `A7`'s OCR image-import work (M3) should double check whether its image-picker flow needs an additional entitlement (likely still just `files.user-selected.read-write` if it's picker-driven, but confirm rather than assume). `ADR-011` (encryption-at-rest, if accepted) will need to add Keychain-related entitlements when it lands — not addressed here.
