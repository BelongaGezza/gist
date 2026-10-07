# ADR 022 — Android FFI binding: UniFFI Kotlin bindings and NDK shared libraries

**Date:** 2026-10-07  
**Status:** Proposed  

## Context

GIST uses Mozilla's [UniFFI](https://github.com/mozilla/uniffi-rs) (v0.32 in proc-macro mode, ADR-001) for foreign function interface binding generation. Apple targets use the built-in Swift generator linked into an `.xcframework`. Windows required an external generator fork (`uniffi-bindgen-cs`, ADR-015) because C# is not supported in the upstream UniFFI core repository.

For Android, we need to determine the FFI bridging strategy:
1. Does upstream UniFFI support Kotlin? **Yes.** UniFFI 0.32 ships with native, first-class Kotlin support (`--language kotlin`) out of the box. The repository's own `crates/gist-ffi/uniffi-bindgen.rs` tool generates Kotlin bindings directly without third-party forks.
2. How does Kotlin load and call into Rust code? UniFFI-generated Kotlin uses Java Native Access (JNA) to bind to a native dynamic library (`libgist_ffi.so`).
3. How are Rust binaries compiled for Android architectures? Rust natively supports Android targets via the Android NDK and `cargo-ndk`.

## Decision

Use **upstream UniFFI Kotlin binding generation** over the unchanged `crates/gist-ffi` crate, coupled with Android NDK native shared libraries (`libgist_ffi.so`).

Key implementation details:
- **Binding Generation:** `tools/gen-bindings-kt.sh` runs `./target/debug/uniffi-bindgen generate <cdylib_path> --language kotlin --out-dir apps/android/core/src/main/java/uniffi/gist/ --no-format`. The generated Kotlin code is gitignored and produced as part of the Android build process.
- **NDK Target ABIs:**
  - `aarch64-linux-android` (`arm64-v8a`) — primary production target for modern Android devices.
  - `armv7-linux-androideabi` (`armeabi-v7a`) — 32-bit ARM fallback for older mobile devices.
  - `x86_64-linux-android` (`x86_64`) — 64-bit target for Android Studio emulators and ChromeOS tablets.
  - `i686-linux-android` (`x86`) — 32-bit emulator target (optional/legacy).
- **Tooling:** Use `cargo-ndk` to drive NDK compilation and cross-linker resolution with NDK r28+. Native shared objects (`libgist_ffi.so`) are placed into `apps/android/app/src/main/jniLibs/<abi>/`.
- **Runtime Dependency:** Include `net.java.dev.jna:jna` (with `@aar` packaging) in the Android Gradle dependencies to support UniFFI JNA bindings.
- **Panic Containment:** UniFFI wraps all exports in `ffi_catch!`, catching panics and returning `GistError.InternalPanic` to Kotlin as checked/typed `GistException.InternalPanic`. The process survives foreign exceptions.

## Consequences

- **Zero API Drift:** The exact same Rust crate (`gist-ffi`) and uniffi macros serve Apple (Swift), Windows (C#), and Android (Kotlin). Any new method, struct, or error variant added in Rust is instantly available across all platforms.
- **Callback Interfaces:** UniFFI Kotlin generates clean interfaces for host-implemented traits, allowing Android to implement `KeyProvider` (ADR-023) and `OcrEngine` (ADR-025) directly in Kotlin.
- **No Third-Party Binding Forks:** Unlike Windows (which required pinning an unmerged PR on `uniffi-bindgen-cs`), Android uses the verified, officially supported upstream UniFFI Kotlin generator already bundled in the repo's Cargo dependencies.
- **Binary Size:** Shipping `arm64-v8a`, `armeabi-v7a`, and `x86_64` `.so` files adds ~15–20 MB per architecture in uncompressed debug builds (~4–6 MB stripped release). Android App Bundles (AAB) split these dynamically so users only download the specific architecture matching their device.
