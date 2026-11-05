//! A lens's colour fringes measured from the photograph itself: lateral
//! chromatic aberration, the lens drawing red and blue a little bigger
//! or smaller than green, so every edge away from the middle wears a
//! red line on one side and a blue one on the other.
//!
//! For a lens the file carries no corrections for, and lensfun has no
//! profile for (or one without its fringes), which is what Lightroom's
//! "Remove chromatic aberration" is. The sensor is read in blocks of its
//! own pattern, each colour on its own (so nothing a demosaic borrows
//! from one colour for another hides the fringe), and across the picture
//! in tiles: in each tile where red (or blue) has the same edges as green,
//! how far it has to move to lie on green is worked out the way an
//! optical flow is — from the brightness's slope, by least squares — and
//! of that only the part along the line from the middle is kept. Those
//! shifts, against how far out each tile is, are a scale: `k(ρ) = k₀ +
//! k₁ρ²` with ρ in half-diagonals, fitted with the tiles that disagree
//! with the rest left out, and measured again from the colour put where
//! the first measure says, until it stops moving.
//!
//! What is fitted is what [`crate::develop::Lens`] calls red's and blue's
//! scale: at a point the corrected picture shows, the colour is read from
//! `1 + k(ρ)` times as far out.

use rayon::prelude::*;

/// Red's and blue's `[k₀, k₁]`: how many times as far out as green (less
/// one) each is to be read from, at ρ half-diagonals out, `k₀ + k₁ρ²`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fringes {
    pub red: [f32; 2],
    pub blue: [f32; 2],
}

impl Fringes {
    /// Red's or blue's scale less one at ρ half-diagonals out.
    pub fn at(k: [f32; 2], rho: f32) -> f32 {
        k[0] + k[1] * rho * rho
    }

    /// The most either colour moves anywhere in the picture, in pixels
    /// of a picture whose half-diagonal is `half_diagonal`.
    pub fn most(&self, half_diagonal: f32) -> f32 {
        (0..=20)
            .map(|i| {
                let rho = i as f32 / 20.0;
                [self.red, self.blue]
                    .iter()
                    .map(|k| (Fringes::at(*k, rho) * rho * half_diagonal).abs())
                    .fold(0.0, f32::max)
            })
            .fold(0.0, f32::max)
    }
}

/// The photograph read in blocks, a colour at a time.
pub struct Planes {
    /// Blocks across and down.
    pub width: usize,
    pub height: usize,
    /// The side of a block in sites.
    pub block: f32,
    /// Red, green and blue at each block: the square root of the mean of
    /// that colour's sites, so an edge in the shadows counts as one in the
    /// light does.
    pub colours: [Vec<f32>; 3],
    /// Blocks that clipped, or are beside one: a fringe there is the
    /// sensor's bloom, not the lens's, and nothing is measured from it.
    pub bad: Vec<bool>,
    /// Where each colour of the block at the corner stands, in sites from
    /// the photograph's middle: the colours of a block are not all at its
    /// centre (a Bayer block's red is at its top left).
    pub origin: [[f32; 2]; 3],
    /// The photograph's half-diagonal, in sites.
    pub half_diagonal: f32,
}

/// A tile's side, in blocks.
const TILE: usize = 32;
/// How many times the colour is put where the measure says and measured
/// again.
const ROUNDS: usize = 4;
/// What no lens fringes by: a scale past this anywhere in the picture
/// is a measure gone wrong.
const MOST: f32 = 0.006;
/// Fewer tiles than this to go by is too few.
const FEWEST: usize = 12;

impl Planes {
    /// Where block (`bx`, `by`)'s colour `c` stands, from the middle.
    #[inline]
    fn place(&self, c: usize, bx: usize, by: usize) -> [f32; 2] {
        [
            bx as f32 * self.block + self.origin[c][0],
            by as f32 * self.block + self.origin[c][1],
        ]
    }

    /// Colour `c` at `p` (sites from the middle), between its blocks;
    /// `None` past the outermost.
    #[inline]
    fn read(&self, c: usize, p: [f32; 2]) -> Option<f32> {
        let fx = (p[0] - self.origin[c][0]) / self.block;
        let fy = (p[1] - self.origin[c][1]) / self.block;
        if !(fx >= 0.0 && fy >= 0.0) {
            return None;
        }
        let (x, y) = (fx as usize, fy as usize);
        if x + 1 >= self.width || y + 1 >= self.height {
            return None;
        }
        let (tx, ty) = (fx - x as f32, fy - y as f32);
        let v = &self.colours[c];
        let i = y * self.width + x;
        let top = v[i] + (v[i + 1] - v[i]) * tx;
        let bottom = v[i + self.width] + (v[i + self.width + 1] - v[i + self.width]) * tx;
        Some(top + (bottom - top) * ty)
    }

    /// Colour `c` read where green's blocks are, from `1 + k(ρ)` times as
    /// far out; `NaN` where there is nothing to read or it clipped.
    fn moved(&self, c: usize, k: [f32; 2]) -> Vec<f32> {
        let (w, h) = (self.width, self.height);
        let per = 1.0 / (self.half_diagonal * self.half_diagonal);
        let mut out = vec![f32::NAN; w * h];
        out.par_chunks_mut(w).enumerate().for_each(|(by, row)| {
            for (bx, o) in row.iter_mut().enumerate() {
                if self.bad[by * w + bx] {
                    continue;
                }
                let [x, y] = self.place(1, bx, by);
                let s = 1.0 + k[0] + k[1] * (x * x + y * y) * per;
                if let Some(v) = self.read(c, [x * s, y * s]) {
                    *o = v;
                }
            }
        });
        out
    }

    /// What one round says is left of colour `c`'s scale, read `moved`:
    /// for each tile worth going by, how far out it is (ρ²), the scale
    /// left there, and how much it is to be believed.
    fn tiles(&self, moved: &[f32]) -> Vec<(f32, f32, f32)> {
        let (w, h) = (self.width, self.height);
        let green = &self.colours[1];
        let rows: Vec<Vec<(f32, f32, f32)>> = (0..h / TILE)
            .into_par_iter()
            .map(|ty| {
                let mut found = Vec::new();
                for tx in 0..w / TILE {
                    let (x0, y0) = (tx * TILE, ty * TILE);
                    // Only blocks whose four neighbours are there too, so a
                    // slope can be read at each.
                    let usable = |i: usize| moved[i].is_finite() && !self.bad[i];
                    let inside = |bx: usize, by: usize| {
                        let i = by * w + bx;
                        usable(i)
                            && usable(i - 1)
                            && usable(i + 1)
                            && usable(i - w)
                            && usable(i + w)
                    };
                    let mut n = 0usize;
                    let (mut sc, mut sg, mut scc, mut sgg, mut scg) =
                        (0f64, 0f64, 0f64, 0f64, 0f64);
                    for by in y0.max(1)..(y0 + TILE).min(h - 1) {
                        for bx in x0.max(1)..(x0 + TILE).min(w - 1) {
                            if !inside(bx, by) {
                                continue;
                            }
                            let i = by * w + bx;
                            let (c, g) = (moved[i] as f64, green[i] as f64);
                            n += 1;
                            sc += c;
                            sg += g;
                            scc += c * c;
                            sgg += g * g;
                            scg += c * g;
                        }
                    }
                    if n < TILE * TILE / 2 {
                        continue;
                    }
                    let nf = n as f64;
                    let (mc, mg) = (sc / nf, sg / nf);
                    let vc = scc / nf - mc * mc;
                    let vg = sgg / nf - mg * mg;
                    // Something to go by: edges, not a smooth sky's grain.
                    if vg < 0.015 * 0.015 || vc < 0.008 * 0.008 {
                        continue;
                    }
                    let corr = (scg / nf - mc * mg) / (vc * vg).sqrt();
                    // The same edges in both: an edge of colour, red against
                    // green, says nothing about where the lens put red.
                    if corr < 0.85 {
                        continue;
                    }
                    let (dc, dg) = (vc.sqrt(), vg.sqrt());
                    let cn = |i: usize| ((moved[i] as f64 - mc) / dc) as f32;
                    let gn = |i: usize| ((green[i] as f64 - mg) / dg) as f32;
                    // Least squares for the shift that lays colour on green:
                    // M s = b, M the slopes' own products, b the slopes times
                    // what green has that the colour has not.
                    let (mut mxx, mut mxy, mut myy, mut bx_, mut by_) =
                        (0f64, 0f64, 0f64, 0f64, 0f64);
                    for by in y0.max(1)..(y0 + TILE).min(h - 1) {
                        for bx in x0.max(1)..(x0 + TILE).min(w - 1) {
                            if !inside(bx, by) {
                                continue;
                            }
                            let i = by * w + bx;
                            let gx = 0.25 * (cn(i + 1) - cn(i - 1) + gn(i + 1) - gn(i - 1));
                            let gy = 0.25 * (cn(i + w) - cn(i - w) + gn(i + w) - gn(i - w));
                            let d = gn(i) - cn(i);
                            mxx += (gx * gx) as f64;
                            mxy += (gx * gy) as f64;
                            myy += (gy * gy) as f64;
                            bx_ += (gx * d) as f64;
                            by_ += (gy * d) as f64;
                        }
                    }
                    // The tile's middle, in blocks from the photograph's: the
                    // shift a scale makes is along it and as long as it is.
                    let [px, py] = self.place(1, x0 + TILE / 2, y0 + TILE / 2);
                    let (ux, uy) = ((px / self.block) as f64, (py / self.block) as f64);
                    let along = ux * ux * mxx + 2.0 * ux * uy * mxy + uy * uy * myy;
                    let far = ux * ux + uy * uy;
                    // Edges across the line from the middle, not along it: an
                    // edge pointing at the middle cannot show a scale.
                    if far < 1.0 || along < 0.15 * far * (mxx + myy) || along <= 0.0 {
                        continue;
                    }
                    let k = (ux * bx_ + uy * by_) / along;
                    let rr = (px * px + py * py) / (self.half_diagonal * self.half_diagonal);
                    found.push((rr, k as f32, along as f32));
                }
                found
            })
            .collect();
        rows.concat()
    }

    /// Colour `c`'s scale against green, `[k₀, k₁]`; `None` where the
    /// photograph has too few edges to say or says something no lens
    /// does.
    fn scale_of(&self, c: usize) -> Option<[f32; 2]> {
        let mut k = [0f32; 2];
        for _ in 0..ROUNDS {
            let left = fit(&self.tiles(&self.moved(c, k)))?;
            k = [k[0] + left[0], k[1] + left[1]];
            if (0..=10).any(|i| Fringes::at(k, i as f32 / 10.0).abs() > MOST) {
                return None;
            }
            if left[0].abs() + left[1].abs() < 2e-6 {
                break;
            }
        }
        Some(k)
    }
}

/// `k₀ + k₁ρ²` through the tiles' `(ρ², k, weight)`, those that disagree
/// with the rest by more than three times the typical disagreement left
/// out and the line drawn again. The bend k₁ is held near nought where
/// the tiles do not reach far enough out to say.
fn fit(tiles: &[(f32, f32, f32)]) -> Option<[f32; 2]> {
    let solve = |keep: &dyn Fn(&(f32, f32, f32)) -> bool| -> Option<([f32; 2], usize)> {
        let (mut s0, mut s1, mut s2, mut t0, mut t1, mut n) = (0f64, 0f64, 0f64, 0f64, 0f64, 0);
        for t in tiles.iter().filter(|t| keep(t)) {
            let (r, k, wt) = (t.0 as f64, t.1 as f64, t.2 as f64);
            s0 += wt;
            s1 += wt * r;
            s2 += wt * r * r;
            t0 += wt * k;
            t1 += wt * k * r;
            n += 1;
        }
        if n < FEWEST || s0 <= 0.0 {
            return None;
        }
        // A little of the weight against bending, so a picture whose
        // edges are all at one distance gets a straight scale.
        let s2 = s2 + 0.02 * s0;
        let det = s0 * s2 - s1 * s1;
        if det.abs() < 1e-12 * s0 * s0 {
            return Some(([(t0 / s0) as f32, 0.0], n));
        }
        Some((
            [
                ((t0 * s2 - t1 * s1) / det) as f32,
                ((s0 * t1 - s1 * t0) / det) as f32,
            ],
            n,
        ))
    };
    let (first, _) = solve(&|_| true)?;
    let off = |t: &(f32, f32, f32)| (t.1 - (first[0] + first[1] * t.0)).abs();
    let mut offs: Vec<f32> = tiles.iter().map(off).collect();
    offs.sort_by(f32::total_cmp);
    let typical = offs[offs.len() / 2].max(1e-5);
    solve(&|t| off(t) <= 3.0 * typical).map(|(k, _)| k)
}

/// Red's and blue's scales against green, from `planes`; `None` where
/// either cannot be said.
pub fn measure(planes: &Planes) -> Option<Fringes> {
    if planes.width < 2 * TILE || planes.height < 2 * TILE {
        return None;
    }
    let red = planes.scale_of(0)?;
    let blue = planes.scale_of(2)?;
    Some(Fringes { red, blue })
}
