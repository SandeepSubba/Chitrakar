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
    let orientation = match raw.orientation {
        O::HorizontalFlip => 2,
        O::Rotate180 => 3,
        O::VerticalFlip => 4,
        O::Transpose => 5,
        O::Rotate90 => 6,
        O::Transverse => 7,
        O::Rotate270 => 8,
        O::Normal | O::Unknown => 1u32,
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
        Samples::Int(v) => {
            out.push(0);
            for s in v {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        Samples::Float(v) => {
            out.push(1);
            for s in v {
                out.extend_from_slice(&s.to_le_bytes());
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
