//! Developing a camera's raw file with settings: the part of making a
//! photograph that can only be done before it is pixels.
//!
//! [`RawPhoto`] is the sensor's picture kept as the file gave it — the
//! black taken off, white at the top of sixteen bits, one value a
//! photosite — so it can be developed again, as often as asked, with
//! [`RawDevelop`]'s settings. What belongs here rather than in an
//! adjustment layer is what needs the sensor's own values:
//!
//! - **White balance** is a gain on each of the sensor's colours before
//!   the camera's matrix, which is where a light's colour is a single
//!   multiplication; done afterwards, on pixels already in sRGB, it is
//!   an approximation that tints the highlights. Asked for as a light's
//!   temperature in kelvin and a tint, as every raw developer asks: the
//!   colour of a light at that temperature (on the Planckian locus, by
//!   Kim's approximation) is carried into the camera's colours through its
//!   matrix, and the gains are what make that colour neutral.
//! - **Exposure and highlights** work on the values past white: the auto
//!   exposure puts the brightest hundredth of the picture at white, so
//!   there is a hundredth more above it that an 8-bit picture has
//!   already clipped, and highlights pulled down bring it back.
//! - The rest — contrast, shadows, whites, blacks, vibrance, saturation —
//!   are here so a photograph can leave the developer looking as meant,
//!   and are worked on brightness alone (a pixel's colour scaled, not
//!   each channel bent), so a curve does not shift a hue.
//!
//! The preview is the same arithmetic over a smaller picture: blocks of
//! photosites averaged a colour at a time ([`RawPreview`]), which skips the
//! demosaic and makes a slider's drag a matter of milliseconds. Both go
//! through [`Look::finish`], so the preview is the develop, smaller.

use crate::raw::{camera_to_srgb, orient, Mosaic};
use crate::{CodecError, SourceImage};
use chitrakar_doc::RawDevelop;

/// The tile a colour filter's pattern is read into: twelve each way is a
/// whole number of every pattern there is (2×2 Bayer, 6×6 X-Trans, and
/// the odd 2×4 and 3×3).
const TILE: usize = 12;

/// The sensor's picture, kept to be developed.
#[derive(Clone)]
pub struct RawPhoto {
    /// The size after the camera's crop, before its orientation.
    width: usize,
    height: usize,
    /// 1 for a mosaic, 3 for a raw already RGB.
    cpp: usize,
    /// Each sample, black off and white at 65535, cropped.
    data: Vec<u16>,
    /// The filter's colour at each site of a `TILE`-square tile, from the
    /// crop's corner: 0 red, 1 green, 2 blue (3, a second green, as 1).
    tile: [u8; TILE * TILE],
    /// Whether the pattern repeats every two sites both ways (Bayer).
    bayer: bool,
    /// The camera's white balance, green as one.
    as_shot: [f32; 3],
    /// XYZ to the camera's colours; sRGB's own when it has none.
    xyz_to_cam: [[f32; 3]; 3],
    to_srgb: [[f32; 3]; 3],
    orientation: u8,
    /// The gain that puts the brightest hundredth at white, under the
    /// camera's own balance — fixed for the photograph, so a change of
    /// white balance is not also a change of exposure.
    auto_gain: f32,
}

/// XYZ to linear sRGB's primaries, which stand in as the camera's matrix
/// for a camera nobody measured.
const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.240_454, -1.537_139, -0.498_531_4],
    [-0.969_266, 1.876_010_8, 0.041_556],
    [0.055_643_4, -0.204_025_9, 1.057_225_2],
];

fn bad(what: impl std::fmt::Display) -> CodecError {
    CodecError::Format(format!("camera raw: {what}"))
}

impl RawPhoto {
    /// Read a camera's raw file into a photograph to develop.
    pub fn read(bytes: &[u8]) -> Result<RawPhoto, CodecError> {
        crate::raw::read_photo(bytes)
    }

    /// Keep a mosaic to develop.
    pub fn from_mosaic<T: Copy + Into<f32>>(m: &Mosaic<T>) -> Result<RawPhoto, CodecError> {
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
        let mut tile = [1u8; TILE * TILE];
        if m.cpp == 1 {
            for r in 0..TILE {
                for c in 0..TILE {
                    let k = (m.color_at)(r + top, c + left).min(3);
                    tile[r * TILE + c] = if k == 3 { 1 } else { k as u8 };
                }
            }
        }
        let bayer = (0..TILE)
            .all(|r| (0..TILE).all(|c| tile[r * TILE + c] == tile[(r % 2) * TILE + c % 2]));
        let mut data = Vec::with_capacity(w * h * m.cpp);
        for y in top..top + h {
            for x in left..left + w {
                for s in 0..m.cpp {
                    let k = if m.cpp == 3 {
                        s
                    } else {
                        (m.color_at)(y, x).min(3)
                    };
                    let span = (m.white[k] - m.black[k]).max(1.0);
                    let v = (m.data[(y * m.width + x) * m.cpp + s].into() - m.black[k]) / span;
                    data.push((v.clamp(0.0, 1.0) * 65535.0).round() as u16);
                }
            }
        }
        let ok = |v: f32| v.is_finite() && v > 0.0;
        let g = if ok(m.wb[1]) { m.wb[1] } else { 1.0 };
        let one = |c: usize| if ok(m.wb[c]) { m.wb[c] / g } else { 1.0 };
        let measured = m.xyz_to_cam[..3].iter().flatten().any(|v| *v != 0.0);
        let xyz_to_cam = if measured {
            [m.xyz_to_cam[0], m.xyz_to_cam[1], m.xyz_to_cam[2]]
        } else {
            XYZ_TO_SRGB
        };
        let mut photo = RawPhoto {
            width: w,
            height: h,
            cpp: m.cpp,
            data,
            tile,
            bayer,
            as_shot: [one(0), 1.0, one(2)],
            xyz_to_cam,
            to_srgb: camera_to_srgb(&m.xyz_to_cam),
            orientation: m.orientation,
            auto_gain: 1.0,
        };
        photo.auto_gain = photo.measure_gain();
        Ok(photo)
    }

    /// The developed size, after the camera's orientation.
    pub fn size(&self) -> (u32, u32) {
        if self.orientation >= 5 && self.orientation <= 8 {
            (self.height as u32, self.width as u32)
        } else {
            (self.width as u32, self.height as u32)
        }
    }

    #[inline]
    fn color(&self, y: usize, x: usize) -> usize {
        self.tile[(y % TILE) * TILE + x % TILE] as usize
    }

    /// A sample, as a fraction of white.
    #[inline]
    fn sample(&self, y: usize, x: usize, s: usize) -> f32 {
        self.data[(y * self.width + x) * self.cpp + s] as f32 * (1.0 / 65535.0)
    }

    /// dcraw's exposure: the brightest hundredth at white, judged from
    /// every fourth site each way under the camera's own balance. Never
    /// darker than the sensor recorded, nor more than four stops brighter.
    fn measure_gain(&self) -> f32 {
        let wb = self.as_shot;
        let mut brightest = Vec::with_capacity((self.width / 4 + 1) * (self.height / 4 + 1));
        for y in (0..self.height).step_by(4) {
            for x in (0..self.width).step_by(4) {
                let c = mul3(&self.to_srgb, self.demosaic_at(y, x, &wb));
                brightest.push(c[0].max(c[1]).max(c[2]));
            }
        }
        if brightest.is_empty() {
            return 1.0;
        }
        let at = (((brightest.len() as f32) * 0.99) as usize).min(brightest.len() - 1);
        let (_, p99, _) = brightest.select_nth_unstable_by(at, f32::total_cmp);
        if *p99 > 1e-4 {
            (1.0 / *p99).clamp(1.0, 16.0)
        } else {
            1.0
        }
    }

    /// The three colours at a site, balanced and clipped: its own, and each
    /// missing one the average of that colour's sites in the 3×3 round it —
    /// or, where there are none (X-Trans), the 5×5. Clipped after the
    /// balance, so where the sensor clipped every colour is one and white
    /// through the matrix is white, rather than magenta.
    fn demosaic_at(&self, y: usize, x: usize, wb: &[f32; 3]) -> [f32; 3] {
        let site = |y: usize, x: usize, c: usize| {
            (self.sample(y, x, if self.cpp == 3 { c } else { 0 }) * wb[c]).min(1.0)
        };
        if self.cpp == 3 {
            return [site(y, x, 0), site(y, x, 1), site(y, x, 2)];
        }
        if self.bayer {
            return self.malvar_at(y, x, wb);
        }
        let own = self.color(y, x);
        let mut out = [0f32; 3];
        for (c, slot) in out.iter_mut().enumerate() {
            if c == own {
                *slot = site(y, x, c);
                continue;
            }
            for reach in [1isize, 2] {
                let (mut sum, mut n) = (0f32, 0u32);
                for dy in -reach..=reach {
                    let yy = y as isize + dy;
                    if yy < 0 || yy >= self.height as isize {
                        continue;
                    }
                    for dx in -reach..=reach {
                        let xx = x as isize + dx;
                        if xx < 0 || xx >= self.width as isize {
                            continue;
                        }
                        let (yy, xx) = (yy as usize, xx as usize);
                        if self.color(yy, xx) == c {
                            sum += site(yy, xx, c);
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
    }

    /// The three colours at a site of a Bayer sensor, by Malvar, He and
    /// Cutler's demosaic (2004): each missing colour is its neighbours'
    /// average corrected by how the site's own colour changes across
    /// them — the green channel's detail lent to red and blue, which is
    /// where a plain average loses it. Five by five, fixed weights, so it
    /// costs little more than the average and is as sharp as methods
    /// several times dearer. At the edges the sensor is mirrored two
    /// sites over, which keeps each site's colour.
    fn malvar_at(&self, y: usize, x: usize, wb: &[f32; 3]) -> [f32; 3] {
        let (h, w) = (self.height as isize, self.width as isize);
        let fold = |v: isize, n: isize| {
            let v = if v < 0 { -v } else { v };
            if v >= n {
                (2 * (n - 1) - v).max(0)
            } else {
                v
            }
        };
        let at = |dy: isize, dx: isize| -> f32 {
            let (yy, xx) = (
                fold(y as isize + dy, h) as usize,
                fold(x as isize + dx, w) as usize,
            );
            (self.sample(yy, xx, 0) * wb[self.color(yy, xx)]).min(1.0)
        };
        let c0 = at(0, 0);
        let cross1 = at(-1, 0) + at(1, 0) + at(0, -1) + at(0, 1);
        let cross2 = at(-2, 0) + at(2, 0) + at(0, -2) + at(0, 2);
        let diag1 = at(-1, -1) + at(-1, 1) + at(1, -1) + at(1, 1);
        let own = self.color(y, x);
        let clamp = |v: f32| v.clamp(0.0, 1.0);
        if own == 1 {
            // A green site: red and blue lie either side of it, one pair
            // along its row and the other along its column.
            let row = at(0, -1) + at(0, 1);
            let col = at(-1, 0) + at(1, 0);
            let row2 = at(0, -2) + at(0, 2);
            let col2 = at(-2, 0) + at(2, 0);
            let along_row = clamp((5.0 * c0 + 4.0 * row - diag1 - row2 + 0.5 * col2) / 8.0);
            let along_col = clamp((5.0 * c0 + 4.0 * col - diag1 - col2 + 0.5 * row2) / 8.0);
            let row_colour = self.color(y, fold(x as isize + 1, w) as usize);
            return if row_colour == 0 {
                [along_row, c0, along_col]
            } else {
                [along_col, c0, along_row]
            };
        }
        // A red or a blue site: green from the cross, the other colour
        // from the diagonals.
        let g = clamp((4.0 * c0 + 2.0 * cross1 - cross2) / 8.0);
        let other = clamp((6.0 * c0 + 2.0 * diag1 - 1.5 * cross2) / 8.0);
        if own == 0 {
            [c0, g, other]
        } else {
            [other, g, c0]
        }
    }

    /// Develop the whole photograph.
    pub fn develop(&self, s: &RawDevelop) -> SourceImage {
        let look = Look::new(self, s);
        let (w, h) = (self.width, self.height);
        let mut upright = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let c = look.finish(self.demosaic_at(y, x, &look.wb));
                upright[(y * w + x) * 4..(y * w + x) * 4 + 4]
                    .copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let upright = detail(&upright, w, h, s.sharpen, s.noise).unwrap_or(upright);
        let (width, height, rgba8) = orient(upright, w, h, self.orientation);
        SourceImage {
            width: width as u32,
            height: height as u32,
            rgba8,
        }
    }

    /// A square of the photograph at its own size — one sensor site a
    /// pixel — developed in full, Detail and all: what sharpening and
    /// noise reduction are judged on, since a preview made smaller shows
    /// neither. Centred at (`fx`, `fy`), fractions of the developed
    /// picture across and down, and held inside it.
    pub fn develop_square(&self, s: &RawDevelop, fx: f32, fy: f32, size: u32) -> SourceImage {
        let look = Look::new(self, s);
        let (uw, uh) = self.size();
        let size = (size as usize).min(self.width).min(self.height).max(1);
        let f = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.5
            }
        };
        let (ux, uy) = (
            ((f(fx) * uw as f32) as usize).min(uw as usize - 1),
            ((f(fy) * uh as f32) as usize).min(uh as usize - 1),
        );
        let (cx, cy) = unturn(ux, uy, self.width, self.height, self.orientation);
        // The square on the sensor, and three sites round it for Detail.
        let x0 = cx.saturating_sub(size / 2).min(self.width - size);
        let y0 = cy.saturating_sub(size / 2).min(self.height - size);
        const M: usize = 3;
        let (mx0, my0) = (x0.saturating_sub(M), y0.saturating_sub(M));
        let (mx1, my1) = (
            (x0 + size + M).min(self.width),
            (y0 + size + M).min(self.height),
        );
        let (mw, mh) = (mx1 - mx0, my1 - my0);
        let mut px = vec![0u8; mw * mh * 4];
        for y in 0..mh {
            for x in 0..mw {
                let c = look.finish(self.demosaic_at(my0 + y, mx0 + x, &look.wb));
                px[(y * mw + x) * 4..(y * mw + x) * 4 + 4]
                    .copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let px = detail(&px, mw, mh, s.sharpen, s.noise).unwrap_or(px);
        let mut square = Vec::with_capacity(size * size * 4);
        for y in (y0 - my0)..(y0 - my0 + size) {
            let from = (y * mw + (x0 - mx0)) * 4;
            square.extend_from_slice(&px[from..from + size * 4]);
        }
        let (width, height, rgba8) = orient(square, size, size, self.orientation);
        SourceImage {
            width: width as u32,
            height: height as u32,
            rgba8,
        }
    }

    /// A smaller picture to develop as a slider moves: blocks of sites
    /// averaged a colour at a time, the longer side about `longest`.
    pub fn preview(&self, longest: u32) -> RawPreview {
        let period = if self.cpp == 3 || self.bayer { 2 } else { 6 };
        let want = self
            .width
            .max(self.height)
            .div_ceil(longest.max(16) as usize)
            .max(1);
        let block = want.div_ceil(period) * period;
        let (pw, ph) = ((self.width / block).max(1), (self.height / block).max(1));
        let mut cam = Vec::with_capacity(pw * ph);
        for by in 0..ph {
            for bx in 0..pw {
                let (mut sum, mut n) = ([0f32; 3], [0u32; 3]);
                for y in by * block..((by + 1) * block).min(self.height) {
                    for x in bx * block..((bx + 1) * block).min(self.width) {
                        if self.cpp == 3 {
                            for c in 0..3 {
                                sum[c] += self.sample(y, x, c);
                                n[c] += 1;
                            }
                        } else {
                            let c = self.color(y, x);
                            sum[c] += self.sample(y, x, 0);
                            n[c] += 1;
                        }
                    }
                }
                cam.push([0, 1, 2].map(|c| if n[c] > 0 { sum[c] / n[c] as f32 } else { 0.0 }));
            }
        }
        RawPreview {
            width: pw,
            height: ph,
            cam,
        }
    }

    /// The gains on red, green and blue that make a light of `kelvin`
    /// and `tint` neutral, green as one.
    pub fn gains_for(&self, kelvin: f32, tint: f32) -> [f32; 3] {
        let neutral = self.neutral_of(kelvin);
        let k = 2f32.powf(tint.clamp(-150.0, 150.0) / 300.0);
        [
            neutral[1] / neutral[0] * k,
            1.0,
            neutral[1] / neutral[2] * k,
        ]
    }

    /// What the camera records of a light of `kelvin`, in its own colours.
    fn neutral_of(&self, kelvin: f32) -> [f32; 3] {
        let (x, y) = planckian_xy(kelvin);
        let xyz = [x / y, 1.0, (1.0 - x - y) / y];
        mul3(&self.xyz_to_cam, xyz).map(|v| v.max(1e-6))
    }

    /// The temperature and tint that give these gains, near enough: the
    /// one a light's temperature cannot say (red against blue) found along
    /// the locus, what is left over (both against green) said as tint.
    pub fn white_of(&self, gains: [f32; 3]) -> [f32; 2] {
        let off = |k: f32| {
            let g = self.gains_for(k, 0.0);
            let (er, eb) = ((gains[0] / g[0]).ln(), (gains[2] / g[2]).ln());
            ((er - eb) * 0.5, (er + eb) * 0.5)
        };
        // A coarse sweep in reciprocal temperature, then halving round
        // the best, since the error is not monotone for every camera.
        let (lo, hi) = (1.0 / 25_000.0f32, 1.0 / 2_000.0f32);
        let mut best = (f32::MAX, 6500.0);
        for i in 0..=96 {
            let k = 1.0 / (lo + (hi - lo) * i as f32 / 96.0);
            let e = off(k).0.abs();
            if e < best.0 {
                best = (e, k);
            }
        }
        let mut step = (hi - lo) / 96.0;
        let mut m = 1.0 / best.1;
        for _ in 0..24 {
            step *= 0.5;
            for cand in [m - step, m + step] {
                if (lo..=hi).contains(&cand) && off(1.0 / cand).0.abs() < off(1.0 / m).0.abs() {
                    m = cand;
                }
            }
        }
        let kelvin = 1.0 / m;
        let tint = off(kelvin).1 / std::f32::consts::LN_2 * 300.0;
        [kelvin.round(), tint.clamp(-150.0, 150.0)]
    }

    /// The camera's own white balance, as a temperature and tint.
    pub fn as_shot_white(&self) -> [f32; 2] {
        self.white_of(self.as_shot)
    }

    /// The white balance that makes the colour at (`x`, `y`) of a preview
    /// grey: what is there, three by three, taken as the light's colour.
    pub fn white_at(&self, preview: &RawPreview, x: u32, y: u32) -> Option<[f32; 2]> {
        let (x, y) = preview.unturn(x as usize, y as usize, self.orientation)?;
        let mut sum = [0f32; 3];
        for yy in y.saturating_sub(1)..(y + 2).min(preview.height) {
            for xx in x.saturating_sub(1)..(x + 2).min(preview.width) {
                let c = preview.cam[yy * preview.width + xx];
                for i in 0..3 {
                    sum[i] += c[i];
                }
            }
        }
        if sum.iter().any(|v| *v <= 1e-6) {
            return None;
        }
        Some(self.white_of([sum[1] / sum[0], 1.0, sum[1] / sum[2]]))
    }
}

/// Sharpening and noise reduction on a developed picture: what a raw
/// developer's Detail panel does, after the tone and colour. Worked in a
/// luma and two colour-difference channels, since a raw file's noise is
/// mostly colour blotches that can be smoothed hard without the picture
/// going soft, and sharpening wants the brightness alone — sharpen the
/// colour too and every edge gets a coloured halo.
///
/// - Noise reduction (0 to 100) does the two kinds differently. Colour
///   noise is blotches several pixels across, and the eye reads colour at
///   a fraction of the resolution it reads brightness, so the colour is
///   blurred outright — a box blur run twice (a tent), wider as the slider
///   goes up, costing the same however wide. Brightness noise is grain,
///   and blurring it would blur the picture, so it is an edge-aware
///   average of the three by three round each pixel: neighbours count by
///   how alike in brightness they are, so an edge stays where it is and
///   grain is averaged away.
/// - Sharpening (0 to 150) is an unsharp mask of radius one on the
///   brightness, after the smoothing.
///
/// Done in bands of rows, each read with the rows either side its filters
/// reach, so a 24-megapixel photograph costs a band's worth of working
/// space rather than three more copies of itself.
pub fn detail(rgba: &[u8], w: usize, h: usize, sharpen: f32, noise: f32) -> Option<Vec<u8>> {
    let sharpen = if sharpen.is_finite() {
        sharpen.clamp(0.0, 150.0) / 100.0
    } else {
        0.0
    };
    let noise = if noise.is_finite() {
        noise.clamp(0.0, 100.0) / 100.0
    } else {
        0.0
    };
    if (sharpen == 0.0 && noise == 0.0) || w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    const BAND: usize = 128;
    // How far the colour blur reaches: a tent of two boxes of this radius.
    let radius = if noise > 0.0 {
        1 + (noise * 3.0).round() as usize
    } else {
        0
    };
    let reach = 2 * radius + 2;
    let mut out = rgba[..w * h * 4].to_vec();
    // The likeness weights for brightness, tabled at a quarter of a level.
    let range = 2.0 + noise * 14.0;
    let like: Vec<f32> = (0..1024)
        .map(|i| {
            let d = i as f32 / 4.0;
            (-(d * d) / (2.0 * range * range)).exp()
        })
        .collect();
    for top in (0..h).step_by(BAND) {
        let bottom = (top + BAND).min(h);
        let (r0, r1) = (top.saturating_sub(reach), (bottom + reach).min(h));
        let rows = r1 - r0;
        let n = rows * w;
        let (mut y, mut cb, mut cr) = (vec![0f32; n], vec![0f32; n], vec![0f32; n]);
        for i in 0..n {
            let p = &rgba[(r0 * w + i) * 4..(r0 * w + i) * 4 + 3];
            let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
            let l = 0.299 * r + 0.587 * g + 0.114 * b;
            y[i] = l;
            cb[i] = b - l;
            cr[i] = r - l;
        }
        if noise > 0.0 {
            for ch in [&mut cb, &mut cr] {
                let blurred = tent(ch, w, rows, radius);
                for (v, b) in ch.iter_mut().zip(blurred) {
                    *v += (b - *v) * noise.sqrt();
                }
            }
            let src = y.clone();
            for ly in 0..rows {
                let (up, down) = (ly.saturating_sub(1), (ly + 1).min(rows - 1));
                for x in 0..w {
                    let (left, right) = (x.saturating_sub(1), (x + 1).min(w - 1));
                    let c = src[ly * w + x];
                    let (mut sum, mut weight) = (0f32, 0f32);
                    for (yy, ky) in [(up, 0.5f32), (ly, 1.0), (down, 0.5)] {
                        let row = &src[yy * w..yy * w + w];
                        for (xx, kx) in [(left, 0.5f32), (x, 1.0), (right, 0.5)] {
                            let v = row[xx];
                            let k = ky * kx * like[(((v - c).abs() * 4.0) as usize).min(1023)];
                            sum += k * v;
                            weight += k;
                        }
                    }
                    y[ly * w + x] = c + (sum / weight - c) * noise;
                }
            }
        }
        for py in top..bottom {
            let ly = py - r0;
            let (up, down) = (ly.saturating_sub(1), (ly + 1).min(rows - 1));
            for px in 0..w {
                let c = ly * w + px;
                let mut l = y[c];
                if sharpen > 0.0 {
                    let (left, right) = (px.saturating_sub(1), (px + 1).min(w - 1));
                    let mut blur = 0f32;
                    for (yy, ky) in [(up, 0.25f32), (ly, 0.5), (down, 0.25)] {
                        let row = &y[yy * w..yy * w + w];
                        blur += ky * (0.25 * row[left] + 0.5 * row[px] + 0.25 * row[right]);
                    }
                    l += (l - blur) * sharpen * 2.0;
                }
                let (b, r) = (cb[c] + l, cr[c] + l);
                let g = (l - 0.299 * r - 0.114 * b) / 0.587;
                let o = &mut out[(py * w + px) * 4..(py * w + px) * 4 + 3];
                o[0] = r.round().clamp(0.0, 255.0) as u8;
                o[1] = g.round().clamp(0.0, 255.0) as u8;
                o[2] = b.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Some(out)
}

/// A box blur of `radius` run twice each way over a `w` by `h` channel —
/// a tent, near enough a gaussian — with the edges held. A running sum,
/// so as cheap at radius four as at one.
fn tent(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let run = |line: &mut [f32], scratch: &mut Vec<f32>| {
        let n = line.len();
        scratch.clear();
        scratch.extend_from_slice(line);
        let at = |i: isize| scratch[i.clamp(0, n as isize - 1) as usize];
        let r = radius as isize;
        let k = 1.0 / (2 * r + 1) as f32;
        let mut sum: f32 = (-r..=r).map(at).sum();
        for (i, v) in line.iter_mut().enumerate() {
            *v = sum * k;
            let i = i as isize;
            sum += at(i + r + 1) - at(i - r);
        }
    };
    let mut out = src.to_vec();
    let mut scratch = Vec::new();
    for _ in 0..2 {
        for row in out.chunks_mut(w) {
            run(row, &mut scratch);
        }
        let mut column = vec![0f32; h];
        for x in 0..w {
            for y in 0..h {
                column[y] = out[y * w + x];
            }
            run(&mut column, &mut scratch);
            for y in 0..h {
                out[y * w + x] = column[y];
            }
        }
    }
    out
}

/// Settings worked out from the picture, as a raw developer's Auto: the
/// exposure that puts its log-average brightness at middle grey (within
/// two stops), highlights pulled down by how far past white that leaves
/// its brightest hundredth, and shadows lifted by how much of it is left
/// near black. White balance and colour are kept as they are, since they
/// say what the photograph is of rather than how it was exposed.
pub fn auto_tone(photo: &RawPhoto, preview: &RawPreview, s: &RawDevelop) -> RawDevelop {
    let look = Look::new(
        photo,
        &RawDevelop {
            white: s.white,
            ..Default::default()
        },
    );
    let mut ys: Vec<f32> = preview
        .cam
        .iter()
        .map(|c| {
            let c = mul3(
                &look.to_srgb,
                [0, 1, 2].map(|k| (c[k] * look.wb[k]).min(1.0)),
            );
            luma(c.map(|v| (v * look.gain).max(0.0)))
        })
        .collect();
    if ys.is_empty() {
        return *s;
    }
    let key = (ys.iter().map(|y| (y + 1e-4).ln()).sum::<f32>() / ys.len() as f32).exp();
    let ev = (0.18 / key).log2().clamp(-2.0, 2.0);
    let k = 2f32.powf(ev);
    let at = |ys: &mut Vec<f32>, q: f32| {
        let i = ((ys.len() as f32 * q) as usize).min(ys.len() - 1);
        *ys.select_nth_unstable_by(i, f32::total_cmp).1
    };
    let top = at(&mut ys, 0.99) * k;
    let highlights = if top > 1.0 {
        -(top.log2() * 60.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let dark = ys.iter().filter(|y| **y * k < 0.02).count() as f32 / ys.len() as f32;
    let shadows = (dark * 150.0).clamp(0.0, 60.0);
    RawDevelop {
        exposure: (ev * 20.0).round() / 20.0,
        highlights: highlights.round(),
        shadows: shadows.round(),
        contrast: 0.0,
        whites: 0.0,
        blacks: 0.0,
        ..*s
    }
}

/// A photograph's blocks, averaged a colour at a time, before balance.
pub struct RawPreview {
    width: usize,
    height: usize,
    cam: Vec<[f32; 3]>,
}

impl RawPreview {
    /// Develop the preview as `photo` would be developed with `s`.
    pub fn develop(&self, photo: &RawPhoto, s: &RawDevelop) -> SourceImage {
        let look = Look::new(photo, s);
        let mut upright = vec![0u8; self.width * self.height * 4];
        for (i, c) in self.cam.iter().enumerate() {
            let c = look.finish([0, 1, 2].map(|k| (c[k] * look.wb[k]).min(1.0)));
            upright[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
        let (width, height, rgba8) = orient(upright, self.width, self.height, photo.orientation);
        SourceImage {
            width: width as u32,
            height: height as u32,
            rgba8,
        }
    }

    /// Where a point of the upright preview was before the camera's turn.
    fn unturn(&self, x: usize, y: usize, o: u8) -> Option<(usize, usize)> {
        let (w, h) = (self.width, self.height);
        let (ow, oh) = if (5..=8).contains(&o) { (h, w) } else { (w, h) };
        if x >= ow || y >= oh {
            return None;
        }
        Some(unturn(x, y, w, h, o))
    }
}

/// Where a point of a picture turned upright by `orient` was before the
/// turn, in a `w` by `h` picture as it was: `orient`'s placing undone.
fn unturn(x: usize, y: usize, w: usize, h: usize, o: u8) -> (usize, usize) {
    match o {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => (x, y),
    }
}

/// The settings, made ready to apply a pixel at a time.
struct Look {
    wb: [f32; 3],
    to_srgb: [[f32; 3]; 3],
    gain: f32,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    vibrance: f32,
    saturation: f32,
    /// The brightness curve tabled against the square root of brightness
    /// (fine in the shadows, where the eye is), up to four times white;
    /// empty when the curve is the identity.
    curve: Vec<f32>,
    encode: chitrakar_color::Srgb8,
}

/// Steps in the tabled curve, and how far past white it reaches.
const CURVE_STEPS: usize = 2048;
const CURVE_REACH: f32 = 2.0; // the square root of four

impl Look {
    fn new(photo: &RawPhoto, s: &RawDevelop) -> Look {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(-100.0, 100.0) / 100.0
            } else {
                0.0
            }
        };
        let wb = match s.white {
            Some([k, t]) if k.is_finite() && t.is_finite() => {
                photo.gains_for(k.clamp(2000.0, 25_000.0), t)
            }
            _ => photo.as_shot,
        };
        let ev = if s.exposure.is_finite() {
            s.exposure.clamp(-5.0, 5.0)
        } else {
            0.0
        };
        let mut look = Look {
            wb,
            to_srgb: photo.to_srgb,
            gain: photo.auto_gain * 2f32.powf(ev),
            contrast: unit(s.contrast),
            highlights: unit(s.highlights),
            shadows: unit(s.shadows),
            whites: unit(s.whites),
            blacks: unit(s.blacks),
            vibrance: unit(s.vibrance),
            saturation: unit(s.saturation),
            curve: Vec::new(),
            encode: chitrakar_color::Srgb8::new(),
        };
        if !look.tones_untouched() {
            look.curve = (0..=CURVE_STEPS)
                .map(|i| {
                    let r = i as f32 / CURVE_STEPS as f32 * CURVE_REACH;
                    look.tone(r * r)
                })
                .collect();
        }
        look
    }

    /// The curve at `y`: from the table, or worked out past its end.
    #[inline]
    fn curve_at(&self, y: f32) -> f32 {
        let at = y.sqrt() * (CURVE_STEPS as f32 / CURVE_REACH);
        if at >= CURVE_STEPS as f32 {
            return self.tone(y);
        }
        let i = at as usize;
        let t = at - i as f32;
        self.curve[i] + (self.curve[i + 1] - self.curve[i]) * t
    }

    fn tones_untouched(&self) -> bool {
        [
            self.contrast,
            self.highlights,
            self.shadows,
            self.whites,
            self.blacks,
        ]
        .iter()
        .all(|v| *v == 0.0)
    }

    /// Balanced camera colours to an 8-bit sRGB pixel.
    fn finish(&self, cam: [f32; 3]) -> [u8; 3] {
        let mut c = mul3(&self.to_srgb, cam).map(|v| (v * self.gain).max(0.0));
        let y = luma(c);
        if y <= 0.0 {
            return [0, 0, 0];
        }
        if !self.curve.is_empty() {
            let y2 = self.curve_at(y);
            c = c.map(|v| v * (y2 / y));
        }
        let y = luma(c);
        if self.saturation != 0.0 || self.vibrance != 0.0 {
            let (hi, lo) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
            let colourful = if hi > 0.0 { (hi - lo) / hi } else { 0.0 };
            let k = ((1.0 + self.saturation) * (1.0 + self.vibrance * (1.0 - colourful))).max(0.0);
            c = c.map(|v| (y + (v - y) * k).max(0.0));
        }
        // A colour brighter than white goes to white by losing its colour,
        // as film does, rather than by losing a channel, which turns a
        // bright sky cyan and a face yellow.
        let hi = c[0].max(c[1]).max(c[2]);
        if hi > 1.0 {
            let y = luma(c);
            c = if y >= 1.0 {
                [1.0; 3]
            } else {
                let t = (hi - 1.0) / (hi - y);
                c.map(|v| v + (y - v) * t)
            };
        }
        c.map(|v| self.encode.channel(v.clamp(0.0, 1.0)))
    }

    /// The brightness curve, worked in a perceptual scale (a 2.2 power),
    /// where a stop at the bottom and a stop at the top are about as far.
    fn tone(&self, y: f32) -> f32 {
        let mut v = y.powf(1.0 / 2.2);
        // Highlights: pulled down, everything above the middle rolls off
        // and what was past white comes back under it; pushed up, the
        // upper tones brighten towards white.
        // At the far end the whole of what lies past white is brought
        // under it: the roll-off's limit is white itself.
        if self.highlights < 0.0 && v > 0.5 {
            let k = -self.highlights * 2.0;
            let x = v - 0.5;
            v = 0.5 + x / (1.0 + k * x);
        } else if self.highlights > 0.0 && (0.5..1.0).contains(&v) {
            let t = (v - 0.5) * 2.0;
            v += self.highlights * 0.15 * (std::f32::consts::PI * t).sin();
        }
        // Whites and blacks move the ends: where white is reached, and
        // where black stops.
        let white = 1.0 - self.whites * 0.25;
        let black = -self.blacks * 0.1;
        v = (v - black) / (white - black);
        // Shadows: the lower tones lifted or sunk, most at a third, none
        // at black or past the middle's far side.
        if self.shadows != 0.0 && (0.0..1.0).contains(&v) {
            let w = v * (1.0 - v) * (1.0 - v) * 6.75;
            v += self.shadows * 0.25 * w;
        }
        // Contrast: an S about middle grey (18%, as a light meter takes
        // it), the ends held; never so steep that the curve turns back.
        if self.contrast != 0.0 && (0.0..=1.0).contains(&v) {
            v += self.contrast * 1.8 * v * (1.0 - v) * (v - MIDDLE_GREY);
        }
        v.max(0.0).powf(2.2)
    }
}

/// 18% grey on the perceptual scale the curve is worked in.
const MIDDLE_GREY: f32 = 0.459_1;

#[inline]
fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[inline]
fn mul3(m: &[[f32; 3]; 3], c: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2])
}

/// The chromaticity of a black body at `kelvin`: Kim et al.'s cubic
/// approximation of the Planckian locus, good from 1667 K to 25000 K.
fn planckian_xy(kelvin: f32) -> (f32, f32) {
    let t = kelvin.clamp(1667.0, 25_000.0) as f64;
    let (t2, t3) = (t * t, t * t * t);
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / t3 - 0.234_358_9e6 / t2 + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / t3 + 2.107_037_9e6 / t2 + 0.222_634_7e3 / t + 0.240_390
    };
    let (x2, x3) = (x * x, x * x * x);
    let y = if t <= 2222.0 {
        -1.106_381_4 * x3 - 1.348_110_20 * x2 + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x3 - 1.374_185_93 * x2 + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x3 - 5.873_386_70 * x2 + 3.751_129_97 * x - 0.370_014_83
    };
    (x as f32, y as f32)
}

/// A DNG written by hand, for tests: a Bayer mosaic (RGGB) of 16-bit
/// samples whose camera is linear sRGB — its colour matrix is sRGB's — so
/// what comes out can be checked against what went in.
#[doc(hidden)]
pub mod sample {
    /// `scene` gives each pixel's linear red, green and blue as a fraction
    /// of white; `neutral` is the camera's as-shot neutral (what a grey
    /// read as, green as one), and the sensor records the scene times it.
    pub fn dng(
        w: u32,
        h: u32,
        white: u16,
        neutral: [f32; 3],
        scene: impl Fn(u32, u32) -> [f32; 3],
    ) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((w * h * 2) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = match (y % 2, x % 2) {
                    (0, 0) => 0,
                    (1, 1) => 2,
                    _ => 1,
                };
                let v = (scene(x, y)[c] * neutral[c]).clamp(0.0, 1.0) * white as f32;
                pixels.extend_from_slice(&(v.round() as u16).to_le_bytes());
            }
        }
        // XYZ to linear sRGB, as rationals over 10000.
        let matrix: [i32; 9] = [32405, -15371, -4985, -9693, 18760, 416, 556, -2040, 10572];
        let mut extra: Vec<u8> = Vec::new();
        let mut entries: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new();
        let short = |v: u16| v.to_le_bytes().to_vec();
        let long = |v: u32| v.to_le_bytes().to_vec();
        entries.push((254, 4, 1, long(0)));
        entries.push((256, 4, 1, long(w)));
        entries.push((257, 4, 1, long(h)));
        entries.push((258, 3, 1, short(16)));
        entries.push((259, 3, 1, short(1)));
        entries.push((262, 3, 1, short(32803)));
        entries.push((271, 2, 10, b"Chitrakar\0".to_vec()));
        entries.push((272, 2, 5, b"Test\0".to_vec()));
        entries.push((273, 4, 1, long(0))); // filled below
        entries.push((274, 3, 1, short(1)));
        entries.push((277, 3, 1, short(1)));
        entries.push((278, 4, 1, long(h)));
        entries.push((279, 4, 1, long(w * h * 2)));
        entries.push((33421, 3, 2, [short(2), short(2)].concat()));
        entries.push((33422, 1, 4, vec![0, 1, 1, 2]));
        entries.push((50706, 1, 4, vec![1, 4, 0, 0]));
        entries.push((50717, 4, 1, long(white as u32)));
        entries.push((
            50721,
            10,
            9,
            matrix
                .iter()
                .flat_map(|v| [v.to_le_bytes(), 10000i32.to_le_bytes()].concat())
                .collect(),
        ));
        entries.push((
            50728,
            5,
            3,
            neutral
                .iter()
                .flat_map(|v| {
                    [((v * 1e6) as u32).to_le_bytes(), 1_000_000u32.to_le_bytes()].concat()
                })
                .collect(),
        ));
        let ifd_len = 2 + entries.len() * 12 + 4;
        let extra_at = 8 + ifd_len;
        let mut ifd = (entries.len() as u16).to_le_bytes().to_vec();
        for (tag, kind, count, value) in &entries {
            ifd.extend_from_slice(&tag.to_le_bytes());
            ifd.extend_from_slice(&kind.to_le_bytes());
            ifd.extend_from_slice(&count.to_le_bytes());
            if value.len() <= 4 {
                let mut v = value.clone();
                v.resize(4, 0);
                ifd.extend_from_slice(&v);
            } else {
                ifd.extend_from_slice(&((extra_at + extra.len()) as u32).to_le_bytes());
                extra.extend_from_slice(value);
                if extra.len() % 2 == 1 {
                    extra.push(0);
                }
            }
        }
        ifd.extend_from_slice(&0u32.to_le_bytes());
        let pixels_at = (extra_at + extra.len()) as u32;
        // The strip's offset is the ninth entry's value.
        let strip = 2 + 8 * 12 + 8;
        ifd[strip..strip + 4].copy_from_slice(&pixels_at.to_le_bytes());
        let mut out = b"II*\0".to_vec();
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&ifd);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&pixels);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey_card(neutral: [f32; 3]) -> RawPhoto {
        // Half the frame a mid grey, half a light grey, through a camera
        // whose light made grey read as `neutral`.
        let dng = sample::dng(32, 16, 4095, neutral, |x, _| {
            if x < 16 {
                [0.18; 3]
            } else {
                [0.6; 3]
            }
        });
        RawPhoto::read(&dng).unwrap()
    }

    fn px(img: &SourceImage, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba8[i], img.rgba8[i + 1], img.rgba8[i + 2]]
    }

    #[test]
    fn a_dng_written_here_is_read_as_the_camera_wrote_it() {
        let photo = grey_card([0.5, 1.0, 0.7]);
        assert_eq!(photo.size(), (32, 16));
        assert_eq!(
            photo.as_shot.map(|v| (v * 100.0).round()),
            [200.0, 100.0, 143.0]
        );
        let img = photo.develop(&RawDevelop::default());
        let [r, g, b] = px(&img, 8, 8);
        assert!(
            r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2,
            "the camera's balance makes grey grey: {r} {g} {b}"
        );
    }

    #[test]
    fn the_preview_is_the_develop_smaller() {
        let photo = grey_card([0.5, 1.0, 0.7]);
        let s = RawDevelop {
            exposure: -1.0,
            contrast: 30.0,
            ..Default::default()
        };
        let full = photo.develop(&s);
        let small = photo.preview(16).develop(&photo, &s);
        assert_eq!((small.width, small.height), (16, 8));
        for (fx, sx) in [(4, 2), (28, 14)] {
            let (a, b) = (px(&full, fx, 8), px(&small, sx, 4));
            assert!(
                a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3),
                "{a:?} against {b:?}"
            );
        }
    }

    #[test]
    fn as_shot_comes_back_as_its_temperature() {
        // Whatever temperature the gains are made for, reading the gains
        // back gives that temperature.
        let photo = grey_card([0.5, 1.0, 0.7]);
        for k in [2850.0, 4000.0, 5500.0, 7500.0, 12000.0] {
            let [kk, t] = photo.white_of(photo.gains_for(k, 0.0));
            assert!(
                (kk - k).abs() / k < 0.02 && t.abs() < 1.0,
                "{k} came back as {kk} {t}"
            );
        }
        let [_, t] = photo.white_of(photo.gains_for(5000.0, 40.0));
        assert!((t - 40.0).abs() < 1.0, "and the tint: {t}");
    }

    #[test]
    fn a_warmer_temperature_is_a_warmer_picture() {
        let photo = grey_card([0.5, 1.0, 0.7]);
        let [k, t] = photo.as_shot_white();
        let warm = photo.develop(&RawDevelop {
            white: Some([k * 1.5, t]),
            ..Default::default()
        });
        let cool = photo.develop(&RawDevelop {
            white: Some([k / 1.5, t]),
            ..Default::default()
        });
        let (w, c) = (px(&warm, 8, 8), px(&cool, 8, 8));
        assert!(
            w[0] > w[2] + 10 && c[2] > c[0] + 10,
            "warm {w:?}, cool {c:?}"
        );
    }

    #[test]
    fn picking_grey_makes_it_grey() {
        let photo = grey_card([0.8, 1.0, 0.6]);
        // Start from a cast, then pick the mid grey.
        let preview = photo.preview(16);
        let white = photo.white_at(&preview, 3, 4).unwrap();
        let img = photo.develop(&RawDevelop {
            white: Some(white),
            ..Default::default()
        });
        let [r, g, b] = px(&img, 8, 8);
        assert!(
            r.abs_diff(g) <= 3 && g.abs_diff(b) <= 3,
            "{white:?} gives {r} {g} {b}"
        );
    }

    #[test]
    fn exposure_is_in_stops() {
        let photo = grey_card([1.0, 1.0, 1.0]);
        let at = |ev: f32| {
            let img = photo.develop(&RawDevelop {
                exposure: ev,
                ..Default::default()
            });
            chitrakar_color::srgb_to_linear(px(&img, 8, 8)[1] as f32 / 255.0)
        };
        let (base, down) = (at(0.0), at(-1.0));
        assert!(
            (base / down - 2.0).abs() < 0.1,
            "a stop down is half the light: {base} {down}"
        );
    }

    #[test]
    fn highlights_pulled_down_bring_back_what_was_past_white() {
        // The light grey is pushed past white by a stop and a half; pulled
        // highlights put it back under, still lighter than the mid grey.
        let photo = grey_card([1.0, 1.0, 1.0]);
        let over = RawDevelop {
            exposure: 1.5,
            ..Default::default()
        };
        let clipped = photo.develop(&over);
        assert_eq!(px(&clipped, 24, 8), [255; 3]);
        let back = photo.develop(&RawDevelop {
            highlights: -100.0,
            ..over
        });
        let (light, mid) = (px(&back, 24, 8)[1], px(&back, 8, 8)[1]);
        assert!(light < 250 && light > mid, "light {light}, mid {mid}");
    }

    #[test]
    fn the_tone_settings_keep_black_black_and_move_the_way_they_say() {
        let photo = grey_card([1.0, 1.0, 1.0]);
        let mid = |s: RawDevelop| px(&photo.develop(&s), 8, 8)[1];
        let base = mid(RawDevelop::default());
        assert!(
            mid(RawDevelop {
                shadows: 100.0,
                ..Default::default()
            }) > base
        );
        assert!(
            mid(RawDevelop {
                blacks: -100.0,
                ..Default::default()
            }) < base
        );
        assert!(
            mid(RawDevelop {
                whites: 100.0,
                ..Default::default()
            }) > base
        );
        // A shadow and a highlight either side of middle grey.
        let dng = sample::dng(32, 16, 4095, [1.0; 3], |x, _| {
            if x < 16 {
                [0.06; 3]
            } else {
                [0.6; 3]
            }
        });
        let two = RawPhoto::read(&dng).unwrap();
        let flat = RawDevelop {
            contrast: -100.0,
            ..Default::default()
        };
        let punchy = RawDevelop {
            contrast: 100.0,
            ..Default::default()
        };
        let spread = |s: RawDevelop| {
            let img = two.develop(&s);
            px(&img, 24, 8)[1] as i32 - px(&img, 8, 8)[1] as i32
        };
        assert!(
            spread(punchy) > spread(flat),
            "contrast spreads the two greys apart"
        );
    }

    #[test]
    fn saturation_to_nothing_is_grey() {
        let dng = sample::dng(16, 16, 4095, [1.0; 3], |_, _| [0.4, 0.2, 0.1]);
        let photo = RawPhoto::read(&dng).unwrap();
        let img = photo.develop(&RawDevelop {
            saturation: -100.0,
            ..Default::default()
        });
        let [r, g, b] = px(&img, 8, 8);
        assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "{r} {g} {b}");
    }

    #[test]
    fn auto_brings_a_dark_picture_up_and_a_bright_one_down() {
        // Most of the frame in deep shadow, a little light: Auto opens it up.
        let dark = sample::dng(32, 32, 4095, [1.0; 3], |x, y| {
            if x < 6 && y < 6 {
                [0.9; 3]
            } else {
                [0.002; 3]
            }
        });
        let photo = RawPhoto::read(&dark).unwrap();
        let s = auto_tone(&photo, &photo.preview(16), &RawDevelop::default());
        assert!(s.exposure > 0.5 && s.shadows > 0.0, "{s:?}");
        assert!(
            s.highlights < 0.0,
            "and pulls the light it pushed past white back: {s:?}"
        );
        // All bright: brought down.
        let bright = sample::dng(32, 32, 4095, [1.0; 3], |_, _| [0.9; 3]);
        let photo = RawPhoto::read(&bright).unwrap();
        let s = auto_tone(
            &photo,
            &photo.preview(16),
            &RawDevelop {
                white: Some([5000.0, 3.0]),
                saturation: 20.0,
                ..Default::default()
            },
        );
        assert!(s.exposure < -0.5, "{s:?}");
        assert_eq!(
            (s.white, s.saturation),
            (Some([5000.0, 3.0]), 20.0),
            "colour is kept"
        );
    }

    /// A grey of `level` with noise of `spread` levels, and a hard edge
    /// down the middle of it.
    fn noisy(level: u8, spread: u8) -> Vec<u8> {
        let mut seed = 7u32;
        let mut out = Vec::new();
        for _y in 0..40 {
            for x in 0..40 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let jitter = (seed >> 16) as i32 % (spread as i32 * 2 + 1) - spread as i32;
                let base = if x < 20 {
                    level as i32
                } else {
                    level as i32 + 80
                };
                let v = (base + jitter).clamp(0, 255) as u8;
                // Colour noise too: the channels jitter apart.
                out.extend_from_slice(&[v, (base - jitter / 2).clamp(0, 255) as u8, v, 255]);
            }
        }
        out
    }

    fn spread_of(px: &[u8], x0: usize, x1: usize) -> f32 {
        let vals: Vec<f32> = (5..35)
            .flat_map(|y| (x0..x1).map(move |x| (y, x)))
            .map(|(y, x)| px[(y * 40 + x) * 4 + 1] as f32)
            .collect();
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        (vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / vals.len() as f32).sqrt()
    }

    #[test]
    fn noise_reduction_smooths_grain_and_keeps_the_edge() {
        let px = noisy(80, 12);
        let smooth = detail(&px, 40, 40, 0.0, 80.0).unwrap();
        assert!(
            spread_of(&smooth, 3, 16) < spread_of(&px, 3, 16) * 0.6,
            "grain smoothed"
        );
        // Across the edge it is still an edge: the two sides as far apart.
        let side = |p: &[u8], x: usize| {
            (5..35).map(|y| p[(y * 40 + x) * 4 + 1] as f32).sum::<f32>() / 30.0
        };
        assert!(
            side(&smooth, 22) - side(&smooth, 17) > 70.0,
            "the edge kept"
        );
        assert!(
            detail(&px, 40, 40, 0.0, 0.0).is_none(),
            "nothing asked, nothing done"
        );
    }

    #[test]
    fn sharpening_steepens_an_edge_and_leaves_flat_alone() {
        let mut px = Vec::new();
        for _y in 0..40 {
            for x in 0..40 {
                let v = if x < 20 { 80 } else { 160 };
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let sharp = detail(&px, 40, 40, 100.0, 0.0).unwrap();
        let at = |x: usize| sharp[(20 * 40 + x) * 4 + 1];
        assert!(
            at(19) < 80 && at(20) > 160,
            "an overshoot either side of the edge: {} {}",
            at(19),
            at(20)
        );
        assert_eq!((at(5), at(35)), (80, 160), "flat ground untouched");
        // Grey stays grey: only the brightness is sharpened.
        let p = &sharp[(20 * 40 + 20) * 4..(20 * 40 + 20) * 4 + 3];
        assert!(
            p[0].abs_diff(p[1]) <= 1 && p[1].abs_diff(p[2]) <= 1,
            "{p:?}"
        );
    }

    #[test]
    fn detail_in_bands_is_detail_all_at_once() {
        // Taller than a band: the seams must not show.
        let (w, h) = (24usize, 300usize);
        let mut seed = 3u32;
        let px: Vec<u8> = (0..w * h * 4)
            .map(|i| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                if i % 4 == 3 {
                    255
                } else {
                    (seed >> 24) as u8
                }
            })
            .collect();
        let whole = detail(&px, w, h, 80.0, 60.0).unwrap();
        // Rows either side of the first seam, worked out on their own.
        let piece = &px[(100 * w * 4)..(160 * w * 4)];
        let part = detail(piece, w, 60, 80.0, 60.0).unwrap();
        for y in 10..50 {
            for x in 0..w * 4 {
                assert_eq!(
                    whole[(100 + y) * w * 4 + x],
                    part[y * w * 4 + x],
                    "row {}",
                    100 + y
                );
            }
        }
    }

    #[test]
    fn a_square_at_its_own_size_is_the_develop_there() {
        let photo = grey_card([0.8, 1.0, 0.6]);
        let s = RawDevelop {
            exposure: 0.5,
            ..Default::default()
        };
        let full = photo.develop(&s);
        let sq = photo.develop_square(&s, 0.75, 0.5, 8);
        assert_eq!((sq.width, sq.height), (8, 8));
        // Centred on (24, 8): its top left is (20, 4).
        for (x, y) in [(0, 0), (7, 7), (3, 4)] {
            let a = &full.rgba8[(((4 + y) * 32 + 20 + x) * 4) as usize..][..4];
            let b = &sq.rgba8[((y * 8 + x) * 4) as usize..][..4];
            assert_eq!(a, b, "at {x},{y}");
        }
        // Held inside the picture at the corner.
        let corner = photo.develop_square(&s, 1.0, 1.0, 8);
        assert_eq!((corner.width, corner.height), (8, 8));
    }

    #[test]
    fn nonsense_settings_are_held_to_what_they_can_be() {
        let photo = grey_card([1.0, 1.0, 1.0]);
        let s = RawDevelop {
            white: Some([f32::NAN, 1e9]),
            exposure: 1e9,
            contrast: f32::INFINITY,
            ..Default::default()
        };
        let img = photo.develop(&s);
        assert_eq!((img.width, img.height), (32, 16));
    }
}
