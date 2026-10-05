# ADR-022: Process isolation for pdfium (finding F33)

Status: **Proposed. Design record plus measured spike; nothing is implemented in the product (user decision D3, M7).** The recommendation at the end is for the user to decide.

## Context

`gist-parse-pdf` loads `libpdfium.dylib` (C++, ADR-002) and feeds it untrusted files inside the app process. `ffi_catch!` contains Rust panics only; a native crash (SIGSEGV/SIGABRT in pdfium) is not catchable and terminates the whole app. A memory-safety bug reachable from a hostile PDF could, at worst, be exploited with the app's privileges. Those privileges are already narrowed by the App Sandbox (ADR-012): user-selected read-write files, outbound network client, the app container.

Existing mitigations (all in force): `ParseLimits` and the PDF text budget (F36) are checked before allocation; the `%PDF-` magic check; encrypted documents are rejected; `gist-parse-pdf` is `forbid(unsafe_code)`; the fuzz target `fuzz_parse_pdf` has run 300 s+ without a crash. None of these contain a crash inside pdfium itself, because pdfium parses xref/objects before any of our limits can run (ADR-002 addendum).

## Threat model

| Aspect | In-process (today) | Separate process |
|---|---|---|
| Native crash from hostile PDF | App dies; in-flight import lost; unsaved UI state lost (library and RSVP progress persist in SQLite) | Helper dies; app survives and shows a typed error |
| Memory-safety exploit | Attacker code runs with all app privileges: the library DB and IR blobs, the Keychain item for ADR-011 keys, user-selected files, the network client | Attacker code runs with the helper's privileges only; with a minimal sandbox that is no file access, no network, no Keychain |
| Resource exhaustion (memory, CPU hang) | Whole-app memory pressure; a hang freezes an import with no kill switch | Helper can be killed on timeout and rlimit-capped; the app stays responsive |
| What the App Sandbox already does | Blocks arbitrary file reads and listening sockets; does **not** separate pdfium from the library data, keys or network | A helper that inherits only the app sandbox (option b) adds fault containment but not privilege separation; one with its own tighter sandbox (option a) adds both |

Realistic risk: pdfium is Chromium's PDF engine, heavily fuzzed, but it is the largest memory-unsafe parser in the product and a recurring CVE source. The local single-user threat model (the user picks the file) makes this Low severity, as recorded for F33.

## Options

| | (a) XPC service in the app bundle | (b) `posix_spawn`ed helper, pipe protocol | (c) Accept, with mitigations |
|---|---|---|---|
| Fault containment | Yes (launchd restarts the service) | Yes | No |
| Privilege separation | Strong: own sandbox profile, no network, no file access beyond bytes passed in | Weak: inherits the app sandbox (children of a sandboxed process inherit it and cannot widen it; narrowing needs `sandbox_init` inside the helper, a private-ish API) | None |
| Second signed binary | Yes: `Contents/XPCServices/GISTPdf.xpc`, same Team ID, own entitlements | Yes: a Mach-O in `Contents/MacOS` or `Helpers`, signed with the app's identity | No |
| Windows analogue | None (Apple-only API) | Same protocol works on Windows via `CreateProcess` plus pipes, and a restricted token or AppContainer | n/a |
| Build and packaging cost | New Xcode target, `Info.plist`, entitlements, an `embed` rule in `project.yml`, DMG and notarisation verified for nested bundles | One extra cargo binary plus a copy-and-sign step | None |
| Cross-platform shared code | Needs Swift glue on Apple only | The helper is the same Rust binary on every OS | None |

## Costs shared by (a) and (b)

**Signing, Hardened Runtime and N8.** Every nested executable and dylib must be signed with the app's Team ID, inside-out, before the app is signed; the current pipeline (`release-macos.yml`, `tools/build-dmg.sh`) signs the app's embedded `libpdfium.dylib` through Xcode's `codeSign: true` and has never been run with real credentials. A helper adds a second Mach-O to that chain. Library validation is satisfied while everything shares the Team ID, so `disable-library-validation` stays unnecessary (ADR-002 rule). N8's lesson applies: the helper must link the Rust core **statically** and load `libpdfium.dylib` by absolute path from a known location; never rely on `-l` search-path resolution. **None of this can be verified here (no signing identity), including notarisation of a nested bundle or helper.**

**Where pdfium lives.** For (a) the dylib moves into the service bundle (`GISTPdf.xpc/Contents/Frameworks/`) and is removed from the app, so the main process no longer maps pdfium at all, which is what makes the isolation real. For (b) it can stay in `Contents/Frameworks` and the helper loads it by path relative to its own executable. In both cases `project.yml` changes (not done here).

**IPC.** Input up to 256 MiB (`ParseLimits.max_bytes`) crosses once. A pipe copies it; passing a file descriptor (stdin redirected to the user-selected file, or an XPC `xpc_fd`/`NSFileHandle`) copies nothing and also keeps the bytes out of the app's memory. The reply is the whole `Document` as JSON (sections plus the full token stream); the spike measures its size. The reply is bounded by the F36 text budget (64 MiB of text), so it is bounded but not small. A leaner protocol (reply only sections and let the host re-tokenise) is possible but changes the core's shape.

**Memory.** During an isolated parse the bytes exist in the host (read from disk) and the helper; the Document exists in the helper and, after deserialisation, in the host. Peak host RSS is therefore roughly the Document's size, not pdfium's working set; peak helper RSS carries pdfium plus layout. Measured in the results document.

**Failure UX.** The host classifies helper termination into a typed error: signal, non-zero exit, protocol error, timeout. The Swift side would map one new `GistError` variant (for example `PdfParserCrashed`) to a calm alert, "this PDF could not be read safely", with nothing persisted for the failed import. No crash report for the app, no lost UI state. A watchdog (the spike uses 20 s) also contains hangs.

**Testing strategy.** A fault-injection mode in the helper (compiled only into test builds) triggers abort, null write and hang on marker bytes, and host tests assert a typed error and an intact process; real-pdfium fixture tests run through both paths and compare Documents byte for byte; the signed launch step goes onto the release checklist next to N8.

**Windows.** The core is shared. Option (b)'s protocol (stdin bytes in, framed reply out) is portable: on Windows the same helper runs under `CreateProcess` with a restricted token or AppContainer. Option (a) is Apple-only and would leave Windows on in-process pdfium unless (b) is built as well. If any isolation is built, the portable helper is the only design that serves both apps from one codebase; an XPC wrapper could later be layered on macOS if stronger sandboxing is wanted.

**What XPC adds over (b), not measured here.** A per-service sandbox profile that can be tighter than the app's (no network, no file access), launchd-managed lifecycle and automatic restart, entitlement-checked connections, and a first-party Swift API. It costs a bundle, an `Info.plist`, an Xcode target, and Objective-C or Swift glue between the Swift app and the Rust core. The spike did not build an XPC bundle: it needs a signed, launchd-registered bundle that cannot be exercised from an unsigned script, so XPC-specific latency, restart behaviour and sandbox enforcement are unmeasured.

## Recommendation

Pending measurements.
