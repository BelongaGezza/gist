# ADR 025 — Android OCR engine: Bundled on-device Google ML Kit Text Recognition

**Date:** 2026-10-07  
**Status:** Proposed  

## Context

GIST supports importing scanned paper documents and image-only PDFs via on-device optical character recognition (OCR) (spec §3.5, ADR-009). The image pre-processing (deskewing, contrast, thresholding) is handled in Rust by `gist-imageprep`, while text recognition is delegated to the host platform via the UniFFI callback interface:
```rust
#[uniffi::export(callback_interface)]
pub trait OcrEngine: Send + Sync {
    fn recognize_page(&self, page_index: u32, image_bytes: Vec<u8>) -> Option<OcrPageResult>;
}
```
On Apple, this is backed by Apple's `Vision` framework. On Windows, it is backed by `Windows.Media.Ocr` (ADR-020).

For Android, we evaluated options for implementing `OcrEngine`:
1. **Google ML Kit Text Recognition (Bundled on-device artifact `com.google.mlkit:text-recognition`):**
   - Bundles the model binaries directly inside the APK/AAB.
   - Runs 100% on-device, fully offline, with zero network calls and no Google Play Services dependency.
   - Provides per-element and per-block text bounding boxes and confidence scores.
2. **Google ML Kit via Google Play Services (`com.google.android.gms:play-services-mlkit-text-recognition`):**
   - Smaller APK download size (~few MBs), but requires Google Play Services to be installed and dynamically downloads the OCR model over the network on first use.
   - Fails on de-Googled Android devices (GrapheneOS, CalyxOS, LineageOS, AOSP tablets, Huawei) and triggers network calls on first use, which conflicts with GIST's offline/privacy model.
3. **Bundled Tesseract / Tesseract4Android:**
   - Standalone open source, but requires shipping large `.traineddata` files (~20–40 MB per language) and native C++ shared libraries (`liblept.so`, `libtesseract.so`), increasing compilation and maintenance complexity.

## Decision

Adopt **bundled on-device Google ML Kit Text Recognition (`com.google.mlkit:text-recognition`)** as the primary OCR engine on Android.

Key design details:
- **Zero Network Guarantee:** The bundled model variant does not require Google Play Services and does not initiate background downloads. All inference occurs on the device's CPU/NNAPI/GPU.
- **Engine Adapter:** `AndroidMlKitOcrEngine` implements the generated `OcrEngine` callback interface.
  - Rust passes pre-processed PNG image bytes.
  - Kotlin decodes the bytes via `BitmapFactory.decodeByteArray()` into an `InputImage`.
  - Runs `TextRecognizer.process(inputImage)` synchronously via `Tasks.await()` on the calling background thread (safe because UniFFI invokes callbacks from dedicated background worker threads).
  - Extracts text and computes page-level average confidence score from text blocks (`Text.TextBlock.lines`).
  - Returns `OcrPageResult(page_index, extracted_text, confidence)`.
- **Camera Scanning Flow:** Multi-page camera capture is implemented using Android Jetpack CameraX (`androidx.camera.view:camera-view`), allowing users to photograph multiple pages sequentially, review confidence highlights, edit misrecognized text, and commit the batch into the library as a single document.

## Consequences

- Full privacy compliance (`docs/PRIVACY.md` §6): No image data ever leaves the device.
- Runs identically on Google Play devices and de-Googled AOSP forks (e.g. GrapheneOS, CalyxOS, F-Droid distributions).
- Adds ~10–15 MB to the APK for the bundled Latin script model weights (mitigated by AAB dynamic delivery).
- Confidence scores are natively supported, allowing the Android UI to highlight low-confidence words during document review (parity with macOS/iOS).
