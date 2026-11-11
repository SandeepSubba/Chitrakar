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
//! first picture of a many-page TIFF is read. Two layouts are refused in
//! words: colours kept in planes of their own, and a fax's compression.
//!
//! The `tiff` crate refuses a palette picture outright (it has no colour
//! type for one), and reads samples narrower than a byte as if each were
//! a byte, running off the end of the strip. So a palette picture, or a
//! grey one of 1, 2 or 4 bits, is read through a copy whose directory says
//! it is plain grey — of 8 bits, as many columns as each row has bytes,
//! when it is narrower — and the decoder hands back the bytes as stored:
//! they are unpacked, and the palette looked up, here.

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
    let as_grey;
    let mut white_is_zero = false;
    let color = match as_plain_grey(bytes) {
        Some(plain) => {
            as_grey = plain.bytes;
            white_is_zero = plain.white_is_zero;
            dec = Decoder::new(Cursor::new(&as_grey[..])).map_err(bad)?;
            if !matches!(dec.colortype().map_err(bad)?, ColorType::Gray(_)) {
                return Err(bad("a grey or palette picture that is neither"));
            }
            if plain.palette {
                ColorType::Palette(plain.bits)
            } else if plain.alpha {
                ColorType::GrayA(plain.bits)
            } else {
                ColorType::Gray(plain.bits)
            }
        }
        None => dec.colortype().map_err(bad)?,
    };
    // Which way up the picture is meant to be seen (a camera turned on
    // its side writes 6 or 8 here and leaves the pixels as they fell).
    let orientation = dec
        .find_tag_unsigned::<u16>(Tag::Orientation)
        .ok()
        .flatten()
        .and_then(|o| u8::try_from(o).ok())
        .and_then(image::metadata::Orientation::from_exif)
        .unwrap_or(image::metadata::Orientation::NoTransforms);
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
    // Two layouts the decoder cannot read, said in words rather than in
    // its own: each colour kept as a plane of its own (Photoshop's "Per
    // Channel" order), and a fax machine's compression.
    if channels > 1
        && dec
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
            .ok()
            .flatten()
            == Some(2)
    {
        return Err(bad(
            "its colours are kept in separate planes, which is not read; save it again with the pixels interleaved",
        ));
    }
    if matches!(
        dec.find_tag_unsigned::<u16>(Tag::Compression)
            .ok()
            .flatten(),
        Some(2..=4)
    ) {
        return Err(bad(
            "it is compressed as a fax is (CCITT), which is not read; save it again uncompressed or with LZW",
        ));
    }
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
    // Read through the plain copy, white-is-zero grey is still the
    // other way up (the decoder turns it only when it reads it itself).
    // Only the grey: an alpha beside it is coverage either way.
    let samples = if white_is_zero {
        samples
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                if i % channels == 0 {
                    255 - v.min(255)
                } else {
                    v
                }
            })
            .collect()
    } else {
        samples
    };

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
    Ok(crate::upright(
        SourceImage {
            width,
            height,
            rgba8,
        },
        orientation,
    ))
}

/// A TIFF the decoder is handed in another's words: see the module's
/// note. `bits` is the width of a sample as stored.
struct PlainGrey {
    bytes: Vec<u8>,
    bits: u8,
    palette: bool,
    /** Grey and alpha, interleaved. */
    alpha: bool,
    white_is_zero: bool,
}

/// A copy of a palette picture, or a grey one narrower than a byte, whose
/// first directory says plain grey; `None` for any other picture, and for
/// one whose directory cannot be read or rewritten in place.
fn as_plain_grey(bytes: &[u8]) -> Option<PlainGrey> {
    let little = match bytes.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let int = |at: usize, n: usize| -> Option<u64> {
        let b = bytes.get(at..at.checked_add(n)?)?;
        Some(if little {
            b.iter().rev().fold(0, |v, &x| v << 8 | x as u64)
        } else {
            b.iter().fold(0, |v, &x| v << 8 | x as u64)
        })
    };
    let big = int(2, 2)? == 43;
    // Where the first directory is, how many entries it has, and how
    // long each is, classic or big.
    let (first, count, entry, head) = if big {
        let first = usize::try_from(int(8, 8)?).ok()?;
        (first, int(first, 8)?, 20, 8)
    } else {
        let first = usize::try_from(int(4, 4)?).ok()?;
        (first, int(first, 2)?, 12, 2)
    };
    let value_at = if big { 12 } else { 8 };
    // Each tag asked about: where its value sits in its entry, its type,
    // the value — only a SHORT or LONG held in the entry — and where its
    // count is and what it says.
    struct Entry {
        value_at: usize,
        kind: u64,
        value: u64,
        count_at: usize,
        count: u64,
    }
    let mut found: [Option<Entry>; 7] = Default::default();
    const TAGS: [u64; 7] = [256, 258, 262, 277, 322, 284, 317];
    for i in 0..usize::try_from(count).ok()?.min(4096) {
        let at = first + head + i * entry;
        let tag = int(at, 2)?;
        if let Some(k) = TAGS.iter().position(|&t| t == tag) {
            let kind = int(at + 2, 2)?;
            let n = int(at + 4, if big { 8 } else { 4 })?;
            let width = match kind {
                3 => 2,
                4 => 4,
                _ => return None,
            };
            // Bits a sample may be given once a sample (grey and its
            // alpha: two SHORTs, still in the entry); everything else once.
            if n != 1 && !(tag == 258 && n == 2 && kind == 3) {
                return None;
            }
            found[k] = Some(Entry {
                value_at: at + value_at,
                kind,
                value: int(at + value_at, width)?,
                count_at: at + 4,
                count: n,
            });
        }
    }
    let [width, bits, photometric, samples, tiled, planar, predictor] = found;
    let width = width?;
    let bits_value = bits.as_ref().map_or(1, |b| b.value);
    let photometric = photometric?;
    let samples_n = samples.as_ref().map_or(1, |s| s.value);
    if !matches!(photometric.value, 0 | 1 | 3) {
        return None;
    }
    let palette = photometric.value == 3;
    let narrow = samples_n == 1 && matches!(bits_value, 1 | 2 | 4);
    // Grey with an alpha beside it, which the crate has no colour type for
    // either: read as plain grey twice as wide, the two interleaved. Not
    // where the samples are in planes of their own, or each differs from
    // the one before (a predictor), or tiles count pixels across.
    let with_alpha = samples_n == 2
        && !palette
        && matches!(bits_value, 8 | 16)
        && planar.as_ref().is_none_or(|p| p.value == 1)
        && predictor.as_ref().is_none_or(|p| p.value == 1)
        && tiled.is_none();
    if samples_n != 1 && !with_alpha {
        return None;
    }
    if !(narrow || with_alpha || palette && matches!(bits_value, 1 | 2 | 4 | 8 | 16)) {
        return None;
    }
    // A tile is a number of pixels across, which a narrower sample does
    // not keep; tiled narrow pictures are left to the decoder.
    if narrow && tiled.is_some() {
        return None;
    }
    let mut copy = bytes.to_vec();
    let mut put = |at: usize, n: usize, v: u64| {
        for i in 0..n {
            let shift = if little { 8 * i } else { 8 * (n - 1 - i) };
            copy[at + i] = (v >> shift) as u8;
        }
    };
    let size = |kind: u64| if kind == 3 { 2 } else { 4 };
    put(photometric.value_at, 2, 1);
    if narrow {
        let b = bits.as_ref()?;
        put(b.value_at, 2, 8);
        put(
            width.value_at,
            size(width.kind),
            (width.value * bits_value).div_ceil(8),
        );
    }
    if with_alpha {
        let s = samples?;
        put(s.value_at, size(s.kind), 1);
        put(width.value_at, size(width.kind), width.value * 2);
        if let Some(b) = bits.filter(|b| b.count == 2) {
            put(b.count_at, if big { 8 } else { 4 }, 1);
        }
    }
    Some(PlainGrey {
        bytes: copy,
        bits: bits_value as u8,
        palette,
        alpha: with_alpha,
        white_is_zero: photometric.value == 0 && !palette,
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

    /// A TIFF from a camera turned on its side says so in its
    /// Orientation tag, and comes in turned: 6 is a quarter turn
    /// clockwise, so the left column is the top row.
    #[test]
    fn a_tiff_turned_on_its_side_comes_in_upright() {
        let mut out = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut out).unwrap();
        let mut img = enc.new_image::<colortype::RGB8>(2, 1).unwrap();
        img.encoder().write_tag(Tag::Orientation, 6u16).unwrap();
        img.write_data(&[255, 0, 0, 0, 0, 255]).unwrap();
        let img = crate::decode(&out.into_inner()).unwrap();
        assert_eq!((img.width, img.height), (1, 2));
        assert_eq!(img.rgba8, vec![255, 0, 0, 255, 0, 0, 255, 255]);
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
    fn a_palette_tiff_reads_through_its_palette() {
        // Four pixels indexing a palette of 256 (16-bit per channel, all
        // reds, then all greens, then all blues): index 0 is black, 1 is
        // pure red, 2 pure green, 3 a mid grey.
        let mut map = vec![0u16; 768];
        for (i, rgb) in [[0, 0, 0], [255, 0, 0], [0, 255, 0], [128, 128, 128]]
            .iter()
            .enumerate()
        {
            for c in 0..3 {
                map[c * 256 + i] = rgb[c] * 257;
            }
        }
        let mut out = Cursor::new(Vec::new());
        let mut tiff = TiffEncoder::new(&mut out).unwrap();
        let mut img = tiff.new_image::<colortype::Gray8>(2, 2).unwrap();
        img.encoder()
            .write_tag(Tag::PhotometricInterpretation, 3u16)
            .unwrap();
        img.encoder().write_tag(Tag::ColorMap, &map[..]).unwrap();
        img.write_data(&[0u8, 1, 2, 3]).unwrap();
        let bytes = out.into_inner();
        // The crate alone refuses it, which is why it goes through grey.
        assert!(Decoder::new(Cursor::new(&bytes[..]))
            .unwrap()
            .colortype()
            .is_err());
        let got = decode_tiff(&bytes).expect("a palette TIFF reads");
        assert_eq!(
            got.rgba8,
            [0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 128, 128, 128, 255]
        );
    }

    /// A TIFF of samples narrower than a byte: written as 8-bit grey of
    /// as many columns as a row has bytes, then its directory set to say
    /// what the bytes are.
    fn narrow(
        width: u32,
        rows: &[&[u8]],
        bits: u16,
        photometric: u16,
        map: Option<&[u16]>,
    ) -> Vec<u8> {
        let row_bytes = rows[0].len() as u32;
        let mut out = Cursor::new(Vec::new());
        let mut tiff = TiffEncoder::new(&mut out).unwrap();
        let mut img = tiff
            .new_image::<colortype::Gray8>(row_bytes, rows.len() as u32)
            .unwrap();
        img.encoder().write_tag(Tag::ImageWidth, width).unwrap();
        img.encoder().write_tag(Tag::BitsPerSample, bits).unwrap();
        img.encoder()
            .write_tag(Tag::PhotometricInterpretation, photometric)
            .unwrap();
        if let Some(map) = map {
            img.encoder().write_tag(Tag::ColorMap, map).unwrap();
        }
        img.write_data(&rows.concat()).unwrap();
        out.into_inner()
    }

    #[test]
    fn narrow_samples_read_whole() {
        // One bit, black is zero: 1 0 1 across ten pixels a row, so a
        // row runs into a second byte.
        let bytes = narrow(
            10,
            &[&[0b1010_1010, 0b1000_0000], &[0b0101_0101, 0b0100_0000]],
            1,
            1,
            None,
        );
        let got = decode_tiff(&bytes).expect("a one-bit TIFF reads");
        let grey: Vec<u8> = got.rgba8.chunks(4).map(|p| p[0]).collect();
        assert_eq!(&grey[..10], &[255, 0, 255, 0, 255, 0, 255, 0, 255, 0]);
        assert_eq!(&grey[10..], &[0, 255, 0, 255, 0, 255, 0, 255, 0, 255]);
        // White is zero: the same bits the other way up.
        let bytes = narrow(10, &[&[0b1010_1010, 0b1000_0000]], 1, 0, None);
        let got = decode_tiff(&bytes).expect("a white-is-zero TIFF reads");
        assert_eq!(got.rgba8[..8], [0, 0, 0, 255, 255, 255, 255, 255]);
        // Four bits through a palette of sixteen.
        let mut map = vec![0u16; 48];
        map[3] = 65535; // red of index 3
        map[16 + 5] = 65535; // green of index 5
        let bytes = narrow(3, &[&[0x35, 0x00]], 4, 3, Some(&map));
        let got = decode_tiff(&bytes).expect("a four-bit palette TIFF reads");
        assert_eq!(got.rgba8, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255]);
        // One bit through a palette of two.
        let map = [0u16, 65535, 0, 0, 0, 65535];
        let bytes = narrow(4, &[&[0b0110_0000]], 1, 3, Some(&map));
        let got = decode_tiff(&bytes).expect("a one-bit palette TIFF reads");
        let px: Vec<&[u8]> = got.rgba8.chunks(4).collect();
        assert_eq!(
            px,
            [
                &[0, 0, 0, 255][..],
                &[255, 0, 255, 255],
                &[255, 0, 255, 255],
                &[0, 0, 0, 255]
            ]
        );
    }

    #[test]
    fn grey_with_alpha_reads_as_both() {
        // Two pixels of grey and coverage, interleaved: written as plain
        // grey of four columns, then said to be two of grey and alpha.
        for (photometric, grey) in [(1u16, [40u8, 200]), (0, [215, 55])] {
            let mut out = Cursor::new(Vec::new());
            let mut tiff = TiffEncoder::new(&mut out).unwrap();
            let mut img = tiff.new_image::<colortype::Gray8>(4, 1).unwrap();
            img.encoder().write_tag(Tag::ImageWidth, 2u32).unwrap();
            img.encoder().write_tag(Tag::SamplesPerPixel, 2u16).unwrap();
            img.encoder()
                .write_tag(Tag::BitsPerSample, &[8u16, 8][..])
                .unwrap();
            img.encoder().write_tag(Tag::ExtraSamples, 2u16).unwrap();
            img.encoder()
                .write_tag(Tag::PhotometricInterpretation, photometric)
                .unwrap();
            img.write_data(&[grey[0], 255, grey[1], 128]).unwrap();
            let bytes = out.into_inner();
            let got = decode_tiff(&bytes).expect("grey and alpha read");
            assert_eq!((got.width, got.height), (2, 1));
            assert_eq!(
                got.rgba8,
                [40, 40, 40, 255, 200, 200, 200, 128],
                "photometric {photometric}"
            );
        }
    }

    #[test]
    fn garbage_that_starts_like_a_tiff_is_refused_not_panicked_over() {
        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&[0xff; 32]);
        assert!(crate::decode(&bytes).is_err());
    }
}
