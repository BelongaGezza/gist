use gist_core::ParseError;

/// A single image page that has been pre-processed and is ready to hand to an
/// OCR engine.
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
/// 1. Decode (PNG or JPEG via the `image` crate).
/// 2. Convert to greyscale (luma8).
/// 3. Resize to fit within 2048×2048 if larger, preserving aspect ratio.
/// 4. Re-encode as PNG.
///
/// Returns `ParseError::ResourceLimitExceeded` if the decoded pixel count
/// would exceed `limits.max_expanded_bytes / 4` pixels (4 bytes per RGBA
/// pixel worst-case).
pub fn prepare_image(
    page_index: u32,
    raw_bytes: &[u8],
    limits: &gist_core::ParseLimits,
) -> Result<PreparedPage, ParseError> {
    let img = image::load_from_memory(raw_bytes)
        .map_err(|e| ParseError::InvalidInput(e.to_string()))?;

    let max_pixels = limits.max_expanded_bytes / 4;
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
    use gist_core::ParseLimits;
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
