//! Reading a camera's raw file to the sensor's values, in a module of its
//! own. `rawler` reads the file; what comes out is the mosaic packed in
//! the form `chitrakar_codecs::raw::unpack` takes, for the engine to
//! develop. Developing is the engine's (`develop.rs`); this is only the
//! reading, which is the heavy and the fragile half: a megabyte of
//! decoders for every camera there is, and a reader that relies on
//! catching its own panics, which WebAssembly cannot do. So in the
//! browser it is fetched only when a raw file is opened, and runs in a
//! worker, where a damaged file can bring down nothing but the worker.
//!
//! In the browser it is built for WASI (`wasm32-wasip1`) rather than
//! with wasm-bindgen, and the worker (`app/src/rawWorker.ts`) gives it
//! the handful of host functions it asks for; see `mod wasi`.
//!
//! The packed form, little-endian: `CKRW`, a version (1), then width,
//! height, samples a site, orientation (u32 each); black, white, white
//! balance (four f32 each); the XYZ-to-camera matrix (twelve f32); the
//! crop (top, right, bottom, left, u32 each); the filter's colour at each
//! site of the 48 by 48 square from the sensor's corner (a byte each,
//! which every pattern repeats within); whether the samples are whole
//! numbers (0) or floats (1); and the samples, u16 or f32.

use rawler::rawimage::{RawImageData, RawPhotometricInterpretation};

/// The side of the square the filter's pattern is written for.
pub const PATTERN: usize = 48;

/// Read a raw file and pack its mosaic.
pub fn read(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let source = rawler::rawsource::RawSource::new_from_slice(bytes);
    let mut raw = rawler::decode(&source, &rawler::decoders::RawDecodeParams::default())
        .map_err(|e| format!("camera raw: {e}"))?;
    correct(&mut raw, bytes);
    // The camera's colours from XYZ, measured under daylight where the file
    // (or rawler's tables) has it, otherwise under whatever light it was;
    // none at all for a camera nobody measured.
    let mut xyz_to_cam = [0f32; 12];
    let matrix = raw
        .color_matrix
        .get(&rawler::imgop::xyz::Illuminant::D65)
        .or_else(|| raw.color_matrix.values().next());
    if let Some(m) = matrix {
        for (slot, v) in xyz_to_cam.iter_mut().zip(m.iter()) {
            *slot = *v;
        }
    }
    if raw.wb_coeffs[..3]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
    {
        if let Some(wb) = daylight(&xyz_to_cam) {
            raw.wb_coeffs = wb;
        }
    }
    let (filter, unusual) = match &raw.photometric {
        RawPhotometricInterpretation::Cfa(c) => (Some(c.cfa.clone()), !c.cfa.is_rgb()),
        _ => (None, false),
    };
    use rawler::Orientation as O;
    // rawler's decoder says "upright" for every Fuji file, whichever way
    // the camera was held; its own reading of the file's EXIF has it
    // (6 or 8 for a photograph taken upright). Asked only when the
    // decoder has nothing better to say.
    let from_exif = || {
        let params = rawler::decoders::RawDecodeParams::default();
        rawler::get_decoder(&source)
            .and_then(|d| d.raw_metadata(&source, &params))
            .ok()
            .and_then(|m| m.exif.orientation)
            .filter(|o| (2..=8).contains(o))
            .map_or(1, u32::from)
    };
    let orientation = match raw.orientation {
        O::HorizontalFlip => 2,
        O::Rotate180 => 3,
        O::VerticalFlip => 4,
        O::Transpose => 5,
        O::Rotate90 => 6,
        O::Transverse => 7,
        O::Rotate270 => 8,
        O::Normal | O::Unknown => from_exif(),
    };
    // A filter of four colours (cyan, magenta, yellow and green; or red,
    // green, blue and emerald) has no red, green and blue of its own to
    // balance, and Fuji's SuperCCD lays its sites out turned by 45
    // degrees: both are developed by rawler as far as three colours on
    // a square grid, and come here as a raw already RGB.
    if unusual || raw.fuji_rotation_width.is_some() {
        use rawler::imgop::develop::{Intermediate, ProcessingStep as S, RawDevelop};
        let steps: &[S] = if unusual {
            // As far as linear sRGB under the camera's balance.
            &[
                S::Rescale,
                S::Demosaic,
                S::CropActiveArea,
                S::WhiteBalance,
                S::Calibrate,
                S::CropDefault,
            ]
        } else {
            &[S::Rescale, S::Demosaic, S::FujiRotate]
        };
        let rgb = match RawDevelop::new_with(steps).develop_intermediate(&raw) {
            Ok(Intermediate::ThreeColor(rgb)) => rgb,
            Ok(_) => return Err("camera raw: this sensor's layout is not read".into()),
            Err(e) => return Err(format!("camera raw: {e}")),
        };
        let (w, h) = (rgb.width, rgb.height);
        let crops = if unusual {
            [0; 4]
        } else {
            crops_of(raw.crop_area, w, h)
        };
        let (wb, xyz_to_cam) = if unusual {
            ([1.0, 1.0, 1.0, f32::NAN], [0f32; 12])
        } else {
            (raw.wb_coeffs, xyz_to_cam)
        };
        let floats: Vec<f32> = rgb.data.iter().flatten().copied().collect();
        return Ok(pack(
            [w as u32, h as u32, 3, orientation],
            [[0.0; 4], [1.0; 4], wb],
            &xyz_to_cam,
            crops,
            |_, _| 0,
            Samples::Float(&floats),
        ));
    }
    let color_at = |row: usize, col: usize| filter.as_ref().map_or(0, |c| c.color_at(row, col));
    // Black is given as a small pattern laid over the sensor, one level a
    // site of it; the develop takes one a colour.
    let mut black = [0f32; 4];
    let levels = &raw.blacklevel;
    if let Some(first) = levels.levels.first() {
        black = [first.as_f32(); 4];
        let (pw, ph, cpp) = (levels.width.max(1), levels.height.max(1), levels.cpp.max(1));
        for row in 0..ph {
            for col in 0..pw {
                if let Some(level) = levels.levels.get((row * pw + col) * cpp) {
                    black[color_at(row, col).min(3)] = level.as_f32();
                }
            }
        }
    }
    let whites = &raw.whitelevel.0;
    let white = [0, 1, 2, 3].map(|i| *whites.get(i).or(whites.first()).unwrap_or(&65535) as f32);
    // The area worth keeping: the camera's crop, else its active area.
    let crops = crops_of(raw.crop_area.or(raw.active_area), raw.width, raw.height);
    let samples = match &raw.data {
        RawImageData::Integer(v) => Samples::Int(v),
        RawImageData::Float(v) => Samples::Float(v),
    };
    Ok(pack(
        [
            raw.width as u32,
            raw.height as u32,
            raw.cpp as u32,
            orientation,
        ],
        [black, white, raw.wb_coeffs],
        &xyz_to_cam,
        crops,
        color_at,
        samples,
    ))
}

/// What rawler 0.8 has wrong about particular cameras, put right the way
/// dcraw and LibRaw have it, each found by a sample from raw.pixls.us
/// developing with a cast that its own maker's software does not have.
fn correct(raw: &mut rawler::RawImage, bytes: &[u8]) {
    // Filters laid the other way from rawler's table. Olympus's E-10 is
    // RGGB where rawler says BGGR: leaves come out cyan and skin blue.
    // Kodak's EasyShare Z981, as rawler reads it, has its greens on the
    // other diagonal from the BGGR it is given (they average the same
    // there, and the other two do not), and GRBG is the one layout that
    // makes white roses white — one sample, so a second would be worth
    // having; the Z990 beside it is BGGR as rawler says.
    let filter = match (raw.clean_make.as_str(), raw.clean_model.as_str()) {
        ("Olympus", "E-10") => Some("RGGB"),
        ("Kodak", "EasyShare Z981") => Some("GRBG"),
        _ => None,
    };
    if let (Some(name), RawPhotometricInterpretation::Cfa(c)) = (filter, &mut raw.photometric) {
        c.cfa = rawler::CFA::new(name);
    }
    // A Kodak's balance as shot is one of a table of presets in the
    // file's own directory (0xFE00), and which one the camera used is
    // tag 0xFA0D; rawler reads another table altogether, and gets
    // anything from a blue cast to a red one sixty times over.
    if raw.make.to_ascii_uppercase().contains("KODAK") {
        if let Some(wb) = kodak_balance(bytes) {
            raw.wb_coeffs = wb;
        }
    }
    // A Canon's small and medium raws (sRAW, mRAW) take their black off
    // when they are turned from brightness and colour to red, green and
    // blue — their darkest sites are nought — and then rawler gives them
    // the full-size raw's black as well (2047 on a 7D, 1023 on a 40D),
    // which takes it off again: most of the blue there is, and a
    // yellow-green cast on every one tried (40D, 7D's mRAW and sRAW).
    if raw.cpp == 3 && raw.clean_make == "Canon" && raw.camera.mode.starts_with("sRaw") {
        raw.blacklevel = rawler::rawimage::BlackLevel::new(&[0u16; 3], 1, 1, 3);
    }
}

/// The balance a Kodak was set to, from its own directory: dcraw's
/// `wbtag`, the preset tags in the order tag 0xFA0D numbers them.
fn kodak_balance(bytes: &[u8]) -> Option<[f32; 4]> {
    const PRESETS: [Option<u16>; 7] = [
        Some(0xFA25),
        Some(0xFA28),
        Some(0xFA27),
        Some(0xFA29),
        None,
        None,
        Some(0xFA2A),
    ];
    let little = match bytes.get(..4)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let b: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| {
        let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    // Each entry of a directory: its tag, type, count and where its
    // value is (in the entry when it fits in four bytes).
    let entries = |dir: usize| -> Option<Vec<(u16, u16, u32, usize)>> {
        let n = u16_at(dir)? as usize;
        (0..n.min(1024))
            .map(|i| {
                let e = dir + 2 + 12 * i;
                let (ty, count) = (u16_at(e + 2)?, u32_at(e + 4)?);
                let size = match ty {
                    1 | 2 | 6 | 7 => 1,
                    3 | 8 => 2,
                    4 | 9 | 11 | 13 => 4,
                    _ => 8,
                } * count as usize;
                let at = if size <= 4 {
                    e + 8
                } else {
                    u32_at(e + 8)? as usize
                };
                Some((u16_at(e)?, ty, count, at))
            })
            .collect()
    };
    let first = entries(u32_at(4)? as usize)?;
    let &(_, _, _, at) = first.iter().find(|e| e.0 == 0xFE00)?;
    let kodak = entries(u32_at(at)? as usize)?;
    let &(_, _, _, at) = kodak.iter().find(|e| e.0 == 0xFA0D)?;
    let preset = PRESETS.get(*bytes.get(at)? as usize).copied().flatten()?;
    let &(_, ty, count, at) = kodak.iter().find(|e| e.0 == preset)?;
    if ty != 4 || count != 3 {
        return None;
    }
    let [r, g, b] = [0, 4, 8].map(|o| u32_at(at + o).map_or(0.0, |v| v as f32));
    (r > 0.0 && g > 0.0 && b > 0.0).then(|| [r / g, 1.0, b / g, f32::NAN])
}

/// The white balance that makes daylight (D65) neutral, from the camera's
/// matrix: what a file that does not say how it was balanced is given, as
/// dcraw does, rather than none at all, which is a green picture. Each
/// colour's gain is one over what it records of daylight, the least of
/// them one.
fn daylight(xyz_to_cam: &[f32; 12]) -> Option<[f32; 4]> {
    const D65: [f32; 3] = [0.950_47, 1.0, 1.088_83];
    let seen = |c: usize| (0..3).map(|k| xyz_to_cam[c * 3 + k] * D65[k]).sum::<f32>();
    let colours = if xyz_to_cam[9..].iter().any(|v| *v != 0.0) {
        4
    } else {
        3
    };
    let mut wb = [f32::NAN; 4];
    for (c, slot) in wb.iter_mut().enumerate().take(colours) {
        let v = seen(c);
        if !(v.is_finite() && v > 1e-6) {
            return None;
        }
        *slot = 1.0 / v;
    }
    let least = wb[..colours].iter().copied().fold(f32::MAX, f32::min);
    Some(wb.map(|v| v / least))
}

/// A rectangle inside a `w` by `h` image as what it takes off each side:
/// top, right, bottom, left.
fn crops_of(r: Option<rawler::imgop::Rect>, w: usize, h: usize) -> [u32; 4] {
    r.map_or([0; 4], |r| {
        [
            r.p.y,
            w.saturating_sub(r.p.x + r.d.w),
            h.saturating_sub(r.p.y + r.d.h),
            r.p.x,
        ]
        .map(|v| v as u32)
    })
}

enum Samples<'a> {
    Int(&'a [u16]),
    Float(&'a [f32]),
}

/// Write the packed form: `size` is width, height, samples a site and
/// orientation; `levels` black, white and white balance.
fn pack(
    size: [u32; 4],
    levels: [[f32; 4]; 3],
    xyz_to_cam: &[f32; 12],
    crops: [u32; 4],
    color_at: impl Fn(usize, usize) -> usize,
    samples: Samples,
) -> Vec<u8> {
    let bytes = match &samples {
        Samples::Int(v) => v.len() * 2,
        Samples::Float(v) => v.len() * 4,
    };
    let mut out = Vec::with_capacity(4 + 4 * 5 + 4 * 24 + 16 + PATTERN * PATTERN + 1 + bytes);
    out.extend_from_slice(b"CKRW");
    for v in std::iter::once(1u32).chain(size) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in levels.iter().flatten().chain(xyz_to_cam) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in crops {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for row in 0..PATTERN {
        for col in 0..PATTERN {
            out.push(color_at(row, col).min(3) as u8);
        }
    }
    match samples {
        // Sized once and written in place: forty million two-byte
        // `extend`s were most of the time the packing took in WASM.
        Samples::Int(v) => {
            out.push(0);
            let at = out.len();
            out.resize(at + v.len() * 2, 0);
            for (to, s) in out[at..].chunks_exact_mut(2).zip(v) {
                to.copy_from_slice(&s.to_le_bytes());
            }
        }
        Samples::Float(v) => {
            out.push(1);
            let at = out.len();
            out.resize(at + v.len() * 4, 0);
            for (to, s) in out[at..].chunks_exact_mut(4).zip(v) {
                to.copy_from_slice(&s.to_le_bytes());
            }
        }
    }
    out
}

/// The module the page's worker loads, built for WASI rather than with
/// wasm-bindgen: the reader's CR3 decoder times itself with
/// `Instant::now()`, which on `wasm32-unknown-unknown` panics and on WASI
/// asks the host for the clock — one function the worker gives it. A
/// file goes in and the packing comes out through memory the worker
/// borrows from the module: `ck_alloc` a buffer, write the file into it,
/// `read_raw` it, and read back what `result` says, `ck_free`ing both.
#[cfg(target_os = "wasi")]
mod wasi {
    use std::cell::RefCell;

    thread_local! {
        static RESULT: RefCell<(Vec<u8>, bool)> = const { RefCell::new((Vec::new(), false)) };
    }

    #[no_mangle]
    pub extern "C" fn ck_alloc(len: usize) -> *mut u8 {
        let mut v = Vec::<u8>::with_capacity(len.max(1));
        let p = v.as_mut_ptr();
        std::mem::forget(v);
        p
    }

    /// # Safety
    /// `p` and `len` are what `ck_alloc` gave and was asked for.
    #[no_mangle]
    pub unsafe extern "C" fn ck_free(p: *mut u8, len: usize) {
        drop(Vec::from_raw_parts(p, 0, len.max(1)));
    }

    /// Read the file at `p`; 1 when it read, 0 when it did not, and either
    /// way `result_ptr`/`result_len` are the packing or the reason.
    ///
    /// # Safety
    /// `p` and `len` are a buffer `ck_alloc` gave, holding the file.
    #[no_mangle]
    pub unsafe extern "C" fn read_raw(p: *const u8, len: usize) -> u32 {
        let bytes = std::slice::from_raw_parts(p, len);
        let (out, ok) = match super::read(bytes) {
            Ok(packed) => (packed, true),
            Err(e) => (e.into_bytes(), false),
        };
        RESULT.with(|r| *r.borrow_mut() = (out, ok));
        ok as u32
    }

    #[no_mangle]
    pub extern "C" fn result_ptr() -> *const u8 {
        RESULT.with(|r| r.borrow().0.as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn result_len() -> usize {
        RESULT.with(|r| r.borrow().0.len())
    }

    /// Let the last result go.
    #[no_mangle]
    pub extern "C" fn result_free() {
        RESULT.with(|r| *r.borrow_mut() = (Vec::new(), false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_codecs::develop::sample;

    #[test]
    fn a_dng_read_here_unpacks_to_the_photograph_the_codecs_wrote() {
        let dng = sample::dng(32, 16, 4095, [0.8, 1.0, 0.6], |x, _| {
            if x < 16 {
                [0.18; 3]
            } else {
                [0.6; 3]
            }
        });
        let packed = read(&dng).unwrap();
        let photo = chitrakar_codecs::raw::unpack(&packed).unwrap();
        assert_eq!(photo.size(), (32, 16));
        let img = photo.develop(&chitrakar_doc::RawDevelop::default());
        let p = &img.rgba8[(8 * 32 + 8) * 4..(8 * 32 + 8) * 4 + 3];
        assert!(
            p[0].abs_diff(p[1]) <= 2 && p[1].abs_diff(p[2]) <= 2,
            "grey as the camera balanced it: {p:?}"
        );
    }

    #[test]
    fn what_is_not_a_raw_file_is_said_so() {
        assert!(read(b"not a raw file at all").is_err());
        let mut cr3 = vec![0, 0, 0, 24];
        cr3.extend_from_slice(b"ftypcrx ");
        cr3.resize(64, 0);
        assert!(read(&cr3).is_err(), "a CR3 that is only its first bytes");
    }

    #[test]
    fn a_file_that_does_not_say_its_balance_is_balanced_for_daylight() {
        // A camera that sees in sRGB's own primaries sees daylight as
        // white already: no gain on any colour.
        let srgb = [
            3.240_454,
            -1.537_139,
            -0.498_531_4,
            -0.969_266,
            1.876_010_8,
            0.041_556,
            0.055_643_4,
            -0.204_025_9,
            1.057_225_2,
            0.0,
            0.0,
            0.0,
        ];
        let wb = daylight(&srgb).unwrap();
        for v in &wb[..3] {
            assert!((v - 1.0).abs() < 0.01, "{wb:?}");
        }
        assert!(wb[3].is_nan(), "three colours, not four");
        // Kodak's DCS760C, whose files say nothing of it: blue wants most.
        let dcs = [
            1.6623, -0.6309, -0.1411, -0.4344, 1.3923, 0.0323, 0.2285, 0.0274, 0.2926, 0.0, 0.0,
            0.0,
        ];
        let wb = daylight(&dcs).unwrap();
        assert!(
            wb[2] > wb[0] && wb[0] > wb[1] && (wb[1] - 1.0).abs() < 1e-6,
            "{wb:?}"
        );
        // A fourth colour is given its gain too; no matrix, no guess.
        let mut four = srgb;
        four[9..].copy_from_slice(&[0.5, 0.5, 0.0]);
        assert!(daylight(&four).unwrap()[3].is_finite());
        assert!(daylight(&[0.0; 12]).is_none());
    }

    /// Real files, when `CHITRAKAR_TEST_RAW` names a folder of them: each
    /// reads, unpacks and develops to a photograph of its own size.
    /// A TIFF of two directories, the first pointing at a Kodak's own
    /// (0xFE00) holding its preset number and the presets.
    fn kodak_tiff(little: bool, preset: u8, tables: &[(u16, [u32; 3])]) -> Vec<u8> {
        let u16b = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let u32b = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut out = if little {
            b"II*\0".to_vec()
        } else {
            b"MM\0*".to_vec()
        };
        out.extend(u32b(8));
        // The first directory: one entry, then no next.
        out.extend(u16b(1));
        let kodak_at = 8 + 2 + 12 + 4;
        out.extend(u16b(0xFE00));
        out.extend(u16b(4));
        out.extend(u32b(1));
        out.extend(u32b(kodak_at));
        out.extend(u32b(0));
        // The Kodak directory, its values after it.
        let n = 1 + tables.len() as u32;
        let mut values_at = kodak_at + 2 + 12 * n + 4;
        out.extend(u16b(n as u16));
        out.extend(u16b(0xFA0D));
        out.extend(u16b(1));
        out.extend(u32b(1));
        out.extend([preset, 0, 0, 0]);
        for (tag, _) in tables {
            out.extend(u16b(*tag));
            out.extend(u16b(4));
            out.extend(u32b(3));
            out.extend(u32b(values_at));
            values_at += 12;
        }
        out.extend(u32b(0));
        for (_, v) in tables {
            for x in v {
                out.extend(u32b(*x));
            }
        }
        out
    }

    #[test]
    fn a_kodaks_balance_is_the_preset_it_says_it_used() {
        let tables = [
            (0xFA25, [120576, 65536, 125184]),
            (0xFA29, [101376, 65536, 128256]),
            (0xFA2A, [169984, 65536, 86784]),
        ];
        for little in [true, false] {
            let wb = kodak_balance(&kodak_tiff(little, 0, &tables)).unwrap();
            assert_eq!(wb[..3], [120576.0 / 65536.0, 1.0, 125184.0 / 65536.0]);
            let wb = kodak_balance(&kodak_tiff(little, 3, &tables)).unwrap();
            assert_eq!(wb[..3], [101376.0 / 65536.0, 1.0, 128256.0 / 65536.0]);
            let wb = kodak_balance(&kodak_tiff(little, 6, &tables)).unwrap();
            assert_eq!(wb[..3], [169984.0 / 65536.0, 1.0, 86784.0 / 65536.0]);
            // A preset with no table, one the file does not have, and a
            // number past the end are no balance, not somebody else's.
            assert_eq!(kodak_balance(&kodak_tiff(little, 4, &tables)), None);
            assert_eq!(kodak_balance(&kodak_tiff(little, 1, &tables)), None);
            assert_eq!(kodak_balance(&kodak_tiff(little, 9, &tables)), None);
        }
        // Cut short anywhere before the preset's own values end, it is
        // nothing rather than a panic; past there, the preset.
        let whole = kodak_tiff(true, 0, &tables);
        let needed = whole.len() - 24;
        for end in 0..whole.len() {
            let got = kodak_balance(&whole[..end]);
            if end < needed {
                assert_eq!(got, None, "cut at {end}");
            } else {
                assert!(got.is_some(), "cut at {end}");
            }
        }
        assert_eq!(kodak_balance(b"not a tiff at all"), None);
    }

    #[test]
    fn real_cameras_files_read_when_there_are_some() {
        let Ok(dir) = std::env::var("CHITRAKAR_TEST_RAW") else {
            return;
        };
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = path.to_string_lossy().to_string();
            if !chitrakar_codecs::raw::is_raw_name(&name) {
                continue;
            }
            let packed =
                read(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            let photo = chitrakar_codecs::raw::unpack(&packed).unwrap();
            let [k, _] = photo.as_shot_white();
            assert!((2500.0..12000.0).contains(&k), "{name}: as shot at {k} K");
        }
    }
}
