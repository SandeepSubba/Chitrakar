//! Reading a TIFF: what scanners, print shops and photographers' archives
//! keep pictures in. The engine wrote TIFFs (a press's CMYK separation)
//! long before it could read one; a TIFF placed in the browser went to the
//! browser's decoder, which only Safari has, so everywhere else it was
//! refused.
//!
//! Every picture comes out as 8-bit sRGB, as a PNG or a JPEG does: grey,
//! a palette, RGB and RGBA, CMYK and YCbCr; one bit a sample up to
//! sixteen, and floating point. An embedded profile is honoured — an RGB
//! one through the same normalisation a photograph's goes through, a CMYK
//! one through the press transform — and associated (premultiplied) alpha
//! is undone, since the engine keeps straight alpha at this edge. Only the
//! first picture of a many-page TIFF is read.

use crate::{CodecError, SourceImage};
use std::io::Cursor;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;
use tiff::ColorType;

/// Whether the bytes are a TIFF (classic or big, either byte order).
pub fn is_tiff(b: &[u8]) -> bool {
    b.len() >= 4 && matches!(&b[..4], b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+")
}

fn bad(what: impl std::fmt::Display) -> CodecError {
    CodecError::Format(format!("TIFF: {what}"))
}

/// The first picture in a TIFF, as 8-bit sRGB with straight alpha.
pub fn decode_tiff(bytes: &[u8]) -> Result<SourceImage, CodecError> {
    let mut dec = Decoder::new(Cursor::new(bytes)).map_err(bad)?;
    let (width, height) = dec.dimensions().map_err(bad)?;
    let color = dec.colortype().map_err(bad)?;
    // The profile's bytes come back as a list of integers of whatever
    // width the reader settles on, which the byte-vector helper refuses.
    let icc = dec
        .find_tag_unsigned_vec::<u8>(Tag::Unknown(34675))
        .ok()
        .flatten()
        .or_else(|| dec.get_tag_u8_vec(Tag::Unknown(34675)).ok());
    // ExtraSamples 1 is associated alpha: colour already multiplied by it.
    let premultiplied = dec
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .ok()
        .flatten()
        .is_some_and(|v| v.first() == Some(&1));
    let palette = if matches!(color, ColorType::Palette(_)) {
        Some(
            dec.find_tag_unsigned_vec::<u16>(Tag::ColorMap)
                .ok()
                .flatten()
                .ok_or_else(|| bad("a palette picture with no palette"))?,
        )
    } else {
        None
    };
    // A JPEG inside a TIFF comes out of its decoder as RGB already.
    let jpeg_inside = matches!(
        dec.find_tag_unsigned::<u16>(Tag::Compression)
            .ok()
            .flatten(),
        Some(6 | 7)
    );
    let (channels, bits) = match color {
        ColorType::Gray(b) | ColorType::Palette(b) => (1, b),
        ColorType::GrayA(b) => (2, b),
        ColorType::RGB(b) | ColorType::YCbCr(b) => (3, b),
        ColorType::RGBA(b) | ColorType::CMYK(b) => (4, b),
    };
    let pixels = width as usize * height as usize;
    let data = dec.read_image().map_err(bad)?;
    // Each sample as 0..=255 — or, for a palette, as its index.
    let samples: Vec<u16> = match data {
        DecodingResult::U8(v) if bits < 8 => {
            let raw = unpack(&v, width as usize, height as usize, channels, bits);
            if palette.is_some() {
                raw
            } else {
                let top = (1u32 << bits) - 1;
                raw.into_iter()
                    .map(|x| (x as u32 * 255 / top) as u16)
                    .collect()
            }
        }
        DecodingResult::U8(v) => v.into_iter().map(u16::from).collect(),
        DecodingResult::U16(v) if palette.is_some() => v,
        DecodingResult::U16(v) => v
            .into_iter()
            .map(|x| ((x as u32 + 128) / 257) as u16)
            .collect(),
        DecodingResult::U32(v) => v.into_iter().map(|x| (x >> 24) as u16).collect(),
        // A float TIFF is light, not code values: encoded for the screen.
        DecodingResult::F32(v) => v.into_iter().map(|x| float_to_srgb8(x as f64)).collect(),
        DecodingResult::F64(v) => v.into_iter().map(float_to_srgb8).collect(),
        _ => return Err(bad("signed or 64-bit samples are not read")),
    };
    if samples.len() < pixels * channels {
        return Err(bad("fewer samples than the picture is big"));
    }

    let mut rgba8 = vec![0u8; pixels * 4];
    let mut cmyk: Option<chitrakar_color::cms::CmykCms> = None;
    if matches!(color, ColorType::CMYK(_)) {
        cmyk = icc
            .as_deref()
            .and_then(|p| chitrakar_color::cms::CmykCms::new(p).ok());
    }
    for i in 0..pixels {
        let s = &samples[i * channels..i * channels + channels];
        let px: [u8; 4] = match color {
            ColorType::Gray(_) => [s[0] as u8, s[0] as u8, s[0] as u8, 255],
            ColorType::GrayA(_) => [s[0] as u8, s[0] as u8, s[0] as u8, s[1] as u8],
            ColorType::RGB(_) => [s[0] as u8, s[1] as u8, s[2] as u8, 255],
            ColorType::RGBA(_) => [s[0] as u8, s[1] as u8, s[2] as u8, s[3] as u8],
            ColorType::YCbCr(_) if jpeg_inside => [s[0] as u8, s[1] as u8, s[2] as u8, 255],
            ColorType::YCbCr(_) => ycbcr(s[0], s[1], s[2]),
            ColorType::Palette(_) => {
                let map = palette.as_deref().unwrap_or(&[]);
                let n = map.len() / 3;
                let k = (s[0] as usize).min(n.saturating_sub(1));
                let at = |c: usize| map.get(c * n + k).map_or(0, |v| (v >> 8) as u8);
                [at(0), at(1), at(2), 255]
            }
            ColorType::CMYK(_) => {
                let ink = |v: u16| v as f32 / 255.0;
                match &cmyk {
                    // Through the press profile it was separated for.
                    Some(t) => {
                        let c = t.to_working(ink(s[0]), ink(s[1]), ink(s[2]), ink(s[3]), 1.0);
                        let srgb = |v: f32| {
                            (chitrakar_color::linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round()
                                as u8
                        };
                        [srgb(c.r), srgb(c.g), srgb(c.b), 255]
                    }
                    // No profile: the plain subtraction every reader falls back on.
                    None => {
                        let k = 1.0 - ink(s[3]);
                        let ch = |v: u16| ((1.0 - ink(v)) * k * 255.0).round() as u8;
                        [ch(s[0]), ch(s[1]), ch(s[2]), 255]
                    }
                }
            }
        };
        rgba8[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    if premultiplied {
        for p in rgba8.as_chunks_mut::<4>().0 {
            let a = p[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
    }
    if let (Some(icc), ColorType::RGB(_) | ColorType::RGBA(_)) = (icc.as_deref(), color) {
        chitrakar_color::cms::normalize_rgba8_to_srgb(icc, &mut rgba8);
    }
    Ok(SourceImage {
        width,
        height,
        rgba8,
    })
}

/// Samples narrower than a byte, packed along each row (a row starts on a
/// byte), spread out one to a sample, unscaled: a palette's are indices,
/// and the caller scales anything else to 0..=255.
fn unpack(packed: &[u8], width: usize, height: usize, channels: usize, bits: u8) -> Vec<u16> {
    let bits = bits as usize;
    let per_row = width * channels;
    let row_bytes = (per_row * bits).div_ceil(8);
    let top = (1u32 << bits) - 1;
    let mut out = Vec::with_capacity(per_row * height);
    for y in 0..height {
        let row =
            &packed[(y * row_bytes).min(packed.len())..((y + 1) * row_bytes).min(packed.len())];
        for i in 0..per_row {
            let bit = i * bits;
            let byte = row.get(bit / 8).copied().unwrap_or(0);
            let v = (byte >> (8 - bits - bit % 8)) as u32 & top;
            out.push(v as u16);
        }
    }
    out
}

/// A floating-point sample is light, 0..=1, encoded here for the screen.
fn float_to_srgb8(x: f64) -> u16 {
    (chitrakar_color::linear_to_srgb(x.clamp(0.0, 1.0) as f32) * 255.0).round() as u16
}

fn ycbcr(y: u16, cb: u16, cr: u16) -> [u8; 4] {
    let (y, cb, cr) = (y as f32, cb as f32 - 128.0, cr as f32 - 128.0);
    let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    [
        c(y + 1.402 * cr),
        c(y - 0.344_136 * cb - 0.714_136 * cr),
        c(y + 1.772 * cb),
        255,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::{colortype, TiffEncoder};

    fn write<C: colortype::ColorType>(w: u32, h: u32, data: &[C::Inner]) -> Vec<u8>
    where
        [C::Inner]: tiff::encoder::TiffValue,
    {
        let mut out = Cursor::new(Vec::new());
        TiffEncoder::new(&mut out)
            .unwrap()
            .write_image::<C>(w, h, data)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn an_rgb_tiff_reads_as_its_pixels() {
        let bytes = write::<colortype::RGB8>(2, 1, &[255, 0, 0, 10, 20, 30]);
        assert!(is_tiff(&bytes));
        let img = crate::decode(&bytes).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgba8, vec![255, 0, 0, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn sixteen_bits_round_to_eight() {
        let bytes = write::<colortype::RGBA16>(1, 1, &[65535, 32896, 0, 65535]);
        let img = crate::decode(&bytes).unwrap();
        assert_eq!(img.rgba8, vec![255, 128, 0, 255]);
    }

    #[test]
    fn grey_reads_as_grey() {
        let bytes = write::<colortype::Gray8>(3, 1, &[0, 128, 255]);
        let img = crate::decode(&bytes).unwrap();
        assert_eq!(
            img.rgba8,
            vec![0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn cmyk_without_a_profile_is_the_plain_subtraction() {
        // Pure cyan, and black at half.
        let bytes = write::<colortype::CMYK8>(2, 1, &[255, 0, 0, 0, 0, 0, 0, 128]);
        let img = crate::decode(&bytes).unwrap();
        assert_eq!(&img.rgba8[..4], &[0, 255, 255, 255]);
        assert_eq!(&img.rgba8[4..], &[127, 127, 127, 255]);
    }

    #[test]
    fn the_engines_own_cmyk_tiff_comes_back_in_through_its_profile() {
        // Only with a real press profile, as the export's own tests are.
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let cms = chitrakar_color::cms::CmykCms::new(&icc).unwrap();
        let mut bytes = Vec::new();
        {
            let mut out = Cursor::new(&mut bytes);
            let mut enc = TiffEncoder::new(&mut out).unwrap();
            let mut img = enc.new_image::<colortype::CMYK8>(1, 1).unwrap();
            img.encoder()
                .write_tag(Tag::Unknown(34675), &icc[..])
                .unwrap();
            img.write_data(&[255, 0, 0, 0]).unwrap();
        }
        let got = crate::decode(&bytes).unwrap();
        let want = cms.to_working(1.0, 0.0, 0.0, 0.0, 1.0);
        let srgb = |v: f32| (chitrakar_color::linear_to_srgb(v) * 255.0).round() as u8;
        assert_eq!(&got.rgba8[..3], &[srgb(want.r), srgb(want.g), srgb(want.b)]);
        // And not the plain subtraction: a press's cyan is not (0, 255, 255).
        assert_ne!(&got.rgba8[..3], &[0, 255, 255]);
    }

    #[test]
    fn narrow_samples_spread_out_along_each_row() {
        // Two rows of three one-bit samples, each row starting on a byte.
        assert_eq!(
            unpack(&[0b1010_0000, 0b0110_0000], 3, 2, 1, 1),
            vec![1, 0, 1, 0, 1, 1]
        );
    }

    #[test]
    fn garbage_that_starts_like_a_tiff_is_refused_not_panicked_over() {
        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&[0xff; 32]);
        assert!(crate::decode(&bytes).is_err());
    }
}
