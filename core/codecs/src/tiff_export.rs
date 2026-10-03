//! CMYK TIFF export for print handoff: the composite is separated into ink
//! values through the document's press profile and written as a 4-channel
//! CMYK TIFF with that same profile embedded, so a printer or RIP
//! reproduces exactly what soft proofing showed.
//!
//! Without a press profile there is nothing authoritative to separate
//! into, so export refuses rather than guessing with a device formula.

use chitrakar_color::LinearRgba;
use std::io::Cursor;
use tiff::encoder::{colortype, TiffEncoder};
use tiff::tags::Tag;

/// ICC profile tag (34675, "InterColorProfile") — TIFF's standard slot for
/// an embedded profile.
const TAG_ICC_PROFILE: u16 = 34675;

#[derive(Debug, thiserror::Error)]
pub enum TiffError {
    #[error("CMYK TIFF export needs a CMYK press profile loaded on the document")]
    NoProfile,
    #[error("tiff encoding failed: {0}")]
    Encode(#[from] tiff::TiffError),
    #[error("color conversion failed: {0}")]
    Color(String),
}

/// Separate a linear-light composite into 8-bit CMYK through `icc`, and
/// write a TIFF with the profile embedded.
pub fn export_cmyk_tiff(
    pixels: &[LinearRgba],
    width: u32,
    height: u32,
    icc: &[u8],
) -> Result<Vec<u8>, TiffError> {
    let sep = chitrakar_color::cms::RgbToCmyk::new(icc).map_err(TiffError::Color)?;

    // Composite over white paper: TIFF CMYK has no alpha, and unprinted
    // areas are paper.
    let mut srgb = Vec::with_capacity(pixels.len() * 3);
    for px in pixels {
        let over_white = |v: f32| (v + (1.0 - px.a)).clamp(0.0, 1.0);
        srgb.push(chitrakar_color::linear_to_srgb(over_white(px.r)));
        srgb.push(chitrakar_color::linear_to_srgb(over_white(px.g)));
        srgb.push(chitrakar_color::linear_to_srgb(over_white(px.b)));
    }
    let cmyk = sep.separate(&srgb).map_err(TiffError::Color)?;

    let mut out = Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut out)?;
        let mut image = encoder.new_image::<colortype::CMYK8>(width, height)?;
        image
            .encoder()
            .write_tag(Tag::Unknown(TAG_ICC_PROFILE), icc)?;
        image.write_data(&cmyk)?;
    }
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Option<Vec<u8>> {
        std::env::var("CHITRAKAR_TEST_CMYK_ICC")
            .ok()
            .and_then(|p| std::fs::read(p).ok())
    }

    #[test]
    fn exports_a_cmyk_tiff_with_the_profile_embedded() {
        let Some(icc) = profile() else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        // 2×1: opaque red, then fully transparent (paper).
        let pixels = vec![
            LinearRgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            LinearRgba::TRANSPARENT,
        ];
        let bytes = export_cmyk_tiff(&pixels, 2, 1, &icc).unwrap();

        assert!(
            bytes.starts_with(b"II") || bytes.starts_with(b"MM"),
            "TIFF byte-order marker"
        );
        // The embedded profile is present verbatim.
        assert!(
            bytes
                .windows(icc.len().min(512))
                .any(|w| w == &icc[..icc.len().min(512)]),
            "ICC profile embedded"
        );

        // Read it back: 4 samples per pixel, CMYK photometric, and red
        // separates to mostly magenta+yellow while paper is near-zero ink.
        let mut dec = tiff::decoder::Decoder::new(Cursor::new(&bytes)).unwrap();
        let (w, h) = dec.dimensions().unwrap();
        assert_eq!((w, h), (2, 1));
        let tiff::decoder::DecodingResult::U8(data) = dec.read_image().unwrap() else {
            panic!("expected 8-bit samples");
        };
        assert_eq!(data.len(), 8, "two CMYK pixels");
        let (red, paper) = (&data[0..4], &data[4..8]);
        assert!(
            red[1] > 100 && red[2] > 100 && red[0] < 90,
            "red is magenta+yellow ink, got {red:?}"
        );
        assert!(
            paper.iter().all(|ink| *ink < 40),
            "transparent area is near-bare paper, got {paper:?}"
        );
    }

    #[test]
    fn non_cmyk_profile_is_refused() {
        let icc = chitrakar_color::cms::display_p3_profile_bytes();
        let err = export_cmyk_tiff(&[LinearRgba::TRANSPARENT], 1, 1, &icc);
        assert!(matches!(err, Err(TiffError::Color(_))));
    }
}
