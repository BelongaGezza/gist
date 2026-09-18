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

> **Note (2026-09-18):** the interface actually shipped in
> `gist-core`/`gist-ffi` diverges from the example above — it's simpler:
> `fn recognize_page(&self, page_index: u32, image_bytes: Vec<u8>) -> Option<OcrPageResult>`
> with `OcrPageResult { page_index, text, confidence }` (no `OcrBlock`
> bounding-box list, no separate `is_cancelled()` — cancellation is signaled
> by returning `None`), and no `OcrError` type (recognition failure also
> returns `None`). This note doesn't change the decision above, just flags
> that the code sample has drifted from what shipped; the addendum below
> describes the interface as it actually exists today.

## Addendum: image-size policy at the OCR boundary (2026-09-18, closes `A7`)

Security register item `A7` flagged that this callback interface passes raw
image bytes across FFI (from user photos/scans) with no `ParseLimits`-style
size/dimension cap, unlike every other importer in the codebase. Investigating
this found the concrete gap already existed one layer down, in
`gist-imageprep::prepare_image` — see security review `N4` — and that finding
is what settles this item, not a separate decision made here.

**The policy:** `gist-imageprep::prepare_image` is the single choke point
between raw, untrusted image bytes and everything downstream. It:

1. Peeks the image header for declared width/height (`ImageReader::into_dimensions`,
   no pixel data touched) and rejects with `ParseError::ResourceLimitExceeded`
   if `width * height > limits.max_expanded_bytes / 4` — the same pixel-budget
   formula the OCR pipeline's doc comment (`gist_core::Core::import_image_with_ocr`)
   already named — **before** attempting a full decode (fixed by `N4`; previously
   this check ran only after `image::load_from_memory` had already fully
   decoded the image, which is the exact decode-before-check pattern `F15`/`F16`
   fixed for zip-based formats).
2. Only once that check passes: decodes, converts to greyscale, and resizes to
   fit within 2048×2048.
3. Re-encodes as PNG and returns that — never the original bytes.

**The invariant this interface must preserve:** `OcrEngine::recognize_page`'s
`image_bytes` parameter must always be `prepare_image`'s output — already
pre-processed, size-capped, ≤2048×2048, greyscale PNG — and never the raw
bytes a user's photo/scan arrived as. As long as that holds, the FFI callback
boundary itself doesn't need its own separate size cap: by the time bytes
reach Swift/Kotlin, they've already passed through the one choke point that
enforces the budget, and 2048×2048 greyscale PNG output has a small, predictable
worst-case size regardless of what the original file claimed.

This is currently unverified against a real call path, not because the
invariant doesn't hold today but because there isn't one yet:
`Core::import_image_with_ocr` (`crates/gist-core/src/lib.rs`) is still
`todo!("OCR import pipeline — Phase M3")`. Its doc comment already states the
intended pipeline order (pre-process with `prepare_image`, then call
`engine.recognize_page` per page) matching this addendum, but that's a stated
intent, not yet enforced by any code. **When M3 implements this function, the
implementation must call `prepare_image` on every page before ever passing
bytes to `recognize_page` — no code path may hand raw, un-preprocessed bytes to
the callback.** Treat this as an invariant to test for explicitly (e.g., a
`gist-core` test asserting `recognize_page` never observes bytes larger than
what a capped, resized PNG could produce, or asserting it's simply never
called with the original input bytes) once that implementation lands, not
just as a comment to trust.
