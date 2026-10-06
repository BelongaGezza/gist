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

## Measured evidence (summary; `docs/pdf-isolation-spike-2026-10-05.md` has the tables)

All figures: synthetic and hostile fixtures plus one generated 400-page PDF only (no real documents exist, D6), one machine (macOS 27, Apple Silicon), unsigned, option (b) only.

- Fixed cost of a helper process: about 4-5 ms per parse (small fixtures: 15 ms in-process vs 20 ms isolated).
- 400-page text-heavy PDF: 294 ms in-process vs 372 ms (pipe) and 381 ms (fd); the extra time is mostly the 17.4 MB Document JSON round trip. Helper peak 58 MiB, host peak 39 MiB (host holds only the deserialised Document), versus 57 MiB in-process.
- Fault injection (abort, SIGSEGV, hang): the host survived all three, returned a typed outcome (`HelperCrashed{signal=6}`, `{signal=11}`, `{timeout}` after a 20 s watchdog) and parsed a normal file afterwards. This proves the mechanism, not a real pdfium bug.
- A deny-default `sandbox-exec` profile that allowed broad reads (but no writes or network) still let pdfium load and parse; a stricter read-restricted profile made the helper abort at startup and was not debugged. Per-service XPC sandboxing was not tested.
- Not measured: XPC, signed or notarised behaviour, Hardened Runtime validation of a second binary, inputs near 256 MiB, Documents near the 64 MiB text budget, real PDFs, macOS 14/15, Windows.

## Recommendation (for the user to decide; this ADR does not decide it)

**Do not build it for v1.0; accept the risk now (option c) with the mitigations already in force, and schedule a portable helper process (option b, hardened toward a) for a post-v1.0 milestone.**

Basis:

1. Severity is Low (F33): a local single-user app, the user chooses the file, the App Sandbox already limits what a compromised process reaches, and the F36 budget, the limits and the fuzzing are in place. The realistic failure today is "importing a hostile PDF closes the app", with library and reading progress safe in SQLite.
2. The spike shows fault containment is cheap (about 5 ms; about 30 % on a large text PDF) and gives a clean typed-error UX, so the cost argument against it is weak. What it cannot show is the part that matters for exploit containment: a helper that inherits the app sandbox gives crash containment but little privilege separation, and a tighter sandbox (XPC, or `sandbox_init` in the helper) was not demonstrated.
3. The risky unknown is signing: a second Mach-O or XPC bundle in the notarised DMG and the Hardened Runtime library-validation interplay with `libpdfium.dylib` (N8) cannot be verified without credentials. Adding it before the first real signed release (still gated on credentials) would add a second unverified piece to a pipeline that has never run signed.
4. If isolation is wanted later, build the portable stdin/stdout helper first (it serves macOS and Windows from one Rust binary, and the spike is a working starting point), keep the Document reply bounded (reply size scales with the text budget; consider returning sections only), and layer XPC sandboxing on macOS only if the signed experiment shows it is worth the bundle.

Alternatives for the user: **implement now** (option b, about the spike's size plus a typed `GistError` variant, a Swift alert, `project.yml`/release-pipeline changes and tests; justified only if a pdfium CVE in the wild changes the severity), or **implement later** as above (this recommendation's second half). Mitigations that apply to the accept choice and cost nothing: keep the pinned pdfium current (fetch script hash bump on each Chromium security release), keep `fuzz_parse_pdf` in the nightly run, and keep the release checklist's signed-launch check for N8.
