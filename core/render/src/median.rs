//! The middle value of every pixel's neighbourhood, channel by channel:
//! what takes specks out of a picture without blurring its edges.
//!
//! Huang's sliding window. A histogram of the window is kept per channel
//! and moved along a row a column at a time — one column out, one in —
//! so a pixel costs a column's worth of updates rather than a window's
//! worth, and a radius doubled costs twice as much rather than four
//! times. The median is held as a running position in the histogram,
//! moved only as far as each slide moves it.
//!
//! A channel is binned by what it shows at — 256 steps of the display
//! encoding, which is what a picture with eight bits a channel is made
//! of — so the median comes back as one of those steps, exactly the
//! value it was where the picture is that picture. Alpha is binned as it
//! is. Light above white is held at white: a median is a vote among
//! what is there, and above white is past where the steps end.

use chitrakar_color::LinearRgba;

/// One channel's histogram of the window, and where its median is.
struct Running {
    bins: [u32; 256],
    /// The bin the median is in.
    at: usize,
    /// How many values lie in bins below it.
    below: u32,
    count: u32,
}

impl Running {
    fn new() -> Self {
        Running {
            bins: [0; 256],
            at: 0,
            below: 0,
            count: 0,
        }
    }

    fn add(&mut self, bin: u8) {
        self.bins[bin as usize] += 1;
        self.count += 1;
        if (bin as usize) < self.at {
            self.below += 1;
        }
    }

    fn remove(&mut self, bin: u8) {
        self.bins[bin as usize] -= 1;
        self.count -= 1;
        if (bin as usize) < self.at {
            self.below -= 1;
        }
    }

    /// The bin holding the lower middle value.
    fn median(&mut self) -> usize {
        let want = (self.count.saturating_sub(1)) / 2;
        while self.below > want {
            self.at -= 1;
            self.below -= self.bins[self.at];
        }
        while self.below + self.bins[self.at] <= want && self.at < 255 {
            self.below += self.bins[self.at];
            self.at += 1;
        }
        self.at
    }
}

/// The light at which the display encoding's step `i` gives way to step
/// `i + 1`: halfway between them as shown, which is where rounding the
/// encoded value changes its mind.
fn step_edges() -> [f32; 255] {
    let mut e = [0.0f32; 255];
    for (i, edge) in e.iter_mut().enumerate() {
        *edge = chitrakar_color::srgb_to_linear((i as f32 + 0.5) / 255.0);
    }
    e
}

/// The median of each pixel's square neighbourhood, `radius` pixels each
/// way, over a `w` by `h` grid of premultiplied pixels — the edge carried
/// on past the grid's own edge. A radius of nothing is the grid itself.
pub fn median(pixels: &[LinearRgba], w: usize, h: usize, radius: usize) -> Vec<LinearRgba> {
    if radius == 0 || w == 0 || h == 0 || pixels.len() != w * h {
        return pixels.to_vec();
    }
    // Every pixel's four bins, worked out once — by where the value
    // falls among the 255 edges between steps, in light, rather than by
    // encoding every value: a search of eight comparisons where the
    // encoding is a power.
    let edges = step_edges();
    let shown = |v: f32| edges.partition_point(|&e| e <= v) as u8;
    let binned: Vec<[u8; 4]> = pixels
        .iter()
        .map(|p| {
            [
                shown(p.r),
                shown(p.g),
                shown(p.b),
                (p.a.clamp(0.0, 1.0) * 255.0).round() as u8,
            ]
        })
        .collect();
    // What each bin stands for, read back.
    let light: Vec<f32> = (0..256)
        .map(|i| chitrakar_color::srgb_to_linear(i as f32 / 255.0))
        .collect();
    let r = radius as isize;
    let col = |x: isize| x.clamp(0, w as isize - 1) as usize;
    let row = |y: isize| y.clamp(0, h as isize - 1) as usize;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h as isize {
        let mut runs = [
            Running::new(),
            Running::new(),
            Running::new(),
            Running::new(),
        ];
        let column = |runs: &mut [Running; 4], x: isize, add: bool| {
            for dy in -r..=r {
                let b = binned[row(y + dy) * w + col(x)];
                for c in 0..4 {
                    if add {
                        runs[c].add(b[c]);
                    } else {
                        runs[c].remove(b[c]);
                    }
                }
            }
        };
        for x in -r..=r {
            column(&mut runs, x, true);
        }
        for x in 0..w as isize {
            if x > 0 {
                column(&mut runs, x - r - 1, false);
                column(&mut runs, x + r, true);
            }
            let a = runs[3].median() as f32 / 255.0;
            // Premultiplied, so a colour is held to the cover it has.
            let mut c = |i: usize| light[runs[i].median()].min(a);
            let (cr, cg, cb) = (c(0), c(1), c(2));
            out.push(LinearRgba {
                r: cr,
                g: cg,
                b: cb,
                a,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(v: f32) -> LinearRgba {
        let l = chitrakar_color::srgb_to_linear(v);
        LinearRgba {
            r: l,
            g: l,
            b: l,
            a: 1.0,
        }
    }

    /// The obvious way: every window gathered and sorted.
    fn by_sorting(pixels: &[LinearRgba], w: usize, h: usize, r: usize) -> Vec<LinearRgba> {
        let shown = |v: f32| (chitrakar_color::linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round();
        // The edges agree with encoding and rounding on every value a
        // channel can hold here, to the step.
        let edges = step_edges();
        for i in 0..=100_000 {
            let v = i as f32 / 100_000.0;
            let by_edges = edges.partition_point(|&e| e <= v) as f32;
            assert!(
                (by_edges - shown(v)).abs() <= 1.0,
                "{v}: {by_edges} against {}",
                shown(v)
            );
        }
        let mut out = Vec::new();
        for y in 0..h as isize {
            for x in 0..w as isize {
                let mut ch: [Vec<f32>; 4] = Default::default();
                for dy in -(r as isize)..=r as isize {
                    for dx in -(r as isize)..=r as isize {
                        let (sx, sy) = (
                            (x + dx).clamp(0, w as isize - 1) as usize,
                            (y + dy).clamp(0, h as isize - 1) as usize,
                        );
                        let p = pixels[sy * w + sx];
                        ch[0].push(shown(p.r));
                        ch[1].push(shown(p.g));
                        ch[2].push(shown(p.b));
                        ch[3].push((p.a.clamp(0.0, 1.0) * 255.0).round());
                    }
                }
                let mid = |v: &mut Vec<f32>| {
                    v.sort_by(f32::total_cmp);
                    v[(v.len() - 1) / 2]
                };
                let a = mid(&mut ch[3]) / 255.0;
                let c = |v: f32| chitrakar_color::srgb_to_linear(v / 255.0).min(a);
                out.push(LinearRgba {
                    r: c(mid(&mut ch[0])),
                    g: c(mid(&mut ch[1])),
                    b: c(mid(&mut ch[2])),
                    a,
                });
            }
        }
        out
    }

    #[test]
    fn the_sliding_window_finds_what_sorting_finds() {
        let mut seed = 31u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / (1u64 << 31) as f32
        };
        for (w, h, r) in [(17, 11, 1), (23, 9, 2), (8, 30, 3), (5, 5, 4), (40, 3, 6)] {
            let pixels: Vec<LinearRgba> = (0..w * h)
                .map(|_| {
                    let a = if rnd() < 0.2 { rnd() } else { 1.0 };
                    LinearRgba {
                        r: rnd() * a,
                        g: rnd() * a,
                        b: rnd() * a,
                        a,
                    }
                })
                .collect();
            let fast = median(&pixels, w, h, r);
            let slow = by_sorting(&pixels, w, h, r);
            for (i, (p, q)) in fast.iter().zip(&slow).enumerate() {
                assert!(
                    (p.r - q.r).abs() < 1e-6
                        && (p.g - q.g).abs() < 1e-6
                        && (p.b - q.b).abs() < 1e-6
                        && (p.a - q.a).abs() < 1e-6,
                    "{w}x{h} at radius {r}, pixel {i}: {p:?} against {q:?}"
                );
            }
        }
    }

    #[test]
    fn a_speck_goes_and_an_edge_stays() {
        // Left half dark, right half light, with a white speck in the dark
        // and a black one in the light.
        let (w, h) = (20usize, 12usize);
        let mut pixels: Vec<LinearRgba> = (0..w * h)
            .map(|i| if i % w < 10 { grey(0.2) } else { grey(0.8) })
            .collect();
        pixels[5 * w + 4] = grey(1.0);
        pixels[6 * w + 15] = grey(0.0);
        let out = median(&pixels, w, h, 1);
        assert_eq!(out[5 * w + 4].r, grey(0.2).r, "the white speck is outvoted");
        assert_eq!(out[6 * w + 15].r, grey(0.8).r, "and the black one");
        // The edge between the halves is exactly where it was: no grey
        // between, which is what a blur would leave.
        for y in 0..h {
            assert_eq!(out[y * w + 9].r, grey(0.2).r);
            assert_eq!(out[y * w + 10].r, grey(0.8).r);
        }
        // A flat picture of eight-bit colours is itself.
        let flat = vec![grey(0.4); 36];
        assert_eq!(median(&flat, 6, 6, 2), flat);
        assert_eq!(median(&pixels, w, h, 0), pixels);
    }
}
