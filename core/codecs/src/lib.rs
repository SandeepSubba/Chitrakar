//! Import/export codecs.
//!
//! Import produces immutable source pixels (the bytes a `RasterRef` points
//! at) plus working-space float pixels for rendering. Profiles are honored at
//! this edge once the ICC engine lands (Phase 3); until then everything is
//! treated as sRGB, which matches the naive pipeline in `chitrakar-color`.

pub mod container;
pub mod pdf;
mod strokes;
mod subset;
pub mod svg;
pub mod svg_import;
pub mod tiff_export;

pub use container::{
    load_chitra, load_chitra_with_fonts, save_chitra, save_chitra_with_fonts, ContainerError,
    FontFile, Opened,
};
pub use pdf::{export_pdf, export_pdf_document, export_pdf_frames, PdfError};
pub use svg::export_svg;
pub use svg_import::{import_svg, ImportedSvg};
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
    #[error("failed to encode image: {0}")]
    Encode(String),
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

/// Encode 8-bit sRGB RGBA pixels as lossless WebP.
///
/// The same pixels a PNG holds, transparency and all, usually in less:
/// what a page wants for a picture on the web that has to stay exact. A
/// lossy WebP is not offered — the encoder has only the lossless half,
/// and a JPEG is already the lossy picture.
///
/// Only the encoder is in the engine. Reading a WebP is the browser's
/// work (the app hands the engine a PNG of it): the decoder is six
/// times the encoder's weight in the WebAssembly, for something every
/// webview already does.
pub fn encode_webp(width: u32, height: u32, rgba8: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(rgba8, width, height, image_webp::ColorType::Rgba8)
        .map_err(|e| CodecError::Encode(e.to_string()))?;
    Ok(out)
}

/// The two pictures that keep transparency, for the exports that can go
/// either way: everything about a region, a slice or a scale is the same
/// up to the last step, and this is the last step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raster {
    Png,
    Webp,
}

impl Raster {
    /// Encode 8-bit sRGB RGBA pixels as this kind of file.
    pub fn encode(self, width: u32, height: u32, rgba8: &[u8]) -> Result<Vec<u8>, CodecError> {
        match self {
            Raster::Png => encode_png(width, height, rgba8),
            Raster::Webp => encode_webp(width, height, rgba8),
        }
    }
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

/// A random page made opaque (`opaque_page`) with its blends put
/// back, but for four kinds left out, each for a reason. The four
/// modes that move a colour as a whole — hue, saturation, colour,
/// luminosity — where resvg's arithmetic is not the W3C's: worked by
/// hand from the spec's SetLum and ClipColor, a blue brush at
/// Luminosity over a gold page is [84, 60, 0], the engine's answer,
/// and resvg draws [115, 60, 0]. Frames and copies holding what works
/// on the page under them — a clone, an adjustment — whose blend the
/// engine brings down only on what they paint (`render_child`), which
/// SVG has no way to say. And layers inside a frame: the engine draws
/// a plain frame where it stands, so a blend inside it reaches the
/// page; SVG says that now by putting the frame's cut on each layer
/// (`svg::passes_through`) and PDF by cutting rather than grouping, so
/// this one is a switch too, kept for frames a reader still isolates.
/// The four modes are a switch since what resvg gets wrong is its own.
#[cfg(test)]
pub(crate) fn blended_page(
    seed: u64,
    all_but_four: bool,
    not_in_frames: bool,
) -> chitrakar_doc::Document {
    let mut doc = chitrakar_doc::fixture::opaque_page(seed);
    let page = chitrakar_doc::fixture::page(seed);
    for (id, n) in page.nodes() {
        let in_place = n.kind.holds_children()
            && chitrakar_render::works_on_what_is_under(&doc, *id).unwrap_or(false);
        let separable = !matches!(
            n.blend,
            chitrakar_doc::BlendMode::Hue
                | chitrakar_doc::BlendMode::Saturation
                | chitrakar_doc::BlendMode::Color
                | chitrakar_doc::BlendMode::Luminosity
        );
        let in_frame = {
            let mut up = page.parent_of(*id);
            let mut found = false;
            while let Some(p) = up {
                found |= matches!(
                    page.node(p).map(|m| &m.kind),
                    Ok(chitrakar_doc::NodeKind::Artboard { .. })
                );
                up = page.parent_of(p);
            }
            found
        };
        if n.blend != chitrakar_doc::BlendMode::Normal
            && (separable || !all_but_four)
            && !in_place
            && (!in_frame || !not_in_frames)
            && doc.node(*id).is_ok_and(|m| m.visible)
        {
            doc.apply(chitrakar_doc::Command::SetBlendMode {
                id: *id,
                blend: n.blend,
            })
            .unwrap();
        }
    }
    doc
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

    /// A tagged picture arrives as the colour it names, not merely as
    /// different numbers.
    ///
    /// What this used to say was `assert_ne!` — that the pixel changed.
    /// Every wrong conversion changes the pixel too, so the only thing
    /// it ruled out was the profile being ignored altogether; channels
    /// swapped, the transfer function applied twice, or the transform
    /// run backwards would all have passed.
    ///
    /// Importing is one-way, so there is no round trip to lean on. What
    /// stands in for one is the definition: Display P3 and sRGB share a
    /// white point and a transfer function and differ in their primaries,
    /// so the answer follows from the two primary matrices alone.
    /// Linearize (200,100,50), through P3's matrix to XYZ, back through
    /// the inverse of sRGB's, and encode: (215, 93, 31). Wider primaries
    /// mean the same colour needs a more extreme triple to say it in
    /// sRGB, which is the sense of the change as well as its size.
    ///
    /// Two levels of slack, which is what a real profile's own
    /// chromatic adaptation and table rounding cost against the
    /// arithmetic — this lands one level under on red and green.
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
        let want = [215i32, 93, 31];
        for (c, name) in [(0usize, "red"), (1, "green"), (2, "blue")] {
            let got = tagged.rgba8[c] as i32;
            assert!(
                (got - want[c]).abs() <= 2,
                "the {name} the file names is {}, got {got} (whole pixel {:?})",
                want[c],
                &tagged.rgba8[..3]
            );
        }
        assert_eq!(tagged.rgba8[3], 255, "alpha preserved");
    }

    /// A WebP read back, for the tests: the engine never reads one.
    pub(crate) fn decode_webp(bytes: &[u8]) -> SourceImage {
        let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).unwrap();
        let (width, height) = d.dimensions();
        assert!(d.has_alpha(), "the alpha was left out");
        let mut rgba8 = vec![0; d.output_buffer_size().unwrap()];
        d.read_image(&mut rgba8).unwrap();
        SourceImage {
            width,
            height,
            rgba8,
        }
    }

    #[test]
    fn a_webp_holds_every_pixel_a_png_does() {
        // Lossless means lossless: every byte back, the half-covered
        // ones and the clear ones too, which is what it is for.
        let (w, h) = (37u32, 23u32);
        let mut seed = 0x2545_f491u32;
        let rgba8: Vec<u8> = (0..w * h * 4)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed >> 24) as u8
            })
            .collect();
        let webp = Raster::Webp.encode(w, h, &rgba8).unwrap();
        assert_eq!(&webp[0..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let back = decode_webp(&webp);
        assert_eq!((back.width, back.height), (w, h));
        assert!(back.rgba8 == rgba8, "a lossless WebP changed pixels");
        // And it is a different file from the PNG of the same pixels.
        assert_eq!(&Raster::Png.encode(w, h, &rgba8).unwrap()[1..4], b"PNG");
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
