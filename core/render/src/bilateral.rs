//! Smoothing that stops at edges: a bilateral filter, one axis at a time.
//!
//! Each pixel becomes a weighted average of the pixels beside it — along
//! its row in the first pass, down its column of what that left in the
//! second. A neighbour weighs by how near it is, a Gaussian of half the
//! radius, times how alike it looks: one when its shown colour is the
//! pixel's own, falling straight to nothing once any channel is
//! `threshold` apart. Likeness is judged in the display encoding, which
//! is where a person sees two colours as near or far — a threshold in
//! linear light would call every pair of shadows alike and every pair of
//! highlights different.
//!
//! The second renderer runs the same two passes with the same weights,
//! so this is the reference it is held to.

use chitrakar_color::{linear_to_srgb, LinearRgba};

/// What a pixel shows: its straight colour in the display encoding,
/// held to 0..1, and its alpha. Nothing for a pixel with nothing in it.
fn shown(px: LinearRgba) -> [f32; 4] {
    if px.a <= 0.0 {
        return [0.0; 4];
    }
    let s = |c: f32| linear_to_srgb((c / px.a).clamp(0.0, 1.0));
    [s(px.r), s(px.g), s(px.b), px.a]
}

/// The weight of a neighbour `k` pixels off, by distance alone.
pub fn nearness(k: i32, radius: usize) -> f32 {
    let sigma = (radius as f32 / 2.0).max(0.5);
    (-((k * k) as f32) / (2.0 * sigma * sigma)).exp()
}

/// The weight of a neighbour by likeness alone: one for the same shown
/// colour, nothing at `threshold` apart or more.
pub fn likeness(a: [f32; 4], b: [f32; 4], threshold: f32) -> f32 {
    let d = (0..4).map(|i| (a[i] - b[i]).abs()).fold(0.0f32, f32::max);
    (1.0 - d / threshold).max(0.0)
}

/// One pass along rows (or columns): every pixel the weighted average of
/// the `radius` each side of it, reads past the edge held at the edge.
fn pass(
    src: &[LinearRgba],
    w: usize,
    h: usize,
    radius: usize,
    threshold: f32,
    vertical: bool,
) -> Vec<LinearRgba> {
    let looks: Vec<[f32; 4]> = src.iter().map(|p| shown(*p)).collect();
    let near: Vec<f32> = (0..=radius as i32).map(|k| nearness(k, radius)).collect();
    let r = radius as i32;
    let mut out = Vec::with_capacity(src.len());
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let c = looks[i];
            let (mut sum, mut total) = (LinearRgba::TRANSPARENT, 0.0f32);
            for k in -r..=r {
                let j = if vertical {
                    (y as i32 + k).clamp(0, h as i32 - 1) as usize * w + x
                } else {
                    y * w + (x as i32 + k).clamp(0, w as i32 - 1) as usize
                };
                let weight = near[k.unsigned_abs() as usize] * likeness(c, looks[j], threshold);
                let p = src[j];
                sum.r += weight * p.r;
                sum.g += weight * p.g;
                sum.b += weight * p.b;
                sum.a += weight * p.a;
                total += weight;
            }
            // The pixel itself always counts, fully, so there is always
            // something to divide by.
            let t = total.max(1e-6);
            out.push(LinearRgba {
                r: sum.r / t,
                g: sum.g / t,
                b: sum.b / t,
                a: sum.a / t,
            });
        }
    }
    out
}

/// The filter over a `w` by `h` picture: along the rows, then down the
/// columns. A radius of nothing, or a threshold of nothing, is the
/// picture as it was.
pub fn surface_blur(
    original: &[LinearRgba],
    w: usize,
    h: usize,
    radius: usize,
    threshold: f32,
) -> Vec<LinearRgba> {
    if radius == 0 || threshold <= 0.0 || w == 0 || h == 0 {
        return original.to_vec();
    }
    let across = pass(original, w, h, radius, threshold, false);
    pass(&across, w, h, radius, threshold, true)
}
