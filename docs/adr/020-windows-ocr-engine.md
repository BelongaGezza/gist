# ADR 020 — Windows OCR engine: `Windows.Media.Ocr` behind ADR-009's `OcrEngine` callback (closes Q4, design only)

**Date:** 2026-10-04
**Status:** Proposed (design note, W5 role R3). No code is written or changed by this ADR. It becomes Accepted when the implementation lands and is verified on Windows.

> **Correction, 2026-10-04 (W5 R5, F68):** this ADR's description of the `OcrEngine` callback contract is wrong. The shipped interface is `recognize_page(page_index, image_bytes) -> Option<OcrPageResult>` (`None` = cancel) with `OcrPageResult { page_index, text, confidence }`: no blocks, no bounding boxes, no `OcrError`, no engine-side `is_cancelled`. ADR-009's older text has the same stale shape, which this ADR inherited. Items 3 and 4 below cannot be implemented as written and must be re-derived from `crates/gist-ffi` before the ADR is Accepted. The engine choice itself (Windows.Media.Ocr) is unaffected.

## Context

Q4 asks which OCR engine the Windows shell uses: the OS-provided `Windows.Media.Ocr` or a bundled Tesseract.

**Correction of stale statements.** `docs/windows-development-plan.md` and `docs/windows-ui-spec.md` §11 said `import_image_with_ocr` "is `todo!()`". That has been false since M3: `Core::import_image_with_ocr` / `GistCore::import_image_with_ocr` are implemented in Rust (role R2b, 2026-09-26), closing `A7`, with the per-page byte cap and the page-count cap (`N9`) enforced before any engine call (ADR-009 addenda 1 and 2, ADR-006 §4). The Apple shell already drives it with a Vision-backed `OcrEngine` and a review screen. What does **not** exist is a Windows `OcrEngine` implementation and a Windows OCR-import UI. This ADR is only about those two.

The contract the engine must satisfy (ADR-009, unchanged): a uniffi callback interface `OcrEngine { recognise(image_data: Vec<u8>, page_index: u32) -> Result<OcrPageResult, OcrError>; is_cancelled() -> bool }`, called once per page, sequentially, from a Rust worker thread; input is a pre-processed greyscale PNG; output is blocks with `text`, `confidence` 0.0-1.0 and a bounding box; no rejection threshold at the FFI layer.

## Decision

Use **`Windows.Media.Ocr.OcrEngine`** as the Windows engine, implemented **in the C# shell** as a class implementing the generated `OcrEngine` callback interface (the same shape as the Swift/Vision implementation on Apple). Do not bundle Tesseract in v1. Revisit only if a measured accuracy or language-coverage gap makes Windows.Media.Ocr unacceptable, and then as an additive second engine (see "Fallback").

### Correction to ADR-009's "Windows support path"

ADR-009 says Windows wraps the WinRT type "in a Rust struct" via the `windows` crate and `tokio::task::block_in_place`. That is superseded for the shipped design: the Windows shell is C# on .NET 10 (ADR-015, ADR-018), the callback interface is already generated for C#, and `Windows.Media.Ocr` is directly projected there. A Rust-side `windows`-crate wrapper would add a WinRT dependency to `gist-ffi` (which also builds for macOS/Linux CI) for no benefit, and `tokio` is not in the dependency tree (ADR-010). ADR-009's trait and calling convention are unaffected; only its Windows table row changes. When this ADR is accepted, ADR-009's table should gain a pointer here.

## Comparison

| Concern | `Windows.Media.Ocr` | Tesseract |
|---|---|---|
| Licence | OS component, nothing redistributed, nothing to add to `docs/THIRD-PARTY.md` | Apache-2.0 (verified from the project README); depends on Leptonica (BSD-2-Clause per the README). Native libs would need notices. |
| Size / packaging | 0 bytes shipped | Engine and Leptonica DLLs plus `.traineddata` per language (sizes not verified; ADR-009's own "10-40 MB" estimate is unverified). The native DLL must be signed with the app and fit the MSIX/sandbox model (ADR-017), with the `N8` load-path discipline |
| Languages | Only OCR language packs installed on the machine. `AvailableRecognizerLanguages` lists them; a pack must be installed (Windows Settings, or the `Language.OCR` Features-on-Demand capability). Power Automate docs list 25 languages (Ukrainian is not among them). `TryCreateFromLanguage` returns `null` if nothing resolves | 100+ languages "out of the box" per the README, via data files we would ship or download |
| Offline / on-device (`docs/PRIVACY.md` §6) | On-device (Power Automate docs: works locally without the cloud). Pack installation is an OS action, not a GIST network path | On-device; language data shipped with the app |
| Confidence | `OcrResult` gives lines and words with text and position only; **no confidence value** (OcrEngine remarks). We would report a fixed placeholder | Per-word confidence available |
| ARM64 | Part of Windows; no per-architecture packaging on our side (ARM64 not tested here) | Needs ARM64 builds of Tesseract, Leptonica and codecs; not verified that they exist |
| Quality | Unmeasured on GIST's scans | Unmeasured here |
| Image limit | Static `OcrEngine.MaxImageDimension` (numeric value not verified; read at runtime) | Memory-bound |
| Fits ADR-009's rationale (OS-vendor engine, free, on-device) | Yes | No (binary growth, toolchain) |

Decisive factors: zero packaging/licence/size cost, no extra native binary to sign or sandbox, parity with the Apple design (platform engine behind the same callback), and the privacy statement stays "OS component, no GIST network path". Accepted costs: language coverage depends on installed packs, and no real confidence scores.

## How it plugs into the existing callback (design)

1. `WindowsOcrEngine : OcrEngine` in the Windows project, constructed per import with a language and a cancellation flag backing `is_cancelled()`.
2. `recognise(bytes, page)` runs on the Rust worker thread, never the UI thread: decode the PNG with `BitmapDecoder` into a `SoftwareBitmap`, check width/height against `OcrEngine.MaxImageDimension` (downscale or return `RecognitionFailed`, never crash), call `RecognizeAsync(...)` and wait synchronously (safe off the UI thread). Dispose bitmap and stream in `finally`.
3. Map `OcrResult.Lines` to `OcrBlock`s (one block per line, box = union of its words' bounding rects, `confidence` = a documented constant). Consequence: the review screen's low-confidence highlighting is inactive on Windows; state that in the UI spec when implemented.
4. Errors: any WinRT exception becomes `OcrError.RecognitionFailed` with a plain message ("OCR failed on page N"), never raw exception text. An exception escaping the callback would surface as an opaque `InternalPanic` (the same reason ADR-016 resolves key errors eagerly).
5. Language: `TryCreateFromUserProfileLanguages()` first, else a chosen language via `TryCreateFromLanguage`. If `null`, show "install an OCR language pack in Windows Settings" before the import starts, not a failure after N pages.
6. ParseLimits-style caps are not reimplemented: Rust already enforces `max_bytes` per page and `max_pages` before calling the engine. C# adds only the engine's own `MaxImageDimension` bound.

## What remains for implementation (not done here)

- `WindowsOcrEngine` plus unit tests; a real-bitmap smoke test on a Windows runner that skips (not fails) when no OCR pack is present (packs on `windows-latest` are unverified).
- Windows import UI: image picker, review screen equivalent, cancel. Image-only PDFs need a Windows page renderer (Apple uses PDFKit); that is a separate decision.
- Decide the placeholder confidence and review behaviour; update `docs/windows-ui-spec.md` §11 and `docs/PRIVACY.md` §6 (name Windows.Media.Ocr beside Vision).
- Verify on hardware: ARM64, packaged (MSIX) run, no-pack behaviour, quality on representative scans (and against Tesseract on the same inputs if quality is in doubt).
- Confirm the MSIX manifest needs no extra capability for `Windows.Media.Ocr` (unverified).

## Fallback

If measured quality or language gaps block v1: add Tesseract as a second `OcrEngine` implementation selectable in Settings, with notices, signed DLLs and a pinned, hash-verified traineddata set (the `tools/fetch-pdfium.sh` fail-closed pattern). No Rust trait change is needed either way.

## Sources

- Microsoft Learn, `Windows.Media.Ocr.OcrEngine` class: `RecognizeAsync`, `OcrResult` lines/words with text and position, introduced in Windows 10 10.0.10240.0.
- Microsoft Learn, `OcrEngine.AvailableRecognizerLanguages` ("a language pack must be installed"), `OcrEngine.TryCreateFromLanguage` (null if unresolved), `OcrEngine.MaxImageDimension`.
- Microsoft Learn, PowerToys Text Extractor (OCR packs via `Language.OCR*` capabilities), Language and region Features on Demand, Power Automate OCR actions (25 languages, local).
- Tesseract README (github.com/tesseract-ocr/tesseract): Apache-2.0, Leptonica dependency, 100+ languages via traineddata.

## Unverified

Windows.Media.Ocr accuracy on GIST scans; the numeric `MaxImageDimension`; ARM64 behaviour of either engine; Tesseract binary and traineddata sizes and Leptonica's exact licence text; existence of upstream ARM64 Tesseract builds; MSIX capability needs; OCR packs on CI images; whether a placeholder confidence is acceptable to the review UX.
