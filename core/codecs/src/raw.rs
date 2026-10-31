//! A camera's raw file, developed: what the sensor saw, before the camera
//! made a JPEG of it. `rawler` (dnglab's reader) reads the file — hundreds of cameras'
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

use crate::develop::RawPhoto;
use crate::{CodecError, SourceImage};

/// The extensions cameras write raw files under. A raw file is told by
/// its name first: many of them are TIFFs inside, and read as a TIFF they
/// give the small preview at their front rather than the photograph.
pub const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "dcr", "dcs", "dng", "drf", "eip",
    "erf", "fff", "gpr", "iiq", "k25", "kdc", "mdc", "mef", "mos", "mrw", "nef", "nrw", "orf",
    "ori", "pef", "ptx", "raf", "raw", "rw2", "rwl", "sr2", "srf", "srw", "sti", "x3f",
];

/// Whether a file's name says it is a camera's raw file.
pub fn is_raw_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| RAW_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The sensor's picture as the reader gives it, in the terms developing
/// it needs — so a mosaic can be made by hand in a test, without a file.
/// A sample as a mosaic holds it: a number, or the bytes of one as the
/// packed form has it (little-endian), read where they lie rather than
/// copied out first — forty million of them is 80 MB.
pub trait Sample: Copy {
    fn value(self) -> f32;
}

impl Sample for u16 {
    fn value(self) -> f32 {
        self as f32
    }
}

impl Sample for f32 {
    fn value(self) -> f32 {
        self
    }
}

impl Sample for [u8; 2] {
    fn value(self) -> f32 {
        u16::from_le_bytes(self) as f32
    }
}

impl Sample for [u8; 4] {
    fn value(self) -> f32 {
        f32::from_le_bytes(self)
    }
}

pub struct Mosaic<'a, T: Sample> {
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

/// A camera's raw file is read by its own module (`core/rawin`), which
/// hands the engine its mosaic packed (`unpack`), to be developed in the
/// develop window; on its own, by name, it is not a picture the codecs
/// can read.
pub fn decode_raw(_bytes: &[u8]) -> Result<SourceImage, CodecError> {
    Err(bad("opens in the develop window, which reads it"))
}

/// A mosaic as `chitrakar-raw-in` packs it (see that crate for the
/// form), kept to be developed. Every length is checked against the
/// bytes there are: the packing crosses from a worker, and a short or
/// garbled one is an error rather than a read past the end.
pub fn unpack(packed: &[u8]) -> Result<RawPhoto, CodecError> {
    const PATTERN: usize = 48;
    struct Cursor<'a>(&'a [u8], usize);
    impl<'a> Cursor<'a> {
        fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
            let got = self
                .1
                .checked_add(n)
                .and_then(|end| self.0.get(self.1..end));
            let got = got.ok_or_else(|| bad("the packed mosaic is shorter than it says"))?;
            self.1 += n;
            Ok(got)
        }
        fn u32s(&mut self, n: usize) -> Result<Vec<u32>, CodecError> {
            Ok(self
                .take(n * 4)?
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect())
        }
    }
    let mut cur = Cursor(packed, 0);
    if cur.take(4)? != b"CKRW" {
        return Err(bad("not a packed mosaic"));
    }
    let head = cur.u32s(5)?;
    let version = head[0];
    if !(1..=3).contains(&version) {
        return Err(bad(format!("a packed mosaic of version {version}")));
    }
    let (width, height, cpp, orientation) = (
        head[1] as usize,
        head[2] as usize,
        head[3] as usize,
        head[4] as u8,
    );
    let floats: Vec<f32> = cur
        .take(24 * 4)?
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let four = |i: usize| [floats[i], floats[i + 1], floats[i + 2], floats[i + 3]];
    let (black, white, wb) = (four(0), four(4), four(8));
    let mut xyz_to_cam = [[0f32; 3]; 4];
    for (i, v) in floats[12..24].iter().enumerate() {
        xyz_to_cam[i / 3][i % 3] = *v;
    }
    let crops = cur.u32s(4)?;
    let crops = [
        crops[0] as usize,
        crops[1] as usize,
        crops[2] as usize,
        crops[3] as usize,
    ];
    let pattern = cur.take(PATTERN * PATTERN)?.to_vec();
    let color_at =
        |row: usize, col: usize| pattern[(row % PATTERN) * PATTERN + col % PATTERN] as usize;
    let kind = cur.take(1)?[0];
    let count = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(cpp))
        .ok_or_else(|| bad("a sensor too big to be one"))?;
    macro_rules! with {
        ($data:expr) => {
            RawPhoto::from_mosaic(&Mosaic {
                width,
                height,
                cpp,
                data: $data,
                color_at: &color_at,
                black,
                white,
                wb,
                xyz_to_cam,
                crops,
                orientation,
            })
        };
    }
    let mut photo = match kind {
        0 => {
            let bytes = count
                .checked_mul(2)
                .ok_or_else(|| bad("a sensor too big to be one"))?;
            let (data, _) = cur.take(bytes)?.as_chunks::<2>();
            with!(data)
        }
        1 => {
            let bytes = count
                .checked_mul(4)
                .ok_or_else(|| bad("a sensor too big to be one"))?;
            let (data, _) = cur.take(bytes)?.as_chunks::<4>();
            with!(data)
        }
        k => Err(bad(format!("samples of kind {k}"))),
    }?;
    // Version 2: the lens corrections the file carries, if any.
    if version >= 2 {
        let n = cur.u32s(1)?[0] as usize;
        if n > 0 {
            if n > 64 {
                return Err(bad("more lens radii than a lens has"));
            }
            let floats: Vec<f32> = cur
                .take(n * 5 * 4)?
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let part = |i: usize| floats[i * n..(i + 1) * n].to_vec();
            photo.lens = Some(crate::develop::Lens {
                radii: part(0),
                distortion: part(1),
                red: part(2),
                blue: part(3),
                light: part(4),
            });
        }
    }
    // Version 3: a DNG's opcode lists, and where the photograph sits in
    // the area they speak of.
    if version >= 3 {
        let f = cur.u32s(4)?;
        let frame = crate::opcodes::Frame {
            left: f[0] as usize,
            top: f[1] as usize,
            width: f[2] as usize,
            height: f[3] as usize,
        };
        let mut list = || -> Result<Vec<crate::opcodes::Op>, CodecError> {
            let n = cur.u32s(1)?[0] as usize;
            Ok(crate::opcodes::parse(cur.take(n)?))
        };
        let (two, three) = (list()?, list()?);
        if frame.width > 0 && frame.height > 0 {
            if !two.is_empty() {
                crate::develop::mosaic_gains(&mut photo, &two, &frame);
                photo.remeasure();
            }
            if !three.is_empty() {
                photo.dng = Some((frame, three));
            }
        }
    }
    Ok(photo)
}

/// Develop a sensor's mosaic into 8-bit sRGB, plainly.
pub fn develop<T: Sample>(m: &Mosaic<T>) -> Result<SourceImage, CodecError> {
    Ok(RawPhoto::from_mosaic(m)?.develop(&chitrakar_doc::RawDevelop::default()))
}

/// From the camera's colours to linear sRGB: the inverse of sRGB → XYZ →
/// camera, each row of which is first made to sum to one so that a
/// white-balanced white stays white (dcraw's `cam_xyz_coeff`). A camera
/// with no matrix gets none.
pub(crate) fn camera_to_srgb(xyz_to_cam: &[[f32; 3]; 4]) -> [[f32; 3]; 3] {
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
pub(crate) fn orient(px: Vec<u8>, w: usize, h: usize, o: u8) -> (usize, usize, Vec<u8>) {
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
        // A real camera's numbers (a Canon's, from rawloader's tables, which rawler's agree with).
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
    fn a_packed_mosaic_unpacks_and_a_short_one_is_refused() {
        let packed = crate::develop::sample::packed(16, 8, 4095, [1.0; 3], |_, _| [0.3; 3]);
        let photo = unpack(&packed).unwrap();
        assert_eq!(photo.size(), (16, 8));
        for cut in [0, 3, 10, 100, packed.len() - 1] {
            assert!(unpack(&packed[..cut]).is_err(), "cut at {cut}");
        }
        let mut wrong = packed.clone();
        wrong[0] = b'X';
        assert!(unpack(&wrong).is_err());
    }
}
