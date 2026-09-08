# ADR-009: OcrEngine callback interface

**Status:** Accepted
**Date:** 2026-09

## Decision

Expose OCR recognition to platform layers via a uniffi callback interface
(`#[uniffi::export(callback_interface)]`). Rust calls into the platform;
the platform implements the trait using its native OCR stack. This resolves
[A2] from the architecture register.

### Interface definition

```rust
// gist-ffi/src/lib.rs
#[uniffi::export(callback_interface)]
pub trait OcrEngine: Send + Sync {
    /// Recognise text in a pre-processed greyscale image.
    /// `image_data`: raw bytes of a grayscale PNG.
    /// Returns recognised text blocks with confidence scores.
    fn recognise(
        &self,
        image_data: Vec<u8>,
        page_index: u32,
    ) -> Result<OcrPageResult, OcrError>;

    /// Called before each page to check for cancellation.
    fn is_cancelled(&self) -> bool;
}

#[derive(uniffi::Record)]
pub struct OcrPageResult {
    pub blocks: Vec<OcrBlock>,
}

#[derive(uniffi::Record)]
pub struct OcrBlock {
    pub text: String,
    pub confidence: f32,   // 0.0–1.0
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, uniffi::Error)]
pub enum OcrError {
    RecognitionFailed(String),
    Cancelled,
}
```

### Calling convention

- Rust calls `recognise()` **once per page, sequentially**, from a `rayon`
  worker thread.
- Before each page Rust calls `is_cancelled()`; if `true`, it returns
  `OcrError::Cancelled` immediately without calling `recognise()`.
- `OcrError::RecognitionFailed` propagates as a user-visible import error:
  "OCR failed on page N".
- Future extension (v1.1): add `on_progress(page_index: u32, total_pages: u32)`
  to the trait for progress reporting without breaking the current binary
  interface (uniffi allows additive trait extension with a default no-op).

### Platform implementations

| Platform | Implementation |
|---|---|
| iOS / macOS | Swift class backed by `Vision.VNRecognizeTextRequest` |
| Windows | Rust struct backed by `Windows.Media.Ocr` (WinRT projection) implementing the same trait |
| Android (future) | Kotlin class backed by ML Kit `TextRecognition` |

## Reasoning

**Why callback interface, not Rust-native OCR?**

Native OCR libraries (tesseract-sys, leptess) add 10–40 MB to the binary,
require platform-specific build toolchains, and lag behind OS-vendor models in
accuracy. Apple Vision and Windows.Media.Ocr are on-device, free, and
maintained by the OS vendor. A callback interface lets each platform use its
best available engine while keeping the import orchestration in Rust.

**Why `Send + Sync`?**

uniffi callback objects cross the FFI boundary and are stored in Rust data
structures. The `rayon` worker that drives page processing may run on any
thread. Rust's type system requires `Send + Sync` to prove the callback is safe
to call from that thread. Swift implements the protocol on a class annotated
`@Sendable`; the Vision framework is thread-safe when called in this pattern.

**Why sequential-per-page rather than parallel?**

- Apple Vision serialises `VNRecognizeTextRequest` internally; spawning
  multiple concurrent requests on iOS does not improve throughput and increases
  peak memory.
- Sequential calls simplify cancellation: `is_cancelled()` is checked at a
  single well-defined point per page.
- Page order is deterministic, which matters for progress display.
- Parallelism can be re-introduced in a future ADR once profiling data exists.

**Windows support path**

The Windows implementation wraps `windows::Media::Ocr::OcrEngine` (the WinRT
type) in a Rust struct that implements the `OcrEngine` trait. The WinRT
projection (`windows` crate) exposes a blocking `RecognizeAsync` call that can
be driven from a `tokio::task::block_in_place` context inside the rayon worker.
No changes to the trait or the FFI layer are required.

## Consequences

- `gist-ffi` gains the `OcrEngine` trait, `OcrPageResult`, `OcrBlock`, and
  `OcrError` types. These are inert until `gist-imageprep` gains real pipeline
  logic.
- The Swift app target must provide a concrete `OcrEngine` implementation at
  link time; the compiler enforces this via the generated protocol.
- Adding `on_progress` in v1.1 requires a uniffi minor bump and a default
  no-op implementation on existing platform classes.
- Confidence scores (`f32` 0.0–1.0) are passed through from the platform engine
  as-is; GIST does not apply a rejection threshold at the FFI layer — callers
  may filter low-confidence blocks in the UI layer.
