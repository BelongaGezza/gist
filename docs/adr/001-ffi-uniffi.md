# ADR-001: FFI — uniffi proc-macro mode

**Status:** Accepted
**Date:** 2026-09

## Decision

Use [uniffi](https://github.com/mozilla/uniffi-rs) 0.32 in **proc-macro mode**
(attribute macros on Rust types) rather than a UDL file or hand-rolled C ABI.

## Reasoning

- UDL is a separate file that duplicates every type signature and drifts from
  Rust types
- Hand-rolled C ABI + Swift wrappers is weeks of ceremony and a permanent
  maintenance tax
- Proc-macro mode annotates the source directly; the compiler enforces
  correctness
- uniffi generates Swift bindings as an Xcode build phase (gitignored);
  contributors never edit them

## Consequences

- `gist-ffi` builds `staticlib` (linked into `.xcframework`); avoids a second
  signed dylib
- Windows C ABI (`extern "C"` shim) is a thin layer over core types, designed
  now, built at Windows kickoff
- All FFI objects must be `Send + Sync`; interior mutability behind
  `Mutex`/`RwLock`
