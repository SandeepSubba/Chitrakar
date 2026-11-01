//! Clarity and Dehaze: the develop window's Presence, the two settings
//! that are about a neighbourhood rather than a pixel.
//!
//! Clarity is contrast in the middle tones at a scale of about a
//! hundredth of the photograph: the difference between each pixel's
//! brightness and an edge-aware smoothing of it (the guided filter, He,
//! Sun and Tang, 2010, guided by itself) is made larger, or smaller for a
//! softer picture. Edge-aware so that a strong edge, a roof against the
//! sky, does not get a halo; in the middle tones because that is where
//! texture is, and lifting it in the shadows and highlights only clips.
//!
//! Dehaze takes off the veil of air between the camera and what is far
//! away (the dark channel prior, He, Sun and Tang, 2009): in a clear
//! photograph almost every small patch has something nearly black in some
//! channel, so where the darkest channel is not dark, it is the air's
//! light added on top, by an amount that says how thick the air is
//! there. That amount is taken off, in linear light, where the veil was
//! added. Negative puts a veil on.
//!
//! Both answers are smooth over the picture, so they are worked out once
//! on a small copy of it (about 1024 pixels across) and laid on any size
//! of the same photograph — the slider's preview, the whole development
//! and the square at its own size alike — as the fast guided filter
//! does (He and Sun, 2015): what is carried up is a straight line per
//! place, from brightness to answer, and the line is drawn through each
//! full-size pixel's own brightness, so the edges are the photograph's.
//! So the room taken does not grow with the photograph.

/// Means over boxes of `2r+1` by `2r+1`, cut at the edges: a running sum
/// along each row and then down each column.
fn box_mean(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut sum: f64 = row[..(r + 1).min(w)].iter().map(|v| *v as f64).sum();
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            out[y * w + x] = (sum / (x1 - x0) as f64) as f32;
            if x + r + 1 < w {
                sum += row[x + r + 1] as f64;
            }
            if x >= r {
                sum -= row[x - r] as f64;
            }
        }
    }
    let mut column = vec![0f32; h];
    for x in 0..w {
        for y in 0..h {
            column[y] = out[y * w + x];
        }
        let mut sum: f64 = column[..(r + 1).min(h)].iter().map(|v| *v as f64).sum();
        for y in 0..h {
            let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
            out[y * w + x] = (sum / (y1 - y0) as f64) as f32;
            if y + r + 1 < h {
                sum += column[y + r + 1] as f64;
            }
            if y >= r {
                sum -= column[y - r] as f64;
            }
        }
    }
    out
}

/// The smallest value within `r` each way, a row and then a column at a
/// time (a square's minimum is the minimum of its rows' minima).
fn min_filter(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut rows = vec![0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            rows[y * w + x] = row[x0..x1].iter().copied().fold(f32::INFINITY, f32::min);
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            out[y * w + x] = (y0..y1)
                .map(|yy| rows[yy * w + x])
                .fold(f32::INFINITY, f32::min);
        }
    }
    out
}

/// The guided filter's lines: `p` explained, within each window of
/// radius `r`, as a straight line of the guide `i`, averaged over the
/// windows each place is in. The filtered `p` is `a * i + b`, and since
/// both are smooth they can be carried to another size of the picture.
fn lines(i: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> (Vec<f32>, Vec<f32>) {
    let mean_i = box_mean(i, w, h, r);
    let mean_p = box_mean(p, w, h, r);
    let mut a = box_mean(&i.iter().map(|v| v * v).collect::<Vec<_>>(), w, h, r);
    let mut b = box_mean(
        &i.iter().zip(p).map(|(x, y)| x * y).collect::<Vec<_>>(),
        w,
        h,
        r,
    );
    for k in 0..w * h {
        let var = a[k] - mean_i[k] * mean_i[k];
        let cov = b[k] - mean_i[k] * mean_p[k];
        a[k] = cov / (var.max(0.0) + eps);
        b[k] = mean_p[k] - a[k] * mean_i[k];
    }
    (box_mean(&a, w, h, r), box_mean(&b, w, h, r))
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Brightness on a scale the eye reads evenly, where texture is judged.
fn seen(c: [f32; 3]) -> f32 {
    luma(c).max(0.0).powf(1.0 / 2.2)
}

/// Clarity and Dehaze worked out for one photograph, ready to lay on a
/// pixel of it at any size.
pub(crate) struct Presence {
    w: usize,
    h: usize,
    /// How much detail is added (or, negative, taken away), -1 to 1.
    clarity: f32,
    /// The base the detail is measured from, as lines of brightness.
    clear: Option<(Vec<f32>, Vec<f32>)>,
    /// The air's own light, in linear light.
    air: [f32; 3],
    /// How much of the light gets through the air, as lines of
    /// brightness; above one where a veil is being put on.
    haze: Option<(Vec<f32>, Vec<f32>)>,
}

impl Presence {
    /// Worked out from `small`, a copy of the photograph in linear light
    /// (`w` by `h`), for settings `clarity` and `dehaze` of -100 to 100.
    /// `None` when both are nought, so nothing is done at all.
    pub(crate) fn new(
        small: &[[f32; 3]],
        w: usize,
        h: usize,
        clarity: f32,
        dehaze: f32,
    ) -> Option<Presence> {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(-100.0, 100.0) / 100.0
            } else {
                0.0
            }
        };
        let (clarity, dehaze) = (unit(clarity), unit(dehaze));
        if (clarity == 0.0 && dehaze == 0.0) || w == 0 || h == 0 || small.len() < w * h {
            return None;
        }
        let longest = w.max(h) as f32;
        let mut px = small[..w * h].to_vec();
        let mut air = [1.0; 3];
        let haze = if dehaze == 0.0 {
            None
        } else {
            // Patches a seventieth of the picture across, as the prior
            // was measured on.
            let patch = ((longest / 70.0).round() as usize).max(1);
            let darkest: Vec<f32> = px.iter().map(|c| c[0].min(c[1]).min(c[2])).collect();
            let dark = min_filter(&darkest, w, h, patch);
            // The air's light: the mean of the places where the dark
            // channel is brightest, the thickest air — a thousandth of
            // them, so one white shirt does not decide it.
            let mut order: Vec<usize> = (0..w * h).collect();
            let n = (w * h / 1000).max(1);
            order.select_nth_unstable_by(w * h - n, |a, b| dark[*a].total_cmp(&dark[*b]));
            let mut sum = [0f32; 3];
            for &k in &order[w * h - n..] {
                for c in 0..3 {
                    sum[c] += px[k][c];
                }
            }
            air = sum.map(|v| (v / n as f32).clamp(0.05, 1.0));
            let guide: Vec<f32> = px.iter().map(|c| seen(*c)).collect();
            if dehaze > 0.0 {
                let ratio: Vec<f32> = px
                    .iter()
                    .map(|c| (0..3).map(|k| c[k] / air[k]).fold(f32::INFINITY, f32::min))
                    .collect();
                let through: Vec<f32> = min_filter(&ratio, w, h, patch)
                    .iter()
                    .map(|v| 1.0 - 0.95 * dehaze * v.clamp(0.0, 1.0))
                    .collect();
                Some(lines(&guide, &through, w, h, patch * 4, 1e-3))
            } else {
                // A veil put on is the same everywhere: through is
                // more than one, so the colours move towards the air's —
                // a quarter of the way at the end of the slider, since a
                // veil laid in linear light reads far thicker to the eye.
                let through = 1.0 / (1.0 + 0.25 * dehaze);
                Some((vec![0.0; w * h], vec![through; w * h]))
            }
        };
        let mut me = Presence {
            w,
            h,
            clarity,
            clear: None,
            air,
            haze,
        };
        if clarity != 0.0 {
            // Clarity is measured on the picture after the air is taken
            // off, as it will be laid on it.
            if me.haze.is_some() {
                for (k, c) in px.iter_mut().enumerate() {
                    let (u, v) = (
                        ((k % w) as f32 + 0.5) / w as f32,
                        ((k / w) as f32 + 0.5) / h as f32,
                    );
                    *c = me.dehazed(*c, u, v);
                }
            }
            let bright: Vec<f32> = px.iter().map(|c| seen(*c)).collect();
            let r = ((longest / 100.0).round() as usize).max(1);
            me.clear = Some(lines(&bright, &bright, w, h, r, 0.01));
        }
        Some(me)
    }

    /// The line at (`u`, `v`), fractions across and down the picture,
    /// read between the four places round it.
    fn line_at(&self, lines: &(Vec<f32>, Vec<f32>), u: f32, v: f32) -> (f32, f32) {
        let (w, h) = (self.w, self.h);
        let x = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |m: &[f32]| {
            let top = m[y0 * w + x0] * (1.0 - fx) + m[y0 * w + x1] * fx;
            let bottom = m[y1 * w + x0] * (1.0 - fx) + m[y1 * w + x1] * fx;
            top * (1.0 - fy) + bottom * fy
        };
        (at(&lines.0), at(&lines.1))
    }

    fn dehazed(&self, c: [f32; 3], u: f32, v: f32) -> [f32; 3] {
        let Some(haze) = &self.haze else { return c };
        let (a, b) = self.line_at(haze, u, v);
        let through = (a * seen(c) + b).clamp(0.1, 2.5);
        [0, 1, 2].map(|k| ((c[k] - self.air[k]) / through + self.air[k]).max(0.0))
    }

    /// A pixel in linear light, at (`u`, `v`) across and down the
    /// picture, with the air taken off and the detail drawn out.
    pub(crate) fn apply(&self, c: [f32; 3], u: f32, v: f32) -> [f32; 3] {
        let mut c = self.dehazed(c, u, v);
        if let Some(clear) = &self.clear {
            let y = luma(c);
            if y > 0.0 {
                let p = y.powf(1.0 / 2.2);
                let (a, b) = self.line_at(clear, u, v);
                let base = a * p + b;
                // Most in the middle tones, none at black or white.
                let middle = (4.0 * p * (1.0 - p)).clamp(0.0, 1.0);
                let p2 = (p + self.clarity * (p - base) * middle).clamp(0.0, 1.0);
                let k = p2.powf(2.2) / y;
                c = c.map(|v| v * k);
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey card picture: a step from dark to light, with fine texture
    /// on both sides.
    fn card(w: usize, h: usize) -> Vec<[f32; 3]> {
        (0..w * h)
            .map(|k| {
                let (x, y) = (k % w, k / w);
                let base = if x < w / 2 { 0.12 } else { 0.30 };
                let tex = if (x / 3 + y / 3) % 2 == 0 { 1.15 } else { 0.87 };
                [base * tex; 3]
            })
            .collect()
    }

    #[test]
    fn nought_is_nothing_at_all() {
        let px = card(64, 48);
        assert!(Presence::new(&px, 64, 48, 0.0, 0.0).is_none());
        assert!(Presence::new(&px, 64, 48, f32::NAN, 0.0).is_none());
    }

    #[test]
    fn clarity_draws_texture_out_and_leaves_the_tone_alone() {
        let (w, h) = (200, 120);
        let px = card(w, h);
        let p = Presence::new(&px, w, h, 100.0, 0.0).unwrap();
        let out: Vec<[f32; 3]> = px
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let (u, v) = (
                    ((k % w) as f32 + 0.5) / w as f32,
                    ((k / w) as f32 + 0.5) / h as f32,
                );
                p.apply(*c, u, v)
            })
            .collect();
        // Texture well inside the dark half: its swing is wider.
        let swing = |m: &[[f32; 3]]| {
            let (mut lo, mut hi) = (f32::MAX, 0f32);
            for y in 40..80 {
                for x in 20..80 {
                    lo = lo.min(m[y * w + x][1]);
                    hi = hi.max(m[y * w + x][1]);
                }
            }
            (lo, hi, hi / lo)
        };
        let (before, after) = (swing(&px), swing(&out));
        assert!(after.2 > before.2 * 1.15, "{before:?} → {after:?}");
        // The half's average brightness hardly moves.
        let mean = |m: &[[f32; 3]]| {
            (40..80)
                .flat_map(|y| (20..80).map(move |x| (y, x)))
                .map(|(y, x)| m[y * w + x][1].powf(1.0 / 2.2))
                .sum::<f32>()
                / 2400.0
        };
        assert!(
            (mean(&out) - mean(&px)).abs() < 0.02,
            "{} → {}",
            mean(&px),
            mean(&out)
        );
        // And negative smooths it.
        let soft = Presence::new(&px, w, h, -100.0, 0.0).unwrap();
        let out: Vec<[f32; 3]> = px
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let (u, v) = (
                    ((k % w) as f32 + 0.5) / w as f32,
                    ((k / w) as f32 + 0.5) / h as f32,
                );
                soft.apply(*c, u, v)
            })
            .collect();
        assert!(swing(&out).2 < before.2 * 0.9);
    }

    #[test]
    fn dehaze_takes_the_veil_off_what_is_far_away() {
        // A scene with colour in every patch (so something in it is near
        // black in some channel), half of it seen through haze: the
        // air's light laid over it, three quarters thick; and above it
        // all a strip of sky, which is the air's light itself.
        let (w, h) = (210, 140);
        let air = [0.8, 0.82, 0.85];
        let clear = |x: usize, y: usize| -> [f32; 3] {
            match (x / 5 + y / 5) % 3 {
                0 => [0.5, 0.05, 0.02],
                1 => [0.03, 0.4, 0.06],
                _ => [0.04, 0.08, 0.45],
            }
        };
        let t = |x: usize| if x >= w / 2 { 0.25 } else { 0.9 };
        let px: Vec<[f32; 3]> = (0..w * h)
            .map(|k| {
                let (x, y) = (k % w, k / w);
                if y < 20 {
                    return air;
                }
                let (j, t) = (clear(x, y), t(x));
                [0, 1, 2].map(|c| j[c] * t + air[c] * (1.0 - t))
            })
            .collect();
        let p = Presence::new(&px, w, h, 0.0, 100.0).unwrap();
        // The air's light is found.
        for (found, truth) in p.air.iter().zip(air) {
            assert!((found - truth).abs() < 0.1, "{:?}", p.air);
        }
        // Inside the hazy half, the colours come back nearer the scene's.
        let (x, y) = (w * 3 / 4, h * 2 / 3);
        let k = y * w + x;
        let back = p.apply(
            px[k],
            (x as f32 + 0.5) / w as f32,
            (y as f32 + 0.5) / h as f32,
        );
        let err = |a: [f32; 3]| (0..3).map(|c| (a[c] - clear(x, y)[c]).abs()).sum::<f32>();
        assert!(
            err(back) < err(px[k]) * 0.5,
            "{:?} from {:?}, truth {:?}",
            back,
            px[k],
            clear(x, y)
        );
        // A veil put on moves every colour towards the air's.
        let veil = Presence::new(&px, w, h, 0.0, -100.0).unwrap();
        let k = (h * 2 / 3) * w + w / 4;
        let on = veil.apply(px[k], 0.25, 2.0 / 3.0);
        for c in 0..3 {
            assert!((on[c] - veil.air[c]).abs() < (px[k][c] - veil.air[c]).abs());
        }
    }
}
