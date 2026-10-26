//! A camera's raw file, developed: what the sensor saw, before the camera
//! made a JPEG of it. `rawloader` reads the file — hundreds of cameras'
//! formats, each with its own packing and compression — and this turns
//! the mosaic it hands back into a picture, which is the half a raw
//! developer does and the half `rawloader`'s companion (`imagepipe`)
//! could not be used for: it runs its passes on threads, and the engine
//! runs where there are none.
//!
//! The development is dcraw's defaults, which is what a raw opened
//! without any settings has looked like for twenty years: the black
//! level taken off and the white level made 1; the camera's own white
//! balance; each missing colour at a photosite averaged from its
//! neighbours of that colour (bilinear for a Bayer sensor, and the same
//! rule over a wider window serves Fuji's X-Trans); highlights where the
//! sensor clipped made white rather than magenta; the camera's colour
//! matrix taken to sRGB; an exposure that puts the brightest hundredth of
//! the picture at white; the camera's crop and orientation. What comes
//! out is a starting point, as an import is — the picture's tone and
//! colour are then the adjustment layers' to change, non-destructively.

use crate::{CodecError, SourceImage};

/// The extensions cameras write raw files under. A raw file is told by
/// its name first: many of them are TIFFs inside, and read as a TIFF they
/// give the small preview at their front rather than the photograph.
pub const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "dcr", "dcs", "dng", "drf", "eip",
    "erf", "fff", "iiq", "k25", "kdc", "mdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "pef",
    "ptx", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "x3f",
];

/// Whether a file's name says it is a camera's raw file.
pub fn is_raw_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| RAW_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The sensor's picture as `rawloader` gives it, in the terms developing
/// it needs — so a mosaic can be made by hand in a test, without a file.
pub struct Mosaic<'a, T: Copy + Into<f32>> {
    pub width: usize,
    pub height: usize,
    /// Samples per photosite: 1 for a colour-filtered sensor, 3 for a
    /// raw already made RGB (a linear DNG, a small raw).
    pub cpp: usize,
    /// The samples, as the file holds them: whole numbers for almost
    /// every camera, floats for some DNGs.
    pub data: &'a [T],
    /// The filter's colour at a photosite, absolute coordinates: 0 red,
    /// 1 green, 2 blue (3, a second green, is read as green).
    pub color_at: &'a dyn Fn(usize, usize) -> usize,
    pub black: [f32; 4],
    pub white: [f32; 4],
    /// The camera's white balance, any scale; NaN or 0 for none.
    pub wb: [f32; 4],
    /// XYZ to the camera's colours, one row per colour; all zero for a
    /// camera nobody measured, which then gets no matrix.
    pub xyz_to_cam: [[f32; 3]; 4],
    /// Top, right, bottom, left: what to cut off the sensor's edges.
    pub crops: [usize; 4],
    /// The EXIF orientation number (1 upright, 3 turned half round, 6 a
    /// quarter clockwise, 8 a quarter anticlockwise; 2, 4, 5, 7 mirrored).
    pub orientation: u8,
}

fn bad(what: impl std::fmt::Display) -> CodecError {
    CodecError::Format(format!("camera raw: {what}"))
}

/// Read and develop a camera's raw file.
pub fn decode_raw(bytes: &[u8]) -> Result<SourceImage, CodecError> {
    // Canon's CR3 is an ISO media file, not a TIFF, and the decoder has
    // no reader for it: say what will work rather than what failed.
    if bytes.len() >= 12 && &bytes[4..12] == b"ftypcrx " {
        return Err(CodecError::Format(
            "raw: a Canon CR3, which this editor cannot develop yet; Adobe's free DNG Converter \
             turns it into a DNG, which it can"
                .into(),
        ));
    }
    let raw = rawloader::decode(&mut std::io::Cursor::new(bytes)).map_err(bad)?;
    let cfa = &raw.cfa;
    let color_at = |row: usize, col: usize| cfa.color_at(row, col);
    use rawloader::Orientation as O;
    let orientation = match raw.orientation {
        O::HorizontalFlip => 2,
        O::Rotate180 => 3,
        O::VerticalFlip => 4,
        O::Transpose => 5,
        O::Rotate90 => 6,
        O::Transverse => 7,
        O::Rotate270 => 8,
        O::Normal | O::Unknown => 1,
    };
    let f = |v: [u16; 4]| v.map(|x| x as f32);
    macro_rules! with {
        ($data:expr) => {
            develop(&Mosaic {
                width: raw.width,
                height: raw.height,
                cpp: raw.cpp,
                data: $data,
                color_at: &color_at,
                black: f(raw.blacklevels),
                white: f(raw.whitelevels),
                wb: raw.wb_coeffs,
                xyz_to_cam: raw.xyz_to_cam,
                crops: raw.crops,
                orientation,
            })
        };
    }
    match &raw.data {
        rawloader::RawImageData::Integer(v) => with!(v),
        rawloader::RawImageData::Float(v) => with!(v),
    }
}

/// Develop a sensor's mosaic into 8-bit sRGB.
pub fn develop<T: Copy + Into<f32>>(m: &Mosaic<T>) -> Result<SourceImage, CodecError> {
    let [top, right, bottom, left] = m.crops;
    if m.cpp != 1 && m.cpp != 3 {
        return Err(bad(format!("{} samples a photosite are not read", m.cpp)));
    }
    if m.width <= left + right || m.height <= top + bottom {
        return Err(bad("cropped to nothing"));
    }
    if m.data.len() < m.width * m.height * m.cpp {
        return Err(bad("fewer samples than the sensor is big"));
    }
    let (w, h) = (m.width - left - right, m.height - top - bottom);

    // White balance, green as one; a missing or nonsense value is one.
    let wb = {
        let ok = |v: f32| v.is_finite() && v > 0.0;
        let g = if ok(m.wb[1]) { m.wb[1] } else { 1.0 };
        let one = |c: usize| if ok(m.wb[c]) { m.wb[c] / g } else { 1.0 };
        [
            one(0),
            1.0,
            one(2),
            if ok(m.wb[3]) { m.wb[3] / g } else { 1.0 },
        ]
    };
    let to_srgb = camera_to_srgb(&m.xyz_to_cam);
    // A photosite's value with black off, white at one, balanced, and
    // clipped: where the sensor clipped, every colour is one, and white
    // through the matrix is white — rather than the green alone at one
    // and red and blue past it, which develops magenta.
    let site = |row: usize, col: usize, c: usize| -> f32 {
        let at = (row * m.width + col) * m.cpp + if m.cpp == 3 { c } else { 0 };
        let k = if m.cpp == 3 {
            c
        } else {
            (m.color_at)(row, col)
        };
        let k = k.min(3);
        let span = (m.white[k] - m.black[k]).max(1.0);
        (((m.data[at].into() - m.black[k]) / span).max(0.0) * wb[k]).min(1.0)
    };
    // The three colours at a photosite: its own, and each missing one the
    // average of that colour's photosites in the 3×3 round it — or, where
    // there are none (X-Trans), the 5×5.
    let rgb_at = |row: usize, col: usize| -> [f32; 3] {
        if m.cpp == 3 {
            return [site(row, col, 0), site(row, col, 1), site(row, col, 2)];
        }
        let own = (m.color_at)(row, col).min(3);
        let own = if own == 3 { 1 } else { own };
        let mut out = [0f32; 3];
        for (c, slot) in out.iter_mut().enumerate() {
            if c == own {
                *slot = site(row, col, c);
                continue;
            }
            for reach in [1isize, 2] {
                let (mut sum, mut n) = (0f32, 0u32);
                for dy in -reach..=reach {
                    for dx in -reach..=reach {
                        let (y, x) = (row as isize + dy, col as isize + dx);
                        if y < top as isize
                            || x < left as isize
                            || y >= (m.height - bottom) as isize
                            || x >= (m.width - right) as isize
                        {
                            continue;
                        }
                        let (y, x) = (y as usize, x as usize);
                        let k = (m.color_at)(y, x).min(3);
                        if (if k == 3 { 1 } else { k }) == c {
                            sum += site(y, x, c);
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    *slot = sum / n as f32;
                    break;
                }
            }
        }
        out
    };
    let linear = |row: usize, col: usize| -> [f32; 3] {
        let c = rgb_at(row, col);
        let mut o = [0f32; 3];
        for (i, slot) in o.iter_mut().enumerate() {
            *slot = to_srgb[i][0] * c[0] + to_srgb[i][1] * c[1] + to_srgb[i][2] * c[2];
        }
        o
    };

    // Exposure: the brightest hundredth of the picture at white, judged
    // from every fourth photosite each way — dcraw's rule, and why a raw
    // does not open as dark as the sensor recorded it. Never darker than
    // the sensor recorded, and never more than four stops brighter.
    let mut brightest: Vec<f32> = Vec::with_capacity((w / 4 + 1) * (h / 4 + 1));
    for y in (top..m.height - bottom).step_by(4) {
        for x in (left..m.width - right).step_by(4) {
            let c = linear(y, x);
            brightest.push(c[0].max(c[1]).max(c[2]));
        }
    }
    let gain = if brightest.is_empty() {
        1.0
    } else {
        let at = ((brightest.len() as f32) * 0.99) as usize;
        let at = at.min(brightest.len() - 1);
        let (_, p99, _) = brightest.select_nth_unstable_by(at, f32::total_cmp);
        if *p99 > 1e-4 {
            (1.0 / *p99).clamp(1.0, 16.0)
        } else {
            1.0
        }
    };

    let mut upright = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let c = linear(y + top, x + left);
            let px = &mut upright[(y * w + x) * 4..(y * w + x) * 4 + 4];
            for i in 0..3 {
                px[i] = chitrakar_color::linear_to_srgb8((c[i] * gain).clamp(0.0, 1.0));
            }
            px[3] = 255;
        }
    }
    let (width, height, rgba8) = orient(upright, w, h, m.orientation);
    Ok(SourceImage {
        width: width as u32,
        height: height as u32,
        rgba8,
    })
}

/// From the camera's colours to linear sRGB: the inverse of sRGB → XYZ →
/// camera, each row of which is first made to sum to one so that a
/// white-balanced white stays white (dcraw's `cam_xyz_coeff`). A camera
/// with no matrix gets none.
fn camera_to_srgb(xyz_to_cam: &[[f32; 3]; 4]) -> [[f32; 3]; 3] {
    const SRGB_TO_XYZ: [[f32; 3]; 3] = [
        [0.412_456_4, 0.357_576_1, 0.180_437_5],
        [0.212_672_9, 0.715_152_2, 0.072_175_0],
        [0.019_333_9, 0.119_192, 0.950_304_1],
    ];
    const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut srgb_to_cam = [[0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            srgb_to_cam[i][j] = (0..3).map(|k| xyz_to_cam[i][k] * SRGB_TO_XYZ[k][j]).sum();
        }
        let sum: f32 = srgb_to_cam[i].iter().sum();
        if sum.abs() < 1e-6 {
            return IDENTITY;
        }
        for v in &mut srgb_to_cam[i] {
            *v /= sum;
        }
    }
    invert3(&srgb_to_cam).unwrap_or(IDENTITY)
}

fn invert3(m: &[[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-9 {
        return None;
    }
    let k = 1.0 / det;
    Some([
        [
            (e * i - f * h) * k,
            (c * h - b * i) * k,
            (b * f - c * e) * k,
        ],
        [
            (f * g - d * i) * k,
            (a * i - c * g) * k,
            (c * d - a * f) * k,
        ],
        [
            (d * h - e * g) * k,
            (b * g - a * h) * k,
            (a * e - b * d) * k,
        ],
    ])
}

/// Turn and mirror a picture by its EXIF orientation number.
fn orient(px: Vec<u8>, w: usize, h: usize, o: u8) -> (usize, usize, Vec<u8>) {
    if o <= 1 || o > 8 {
        return (w, h, px);
    }
    let swap = o >= 5;
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let mut out = vec![0u8; px.len()];
    for y in 0..h {
        for x in 0..w {
            // Where the photosite at (x, y) of the sensor goes on the page.
            let (nx, ny) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                _ => (y, w - 1 - x), // 8
            };
            let from = (y * w + x) * 4;
            let to = (ny * ow + nx) * 4;
            out[to..to + 4].copy_from_slice(&px[from..from + 4]);
        }
    }
    (ow, oh, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rggb(row: usize, col: usize) -> usize {
        match (row % 2, col % 2) {
            (0, 0) => 0,
            (1, 1) => 2,
            _ => 1,
        }
    }

    fn mosaic<'a>(
        w: usize,
        h: usize,
        data: &'a [f32],
        cfa: &'a dyn Fn(usize, usize) -> usize,
    ) -> Mosaic<'a, f32> {
        Mosaic {
            width: w,
            height: h,
            cpp: 1,
            data,
            color_at: cfa,
            black: [0.0; 4],
            white: [1000.0; 4],
            wb: [1.0, 1.0, 1.0, f32::NAN],
            xyz_to_cam: [[0.0; 3]; 4],
            crops: [0; 4],
            orientation: 1,
        }
    }

    #[test]
    fn a_grey_scene_develops_grey() {
        // Every photosite at a fifth of white, through a Bayer filter, no
        // matrix: grey, brightened so its brightest hundredth is white.
        let data = vec![200.0; 8 * 8];
        let img = develop(&mosaic(8, 8, &data, &rggb)).unwrap();
        assert_eq!((img.width, img.height), (8, 8));
        for p in img.rgba8.as_chunks::<4>().0 {
            assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 255, "{p:?}");
            assert!(p[0] > 250, "brightened to white: {p:?}");
        }
    }

    #[test]
    fn a_red_scene_develops_red() {
        // Only the red photosites see light.
        let data: Vec<f32> = (0..64)
            .map(|i| if rggb(i / 8, i % 8) == 0 { 500.0 } else { 0.0 })
            .collect();
        let img = develop(&mosaic(8, 8, &data, &rggb)).unwrap();
        let p = &img.rgba8[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4];
        assert!(p[0] > 200 && p[1] < 20 && p[2] < 20, "{p:?}");
    }

    #[test]
    fn the_cameras_white_balance_neutralises_its_cast() {
        // Green photosites see twice what red and blue do — a scene under
        // a light the camera balanced by doubling red and blue.
        let data: Vec<f32> = (0..64)
            .map(|i| {
                if rggb(i / 8, i % 8) == 1 {
                    400.0
                } else {
                    200.0
                }
            })
            .collect();
        let mut m = mosaic(8, 8, &data, &rggb);
        m.wb = [2.0, 1.0, 2.0, f32::NAN];
        let img = develop(&m).unwrap();
        let p = &img.rgba8[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 3];
        assert!(
            p[0].abs_diff(p[1]) <= 2 && p[1].abs_diff(p[2]) <= 2,
            "{p:?}"
        );
    }

    #[test]
    fn where_the_sensor_clipped_is_white_not_magenta() {
        let data = vec![1000.0; 64];
        let mut m = mosaic(8, 8, &data, &rggb);
        m.wb = [2.4, 1.0, 1.6, f32::NAN];
        let img = develop(&m).unwrap();
        assert_eq!(&img.rgba8[..3], &[255, 255, 255]);
    }

    #[test]
    fn the_cameras_crop_and_turn_are_kept() {
        let data = vec![300.0; 10 * 6];
        let mut m = mosaic(10, 6, &data, &rggb);
        m.crops = [1, 2, 1, 0]; // 8 × 4 left
        m.orientation = 6; // a quarter turn: 4 × 8
        let img = develop(&m).unwrap();
        assert_eq!((img.width, img.height), (4, 8));
    }

    #[test]
    fn a_turn_puts_the_corner_where_it_belongs() {
        // A 2 × 1 picture, red then green, a quarter turn clockwise: red on top.
        let px = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let (w, h, out) = orient(px, 2, 1, 6);
        assert_eq!((w, h), (1, 2));
        assert_eq!(&out[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn a_camera_matrix_keeps_white_white() {
        // A real camera's numbers (a Canon's, from rawloader's tables).
        let m = camera_to_srgb(&[
            [4716.0, 603.0, -830.0],
            [-7798.0, 15474.0, 2480.0],
            [-1496.0, 1937.0, 6651.0],
            [0.0; 3],
        ]);
        for row in m {
            assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-4, "{row:?}");
        }
    }

    #[test]
    fn a_raw_file_is_told_by_its_name() {
        assert!(is_raw_name("IMG_0051.CR2") && is_raw_name("a.nef") && is_raw_name("x.dng"));
        assert!(!is_raw_name("photo.jpg") && !is_raw_name("scan.tif") && !is_raw_name("nef"));
    }

    #[test]
    fn something_that_is_not_a_raw_file_is_refused() {
        assert!(decode_raw(b"not a raw file at all").is_err());
    }

    /// A real camera's raw file, when one is named — none is committed
    /// (they are tens of megabytes): `CHITRAKAR_TEST_RAW=/path/to/file.cr2`.
    #[test]
    fn a_cameras_raw_file_develops_into_a_photograph() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_RAW") else {
            return;
        };
        let img = decode_raw(&std::fs::read(path).unwrap()).unwrap();
        assert!(img.width > 1000 && img.height > 1000);
        let n = (img.width * img.height) as f64;
        let mean: f64 = img
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
            .sum::<f64>()
            / n;
        assert!(
            (25.0..230.0).contains(&mean),
            "a photograph, not black or white: {mean}"
        );
    }

    #[test]
    fn a_cr3_says_what_will_work() {
        let mut cr3 = vec![0, 0, 0, 24];
        cr3.extend_from_slice(b"ftypcrx ");
        cr3.resize(64, 0);
        let said = decode_raw(&cr3).err().unwrap().to_string();
        assert!(said.contains("DNG Converter"), "{said}");
    }
}
