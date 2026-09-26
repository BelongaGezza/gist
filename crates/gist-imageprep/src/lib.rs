use gist_model::ParseError;

/// A single image page that has been pre-processed and is ready to hand to an
/// OCR engine.
#[derive(Debug)]
pub struct PreparedPage {
    pub page_index: u32,
    /// PNG-encoded pre-processed image bytes, ready to send to OCR engine.
    pub png_bytes: Vec<u8>,
    /// Dimensions after pre-processing.
    pub width_px: u32,
    pub height_px: u32,
}

/// Pre-process a single raw image file for OCR.
///
/// Steps applied:
/// 1. Peek the header-declared dimensions and reject an oversized image
///    *before* decoding (N4 — see below).
/// 2. Decode (PNG or JPEG via the `image` crate).
/// 3. Convert to greyscale (luma8).
/// 4. Resize to fit within 2048×2048 if larger, preserving aspect ratio.
/// 5. Re-encode as PNG.
///
/// Returns `ParseError::ResourceLimitExceeded` if the header-declared pixel
/// count would exceed `limits.max_expanded_bytes / 4` pixels (4 bytes per
/// RGBA pixel worst-case).
///
/// **N4:** this check used to run on the *decoded* image, after
/// `image::load_from_memory` had already fully decoded it — meaning a
/// decompression bomb (a small file whose header declares enormous
/// dimensions, e.g. a "PNG bomb") would have its full pixel buffer allocated
/// before this function ever got a chance to reject it, the same
/// decode/decompress-before-check pattern `F15`/`F16` fixed for zip-based
/// formats. Fixed by reading only the header via `ImageReader::into_dimensions`
/// (no pixel data touched) and rejecting on that before `.decode()` ever
/// runs.
pub fn prepare_image(
    page_index: u32,
    raw_bytes: &[u8],
    limits: &gist_model::ParseLimits,
) -> Result<PreparedPage, ParseError> {
    let max_pixels = limits.max_expanded_bytes / 4;

    let reader = image::ImageReader::new(std::io::Cursor::new(raw_bytes))
        .with_guessed_format()
        .map_err(|e| ParseError::InvalidInput(e.to_string()))?;

    let (decl_width, decl_height) = reader
        .into_dimensions()
        .map_err(|e| ParseError::InvalidInput(e.to_string()))?;
    let declared_pixel_count = (decl_width as usize) * (decl_height as usize);
    if declared_pixel_count > max_pixels {
        return Err(ParseError::ResourceLimitExceeded);
    }

    // Dimensions passed the pre-decode check — safe to fully decode now.
    // `into_dimensions` above consumed the reader (it doesn't rewind), so a
    // fresh reader/cursor is built for the actual decode.
    let img =
        image::load_from_memory(raw_bytes).map_err(|e| ParseError::InvalidInput(e.to_string()))?;

    // Defence in depth: a format whose header dimensions could somehow
    // disagree with the decoded buffer would still be caught here, though
    // with the decode having already happened for that adversarial case.
    let pixel_count = (img.width() as usize) * (img.height() as usize);
    if pixel_count > max_pixels {
        return Err(ParseError::ResourceLimitExceeded);
    }

    let grey = img.to_luma8();

    // Resize if larger than 2048 on either side, preserving aspect ratio.
    let (w, h) = (grey.width(), grey.height());
    let resized = if w > 2048 || h > 2048 {
        let scale = 2048.0 / (w.max(h) as f32);
        let nw = (w as f32 * scale) as u32;
        let nh = (h as f32 * scale) as u32;
        image::imageops::resize(&grey, nw, nh, image::imageops::FilterType::Lanczos3)
    } else {
        grey
    };

    let width_px = resized.width();
    let height_px = resized.height();

    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::from(resized)
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| ParseError::InvalidInput(e.to_string()))?;

    Ok(PreparedPage {
        page_index,
        png_bytes,
        width_px,
        height_px,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gist_model::ParseLimits;
    use image::{ImageBuffer, Rgb};

    /// Encode a solid-colour RGB image as PNG bytes.
    fn make_rgb_png(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(width, height, |_, _| Rgb([r, g, b]));
        let mut bytes = Vec::new();
        image::DynamicImage::from(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("test helper: PNG encode failed");
        bytes
    }

    /// A 4×4 red PNG should come back as a valid PNG at the same dimensions.
    #[test]
    fn test_prepare_greyscale() {
        let png = make_rgb_png(4, 4, 255, 0, 0);
        let limits = ParseLimits::default();
        let page = prepare_image(0, &png, &limits).expect("prepare_image failed");

        assert_eq!(page.page_index, 0);
        assert_eq!(page.width_px, 4);
        assert_eq!(page.height_px, 4);
        // PNG magic bytes: \x89 P N G
        assert!(
            page.png_bytes.starts_with(b"\x89PNG"),
            "output must be PNG-encoded"
        );
    }

    /// A 4096×4096 image must be scaled down to at most 2048 on each side.
    #[test]
    fn test_resize_large() {
        let png = make_rgb_png(4096, 4096, 0, 128, 255);
        let limits = ParseLimits::default();
        let page = prepare_image(0, &png, &limits).expect("prepare_image failed");

        assert!(
            page.width_px <= 2048,
            "width {} must be ≤ 2048",
            page.width_px
        );
        assert!(
            page.height_px <= 2048,
            "height {} must be ≤ 2048",
            page.height_px
        );
    }

    /// Hand-builds a minimal PNG byte stream: signature + IHDR (declaring
    /// `width`x`height`) + a tiny placeholder IDAT + IEND. The IDAT chunk is
    /// syntactically present (satisfying `png::Decoder::read_info`, which
    /// `ImageReader::into_dimensions` uses and requires at least one IDAT to
    /// exist) but far too small to actually contain `width * height` pixels
    /// of real image data — this is exactly the shape of a real "PNG bomb":
    /// a tiny file whose header claims enormous dimensions, relying on the
    /// gap between "header says X" and "a full decode of X is attempted".
    ///
    /// If `prepare_image` ever regresses to calling `image::load_from_memory`
    /// (a full decode) before checking declared dimensions, decoding this
    /// file would fail with a generic `ParseError::InvalidInput` (corrupt/
    /// truncated pixel stream) instead of the size-limit rejection this test
    /// expects — so asserting specifically on `ResourceLimitExceeded` (not
    /// just "is an error") is what proves the pre-decode header check ran
    /// first, and that no attempt was made to inflate/decode pixel data for
    /// declared dimensions this large (N4).
    fn build_headers_only_png(width: u32, height: u32) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc: u32 = 0xFFFF_FFFF;
            for &b in bytes {
                crc ^= b as u32;
                for _ in 0..8 {
                    let mask = (crc & 1).wrapping_neg();
                    crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
                }
            }
            !crc
        }

        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let mut crc_input = Vec::new();
            crc_input.extend_from_slice(kind);
            crc_input.extend_from_slice(data);
            out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
            out
        }

        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n"); // PNG signature

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.push(8); // bit depth
        ihdr.push(2); // color type: truecolor
        ihdr.push(0); // compression method
        ihdr.push(0); // filter method
        ihdr.push(0); // interlace method
        png.extend_from_slice(&chunk(b"IHDR", &ihdr));

        // `png::Decoder::read_info` (which `ImageReader::into_dimensions`
        // uses under the hood) requires at least one IDAT chunk to be
        // *present* before it will return header info, but does not inflate
        // its contents at that point — decompression only happens once
        // actual pixel data is requested (a later, separate `.decode()`
        // call). A syntactically valid but tiny zlib stream is therefore
        // enough to satisfy header parsing without ever producing (or
        // requiring) the billions of pixel bytes the declared dimensions
        // would imply — real "PNG bomb" files rely on exactly this gap
        // between "header says X" and "decoding X is attempted".
        png.extend_from_slice(&chunk(
            b"IDAT",
            &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
        ));
        png.extend_from_slice(&chunk(b"IEND", &[]));
        png
    }

    /// N4 regression test: a file whose PNG header declares dimensions that
    /// exceed the pixel-count limit must be rejected *before* a full decode
    /// is attempted — proven by using a file with no IDAT at all, which
    /// would only ever produce `ResourceLimitExceeded` (not a decode error)
    /// if the size check runs first, header-only, as it now does.
    #[test]
    fn test_declared_dimensions_rejected_before_full_decode() {
        let limits = ParseLimits {
            max_expanded_bytes: 64, // max_pixels = 64 / 4 = 16
            ..ParseLimits::default()
        };
        // 10000x10000 declared in the header, but no actual pixel data at
        // all — a full decode of this file must fail if ever attempted.
        let png = build_headers_only_png(10_000, 10_000);
        let result = prepare_image(0, &png, &limits);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded)),
            "expected ResourceLimitExceeded from the header-only dimension \
             check, got {:?} (a generic InvalidInput here would mean the \
             code tried to fully decode this IDAT-less file instead of \
             rejecting it by header alone)",
            result
        );
    }

    /// A well-formed, fully decodable image whose declared header dimensions
    /// are within the limit must still round-trip correctly through the new
    /// pre-decode check — i.e. the check doesn't just reject everything.
    #[test]
    fn test_declared_dimensions_within_limit_still_decodes() {
        let limits = ParseLimits::default();
        let png = make_rgb_png(8, 8, 10, 20, 30);
        let page = prepare_image(0, &png, &limits).expect("prepare_image failed");
        assert_eq!(page.width_px, 8);
        assert_eq!(page.height_px, 8);
    }

    /// An image whose raw pixel count exceeds the computed limit must be
    /// rejected with `ParseError::ResourceLimitExceeded`.
    ///
    /// Strategy: use a tiny `max_expanded_bytes` so even a small image
    /// (5×5 = 25 pixels) exceeds `max_pixels = 64 / 4 = 16`.
    #[test]
    fn test_pixel_limit() {
        let limits = ParseLimits {
            max_expanded_bytes: 64, // max_pixels = 64 / 4 = 16
            ..ParseLimits::default()
        };
        // 5×5 = 25 pixels > 16 → must fail
        let png = make_rgb_png(5, 5, 0, 0, 0);
        let result = prepare_image(0, &png, &limits);
        assert!(
            matches!(result, Err(ParseError::ResourceLimitExceeded)),
            "expected ResourceLimitExceeded, got {:?}",
            result
        );
    }
}
