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

Filled in below.

## Recommendation

Pending measurements.
