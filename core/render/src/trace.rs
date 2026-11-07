//! A picture said again as flat shapes: its colours brought down to a
//! few, each colour's pixels traced and their outlines fitted with
//! curves. What a logo scanned from paper, a drawing photographed on a
//! desk or a screenshot of an icon becomes when it is wanted as vectors.
//!
//! The colours are found by median cut over what the picture shows —
//! the display encoding, where two colours a person calls alike are
//! near — and settled by a few rounds of moving each to the middle of
//! the pixels it was given. A pixel with nothing on its own after that
//! (one whose neighbours all went another way) is given to them, so
//! sensor noise does not come out as a confetti of specks.
//!
//! The shapes are stacked rather than fitted edge to edge. Each colour,
//! the most common at the bottom, covers its own pixels *and every
//! pixel of the colours above it*, which are then laid over it. Two
//! shapes meeting along an edge would each antialias their half of it
//! and leave a hairline of whatever is behind between them; stacked,
//! the one below runs on under the one above and there is no seam to
//! see.
//!
//! An outline traced from pixels is a staircase, and a staircase turns
//! a right angle at every step — which the curve fitter rightly keeps
//! as a corner. So the outline handed to it runs through the middle of
//! the first and the last pixel step of every straight run: a slope of
//! steps becomes the slope it stands for, and a real corner is cut by
//! half a pixel, which the fit then rounds.

/// The fewest colours a picture is traced in, and the most.
pub const TRACE_FEWEST: usize = 2;
pub const TRACE_MOST: usize = 32;

/// The longest side a picture is traced at. Past this the picture is
/// averaged down first: a trace is a drawing of a picture, not a copy of
/// it, and a photograph's twelve million pixels would be twelve million
/// reasons for an outline to wiggle.
pub const TRACE_SIDE: u32 = 1024;

/// One colour's shapes, in the picture's own pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Traced {
    /// The colour, as the picture shows it: eight-bit sRGB.
    pub color: [u8; 3],
    pub shapes: Vec<TracedShape>,
}

/// One island of a colour: an outline in curves, and the holes in it.
#[derive(Debug, Clone, PartialEq)]
pub struct TracedShape {
    pub anchors: Vec<[f32; 2]>,
    /// As a path keeps them: `[in_x, in_y, out_x, out_y]` from each
    /// anchor. Empty for a shape whose outline came out straight.
    pub handles: Vec<[f32; 4]>,
    /// Straight-sided, as a path's subpaths are.
    pub holes: Vec<Vec<[f32; 2]>>,
}

/// Trace a picture of `w` by `h` straight eight-bit RGBA pixels in
/// `colors` colours, every outline within `tolerance` pixels of the
/// pixels it was traced from. Bottom colour first. A pixel under half
/// covered is not traced at all.
pub fn trace_picture(rgba8: &[u8], w: u32, h: u32, colors: usize, tolerance: f32) -> Vec<Traced> {
    if w == 0 || h == 0 || rgba8.len() < (w as usize) * (h as usize) * 4 {
        return Vec::new();
    }
    let tolerance = if tolerance.is_finite() {
        tolerance.clamp(0.1, 20.0)
    } else {
        1.0
    };
    let colors = colors.clamp(TRACE_FEWEST, TRACE_MOST);
    let (pixels, gw, gh, factor) = averaged_down(rgba8, w, h);
    let shown: Vec<Option<[f32; 3]>> = pixels
        .chunks_exact(4)
        .map(|p| (p[3] >= 128).then(|| [p[0] as f32, p[1] as f32, p[2] as f32]))
        .collect();
    let palette = palette_of(&shown, colors);
    if palette.is_empty() {
        return Vec::new();
    }
    let mut label: Vec<Option<usize>> = shown
        .iter()
        .map(|c| c.map(|c| nearest(&palette, c)))
        .collect();
    despeckle(&mut label, gw as usize, gh as usize);
    // Bottom to top by how much of the picture each colour has.
    let mut count = vec![0usize; palette.len()];
    for l in label.iter().flatten() {
        count[*l] += 1;
    }
    let mut order: Vec<usize> = (0..palette.len()).filter(|&k| count[k] > 0).collect();
    order.sort_by(|a, b| count[*b].cmp(&count[*a]).then(a.cmp(b)));
    let mut rank = vec![usize::MAX; palette.len()];
    for (i, k) in order.iter().enumerate() {
        rank[*k] = i;
    }
    // A speck smaller than this is left to the colour under it: anything
    // the outline's own allowance could swallow, and never less than a
    // few pixels, which is what grain leaves after the vote.
    let least = (2.0 * tolerance).powi(2).max(3.0);
    let mut out = Vec::with_capacity(order.len());
    for (i, &k) in order.iter().enumerate() {
        let inside: Vec<bool> = label
            .iter()
            .map(|l| l.is_some_and(|l| rank[l] >= i))
            .collect();
        let rings = crate::trace_pixels(&inside, gw, gh);
        let shapes = islands(&rings, least, tolerance, factor as f32);
        if shapes.is_empty() {
            continue;
        }
        let c = palette[k];
        out.push(Traced {
            color: [
                c[0].round().clamp(0.0, 255.0) as u8,
                c[1].round().clamp(0.0, 255.0) as u8,
                c[2].round().clamp(0.0, 255.0) as u8,
            ],
            shapes,
        });
    }
    out
}

/// The picture averaged down by a whole factor until its longer side is
/// at most [`TRACE_SIDE`], and the factor. Colour is averaged by what
/// covers it, so a half-covered edge pixel does not pull its neighbour
/// towards black.
fn averaged_down(rgba8: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32, u32) {
    let factor = w.max(h).div_ceil(TRACE_SIDE).max(1);
    if factor == 1 {
        return (rgba8[..(w * h * 4) as usize].to_vec(), w, h, 1);
    }
    let (gw, gh) = (w.div_ceil(factor), h.div_ceil(factor));
    let mut out = Vec::with_capacity((gw * gh * 4) as usize);
    for gy in 0..gh {
        for gx in 0..gw {
            let (mut sum, mut cover, mut n) = ([0f32; 3], 0f32, 0f32);
            for y in gy * factor..((gy + 1) * factor).min(h) {
                for x in gx * factor..((gx + 1) * factor).min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    let a = rgba8[i + 3] as f32;
                    for c in 0..3 {
                        sum[c] += rgba8[i + c] as f32 * a;
                    }
                    cover += a;
                    n += 1.0;
                }
            }
            for s in sum {
                out.push(if cover > 0.0 {
                    (s / cover).round() as u8
                } else {
                    0
                });
            }
            out.push((cover / n.max(1.0)).round() as u8);
        }
    }
    (out, gw, gh, factor)
}

/// Up to `colors` colours that stand for the picture.
///
/// Counted first, at five bits a channel, so a picture is a few
/// thousand colours however many pixels it has. The first colour is the
/// commonest; each one after it is the colour with the most pixels
/// standing far from every colour already chosen — how many there are
/// times how far, squared, which is k-means++ made deterministic. A
/// colour a few stray pixels have never outweighs one a shape is made
/// of, however different it is. Then each colour moves to the middle of
/// the pixels nearest it, a few times over.
///
/// Median cut was the first way this was done, and it is the wrong way
/// for a drawing: it splits by how many pixels lie either side, and when
/// one colour is most of the picture every split lands inside that
/// colour — a red disc and a blue frame on white came back as white and
/// one purple.
fn palette_of(shown: &[Option<[f32; 3]>], colors: usize) -> Vec<[f32; 3]> {
    let mut bins: std::collections::BTreeMap<u32, ([f32; 3], f32)> = Default::default();
    for c in shown.iter().flatten() {
        let key = ((c[0] as u32 >> 3) << 10) | ((c[1] as u32 >> 3) << 5) | (c[2] as u32 >> 3);
        let bin = bins.entry(key).or_insert(([0.0; 3], 0.0));
        for (sum, v) in bin.0.iter_mut().zip(c) {
            *sum += v;
        }
        bin.1 += 1.0;
    }
    let found: Vec<([f32; 3], f32)> = bins
        .values()
        .map(|(s, n)| ([s[0] / n, s[1] / n, s[2] / n], *n))
        .collect();
    let Some(first) = found.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        return Vec::new();
    };
    let apart = |a: &[f32; 3], b: &[f32; 3]| {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
    };
    let mut palette = vec![first.0];
    let mut far: Vec<f32> = found.iter().map(|(c, _)| apart(c, &first.0)).collect();
    while palette.len() < colors {
        let Some((at, score)) = found
            .iter()
            .enumerate()
            .map(|(i, (_, n))| (i, n * far[i]))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        else {
            break;
        };
        if score <= 0.0 {
            break;
        }
        let chosen = found[at].0;
        palette.push(chosen);
        for (i, (c, _)) in found.iter().enumerate() {
            far[i] = far[i].min(apart(c, &chosen));
        }
    }
    for _ in 0..6 {
        let mut sums = vec![([0f32; 3], 0f32); palette.len()];
        for (c, n) in &found {
            let k = nearest(&palette, *c);
            for (sum, v) in sums[k].0.iter_mut().zip(c) {
                *sum += v * n;
            }
            sums[k].1 += n;
        }
        for (k, (s, n)) in sums.iter().enumerate() {
            if *n > 0.0 {
                palette[k] = [s[0] / n, s[1] / n, s[2] / n];
            }
        }
    }
    palette
}

fn nearest(palette: &[[f32; 3]], c: [f32; 3]) -> usize {
    let d = |p: &[f32; 3]| (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
    (0..palette.len())
        .min_by(|a, b| d(&palette[*a]).total_cmp(&d(&palette[*b])).then(a.cmp(b)))
        .unwrap_or(0)
}

/// A pixel that at most one of its four side neighbours shares a colour
/// with — a speck on its own, or a bump of one pixel on an edge — takes
/// the colour most of its eight neighbours have. A line one pixel wide
/// has a neighbour along it on each side and stays. Once, over the
/// labels as they were, so the answer does not depend on which way the
/// picture is walked.
fn despeckle(label: &mut [Option<usize>], w: usize, h: usize) {
    let before = label.to_vec();
    let at = |x: i32, y: i32| -> Option<usize> {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            None
        } else {
            before[y as usize * w + x as usize]
        }
    };
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let Some(me) = at(x, y) else {
                continue;
            };
            let sides = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter(|(dx, dy)| at(x + dx, y + dy) == Some(me))
                .count();
            if sides > 1 {
                continue;
            }
            let mut votes: Vec<(usize, u32)> = Vec::new();
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let Some(n) = at(x + dx, y + dy) else {
                        continue;
                    };
                    match votes.iter_mut().find(|(k, _)| *k == n) {
                        Some(v) => v.1 += 1,
                        None => votes.push((n, 1)),
                    }
                }
            }
            if let Some((k, _)) = votes
                .iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            {
                label[(y as usize) * w + x as usize] = Some(*k);
            }
        }
    }
}

/// Twice the signed area of a ring: positive for the way `trace_pixels`
/// winds an outline, negative for a hole.
fn area2(ring: &[[f32; 2]]) -> f32 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

fn contains(ring: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut inside = false;
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

/// A staircase said as the slope it stands for: on each side of every
/// corner, the middle of the pixel step that meets it — so a slope of
/// steps becomes the slope they stand for. A corner between two runs of
/// three pixels or more is a corner of the drawing rather than a step,
/// and is kept exactly where it is.
fn through_the_steps(ring: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let n = ring.len();
    let mut out = Vec::with_capacity(n * 2);
    let step = |a: [f32; 2], b: [f32; 2]| {
        let len = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
        (
            len,
            [(b[0] - a[0]) / len.max(1e-6), (b[1] - a[1]) / len.max(1e-6)],
        )
    };
    for i in 0..n {
        let (prev, here, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        let ((lin, din), (lout, dout)) = (step(prev, here), step(here, next));
        if lin >= 3.0 && lout >= 3.0 {
            out.push(here);
        } else {
            out.push([here[0] - din[0] * 0.5, here[1] - din[1] * 0.5]);
            out.push([here[0] + dout[0] * 0.5, here[1] + dout[1] * 0.5]);
        }
    }
    out
}

/// The rings of one colour's pixels, as islands with their holes, each
/// outline fitted and every point scaled by `scale`. Anything smaller
/// than `least` square pixels is left out: an island of that size is a
/// speck the colour below covers, a hole of it is filled.
fn islands(rings: &[Vec<[f32; 2]>], least: f32, tolerance: f32, scale: f32) -> Vec<TracedShape> {
    let mut outer: Vec<&Vec<[f32; 2]>> = Vec::new();
    let mut holes: Vec<&Vec<[f32; 2]>> = Vec::new();
    for r in rings {
        let a = area2(r) / 2.0;
        if a.abs() < least {
            continue;
        }
        if a > 0.0 {
            outer.push(r);
        } else {
            holes.push(r);
        }
    }
    let mut held: Vec<Vec<&Vec<[f32; 2]>>> = vec![Vec::new(); outer.len()];
    for hole in holes {
        // The smallest outline that holds it: an island inside a hole in
        // an island is its own island, and its holes are its own.
        let p = hole[0];
        let probe = [p[0] + 0.25, p[1] + 0.25];
        if let Some(k) = (0..outer.len())
            .filter(|&k| contains(outer[k], probe) || contains(outer[k], p))
            .min_by(|a, b| area2(outer[*a]).total_cmp(&area2(outer[*b])))
        {
            held[k].push(hole);
        }
    }
    let scaled = |p: [f32; 2]| [p[0] * scale, p[1] * scale];
    outer
        .iter()
        .zip(held)
        .map(|(ring, holes)| {
            let (anchors, handles) =
                crate::fit::fit_curves(&through_the_steps(ring), true, tolerance);
            TracedShape {
                anchors: anchors.into_iter().map(scaled).collect(),
                handles: handles
                    .into_iter()
                    .map(|h| [h[0] * scale, h[1] * scale, h[2] * scale, h[3] * scale])
                    .collect(),
                holes: holes
                    .into_iter()
                    .map(|h| {
                        crate::straightened(&through_the_steps(h), tolerance)
                            .into_iter()
                            .map(scaled)
                            .collect()
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// White, a red disc of radius 20 at (24, 24), and a blue frame —
    /// a square with a square hole — at the right.
    fn drawing() -> (Vec<u8>, u32, u32) {
        let (w, h) = (80u32, 48u32);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 + 0.5 - 24.0, y as f32 + 0.5 - 24.0);
                let disc = fx * fx + fy * fy <= 400.0;
                let frame = (52..76).contains(&x)
                    && (10..34).contains(&y)
                    && !((58..70).contains(&x) && (16..28).contains(&y));
                let c = if disc {
                    [220, 30, 40]
                } else if frame {
                    [20, 40, 200]
                } else {
                    [250, 250, 250]
                };
                px.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        (px, w, h)
    }

    #[test]
    fn a_drawing_comes_back_as_its_shapes() {
        let (px, w, h) = drawing();
        let traced = trace_picture(&px, w, h, 3, 1.0);
        let colours: Vec<[u8; 3]> = traced.iter().map(|t| t.color).collect();
        assert_eq!(colours.len(), 3, "three colours: {colours:?}");
        assert_eq!(colours[0], [250, 250, 250], "the ground is at the bottom");
        let red = traced.iter().find(|t| t.color == [220, 30, 40]).unwrap();
        let blue = traced.iter().find(|t| t.color == [20, 40, 200]).unwrap();
        // The disc is a few curves rather than a staircase, and every
        // anchor is on its circle. Red is laid under blue, so it runs on
        // under the frame as well: a second island, the frame's outline.
        assert_eq!(red.shapes.len(), 2, "the disc, and under the frame");
        let disc = red
            .shapes
            .iter()
            .find(|s| s.anchors.iter().all(|p| p[0] < 48.0))
            .unwrap();
        assert!(
            disc.anchors.len() <= 16 && !disc.handles.is_empty(),
            "a circle in a few curves: {} anchors",
            disc.anchors.len()
        );
        for p in &disc.anchors {
            let r = ((p[0] - 24.0).powi(2) + (p[1] - 24.0).powi(2)).sqrt();
            assert!((r - 20.0).abs() < 1.2, "{p:?} is {r} from the centre");
        }
        // The frame, which is not the ground's colour and is drawn over
        // it, keeps its hole — and its corners stay corners.
        assert_eq!(blue.shapes.len(), 1);
        let frame = &blue.shapes[0];
        assert_eq!(frame.holes.len(), 1, "the frame keeps its hole");
        assert!(
            frame.anchors.len() == 4 && frame.holes[0].len() == 4,
            "a square is its four corners, outside and in: {:?} and {:?}",
            frame.anchors,
            frame.holes[0]
        );
        assert!(
            frame.anchors.contains(&[52.0, 10.0]) && frame.holes[0].contains(&[58.0, 16.0]),
            "exactly where they were: {:?} / {:?}",
            frame.anchors,
            frame.holes[0]
        );
        // The ground lies under everything, so it is the whole picture
        // and has no holes where the others sit on it.
        let ground = &traced[0].shapes;
        assert_eq!(ground.len(), 1);
        assert!(
            ground[0].holes.is_empty(),
            "stacked: no hole under the disc"
        );
    }

    #[test]
    fn grain_does_not_come_out_as_specks() {
        let (mut px, w, h) = drawing();
        let mut seed = 0x1234_5678u32;
        for p in px.chunks_exact_mut(4) {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            // One pixel in twelve a stray colour from the drawing.
            if seed.is_multiple_of(12) {
                let c = [[220, 30, 40], [20, 40, 200], [250, 250, 250]][(seed >> 8) as usize % 3];
                p[..3].copy_from_slice(&c);
            }
        }
        let traced = trace_picture(&px, w, h, 3, 1.0);
        let shapes: usize = traced.iter().map(|t| t.shapes.len()).sum();
        assert!(
            shapes <= 4,
            "the drawing's shapes and no confetti: {shapes}"
        );
        // And a stray pixel against an edge is given to what is round it
        // rather than left as a bump: the frame is still a square.
        let blue = traced
            .iter()
            .find(|t| t.color[2] > 150 && t.color[0] < 100)
            .unwrap();
        assert!(
            blue.shapes[0].anchors.len() <= 5,
            "the frame is still four corners: {}",
            blue.shapes[0].anchors.len()
        );
    }

    #[test]
    fn a_big_picture_is_traced_smaller_and_brought_back_to_size() {
        // Twice the side a trace is taken at: averaged down by two, and
        // the shapes given back at the picture's own size.
        let (w, h) = (TRACE_SIDE * 2, 64);
        let mut px = vec![255u8; (w * h * 4) as usize];
        for y in 16..48 {
            for x in 600..1400 {
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let traced = trace_picture(&px, w, h, 2, 1.0);
        let bar = &traced[1].shapes[0];
        let xs: Vec<f32> = bar.anchors.iter().map(|p| p[0]).collect();
        let (lo, hi) = (
            xs.iter().cloned().fold(f32::MAX, f32::min),
            xs.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(
            (lo - 600.0).abs() < 3.0 && (hi - 1400.0).abs() < 3.0,
            "the bar spans 600..1400: {lo}..{hi}"
        );
        // Nothing covered, nothing traced.
        assert!(trace_picture(&vec![0u8; 16 * 16 * 4], 16, 16, 4, 1.0).is_empty());
    }
}
