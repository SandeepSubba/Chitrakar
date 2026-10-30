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

use crate::raw::{camera_to_srgb, orient, Mosaic, Sample};
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
    /// For a pattern that is not Bayer (X-Trans): at each place in the
    /// tile and for each colour, the neighbours a site there averages for
    /// it — the 3×3's sites of that colour, or the 5×5's where the 3×3 has
    /// none. Worked out once rather than searched for at every site.
    taps: Vec<Vec<(i8, i8)>>,
    /// X-Trans's own demosaic, worked out once for the tile: at each
    /// place that is not green, for each of the four directions through
    /// it (across, down, and the two diagonals), how many sites away the
    /// nearest green is on either side (nought where it is more than
    /// three); and at each place, for red and for blue, the 5×5's sites
    /// of that colour. Empty for Bayer.
    xt_green: Vec<[(u8, u8); 4]>,
    xt_near: Vec<Vec<(i8, i8)>>,
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
    /// The lens corrections the camera wrote into the file, if any.
    pub(crate) lens: Option<Lens>,
}

/// A lens's corrections as the camera wrote them for one photograph (a
/// Fuji's RAF; `core/rawin` reads them): at each radius from the middle,
/// a fraction of the half-diagonal, the distortion in per cent, red's and
/// blue's scale against green as fractions, and the light that reaches
/// there in per cent of the middle's. Applied the way darktable applies
/// them: the light made even where the sensor caught it, then each point
/// of the picture taken from `1 + distortion/100` times as far out as it
/// lands (red and blue a little more or less), zoomed in just enough
/// that no corner is left with nothing in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Lens {
    pub radii: Vec<f32>,
    pub distortion: Vec<f32>,
    pub red: Vec<f32>,
    pub blue: Vec<f32>,
    pub light: Vec<f32>,
}

/// How finely the lens's curves are tabled, over the square of the radius
/// (so that a pixel needs no square root to look one up).
const LENS_STEPS: usize = 1024;

impl Lens {
    /// Each list as long as the radii, the radii rising, every value
    /// finite: what is worth applying.
    fn usable(&self) -> bool {
        let n = self.radii.len();
        n >= 2
            && [&self.distortion, &self.red, &self.blue, &self.light]
                .iter()
                .all(|v| v.len() == n)
            && self.radii.windows(2).all(|w| w[0] < w[1])
            && [
                &self.radii,
                &self.distortion,
                &self.red,
                &self.blue,
                &self.light,
            ]
            .iter()
            .all(|v| v.iter().all(|x| x.is_finite()))
    }

    /// One of the curves at radius `r`, straight between the radii, held
    /// at the ends — from `at_nought` at the middle where the first
    /// radius is past it.
    fn at(&self, values: &[f32], at_nought: f32, r: f32) -> f32 {
        let (xs, ys) = (&self.radii, values);
        if r <= xs[0] {
            return if xs[0] > 0.0 {
                at_nought + (ys[0] - at_nought) * (r / xs[0]).max(0.0)
            } else {
                ys[0]
            };
        }
        let i = xs.partition_point(|&x| x <= r);
        if i >= xs.len() {
            return ys[ys.len() - 1];
        }
        let t = (r - xs[i - 1]) / (xs[i] - xs[i - 1]);
        ys[i - 1] + (ys[i] - ys[i - 1]) * t
    }

    /// What brightens a site whose radius squared is `rr`, tabled over
    /// `rr` from nought to one (the corner) and a little past.
    fn light_table(&self) -> Vec<f32> {
        (0..=LENS_STEPS)
            .map(|i| {
                let r = (i as f32 / LENS_STEPS as f32 * 1.25).sqrt();
                100.0 / self.at(&self.light, 100.0, r).max(10.0)
            })
            .collect()
    }

    /// The warp: for each channel, how many times as far out as a point
    /// lands its source is, tabled over the landing radius squared; and
    /// the zoom that keeps a picture `w` by `h` full to its edges. `None`
    /// where the curve folds back on itself, which no lens does.
    fn warp(&self, w: f32, h: f32) -> Option<Warp> {
        // From where a point comes from (a radius in the source) to where
        // it lands: `rin / m`, `m = 1 + distortion / 100`.
        const FINE: usize = 512;
        let mut lands = Vec::with_capacity(FINE + 1);
        for k in 0..=FINE {
            let rin = k as f32 / FINE as f32 * 1.2;
            let m = 1.0 + self.at(&self.distortion, 0.0, rin) / 100.0;
            if m <= 0.0 {
                return None;
            }
            let scale = [
                m * (1.0 + self.at(&self.red, 0.0, rin)),
                m,
                m * (1.0 + self.at(&self.blue, 0.0, rin)),
            ];
            lands.push((rin / m, scale));
        }
        if lands.windows(2).any(|w| w[1].0 <= w[0].0) {
            return None;
        }
        let most = lands[FINE].0 * lands[FINE].0;
        // Read back over where points land, by the radius squared.
        let mut table = Vec::with_capacity(LENS_STEPS + 1);
        let mut k = 0;
        for i in 0..=LENS_STEPS {
            let rr = i as f32 / LENS_STEPS as f32 * most;
            let r = rr.sqrt();
            while k + 1 < FINE && lands[k + 1].0 < r {
                k += 1;
            }
            let (a, b) = (lands[k], lands[(k + 1).min(FINE)]);
            let t = if b.0 > a.0 {
                ((r - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            table.push([0, 1, 2].map(|c| a.1[c] + (b.1[c] - a.1[c]) * t));
        }
        let mut warp = Warp {
            table,
            per_rr: LENS_STEPS as f32 / most,
            half_diagonal: (w * w + h * h).sqrt() * 0.5,
            zoom: 1.0,
        };
        // The least zoom that keeps every edge's source inside the
        // picture, found by halving.
        let fits = |warp: &Warp| {
            let (hw, hh) = (w * 0.5, h * 0.5);
            (0..=64).all(|i| {
                let f = i as f32 / 64.0;
                [
                    (-hw + f * w, -hh),
                    (-hw + f * w, hh),
                    (-hw, -hh + f * h),
                    (hw, -hh + f * h),
                ]
                .iter()
                .all(|&(x, y)| {
                    (0..3).all(|c| {
                        let (sx, sy) = warp.source(x, y, c);
                        sx.abs() <= hw + 0.5 && sy.abs() <= hh + 0.5
                    })
                })
            })
        };
        if !fits(&warp) {
            let (mut lo, mut hi) = (1.0f32, 2.0f32);
            warp.zoom = hi;
            if !fits(&warp) {
                return None;
            }
            for _ in 0..30 {
                warp.zoom = 0.5 * (lo + hi);
                if fits(&warp) {
                    hi = warp.zoom;
                } else {
                    lo = warp.zoom;
                }
            }
            warp.zoom = hi;
        }
        Some(warp)
    }
}

/// Brighten the sites of a region (rows `y0..y1`, columns `x0..x1`, as
/// `region` lays them out) of a photograph `w` by `h` by the lens's light
/// table, each by how far it is from the middle.
fn brighten(
    light: &[f32],
    sites: &mut [[f32; 3]],
    (w, h): (usize, usize),
    (y0, y1, x0, x1): (usize, usize, usize, usize),
) {
    let (hw, hh) = (w as f32 * 0.5, h as f32 * 0.5);
    let per_rr = LENS_STEPS as f32 / 1.25 / (hw * hw + hh * hh);
    let width = x1 - x0;
    for (row, y) in sites.chunks_mut(width.max(1)).zip(y0..y1) {
        let dy = y as f32 + 0.5 - hh;
        for (px, x) in row.iter_mut().zip(x0..x1) {
            let dx = x as f32 + 0.5 - hw;
            let f = ((dx * dx + dy * dy) * per_rr).min(LENS_STEPS as f32);
            let i = (f as usize).min(LENS_STEPS - 1);
            let gain = light[i] + (light[i + 1] - light[i]) * (f - i as f32);
            for v in px.iter_mut() {
                *v *= gain;
            }
        }
    }
}

/// [`Lens::warp`] made ready: for a point of the corrected picture, where
/// in the photograph each channel of it comes from.
struct Warp {
    /// Per channel, how many times as far out the source is, over the
    /// landing radius squared (in half-diagonals) times `per_rr`.
    table: Vec<[f32; 3]>,
    per_rr: f32,
    half_diagonal: f32,
    zoom: f32,
}

impl Warp {
    /// The source of channel `c` at (`x`, `y`) from the middle, in pixels
    /// from the middle.
    #[inline]
    fn source(&self, x: f32, y: f32, c: usize) -> (f32, f32) {
        let s = self.scales(x, y)[c];
        (x / self.zoom * s, y / self.zoom * s)
    }

    #[inline]
    fn scales(&self, x: f32, y: f32) -> [f32; 3] {
        let (qx, qy) = (x / self.zoom, y / self.zoom);
        let rr = (qx * qx + qy * qy) / (self.half_diagonal * self.half_diagonal);
        let f = (rr * self.per_rr).min(LENS_STEPS as f32);
        let i = (f as usize).min(LENS_STEPS - 1);
        let t = f - i as f32;
        let (a, b) = (self.table[i], self.table[i + 1]);
        [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * t)
    }
}

/// Turn a picture upright by its EXIF orientation (as `orient` does) and,
/// where there is a `warp`, take each point from where the lens put it,
/// red, green and blue each from its own place, between pixels by their
/// four neighbours.
fn orient_through(
    px: Vec<u8>,
    w: usize,
    h: usize,
    o: u8,
    warp: Option<&Warp>,
) -> (usize, usize, Vec<u8>) {
    let Some(warp) = warp else {
        return orient(px, w, h, o);
    };
    let swap = (5..=8).contains(&o);
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let mut out = vec![0u8; px.len()];
    let (hw, hh) = (w as f32 * 0.5, h as f32 * 0.5);
    let inv_zoom = 1.0 / warp.zoom;
    let per_rr = warp.per_rr / (warp.half_diagonal * warp.half_diagonal);
    // Source positions in 256ths of a pixel, kept a pixel short of the
    // far edges so that the four neighbours are always there.
    let (most_x, most_y) = (((w - 1) * 256) as f32, ((h - 1) * 256) as f32);
    let (hw256, hh256) = ((hw - 0.5) * 256.0, (hh - 0.5) * 256.0);
    let (right, down) = (if w > 1 { 4 } else { 0 }, if h > 1 { w * 4 } else { 0 });
    // Where (x, y) of the photograph goes in the turned picture, as a
    // start for each row and a step along it.
    let place = |x: usize, y: usize| match o {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (h - 1 - y, x),
        7 => (h - 1 - y, w - 1 - x),
        8 => (y, w - 1 - x),
        _ => (x, y),
    };
    let index = |(nx, ny): (usize, usize)| (ny * ow + nx) as isize * 4;
    let step = if w > 1 {
        index(place(1, 0)) - index(place(0, 0))
    } else {
        0
    };
    // Walked in the photograph's own rows, so that what each point reads
    // is near what the last one read; written where `orient` puts it.
    for y in 0..h {
        let qy = (y as f32 + 0.5 - hh) * inv_zoom;
        let qyy = qy * qy;
        let mut to = index(place(0, y));
        for x in 0..w {
            let qx = (x as f32 + 0.5 - hw) * inv_zoom;
            let f = ((qx * qx + qyy) * per_rr).min(LENS_STEPS as f32 - 0.001);
            let i = f as usize;
            let t = f - i as f32;
            let (a, b) = (warp.table[i], warp.table[i + 1]);
            let at_out = to as usize;
            for c in 0..3 {
                let k = (a[c] + (b[c] - a[c]) * t) * 256.0;
                // Pixel centres are at the halves.
                let fx = (qx * k + hw256).clamp(0.0, most_x) as u32;
                let fy = (qy * k + hh256).clamp(0.0, most_y) as u32;
                let (x0, y0) = ((fx >> 8) as usize, (fy >> 8) as usize);
                let (tx, ty) = (fx & 255, fy & 255);
                let r = if x0 + 1 < w { right } else { 0 };
                let d = if y0 + 1 < h { down } else { 0 };
                let at = (y0 * w + x0) * 4 + c;
                let top = px[at] as u32 * (256 - tx) + px[at + r] as u32 * tx;
                let bottom = px[at + d] as u32 * (256 - tx) + px[at + d + r] as u32 * tx;
                out[at_out + c] = ((top * (256 - ty) + bottom * ty + 32768) >> 16) as u8;
            }
            out[at_out + 3] = 255;
            to += step;
        }
    }
    (ow, oh, out)
}

/// XYZ to linear sRGB's primaries, which stand in as the camera's matrix
/// for a camera nobody measured.
const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.240_454, -1.537_139, -0.498_531_4],
    [-0.969_266, 1.876_010_8, 0.041_556],
    [0.055_643_4, -0.204_025_9, 1.057_225_2],
];

/// For each place in a tile and each colour, the offsets of the sites of
/// that colour in the 3×3 round it, or in the 5×5 where the 3×3 has none,
/// in the order `demosaic_at` visits them (row by row).
fn taps_of(tile: &[u8; TILE * TILE]) -> Vec<Vec<(i8, i8)>> {
    let colour = |y: isize, x: isize| {
        tile[(y.rem_euclid(TILE as isize) as usize) * TILE + x.rem_euclid(TILE as isize) as usize]
            as usize
    };
    let mut out = Vec::with_capacity(TILE * TILE * 3);
    for y in 0..TILE as isize {
        for x in 0..TILE as isize {
            for c in 0..3 {
                let mut taps = Vec::new();
                for reach in [1isize, 2] {
                    for dy in -reach..=reach {
                        for dx in -reach..=reach {
                            if colour(y + dy, x + dx) == c {
                                taps.push((dy as i8, dx as i8));
                            }
                        }
                    }
                    if !taps.is_empty() {
                        break;
                    }
                }
                out.push(taps);
            }
        }
    }
    out
}

/// The directions X-Trans's green is looked for along: across, down,
/// and the two diagonals.
const DIRECTIONS: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];

/// For each place in a tile, for each of [`DIRECTIONS`], the steps to the
/// nearest green ahead and behind, within three; (0, 0) where there is
/// none on one side or the place is green itself.
fn greens_of(tile: &[u8; TILE * TILE]) -> Vec<[(u8, u8); 4]> {
    let colour = |y: isize, x: isize| {
        tile[(y.rem_euclid(TILE as isize) as usize) * TILE + x.rem_euclid(TILE as isize) as usize]
    };
    let mut out = Vec::with_capacity(TILE * TILE);
    for y in 0..TILE as isize {
        for x in 0..TILE as isize {
            out.push(DIRECTIONS.map(|(dy, dx)| {
                if colour(y, x) == 1 {
                    return (0, 0);
                }
                let first = |sign: isize| {
                    (1..=3)
                        .find(|k| colour(y + sign * dy * k, x + sign * dx * k) == 1)
                        .unwrap_or(0) as u8
                };
                match (first(1), first(-1)) {
                    (0, _) | (_, 0) => (0, 0),
                    found => found,
                }
            }));
        }
    }
    out
}

/// For each place in a tile and each colour, the 5×5's sites of that
/// colour, the place itself left out.
fn near_of(tile: &[u8; TILE * TILE]) -> Vec<Vec<(i8, i8)>> {
    let colour = |y: isize, x: isize| {
        tile[(y.rem_euclid(TILE as isize) as usize) * TILE + x.rem_euclid(TILE as isize) as usize]
            as usize
    };
    let mut out = Vec::with_capacity(TILE * TILE * 3);
    for y in 0..TILE as isize {
        for x in 0..TILE as isize {
            for c in 0..3 {
                let mut near = Vec::new();
                for dy in -2isize..=2 {
                    for dx in -2isize..=2 {
                        if (dy, dx) != (0, 0) && colour(y + dy, x + dx) == c {
                            near.push((dy as i8, dx as i8));
                        }
                    }
                }
                out.push(near);
            }
        }
    }
    out
}

fn bad(what: impl std::fmt::Display) -> CodecError {
    CodecError::Format(format!("camera raw: {what}"))
}

/// The sites of rows `ry0..ry1`, columns `rx0..rx1`, each as its index in
/// planes `stride` wide whose first is at `base`, its place in the tile,
/// and its colour.
fn each_site(
    tile: &[u8; TILE * TILE],
    base: usize,
    stride: usize,
    (ry0, ry1, rx0, rx1): (usize, usize, usize, usize),
) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    (ry0..ry1).flat_map(move |y| {
        let tile_row = (y % TILE) * TILE;
        let row = base + (y - ry0) * stride;
        let first = rx0 % TILE;
        (0..rx1.saturating_sub(rx0)).map(move |k| {
            let p = tile_row + (first + k) % TILE;
            (row + k, p, tile[p] as usize)
        })
    })
}

/// X-Trans's working space for a region (`RawPhoto::region`): each
/// site's balanced value, the first green, red and blue at the greens,
/// and the second green, each over the region and the sites round it.
#[derive(Default)]
struct Planes {
    site: Vec<f32>,
    green: Vec<f32>,
    red: Vec<f32>,
    blue: Vec<f32>,
    second: Vec<f32>,
}

impl RawPhoto {
    /// Keep a mosaic to develop.
    pub fn from_mosaic<T: Sample>(m: &Mosaic<T>) -> Result<RawPhoto, CodecError> {
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
        // Black off and white at 65535, a colour's levels each. The
        // filter's colours are read once for a square the pattern repeats
        // within (the packed form's 48) rather than asked of `color_at` at
        // every one of forty million sites; a raw already RGB takes each
        // sample's colour from its place in the site.
        // Rounded by adding a half and cutting, which for what is never
        // negative is `round` without its library call — the call kept the
        // loop from being vectorised, and was most of its time.
        let scale = [0, 1, 2, 3].map(|k| 65535.0 / (m.white[k] - m.black[k]).max(1.0));
        let level =
            |v: f32, k: usize| (((v - m.black[k]) * scale[k]).clamp(0.0, 65535.0) + 0.5) as u16;
        const SQUARE: usize = 48;
        let mut data = Vec::with_capacity(w * h * m.cpp);
        if m.cpp == 3 {
            for y in top..top + h {
                let row = &m.data[(y * m.width + left) * 3..(y * m.width + left + w) * 3];
                data.extend(row.iter().enumerate().map(|(i, v)| level(v.value(), i % 3)));
            }
        } else {
            let mut kinds = [0u8; SQUARE * SQUARE];
            for (i, k) in kinds.iter_mut().enumerate() {
                *k = (m.color_at)(top + i / SQUARE, left + i % SQUARE).min(3) as u8;
            }
            for (r, y) in (top..top + h).enumerate() {
                let along = &kinds[(r % SQUARE) * SQUARE..][..SQUARE];
                let row = &m.data[y * m.width + left..y * m.width + left + w];
                for part in row.chunks(SQUARE) {
                    data.extend(
                        part.iter()
                            .zip(along)
                            .map(|(v, k)| level(v.value(), *k as usize)),
                    );
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
            taps: if bayer || m.cpp == 3 {
                Vec::new()
            } else {
                taps_of(&tile)
            },
            xt_green: if bayer || m.cpp == 3 {
                Vec::new()
            } else {
                greens_of(&tile)
            },
            xt_near: if bayer || m.cpp == 3 {
                Vec::new()
            } else {
                near_of(&tile)
            },
            as_shot: [one(0), 1.0, one(2)],
            xyz_to_cam,
            to_srgb: camera_to_srgb(&m.xyz_to_cam),
            orientation: m.orientation,
            auto_gain: 1.0,
            lens: None,
        };
        photo.auto_gain = photo.measure_gain();
        Ok(photo)
    }

    /// The lens corrections to develop with: the file's, unless the
    /// settings leave them off or they are not worth applying.
    fn lens_for(&self, s: &RawDevelop) -> Option<&Lens> {
        self.lens.as_ref().filter(|l| !s.uncorrected && l.usable())
    }

    /// Whether the file carries lens corrections this photograph can use.
    pub fn has_lens(&self) -> bool {
        self.lens.as_ref().is_some_and(Lens::usable)
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
        // Two sites in from every edge the whole 5×5 is there, and the
        // neighbours are the tile's list, summed in the same order.
        if y >= 2 && x >= 2 && y + 2 < self.height && x + 2 < self.width {
            let at = ((y % TILE) * TILE + x % TILE) * 3;
            for (c, slot) in out.iter_mut().enumerate() {
                if c == own {
                    *slot = site(y, x, c);
                    continue;
                }
                let taps = &self.taps[at + c];
                if !taps.is_empty() {
                    let sum: f32 = taps
                        .iter()
                        .map(|&(dy, dx)| {
                            site(
                                (y as isize + dy as isize) as usize,
                                (x as isize + dx as isize) as usize,
                                c,
                            )
                        })
                        .fold(0.0, |a, b| a + b);
                    *slot = sum / taps.len() as f32;
                }
            }
            return out;
        }
        self.searched(y, x, wb)
    }

    /// The colours a site of a pattern other than Bayer lacks, found by
    /// looking round it: the 3×3's sites of each, or the 5×5's. What
    /// `demosaic_at` does at the edges, and what its tile's lists stand
    /// for everywhere else.
    fn searched(&self, y: usize, x: usize, wb: &[f32; 3]) -> [f32; 3] {
        let site = |y: usize, x: usize, c: usize| (self.sample(y, x, 0) * wb[c]).min(1.0);
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

    /// The three colours over a rectangle of sites (rows `y0..y1`,
    /// columns `x0..x1`), row by row: each site as [`demosaic_at`] has it,
    /// except X-Trans away from the edges, which has a demosaic of its
    /// own ([`x_trans_region`]).
    ///
    /// Written into `out` (emptied first); `planes` is X-Trans's working
    /// space, kept from one call to the next so a photograph developed in
    /// bands asks for it once rather than once a band.
    fn region(
        &self,
        (y0, y1, x0, x1): (usize, usize, usize, usize),
        wb: &[f32; 3],
        planes: &mut Planes,
        out: &mut Vec<[f32; 3]>,
    ) {
        out.clear();
        if self.xt_green.is_empty() {
            for y in y0..y1 {
                for x in x0..x1 {
                    out.push(self.demosaic_at(y, x, wb));
                }
            }
            return;
        }
        self.x_trans_region((y0, y1, x0, x1), wb, planes, out)
    }

    /// X-Trans, green first and the rest by how they differ from it.
    ///
    /// Green is most of the sensor (twenty sites in thirty-six) and holds
    /// most of the detail, so it is made whole first: at a red or blue
    /// site, from the nearest greens either side along each of four
    /// directions, each line weighted by how little green changes along
    /// it — so an edge is followed rather than averaged across. Then red
    /// and blue: not their neighbours' average, which blurs them and
    /// fringes edges with colour, but green here plus how far red (or
    /// blue) stood from green at the 5×5's sites of it, those most like
    /// this site in green counting most — a colour's difference from
    /// green changes slowly where green changes fast. Then green once more
    /// at red and blue, now from the site's own colour and how far green
    /// stood from that colour along the same lines, half and half with
    /// the first green (the site's own colour brings detail, and its
    /// noise), and red and blue again from it.
    ///
    /// Judged against photographs whose every colour was measured
    /// (`demosaic_against_truth`): 0.4 to 2.5 dB closer than the plain
    /// average it replaced, and closer than Bayer's own demosaic here on
    /// six pictures of seven. Ten sites in from the edges; the plain
    /// average there.
    fn x_trans_region(
        &self,
        (y0, y1, x0, x1): (usize, usize, usize, usize),
        wb: &[f32; 3],
        planes: &mut Planes,
        out: &mut Vec<[f32; 3]>,
    ) {
        const REACH: usize = 10;
        let (w, h) = (self.width, self.height);
        // Every plane is laid out over the region and the ten sites round
        // it, as far as the sensor goes; each pass fills what the next
        // reads, and nothing it reads is past the sensor.
        let (fy0, fy1) = (y0.saturating_sub(REACH), (y1 + REACH).min(h));
        let (fx0, fx1) = (x0.saturating_sub(REACH), (x1 + REACH).min(w));
        let fw = fx1 - fx0;
        let at = |y: usize, x: usize| (y - fy0) * fw + x - fx0;
        // The rows and columns a pass fills: the region grown by what the
        // passes after it read, kept `m` sites in from the sensor's edges.
        let span = |m: usize| {
            let grow = REACH - m;
            (
                y0.saturating_sub(grow).max(m),
                (y1 + grow).min(h.saturating_sub(m)),
                x0.saturating_sub(grow).max(m),
                (x1 + grow)
                    .min(w.saturating_sub(m))
                    .max(x0.saturating_sub(grow).max(m)),
            )
        };
        let Planes {
            site,
            green,
            red,
            blue,
            second,
        } = planes;
        let size = (fy1 - fy0) * fw;
        for plane in [&mut *site, &mut *green, &mut *red, &mut *blue, &mut *second] {
            plane.clear();
            plane.resize(size, 0.0);
        }
        for y in fy0..fy1 {
            let row = &mut site[at(y, fx0)..][..fw];
            let tile_row = &self.tile[(y % TILE) * TILE..][..TILE];
            let samples = &self.data[y * w + fx0..y * w + fx1];
            let mut t = fx0 % TILE;
            for (v, &raw) in row.iter_mut().zip(samples) {
                *v = (raw as f32 * (1.0 / 65535.0) * wb[tile_row[t] as usize]).min(1.0);
                t = if t + 1 == TILE { 0 } else { t + 1 };
            }
        }
        // For each place in the tile, the lines through it to the greens
        // either side — as offsets in the planes, with the weights that
        // put a line's value at the site — and red's and blue's sites in
        // the 5×5; all in two flat lists, each place a range of them.
        struct Line {
            ahead: isize,
            behind: isize,
            to_ahead: f32,
            to_behind: f32,
            per_step: f32,
        }
        let mut lines = Vec::new();
        let mut lines_at = [(0u16, 0u16); TILE * TILE];
        for (p, ways) in self.xt_green.iter().enumerate() {
            let first = lines.len();
            for (&(a, b), (dy, dx)) in ways.iter().zip(DIRECTIONS) {
                if a == 0 {
                    continue;
                }
                let (a, b) = (a as isize, b as isize);
                let step = dy * fw as isize + dx;
                lines.push(Line {
                    ahead: a * step,
                    behind: -b * step,
                    to_ahead: b as f32 / (a + b) as f32,
                    to_behind: a as f32 / (a + b) as f32,
                    per_step: 1.0 / (a + b) as f32,
                });
            }
            lines_at[p] = (first as u16, lines.len() as u16);
        }
        let mut near = Vec::new();
        let mut near_at = [(0u16, 0u16); TILE * TILE * 2];
        for p in 0..TILE * TILE {
            for (which, c) in [0, 2].into_iter().enumerate() {
                let first = near.len();
                near.extend(
                    self.xt_near[p * 3 + c]
                        .iter()
                        .map(|&(dy, dx)| dy as isize * fw as isize + dx as isize),
                );
                near_at[p * 2 + which] = (first as u16, near.len() as u16);
            }
        }
        // The weight of a line whose green changes by `d` a step along it.
        let by_change = |d: f32| 1.0 / (1e-4 + d * d);
        // Each site of the rows and columns given: its index in the
        // planes, its place in the tile and its colour.
        let tile = &self.tile;
        let each = |(ry0, ry1, rx0, rx1): (usize, usize, usize, usize)| {
            let base = if ry0 < ry1 && rx0 < rx1 {
                at(ry0, rx0)
            } else {
                0
            };
            each_site(tile, base, fw, (ry0, ry1, rx0, rx1))
        };
        // Green, first: along the lines through each red and blue site.
        green.copy_from_slice(site);
        each(span(3)).for_each(|(i, p, own)| {
            if own == 1 {
                return;
            }
            let (from, to) = lines_at[p];
            let (mut sum, mut weights) = (0f32, 0f32);
            for l in &lines[from as usize..to as usize] {
                let p = site[(i as isize + l.ahead) as usize];
                let q = site[(i as isize + l.behind) as usize];
                let weight = by_change((p - q) * l.per_step);
                sum += (p * l.to_ahead + q * l.to_behind) * weight;
                weights += weight;
            }
            green[i] = sum / weights;
        });
        // Red or blue at a site, from a green plane: green there plus the
        // colour's difference from green at its sites in the 5×5.
        let colour = |green: &[f32], i: usize, p: usize, which: usize| {
            let g = green[i];
            let (from, to) = near_at[p * 2 + which];
            let (mut sum, mut weights) = (0f32, 0f32);
            for &o in &near[from as usize..to as usize] {
                let n = (i as isize + o) as usize;
                let weight = 1.0 / (0.01 + (green[n] - g).abs());
                sum += (site[n] - green[n]) * weight;
                weights += weight;
            }
            (g + sum / weights).clamp(0.0, 1.0)
        };
        // Red and blue at the greens the second green reads.
        each(span(5)).for_each(|(i, p, own)| {
            if own == 1 {
                red[i] = colour(green, i, p, 0);
                blue[i] = colour(green, i, p, 1);
            }
        });
        // Green again at red and blue, half and half with the first.
        second.copy_from_slice(green);
        each(span(8)).for_each(|(i, p, own)| {
            if own == 1 {
                return;
            }
            let theirs: &[f32] = if own == 0 { red } else { blue };
            let (from, to) = lines_at[p];
            let (mut sum, mut weights) = (0f32, 0f32);
            for l in &lines[from as usize..to as usize] {
                let (a, b) = (
                    (i as isize + l.ahead) as usize,
                    (i as isize + l.behind) as usize,
                );
                let weight = by_change((site[a] - site[b]) * l.per_step);
                sum += ((site[a] - theirs[a]) * l.to_ahead + (site[b] - theirs[b]) * l.to_behind)
                    * weight;
                weights += weight;
            }
            let again = (site[i] + sum / weights).clamp(0.0, 1.0);
            second[i] = 0.5 * (green[i] + again);
        });
        // The region: the full road ten sites in from the edges, the
        // plain average nearer them.
        let (iy0, iy1, ix0, ix1) = span(REACH);
        for y in y0..y1 {
            if y < iy0 || y >= iy1 {
                out.extend((x0..x1).map(|x| self.searched(y, x, wb)));
                continue;
            }
            out.extend((x0..ix0.max(x0)).map(|x| self.searched(y, x, wb)));
            let (from, to) = (ix0.max(x0), ix1.min(x1).max(ix0.max(x0)));
            each((y, y + 1, from, to)).for_each(|(i, p, own)| {
                let mut px = [0f32; 3];
                px[1] = second[i];
                px[0] = if own == 0 {
                    site[i]
                } else {
                    colour(second, i, p, 0)
                };
                px[2] = if own == 2 {
                    site[i]
                } else {
                    colour(second, i, p, 1)
                };
                out.push(px);
            });
            out.extend((to..x1).map(|x| self.searched(y, x, wb)));
        }
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
        // A Bayer pattern repeats every two sites, so a site's colour is
        // its row's and column's evenness — not the tile's two divisions,
        // asked thirteen times a site. Two sites in from every edge the
        // neighbours need no folding either.
        let colour = |yy: usize, xx: usize| self.tile[(yy & 1) * TILE + (xx & 1)] as usize;
        let inside = y >= 2 && x >= 2 && y + 2 < self.height && x + 2 < self.width;
        let at = |dy: isize, dx: isize| -> f32 {
            let (yy, xx) = if inside {
                ((y as isize + dy) as usize, (x as isize + dx) as usize)
            } else {
                (
                    fold(y as isize + dy, h) as usize,
                    fold(x as isize + dx, w) as usize,
                )
            };
            (self.data[yy * self.width + xx] as f32 * (1.0 / 65535.0) * wb[colour(yy, xx)]).min(1.0)
        };
        // Each of the thirteen sites read once, and summed in the same
        // order as the weights below were first written.
        let c0 = at(0, 0);
        let (n, s, wst, e) = (at(-1, 0), at(1, 0), at(0, -1), at(0, 1));
        let (nn, ss, ww, ee) = (at(-2, 0), at(2, 0), at(0, -2), at(0, 2));
        let cross2 = nn + ss + ww + ee;
        let diag1 = at(-1, -1) + at(-1, 1) + at(1, -1) + at(1, 1);
        let own = colour(y, x);
        let clamp = |v: f32| v.clamp(0.0, 1.0);
        if own == 1 {
            // A green site: red and blue lie either side of it, one pair
            // along its row and the other along its column.
            let row = wst + e;
            let col = n + s;
            let row2 = ww + ee;
            let col2 = nn + ss;
            let along_row = clamp((5.0 * c0 + 4.0 * row - diag1 - row2 + 0.5 * col2) / 8.0);
            let along_col = clamp((5.0 * c0 + 4.0 * col - diag1 - col2 + 0.5 * row2) / 8.0);
            let row_colour = colour(y, fold(x as isize + 1, w) as usize);
            return if row_colour == 0 {
                [along_row, c0, along_col]
            } else {
                [along_col, c0, along_row]
            };
        }
        // A red or a blue site: green from the cross, the other colour
        // from the diagonals.
        let cross1 = n + s + wst + e;
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
        // In bands, so X-Trans's working planes are a band's, not the
        // photograph's.
        const BAND: usize = 128;
        let lens = self.lens_for(s);
        let light = lens.map(Lens::light_table);
        let (mut planes, mut band) = (Planes::default(), Vec::new());
        for y0 in (0..h).step_by(BAND) {
            let y1 = (y0 + BAND).min(h);
            self.region((y0, y1, 0, w), &look.wb, &mut planes, &mut band);
            if let Some(light) = &light {
                brighten(light, &mut band, (w, h), (y0, y1, 0, w));
            }
            for (px, out) in band
                .iter()
                .zip(upright[y0 * w * 4..y1 * w * 4].chunks_exact_mut(4))
            {
                let c = look.finish(*px);
                out.copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let upright = detail(&upright, w, h, s.sharpen, s.noise).unwrap_or(upright);
        let warp = lens.and_then(|l| l.warp(w as f32, h as f32));
        let (width, height, rgba8) = orient_through(upright, w, h, self.orientation, warp.as_ref());
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
        let mut sites = Vec::new();
        self.region(
            (my0, my1, mx0, mx1),
            &look.wb,
            &mut Planes::default(),
            &mut sites,
        );
        // The light made even, as in the develop; the square is not moved
        // by the lens's distortion, which at one site a pixel is a shift
        // of where it is taken from, not of what it shows.
        if let Some(lens) = self.lens_for(s) {
            brighten(
                &lens.light_table(),
                &mut sites,
                (self.width, self.height),
                (my0, my1, mx0, mx1),
            );
        }
        for (site, out) in sites.iter().zip(px.chunks_exact_mut(4)) {
            let c = look.finish(*site);
            out.copy_from_slice(&[c[0], c[1], c[2], 255]);
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
        let (pw, ph) = (self.width, self.height);
        let mut sites: Vec<[f32; 3]> = self
            .cam
            .iter()
            .map(|c| [0, 1, 2].map(|k| (c[k] * look.wb[k]).min(1.0)))
            .collect();
        // The lens's corrections as the develop makes them, at the
        // preview's size: they are in fractions of the picture.
        let lens = photo.lens_for(s);
        if let Some(lens) = lens {
            brighten(&lens.light_table(), &mut sites, (pw, ph), (0, ph, 0, pw));
        }
        let mut upright = vec![0u8; pw * ph * 4];
        for (i, c) in sites.iter().enumerate() {
            let c = look.finish(*c);
            upright[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
        let warp = lens.and_then(|l| l.warp(pw as f32, ph as f32));
        let (width, height, rgba8) =
            orient_through(upright, pw, ph, photo.orientation, warp.as_ref());
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
    fn rggb(y: usize, x: usize) -> usize {
        match (y % 2, x % 2) {
            (0, 0) => 0,
            (1, 1) => 2,
            _ => 1,
        }
    }

    fn mosaic(
        w: u32,
        h: u32,
        white: u16,
        neutral: [f32; 3],
        scene: impl Fn(u32, u32) -> [f32; 3],
    ) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((w * h * 2) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = rggb(y as usize, x as usize);
                let v = (scene(x, y)[c] * neutral[c]).clamp(0.0, 1.0) * white as f32;
                pixels.extend_from_slice(&(v.round() as u16).to_le_bytes());
            }
        }
        pixels
    }

    /// The same photograph as [`dng`], packed as the raw module hands it to
    /// the engine (`crate::raw::unpack`), for tests that develop without
    /// reading a file.
    pub fn packed(
        w: u32,
        h: u32,
        white: u16,
        neutral: [f32; 3],
        scene: impl Fn(u32, u32) -> [f32; 3],
    ) -> Vec<u8> {
        let mut out = b"CKRW".to_vec();
        for v in [1u32, w, h, 1, 1] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let matrix = [
            3.2405f32, -1.5371, -0.4985, -0.9693, 1.876, 0.0416, 0.0556, -0.204, 1.0572, 0.0, 0.0,
            0.0,
        ];
        let floats = [0.0f32; 4]
            .into_iter()
            .chain([white as f32; 4])
            .chain([
                1.0 / neutral[0],
                1.0 / neutral[1],
                1.0 / neutral[2],
                f32::NAN,
            ])
            .chain(matrix);
        for v in floats {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0u8; 16]);
        for row in 0..48 {
            for col in 0..48 {
                out.push(rggb(row, col) as u8);
            }
        }
        out.push(0);
        out.extend_from_slice(&mosaic(w, h, white, neutral, scene));
        out
    }

    pub fn dng(
        w: u32,
        h: u32,
        white: u16,
        neutral: [f32; 3],
        scene: impl Fn(u32, u32) -> [f32; 3],
    ) -> Vec<u8> {
        let pixels = mosaic(w, h, white, neutral, scene);
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
        let dng = sample::packed(32, 16, 4095, neutral, |x, _| {
            if x < 16 {
                [0.18; 3]
            } else {
                [0.6; 3]
            }
        });
        crate::raw::unpack(&dng).unwrap()
    }

    /// A lens of one curve each, straight from the middle to past the
    /// corners.
    fn lens_of(distortion: [f32; 2], light: [f32; 2], red: f32, blue: f32) -> Lens {
        Lens {
            radii: vec![0.0, 1.2],
            distortion: distortion.to_vec(),
            red: vec![0.0, red],
            blue: vec![0.0, blue],
            light: light.to_vec(),
        }
    }

    fn luma(img: &SourceImage, x: u32, y: u32) -> f32 {
        let p = px(img, x, y);
        0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32
    }

    #[test]
    fn a_lens_s_dark_corners_are_made_even() {
        let lens = lens_of([0.0, 0.0], [100.0, 40.0], 0.0, 0.0);
        let (w, h) = (120u32, 80u32);
        // A grey card through a lens that darkens towards the corners
        // as the lens says it does.
        let r = |x: u32, y: u32| {
            let (dx, dy) = (x as f32 + 0.5 - 60.0, y as f32 + 0.5 - 40.0);
            (dx * dx + dy * dy).sqrt() / (60f32 * 60.0 + 40.0 * 40.0).sqrt()
        };
        let packed = sample::packed(w, h, 4095, [1.0; 3], |x, y| {
            [0.3 * (1.0 - 0.5 * r(x, y)); 3]
        });
        let mut photo = crate::raw::unpack(&packed).unwrap();
        photo.lens = Some(lens);
        assert!(photo.has_lens());
        let s = RawDevelop::default();
        let on = photo.develop(&s);
        let off = photo.develop(&RawDevelop {
            uncorrected: true,
            ..s
        });
        let (middle, corner) = ((60, 40), (4, 4));
        assert!(luma(&off, corner.0, corner.1) < luma(&off, middle.0, middle.1) - 20.0);
        assert!(
            (luma(&on, corner.0, corner.1) - luma(&on, middle.0, middle.1)).abs() < 4.0,
            "{} against {}",
            luma(&on, corner.0, corner.1),
            luma(&on, middle.0, middle.1)
        );
        // The preview is the develop, smaller, here too.
        let small = photo.preview(60).develop(&photo, &s);
        let (sw, sh) = (small.width, small.height);
        let (pm, pc) = (luma(&small, sw / 2, sh / 2), luma(&small, 1, 1));
        assert!((pm - pc).abs() < 5.0, "{pc} against {pm}");
    }

    /// Where the bright part of a picture is, on average.
    fn centroid(img: &SourceImage) -> (f32, f32) {
        let (mut sx, mut sy, mut n) = (0f32, 0f32, 0f32);
        for y in 0..img.height {
            for x in 0..img.width {
                let v = luma(img, x, y);
                if v > 128.0 {
                    sx += x as f32 + 0.5;
                    sy += y as f32 + 0.5;
                    n += 1.0;
                }
            }
        }
        (sx / n, sy / n)
    }

    #[test]
    fn a_lens_s_barrel_is_put_straight_where_it_says() {
        // Four in a hundred short everywhere: a point lands 1/0.96 as far
        // out as the sensor caught it.
        let (w, h) = (200u32, 150u32);
        let packed = sample::packed(w, h, 4095, [1.0; 3], |x, y| {
            if (146..154).contains(&x) && (30..38).contains(&y) {
                [0.9; 3]
            } else {
                [0.02; 3]
            }
        });
        let mut photo = crate::raw::unpack(&packed).unwrap();
        let s = RawDevelop::default();
        let (bx, by) = centroid(&photo.develop(&s));
        photo.lens = Some(lens_of([-4.0, -4.0], [100.0, 100.0], 0.0, 0.0));
        let (cx, cy) = centroid(&photo.develop(&s));
        let (ex, ey) = (100.0 + (bx - 100.0) / 0.96, 75.0 + (by - 75.0) / 0.96);
        assert!(
            (cx - ex).abs() < 0.3 && (cy - ey).abs() < 0.3,
            "({cx}, {cy}) against ({ex}, {ey})"
        );
        // Turned, it lands in the same place of the turned picture.
        photo.orientation = 6;
        let turned = photo.develop(&s);
        let (tx, ty) = centroid(&turned);
        assert!(
            (tx - (150.0 - ey)).abs() < 0.3 && (ty - ex).abs() < 0.3,
            "({tx}, {ty})"
        );
    }

    #[test]
    fn a_lens_s_pincushion_is_zoomed_so_no_corner_is_empty() {
        // Points taken from further out than they land: without a zoom
        // the corners would be taken from past the sensor's edge.
        let (w, h) = (120f32, 90f32);
        for (lens, least, most) in [
            (lens_of([0.0, 6.0], [100.0, 100.0], 0.0, 0.0), 1.04, 1.06),
            (lens_of([0.0, 3.0], [100.0, 100.0], 0.02, -0.02), 1.02, 1.05),
            (lens_of([0.0, -6.0], [100.0, 100.0], 0.0, 0.0), 1.0, 1.0),
        ] {
            let warp = lens.warp(w, h).unwrap();
            assert!(
                (least..=most).contains(&warp.zoom),
                "zoom {} for {:?}",
                warp.zoom,
                lens.distortion
            );
            for i in 0..=200 {
                let f = i as f32 / 200.0;
                for (x, y) in [
                    (-w / 2.0 + f * w, -h / 2.0),
                    (-w / 2.0 + f * w, h / 2.0),
                    (-w / 2.0, -h / 2.0 + f * h),
                    (w / 2.0, -h / 2.0 + f * h),
                ] {
                    for c in 0..3 {
                        let (sx, sy) = warp.source(x, y, c);
                        assert!(sx.abs() <= w / 2.0 + 0.5 && sy.abs() <= h / 2.0 + 0.5);
                    }
                }
            }
        }
    }

    #[test]
    fn a_lens_s_colour_fringes_move_red_and_blue_apart_from_green() {
        // Red eight in a hundred further out than green at the corners:
        // a white square near the edge comes out with red's part moved.
        let (w, h) = (200u32, 150u32);
        let packed = sample::packed(w, h, 4095, [1.0; 3], |x, y| {
            if (170..180).contains(&x) && (70..80).contains(&y) {
                [0.9; 3]
            } else {
                [0.02; 3]
            }
        });
        let mut photo = crate::raw::unpack(&packed).unwrap();
        photo.lens = Some(lens_of([0.0, 0.0], [100.0, 100.0], 0.08, 0.0));
        let img = photo.develop(&RawDevelop::default());
        // Red is taken from about 3.7% further out there, so the square
        // in red sits that much further in than in green: it starts and
        // ends a few pixels sooner.
        let span = |c: usize| {
            let lit: Vec<u32> = (100..200).filter(|&x| px(&img, x, 75)[c] > 200).collect();
            (lit[0], lit[lit.len() - 1])
        };
        let (red, green) = (span(0), span(1));
        assert!(
            red.0 + 2 < green.0 && red.1 + 2 < green.1,
            "red {red:?}, green {green:?}"
        );
    }

    #[test]
    fn corrections_left_off_are_the_develop_without_a_lens() {
        let mut photo = grey_card([0.5, 1.0, 0.7]);
        let s = RawDevelop {
            exposure: 0.3,
            ..Default::default()
        };
        let plain = photo.develop(&s);
        photo.lens = Some(lens_of([-3.0, -6.0], [100.0, 60.0], 0.01, -0.01));
        let off = photo.develop(&RawDevelop {
            uncorrected: true,
            ..s
        });
        assert_eq!(plain.rgba8, off.rgba8);
        assert_ne!(plain.rgba8, photo.develop(&s).rgba8);
    }

    #[test]
    fn a_packed_mosaic_carries_its_lens() {
        let mut packed = sample::packed(32, 16, 4095, [1.0; 3], |_, _| [0.3; 3]);
        // Version 1, as before: no lens.
        assert!(crate::raw::unpack(&packed).unwrap().lens.is_none());
        packed[4..8].copy_from_slice(&2u32.to_le_bytes());
        let lens = lens_of([-1.0, -2.0], [100.0, 80.0], 0.001, -0.002);
        let mut with = packed.clone();
        with.extend_from_slice(&2u32.to_le_bytes());
        for v in [
            &lens.radii,
            &lens.distortion,
            &lens.red,
            &lens.blue,
            &lens.light,
        ]
        .into_iter()
        .flatten()
        {
            with.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(crate::raw::unpack(&with).unwrap().lens, Some(lens));
        // Version 2 with none, and cut short anywhere in the lens.
        let mut none = packed.clone();
        none.extend_from_slice(&0u32.to_le_bytes());
        assert!(crate::raw::unpack(&none).unwrap().lens.is_none());
        for end in packed.len()..with.len() {
            assert!(crate::raw::unpack(&with[..end]).is_err(), "cut at {end}");
        }
    }

    /// X-Trans away from the edges reads its neighbours from the tile's
    /// lists; they are the search's own answer, site for site, crop and
    /// all, so a photograph develops exactly as it did when every site
    /// searched.
    #[test]
    fn x_trans_neighbours_from_the_tile_are_the_ones_searched_for() {
        const XTRANS: [[usize; 6]; 6] = [
            [1, 1, 0, 1, 1, 2],
            [1, 1, 2, 1, 1, 0],
            [2, 0, 1, 0, 2, 1],
            [1, 1, 2, 1, 1, 0],
            [1, 1, 0, 1, 1, 2],
            [0, 2, 1, 2, 0, 1],
        ];
        let (w, h) = (40usize, 30usize);
        let mut seed = 12345u32;
        let data: Vec<u16> = (0..w * h)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 16) as u16
            })
            .collect();
        let color_at = |y: usize, x: usize| XTRANS[y % 6][x % 6];
        // An odd crop, so the tile starts part way into the pattern.
        let photo = RawPhoto::from_mosaic(&Mosaic {
            width: w,
            height: h,
            cpp: 1,
            data: &data,
            color_at: &color_at,
            black: [0.0; 4],
            white: [65535.0; 4],
            wb: [2.0, 1.0, 1.5, f32::NAN],
            xyz_to_cam: [[0.0; 3]; 4],
            crops: [1, 2, 3, 5],
            orientation: 1,
        })
        .unwrap();
        assert!(!photo.bayer);
        let wb = [2.0, 1.0, 1.5];
        for y in 0..photo.height {
            for x in 0..photo.width {
                let (fast, searched) = (photo.demosaic_at(y, x, &wb), photo.searched(y, x, &wb));
                assert_eq!(
                    fast.map(f32::to_bits),
                    searched.map(f32::to_bits),
                    "at {x}, {y}"
                );
            }
        }
    }

    /// An X-Trans photograph of `scene` (linear RGB, 0 to 1, by site),
    /// cropped so the tile starts part way into the pattern.
    fn x_trans_photo(w: usize, h: usize, scene: impl Fn(usize, usize) -> [f32; 3]) -> RawPhoto {
        const XTRANS: [[usize; 6]; 6] = [
            [1, 1, 0, 1, 1, 2],
            [1, 1, 2, 1, 1, 0],
            [2, 0, 1, 0, 2, 1],
            [1, 1, 2, 1, 1, 0],
            [1, 1, 0, 1, 1, 2],
            [0, 2, 1, 2, 0, 1],
        ];
        let color_at = |y: usize, x: usize| XTRANS[y % 6][x % 6];
        let data: Vec<u16> = (0..w * h)
            .map(|i| {
                let (y, x) = (i / w, i % w);
                (scene(y, x)[color_at(y, x)] * 65535.0).round() as u16
            })
            .collect();
        RawPhoto::from_mosaic(&Mosaic {
            width: w,
            height: h,
            cpp: 1,
            data: &data,
            color_at: &color_at,
            black: [0.0; 4],
            white: [65535.0; 4],
            wb: [1.0, 1.0, 1.0, f32::NAN],
            xyz_to_cam: [[0.0; 3]; 4],
            crops: [1, 0, 0, 3],
            orientation: 1,
        })
        .unwrap()
    }

    /// Shapes with hard edges over gradients, and fine stripes: what a
    /// demosaic gets wrong.
    fn busy(y: usize, x: usize) -> [f32; 3] {
        let (fy, fx) = (y as f32, x as f32);
        let mut c = [0.2 + fx / 300.0, 0.3 + fy / 400.0, 0.5 - fx / 600.0];
        if ((fx - 40.0).powi(2) + (fy - 50.0).powi(2)) < 900.0 {
            c = [0.8, 0.15, 0.1];
        }
        if (70.0..110.0).contains(&fx) && (20.0..60.0).contains(&fy) {
            c = [0.1, 0.6, 0.2];
        }
        if fy > 80.0 && (x / 3).is_multiple_of(2) {
            c = c.map(|v| v * 0.3);
        }
        c
    }

    #[test]
    fn x_trans_in_bands_is_x_trans_all_at_once() {
        let photo = x_trans_photo(131, 97, busy);
        let (w, h) = (photo.width, photo.height);
        let wb = [1.3, 1.0, 0.8];
        let (mut planes, mut whole) = (Planes::default(), Vec::new());
        photo.region((0, h, 0, w), &wb, &mut planes, &mut whole);
        // Bands of seven rows, and a square cut from the middle, each
        // with the same planes used again.
        let mut banded = Vec::new();
        let mut band = Vec::new();
        for y0 in (0..h).step_by(7) {
            photo.region((y0, (y0 + 7).min(h), 0, w), &wb, &mut planes, &mut band);
            banded.extend_from_slice(&band);
        }
        let bits = |v: &[[f32; 3]]| v.iter().flatten().map(|f| f.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&banded), bits(&whole));
        let (sx0, sx1, sy0, sy1) = (13, 77, 4, 61);
        photo.region((sy0, sy1, sx0, sx1), &wb, &mut planes, &mut band);
        let cut: Vec<[f32; 3]> = (sy0..sy1)
            .flat_map(|y| whole[y * w + sx0..y * w + sx1].to_vec())
            .collect();
        assert_eq!(bits(&band), bits(&cut));
    }

    /// How far a demosaic is from `scene` (in light, over the photograph
    /// less its edges): the root of the mean square, and the mean of how
    /// far red and blue stand from green beyond what they should.
    fn off_from(
        photo: &RawPhoto,
        got: &[[f32; 3]],
        scene: impl Fn(usize, usize) -> [f32; 3],
    ) -> (f32, f32) {
        let (w, h) = (photo.width, photo.height);
        let (mut sq, mut colour, mut n) = (0f64, 0f64, 0f64);
        for y in 12..h - 12 {
            for x in 12..w - 12 {
                // The photograph was cropped by one row and three columns.
                let t = scene(y + 1, x + 3);
                let g = got[y * w + x];
                for c in 0..3 {
                    sq += ((g[c] - t[c]) as f64).powi(2);
                }
                colour += (((g[0] - g[1]) - (t[0] - t[1])).abs()
                    + ((g[2] - g[1]) - (t[2] - t[1])).abs()) as f64;
                n += 1.0;
            }
        }
        ((sq / (n * 3.0)).sqrt() as f32, (colour / n) as f32)
    }

    #[test]
    fn x_trans_is_closer_than_the_average_it_replaced() {
        let photo = x_trans_photo(160, 120, busy);
        let (w, h) = (photo.width, photo.height);
        let mut got = Vec::new();
        photo.region((0, h, 0, w), &[1.0; 3], &mut Planes::default(), &mut got);
        let averaged: Vec<[f32; 3]> = (0..w * h)
            .map(|i| photo.searched(i / w, i % w, &[1.0; 3]))
            .collect();
        let (new, new_colour) = off_from(&photo, &got, busy);
        let (old, old_colour) = off_from(&photo, &averaged, busy);
        assert!(new < old * 0.5, "{new} against {old}");
        assert!(
            new_colour < old_colour * 0.4,
            "{new_colour} against {old_colour}"
        );
    }

    /// A grey scene with hard edges: what the old average fringed with
    /// colour, and what following green should leave nearly grey.
    #[test]
    fn x_trans_keeps_a_grey_edge_grey() {
        let grey = |y: usize, x: usize| {
            let v = if (x + y / 3) % 17 < 8 { 0.8 } else { 0.1 };
            [v; 3]
        };
        let photo = x_trans_photo(120, 90, grey);
        let (w, h) = (photo.width, photo.height);
        let mut got = Vec::new();
        photo.region((0, h, 0, w), &[1.0; 3], &mut Planes::default(), &mut got);
        let averaged: Vec<[f32; 3]> = (0..w * h)
            .map(|i| photo.searched(i / w, i % w, &[1.0; 3]))
            .collect();
        let (_, new_colour) = off_from(&photo, &got, grey);
        let (_, old_colour) = off_from(&photo, &averaged, grey);
        assert!(
            new_colour < old_colour * 0.25,
            "{new_colour} against {old_colour}"
        );
    }

    /// How close each demosaic comes to a photograph whose every colour
    /// was measured — files written by `core/rawin`'s `truth` example (a
    /// Bayer raw's 2×2 blocks, one pixel each), named in
    /// `CHITRAKAR_TEST_TRUTH`, comma-separated. Each is sampled through
    /// Bayer's pattern and X-Trans's and developed back; what is printed
    /// is the error in the encoding a page is shown in. Run by name:
    /// `cargo test --release -p chitrakar-codecs demosaic_against_truth
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn demosaic_against_truth() {
        let Ok(files) = std::env::var("CHITRAKAR_TEST_TRUTH") else {
            return;
        };
        const XTRANS: [[usize; 6]; 6] = [
            [1, 1, 0, 1, 1, 2],
            [1, 1, 2, 1, 1, 0],
            [2, 0, 1, 0, 2, 1],
            [1, 1, 2, 1, 1, 0],
            [1, 1, 0, 1, 1, 2],
            [0, 2, 1, 2, 0, 1],
        ];
        let bayer = |y: usize, x: usize| [[0usize, 1], [1, 2]][y % 2][x % 2];
        let xtrans = |y: usize, x: usize| XTRANS[y % 6][x % 6];
        let encode = |v: f32| chitrakar_color::linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0;
        for file in files.split(',') {
            let bytes = std::fs::read(file).unwrap();
            let w = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
            let h = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            let truth: Vec<f32> = bytes[8..]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            for (name, color_at) in [
                ("bayer  ", &bayer as &dyn Fn(usize, usize) -> usize),
                ("x-trans", &xtrans),
            ] {
                let data: Vec<u16> = (0..w * h)
                    .map(|i| {
                        let v = truth[i * 3 + color_at(i / w, i % w)];
                        (v * 65535.0).round() as u16
                    })
                    .collect();
                let photo = RawPhoto::from_mosaic(&Mosaic {
                    width: w,
                    height: h,
                    cpp: 1,
                    data: &data,
                    color_at,
                    black: [0.0; 4],
                    white: [65535.0; 4],
                    wb: [1.0, 1.0, 1.0, f32::NAN],
                    xyz_to_cam: [[0.0; 3]; 4],
                    crops: [0; 4],
                    orientation: 1,
                })
                .unwrap();
                let started = std::time::Instant::now();
                let mut got = Vec::new();
                photo.region((0, h, 0, w), &[1.0; 3], &mut Planes::default(), &mut got);
                let took = started.elapsed();
                let (mut sq, mut n) = (0f64, 0f64);
                let mut per = [0f64; 3];
                let mut chroma = Vec::new();
                for y in 4..h - 4 {
                    for x in 4..w - 4 {
                        let i = y * w + x;
                        let t = [0, 1, 2].map(|c| encode(truth[i * 3 + c]));
                        let g = got[i].map(encode);
                        for c in 0..3 {
                            sq += ((g[c] - t[c]) as f64).powi(2);
                            per[c] += ((g[c] - t[c]) as f64).powi(2);
                        }
                        n += 3.0;
                        let off = ((g[0] - g[1]) - (t[0] - t[1])).abs()
                            + ((g[2] - g[1]) - (t[2] - t[1])).abs();
                        chroma.push(off);
                    }
                }
                chroma.sort_by(f32::total_cmp);
                let mean = chroma.iter().sum::<f32>() / chroma.len() as f32;
                let p99 = chroma[chroma.len() * 99 / 100];
                let psnr = 10.0 * (255.0f64.powi(2) / (sq / n)).log10();
                let per = per.map(|e| 10.0 * (255.0f64.powi(2) / (e / (n / 3.0))).log10());
                println!(
                    "{file}: {name} psnr {psnr:.2} dB (r {:.1} g {:.1} b {:.1}), colour off {mean:.2} mean, {p99:.1} at the 99th, {:.0} ns a site",
                    per[0], per[1], per[2],
                    took.as_nanos() as f64 / (w * h) as f64
                );
            }
        }
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
        let dng = sample::packed(32, 16, 4095, [1.0; 3], |x, _| {
            if x < 16 {
                [0.06; 3]
            } else {
                [0.6; 3]
            }
        });
        let two = crate::raw::unpack(&dng).unwrap();
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
        let dng = sample::packed(16, 16, 4095, [1.0; 3], |_, _| [0.4, 0.2, 0.1]);
        let photo = crate::raw::unpack(&dng).unwrap();
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
        let dark = sample::packed(32, 32, 4095, [1.0; 3], |x, y| {
            if x < 6 && y < 6 {
                [0.9; 3]
            } else {
                [0.002; 3]
            }
        });
        let photo = crate::raw::unpack(&dark).unwrap();
        let s = auto_tone(&photo, &photo.preview(16), &RawDevelop::default());
        assert!(s.exposure > 0.5 && s.shadows > 0.0, "{s:?}");
        assert!(
            s.highlights < 0.0,
            "and pulls the light it pushed past white back: {s:?}"
        );
        // All bright: brought down.
        let bright = sample::packed(32, 32, 4095, [1.0; 3], |_, _| [0.9; 3]);
        let photo = crate::raw::unpack(&bright).unwrap();
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
