//! Release-mode proof that `ffi_catch!` contains panics.
//!
//! Unit tests always run with unwinding, so they cannot detect a release
//! profile that sets `panic = "abort"` (which turns every panic into a process
//! kill and makes `catch_unwind` a no-op). Examples are ordinary binaries that
//! honour the active profile, so CI runs this one with `--release`:
//!
//!   cargo run --release -p gist-ffi --features test-panic --example panic_containment
//!
//! Exit 0 = panic contained as `InternalPanic`. Any other outcome (including the
//! abnormal termination produced by `panic = "abort"`) fails the CI step.

use gist_ffi::{test_support::ffi_panic_probe, GistError};

fn main() {
    match ffi_panic_probe() {
        Err(GistError::InternalPanic) => println!("ok: panic contained as InternalPanic"),
        other => {
            eprintln!("FAIL: expected Err(InternalPanic), got {other:?}");
            std::process::exit(1);
        }
    }
}
