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

## Addendum 2: byte-size cap at the `import_image_with_ocr` FFI entry point (2026-09-26, closes `[A7]`)

The addendum above settled the *outbound* leg of this interface —
`OcrEngine::recognize_page`'s `image_bytes` parameter, where Rust calls out to
the platform with bytes that are already pre-processed, capped, and small by
construction. It did not define a cap for the *inbound* leg: the raw,
wholly-untrusted bytes of the user's photo/scan file, at the point they first
cross the FFI boundary into `gist-core` — before `gist-imageprep::prepare_image`
(the previous addendum's choke point) ever gets a chance to run. That gap is
what this addendum closes, completing `[A7]` as a documentation gate.

**Why this is a distinct choke point from `prepare_image`'s existing check:**
`prepare_image` (fixed by `N4`) peeks the *decoded image header's declared
pixel dimensions* on a `&[u8]` slice and rejects before a full pixel decode —
but by the time `prepare_image` sees that slice, something has already read
the entire file into a Rust-owned buffer. `prepare_image` has no way to reject
a file before its bytes exist in memory; bounding the raw byte count *before*
that read happens is a separate, earlier responsibility — exactly the one
`import_txt` and `import_file` already discharge for every other import path
today (`std::fs::metadata(path)?.len()` checked against `limits.max_bytes`
*before* `std::fs::read(path)`, in `crates/gist-core/src/lib.rs`).
`import_image_with_ocr` is the one importer-adjacent entry point that doesn't
yet do this, because it is still `todo!()`.

**The policy:**

1. **Constant:** reuse `gist_model::ParseLimits::max_bytes`. Do **not**
   introduce a second, OCR-specific byte-size constant. Every other importer
   already treats `max_bytes` as "maximum size in bytes of the raw file this
   import call will read off disk," and a user's photo/scan file is exactly
   that — a raw file read off disk. A separate `MAX_OCR_IMAGE_BYTES` constant
   would duplicate this policy for no behavioral benefit and be one more
   number to keep in sync by hand. Default value, unchanged: **256 MiB**
   (`268_435_456` bytes, `ParseLimits::default().max_bytes`) — generous
   relative to a typical phone photo or flatbed scan (single-digit-to-low-tens
   of MB), but this is a resource-exhaustion backstop, not a quality-of-life
   limit, so it has no reason to be tighter than the value every other
   importer already enforces.

2. **Enforcement point:** when `Core::import_image_with_ocr` is implemented
   (M3), its first lines of real logic — before calling
   `gist_imageprep::prepare_image`, before any OCR-related allocation, before
   the observer/cancellation check, before anything else — must be:

   ```rust
   let limits = ParseLimits::default();
   let declared_len = std::fs::metadata(path)?.len() as usize;
   if declared_len > limits.max_bytes {
       return Err(ImportError::ResourceLimitExceeded {
           limit: format!("max_bytes={}", limits.max_bytes),
           attempted: declared_len,
       });
   }
   let bytes = std::fs::read(path)?;
   ```

   copied verbatim from `import_file`'s/`import_txt`'s existing pattern
   (`crates/gist-core/src/lib.rs`). This is a hard ordering requirement,
   matching this codebase's non-negotiable "check `ParseLimits` before
   allocation" rule (`CLAUDE.md`, "Security policies — must not be relaxed").

3. **The declared-dimension cap stays in `prepare_image` — not duplicated
   here.** `prepare_image`'s existing pre-decode pixel-count check
   (`ImageReader::into_dimensions()` vs. `limits.max_expanded_bytes / 4`,
   fixed by `N4`) runs on the bytes this new check admits, and it bounds a
   different resource (decoded pixel count / decode-time allocation) than
   this check bounds (raw file byte count / disk-read allocation).
   Duplicating a dimension check at the FFI entry point — before the bytes
   are even decoded — would require parsing the image header at two separate
   call sites for no additional protection, since `prepare_image` is
   guaranteed to run immediately afterward by the pipeline-ordering invariant
   the addendum above already establishes. One check per resource axis, at
   the earliest point that axis is actually measurable: byte count is
   measurable via `fs::metadata` before any read at all; pixel count is only
   measurable once enough of the file has been header-parsed, which is
   `prepare_image`'s job, not this entry point's.

4. **TOCTOU note (informational, matches `F13`):** like `import_txt`/
   `import_file`, the `fs::metadata` check and the subsequent `fs::read` are
   not atomic — a file swapped on disk between the two calls could in
   principle bypass this gate. Accepted for the same reason `F13` accepts it
   elsewhere in this codebase: a local single-user app where the user selects
   the file via an OS picker. No new decision is needed here; this note exists
   only so a future reviewer doesn't mistake it for a gap newly introduced by
   this addendum.

5. **Test to add when M3 implements this function:** a `gist-core` test
   asserting `import_image_with_ocr` returns
   `ImportError::ResourceLimitExceeded` (not a panic, not a generic IO error,
   not a silently-truncated read) for a file whose on-disk size exceeds
   `ParseLimits::default().max_bytes`, mirroring however `import_txt`'s/
   `import_file`'s own `max_bytes`-rejection tests are structured (check
   `crates/gist-core/src/lib.rs`'s existing tests for the pattern) — without
   needing to actually materialize a real 256 MiB image fixture.

With this, `[A7]` is closed as a documentation gate: the OCR pipeline
implementation role (`R2b`) may proceed against the constant, enforcement
point, and division of responsibility named above without further judgment
calls.
