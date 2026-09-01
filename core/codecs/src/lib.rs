//! Import/export codecs.
//!
//! Import produces immutable source pixels (the bytes a `RasterRef` points
//! at) plus working-space float pixels for rendering. Profiles are honored at
//! this edge once the ICC engine lands (Phase 3); until then everything is
//! treated as sRGB, which matches the naive pipeline in `chitrakar-color`.

use chitrakar_color::LinearRgba;
use image::ImageFormat;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("unsupported export format: {0}")]
    UnsupportedFormat(String),
}

/// Decoded source image: original dimensions and 8-bit sRGB RGBA bytes.
pub struct SourceImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

impl SourceImage {
    /// Convert to working-space pixels (linear float, premultiplied).
    pub fn to_working(&self) -> Vec<LinearRgba> {
        self.rgba8
            .chunks_exact(4)
            .map(|p| LinearRgba::from_srgb8(p[0], p[1], p[2], p[3]))
            .collect()
    }
}

/// Decode PNG or JPEG bytes (format sniffed from content).
pub fn decode(bytes: &[u8]) -> Result<SourceImage, CodecError> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    Ok(SourceImage {
        width: img.width(),
        height: img.height(),
        rgba8: img.into_raw(),
    })
}

/// Encode 8-bit sRGB RGBA pixels as PNG.
pub fn encode_png(width: u32, height: u32, rgba8: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        rgba8,
        width,
        height,
        image::ExtendedColorType::Rgba8,
        ImageFormat::Png,
    )?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_roundtrip_preserves_pixels() {
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255, /**/ 0, 255, 0, 255, //
            0, 0, 255, 255, /**/ 255, 255, 255, 128,
        ];
        let png = encode_png(2, 2, &pixels).unwrap();
        let decoded = decode(&png).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert_eq!(decoded.rgba8, pixels);
    }

    #[test]
    fn working_conversion_premultiplies() {
        let png = encode_png(1, 1, &[255, 255, 255, 128]).unwrap();
        let working = decode(&png).unwrap().to_working();
        let px = working[0];
        assert!((px.a - 128.0 / 255.0).abs() < 1e-3);
        assert!(px.r <= px.a, "premultiplied channel can't exceed alpha");
    }
}
