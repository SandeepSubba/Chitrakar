//! Import/export codecs.
//!
//! Import produces immutable source pixels (the bytes a `RasterRef` points
//! at) plus working-space float pixels for rendering. Profiles are honored at
//! this edge once the ICC engine lands (Phase 3); until then everything is
//! treated as sRGB, which matches the naive pipeline in `chitrakar-color`.

pub mod container;
pub mod pdf;
pub mod svg;
pub mod tiff_export;

pub use container::{
    load_chitra, load_chitra_with_fonts, save_chitra, save_chitra_with_fonts, ContainerError,
    FontFile, Opened,
};
pub use pdf::{export_pdf, export_pdf_document, PdfError};
pub use svg::export_svg;
pub use tiff_export::{export_cmyk_tiff, TiffError};

use chitrakar_color::LinearRgba;
use image::ImageFormat;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| LinearRgba::from_srgb8(p[0], p[1], p[2], p[3]))
            .collect()
    }
}

/// Decode PNG or JPEG bytes (format sniffed from content). An embedded ICC
/// profile is honored: pixels are normalized to sRGB at this edge, so the
/// engine holds exactly one internal encoding.
pub fn decode(bytes: &[u8]) -> Result<SourceImage, CodecError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let icc = image::ImageDecoder::icc_profile(&mut decoder)
        .ok()
        .flatten();
    let img = image::DynamicImage::from_decoder(decoder)?.to_rgba8();
    let (width, height) = (img.width(), img.height());
    let mut rgba8 = img.into_raw();
    if let Some(icc) = icc {
        // Best effort: an unparseable or non-RGB profile leaves pixels as-is.
        chitrakar_color::cms::normalize_rgba8_to_srgb(&icc, &mut rgba8);
    }
    Ok(SourceImage {
        width,
        height,
        rgba8,
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

/// Encode as JPEG, compositing over white first.
///
/// JPEG has no alpha channel, so transparency has to become something:
/// white, the same choice the print exports make for unprinted paper. The
/// composite is over *linear* values, before sRGB encoding, because that is
/// where "half covered" actually means half — blending the encoded bytes
/// instead would darken every antialiased edge.
pub fn encode_jpeg(
    width: u32,
    height: u32,
    pixels: &[LinearRgba],
    quality: u8,
) -> Result<Vec<u8>, CodecError> {
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for px in pixels {
        let over_white = |v: f32| (v + (1.0 - px.a)).clamp(0.0, 1.0);
        rgb.push((chitrakar_color::linear_to_srgb(over_white(px.r)) * 255.0).round() as u8);
        rgb.push((chitrakar_color::linear_to_srgb(over_white(px.g)) * 255.0).round() as u8);
        rgb.push((chitrakar_color::linear_to_srgb(over_white(px.b)) * 255.0).round() as u8);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100)).encode(
        &rgb,
        width,
        height,
        image::ExtendedColorType::Rgb8,
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
    fn embedded_icc_profile_is_honored_on_import() {
        use image::ImageEncoder;
        // Encode a PNG tagged as Display P3.
        let pixels = vec![200u8, 100, 50, 255];
        let mut png_p3 = std::io::Cursor::new(Vec::new());
        let mut enc = image::codecs::png::PngEncoder::new(&mut png_p3);
        enc.set_icc_profile(chitrakar_color::cms::display_p3_profile_bytes())
            .unwrap();
        enc.write_image(&pixels, 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();

        // The same pixel untagged decodes verbatim; tagged, it converts.
        let plain = decode(&encode_png(1, 1, &pixels).unwrap()).unwrap();
        assert_eq!(plain.rgba8, pixels);
        let tagged = decode(&png_p3.into_inner()).unwrap();
        assert_ne!(tagged.rgba8, pixels, "P3-tagged pixels must be normalized");
        assert_eq!(tagged.rgba8[3], 255, "alpha preserved");
    }

    #[test]
    fn jpeg_flattens_transparency_onto_white() {
        // Two pixels: opaque red, and a half-covered red. JPEG has no alpha,
        // so the second must land halfway to white rather than being
        // dropped or coming out fully red.
        let half = LinearRgba {
            r: 0.5,
            g: 0.0,
            b: 0.0,
            a: 0.5,
        };
        let opaque = LinearRgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let jpeg = encode_jpeg(2, 1, &[opaque, half], 92).unwrap();
        assert_eq!(&jpeg[0..2], &[0xff, 0xd8], "JPEG SOI marker");

        let decoded = decode(&jpeg).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        let px = |i: usize| &decoded.rgba8[i * 4..i * 4 + 3];
        assert!(
            px(0)[0] > 240 && px(0)[1] < 30,
            "opaque red survives: {:?}",
            px(0)
        );
        // Half coverage over white: red stays high, the other channels lift
        // toward white rather than staying at zero.
        assert!(
            px(1)[1] > 150 && px(1)[2] > 150,
            "half-covered pixel blends toward white: {:?}",
            px(1)
        );
        assert!(px(1)[0] > 200, "and keeps its red: {:?}", px(1));
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
