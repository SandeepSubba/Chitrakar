//! Gaussian blur, approximated by three iterated box blurs (the W3C
//! feGaussianBlur approach). Sliding-window passes make it O(pixels) per
//! pass regardless of sigma — fast enough for interactive CPU use. Operates
//! on premultiplied linear pixels, which is the correct space to blur in.

use crate::{ClipRect, Surface};
use chitrakar_color::LinearRgba;

/// Blur a region of the surface in place with Gaussian standard deviation
/// `sigma` (document pixels). Samples are clamped at the region edges.
/// What a box pass finds when its window runs off the end of a line.
///
/// A blur *filter* reads what is under it over the region being
/// repainted, and past that region the picture goes on — so the edge is
/// repeated, which is also what keeps a page redrawn a region at a time
/// from showing a seam at every boundary. A live effect's field is the
/// other case: it is built over the layer's own box grown by how far the
/// effect reaches, and past that box there is genuinely nothing. Reading
/// nothing there matters where the *surface* cut that box short — a
/// layer near the page's edge — because repeating the edge then invents
/// silhouette that was never there and casts a heavier shadow for it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Beyond {
    /// The last value on the line, over and over.
    Edge,
    /// Nothing at all.
    Nothing,
}

pub fn gaussian_blur(surface: &mut Surface, clip: ClipRect, sigma: f32, beyond: Beyond) {
    if sigma <= 0.01 || clip.is_empty() {
        return;
    }
    // W3C: box size approximating a Gaussian after three passes.
    let d = ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as i32;
    let radius = (d.max(1) / 2).max(1);
    for _ in 0..3 {
        box_blur_axis(surface, clip, radius, true, beyond);
        box_blur_down(surface, clip, radius, beyond);
    }
}

/// The same blur over a plain grid of numbers — a mask's coverage rather
/// than a picture's pixels.
///
/// Written against the same box radius the picture blur uses, so a
/// feathered edge and a blur filter of the same sigma soften by the same
/// amount. Edges clamp, which for a coverage means the value at the edge
/// carries on outwards rather than fading into nothing that was never
/// worked out.
/// The box radius [`blur_plane`] runs with, which is not `sigma` and is
/// not proportional to it either: the W3C width is halved by an integer
/// division and then held at one, so every small sigma comes out at one.
pub fn plane_radius(sigma: f32) -> usize {
    let d = ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as i32;
    ((d.max(1) / 2).max(1)) as usize
}

/// How far [`blur_plane`] reads from a pixel: three box passes, each
/// reaching its own radius.
///
/// A caller working a plane out to be softened has to hold this much
/// margin around what it will read back, or the softening runs out of
/// neighbours at the plane's own edge and clamps. Taking the margin from
/// `sigma` instead is what went wrong: at sigma 0.04 that gives two and
/// the blur reaches three, because the radius is held at one however
/// small the sigma is.
pub fn plane_reach(sigma: f32) -> u32 {
    if sigma <= 0.01 {
        return 0;
    }
    (3 * plane_radius(sigma)) as u32
}

pub fn blur_plane(cover: &mut [f32], width: u32, height: u32, sigma: f32) {
    let (w, h) = (width as usize, height as usize);
    if sigma <= 0.01 || w == 0 || h == 0 || cover.len() < w * h {
        return;
    }
    let radius = plane_radius(sigma);
    let mut line = vec![0.0f32; w.max(h)];
    // A box wider than the line it runs along is a fade over the whole
    // of it, and there is nothing further to say past that: the window
    // is primed by walking it once, so a radius of a few million — a
    // softness typed at a page that cannot hold it, or read out of a
    // file — is not a wrong answer but a wait nobody comes back from.
    let radius = radius.min(w.max(h));
    for _ in 0..3 {
        for horizontal in [true, false] {
            let (lanes, len) = if horizontal { (h, w) } else { (w, h) };
            if len == 0 {
                continue;
            }
            let norm = 1.0 / (2 * radius + 1) as f32;
            if !horizontal {
                plane_down(cover, w, h, radius, norm);
                continue;
            }
            for lane in 0..lanes {
                for (i, slot) in line[..len].iter_mut().enumerate() {
                    *slot = cover[if horizontal {
                        lane * w + i
                    } else {
                        i * w + lane
                    }];
                }
                let read = |i: isize| line[i.clamp(0, len as isize - 1) as usize];
                let r = radius as isize;
                let mut sum: f32 = (-r..=r).map(read).sum();
                for i in 0..len {
                    let at = if horizontal {
                        lane * w + i
                    } else {
                        i * w + lane
                    };
                    cover[at] = sum * norm;
                    sum += read(i as isize + r + 1) - read(i as isize - r);
                }
            }
        }
    }
}

/// [`blur_plane`]'s pass down the columns, worked a row at a time, as
/// [`box_blur_down`] works a picture's: every column's running sum moves
/// down together, the same additions in the same order as walking each
/// column to its end, with the rows the window still reads kept as they
/// were in a ring and the first row on its own.
fn plane_down(cover: &mut [f32], w: usize, h: usize, radius: usize, norm: f32) {
    let r = radius as isize;
    let len = h as isize;
    let first: Vec<f32> = cover[..w].to_vec();
    let ring_len = radius + 1;
    let mut ring = vec![0.0f32; ring_len * w];
    // Row `j`, clamped to the plane, as it was before this pass, once the
    // rows up to `done` have been written.
    let row = |cover: &[f32], ring: &[f32], j: isize, done: isize, x: usize| -> f32 {
        let j = j.clamp(0, len - 1);
        if j > done {
            cover[j as usize * w + x]
        } else if j == 0 {
            first[x]
        } else {
            ring[(j as usize % ring_len) * w + x]
        }
    };
    let mut sum: Vec<f32> = (0..w)
        .map(|x| (-r..=r).map(|j| row(cover, &ring, j, -1, x)).sum())
        .collect();
    for i in 0..len {
        let at = (i as usize % ring_len) * w;
        let start = i as usize * w;
        ring[at..at + w].copy_from_slice(&cover[start..start + w]);
        for (x, s) in sum.iter_mut().enumerate() {
            cover[start + x] = *s * norm;
            *s += row(cover, &ring, i + r + 1, i, x) - row(cover, &ring, i - r, i, x);
        }
    }
}

/// Copy of a region's pixels, for filters that need the pre-blur original.
pub fn snapshot(surface: &Surface, clip: ClipRect) -> Vec<LinearRgba> {
    let (w, h) = (clip.x1 - clip.x0, clip.y1 - clip.y0);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in clip.y0..clip.y1 {
        let row = (y * surface.width) as usize;
        out.extend_from_slice(&surface.pixels[row + clip.x0 as usize..row + clip.x1 as usize]);
    }
    out
}

fn add(a: LinearRgba, b: LinearRgba) -> LinearRgba {
    LinearRgba {
        r: a.r + b.r,
        g: a.g + b.g,
        b: a.b + b.b,
        a: a.a + b.a,
    }
}

fn sub(a: LinearRgba, b: LinearRgba) -> LinearRgba {
    LinearRgba {
        r: a.r - b.r,
        g: a.g - b.g,
        b: a.b - b.b,
        a: a.a - b.a,
    }
}

fn scale(a: LinearRgba, s: f32) -> LinearRgba {
    LinearRgba {
        r: a.r * s,
        g: a.g * s,
        b: a.b * s,
        a: a.a * s,
    }
}

/// One box-blur pass along an axis with a sliding-window running sum.
fn box_blur_axis(
    surface: &mut Surface,
    clip: ClipRect,
    radius: i32,
    horizontal: bool,
    beyond: Beyond,
) {
    let (lanes, len) = if horizontal {
        (clip.y1 - clip.y0, (clip.x1 - clip.x0) as i32)
    } else {
        (clip.x1 - clip.x0, (clip.y1 - clip.y0) as i32)
    };
    if len == 0 {
        return;
    }
    // As in `blur_plane`: a box wider than the line is a fade over the
    // whole of it, and priming a window of a few million costs a wait
    // nobody comes back from.
    let radius = radius.min(len);
    let norm = 1.0 / (2 * radius + 1) as f32;
    let mut line: Vec<LinearRgba> = vec![LinearRgba::TRANSPARENT; len as usize];

    let index = |lane: u32, i: i32| -> usize {
        let (x, y) = if horizontal {
            (clip.x0 + i as u32, clip.y0 + lane)
        } else {
            (clip.x0 + lane, clip.y0 + i as u32)
        };
        (y * surface.width + x) as usize
    };

    for lane in 0..lanes {
        for (i, slot) in line.iter_mut().enumerate() {
            *slot = surface.pixels[index(lane, i as i32)];
        }
        let at = |i: i32| {
            if beyond == Beyond::Nothing && (i < 0 || i >= len) {
                return LinearRgba::TRANSPARENT;
            }
            line[i.clamp(0, len - 1) as usize]
        };
        // Prime the window centered on i = 0.
        let mut sum = LinearRgba::TRANSPARENT;
        for i in -radius..=radius {
            sum = add(sum, at(i));
        }
        // Only the first `radius` and the last `radius + 1` can read past
        // the line's ends; between them every read is inside it and needs
        // no asking. The same additions in the same order, in three runs
        // (`a_blur_in_three_runs_is_the_same_blur`).
        let head = radius.min(len);
        let tail = (len - radius - 1).max(head);
        let step = |i: i32, sum: &mut LinearRgba, inside: bool| {
            *sum = if inside {
                add(*sum, line[(i + radius + 1) as usize])
            } else {
                add(*sum, at(i + radius + 1))
            };
            *sum = if inside {
                sub(*sum, line[(i - radius) as usize])
            } else {
                sub(*sum, at(i - radius))
            };
        };
        if horizontal {
            let row = ((clip.y0 + lane) * surface.width + clip.x0) as usize;
            let out = &mut surface.pixels[row..row + len as usize];
            for i in 0..len {
                out[i as usize] = scale(sum, norm);
                step(i, &mut sum, i >= head && i < tail);
            }
        } else {
            for i in 0..len {
                surface.pixels[index(lane, i)] = scale(sum, norm);
                step(i, &mut sum, i >= head && i < tail);
            }
        }
    }
}

/// [`box_blur_axis`] down the columns, worked a row at a time.
///
/// Walking one column to its end and then the next reads a pixel a whole
/// row away at every step, and a row of a phone's view is eighteen
/// kilobytes: the pass down the columns was most of what a blur cost.
/// Here every column keeps its own running sum and all of them move down
/// together, so each row is read in order, once. Each column's sum is
/// built with the same additions in the same order as the column-at-a-
/// time pass, so the answer is the same to the bit
/// (`down_the_columns_a_row_at_a_time_is_the_same_blur`).
///
/// A row is overwritten with its answer before the window has finished
/// with it, so the rows the window still reads are kept as they were: a
/// ring of the last `radius + 1` — the window reaches `radius` back from
/// the row just written — and the first row on its own, for a window
/// that repeats the top edge.
fn box_blur_down(surface: &mut Surface, clip: ClipRect, radius: i32, beyond: Beyond) {
    let (cols, len) = ((clip.x1 - clip.x0) as usize, (clip.y1 - clip.y0) as i32);
    if len == 0 || cols == 0 {
        return;
    }
    let radius = radius.min(len);
    let norm = 1.0 / (2 * radius + 1) as f32;
    let width = surface.width as usize;
    let row_at = |j: i32| (clip.y0 as usize + j as usize) * width + clip.x0 as usize;
    let first: Vec<LinearRgba> = surface.pixels[row_at(0)..row_at(0) + cols].to_vec();
    let ring_len = (radius + 1) as usize;
    let mut ring: Vec<LinearRgba> = vec![LinearRgba::TRANSPARENT; ring_len * cols];
    let mut sum = vec![LinearRgba::TRANSPARENT; cols];
    let add_row = |sum: &mut [LinearRgba],
                   surface: &Surface,
                   ring: &[LinearRgba],
                   j: i32,
                   done: i32,
                   sign: f32| {
        if beyond == Beyond::Nothing && (j < 0 || j >= len) {
            // Nothing: adding or taking away transparent leaves the sum
            // as it was, bit for bit.
            for s in sum.iter_mut() {
                *s = if sign > 0.0 {
                    add(*s, LinearRgba::TRANSPARENT)
                } else {
                    sub(*s, LinearRgba::TRANSPARENT)
                };
            }
            return;
        }
        // Row `j` as it was before this pass, with the rows up to `done`
        // already written: still on the surface if it is past them, in
        // the ring if it is one of the last few — the window never
        // reaches further back — or the first row, which a window
        // repeating the top edge reads to the end.
        let j = j.clamp(0, len - 1);
        let row: &[LinearRgba] = if j > done {
            &surface.pixels[row_at(j)..row_at(j) + cols]
        } else if j == 0 {
            &first
        } else {
            let at = (j as usize % ring_len) * cols;
            &ring[at..at + cols]
        };
        for (s, p) in sum.iter_mut().zip(row) {
            *s = if sign > 0.0 { add(*s, *p) } else { sub(*s, *p) };
        }
    };
    // Prime every column's window centred on the first row.
    for j in -radius..=radius {
        add_row(&mut sum, surface, &ring, j, -1, 1.0);
    }
    for i in 0..len {
        // Keep the row as it was before its answer goes over it.
        let at = (i as usize % ring_len) * cols;
        let start = row_at(i);
        ring[at..at + cols].copy_from_slice(&surface.pixels[start..start + cols]);
        for (out, s) in surface.pixels[start..start + cols].iter_mut().zip(&sum) {
            *out = scale(*s, norm);
        }
        add_row(&mut sum, surface, &ring, i + radius + 1, i, 1.0);
        add_row(&mut sum, surface, &ring, i - radius, i, -1.0);
    }
}

#[cfg(test)]
mod tests {
    /// A softness wider than what it is being drawn on.
    ///
    /// Typed at a page that cannot hold it, or read out of a file that
    /// can say anything: the box that averages a line is primed by
    /// walking it once, so a radius of a few million is not a wrong
    /// answer but a wait nobody comes back from. A box wider than the
    /// line is a fade over the whole of it, and there is nothing
    /// further to say past that.
    #[test]
    fn a_softness_wider_than_the_page_is_a_fade_over_the_page() {
        let (w, h) = (40u32, 24u32);
        let mut cover = vec![0.0f32; (w * h) as usize];
        for y in 8..16 {
            for x in 12..28 {
                cover[(y * w + x) as usize] = 1.0;
            }
        }
        let started = std::time::Instant::now();
        super::blur_plane(&mut cover, w, h, 1.0e7);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "it came back"
        );
        let (lo, hi) = cover
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        assert!(hi <= 1.0 && lo >= 0.0, "still a coverage: {lo}..{hi}");
        assert!(
            hi - lo < 0.2,
            "and spread over the whole of it rather than left in a shape: {lo}..{hi}"
        );
    }

    use super::*;

    /// The pass down the columns, worked a row at a time, is the same
    /// blur as the one that walks each column to its end — the same
    /// additions in the same order for every column — on regions that
    /// start inside the surface, are shorter than the window and taller,
    /// repeating the edge and reading nothing past it.
    #[test]
    fn down_the_columns_a_row_at_a_time_is_the_same_blur() {
        let (w, h) = (37u32, 53u32);
        let mut seed = 0x2545_f491u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 1000) as f32 / 999.0
        };
        let mut original = Surface::new(w, h);
        for p in &mut original.pixels {
            let a = next();
            *p = LinearRgba {
                r: next() * a,
                g: next() * a,
                b: next() * a,
                a,
            };
        }
        let clips = [
            ClipRect {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
            ClipRect {
                x0: 3,
                y0: 7,
                x1: 30,
                y1: 41,
            },
            ClipRect {
                x0: 5,
                y0: 10,
                x1: 9,
                y1: 13,
            },
        ];
        for clip in clips {
            for radius in [1, 2, 5, 17, 80] {
                for beyond in [Beyond::Edge, Beyond::Nothing] {
                    let mut by_columns = original.clone();
                    box_blur_axis(&mut by_columns, clip, radius, false, beyond);
                    let mut by_rows = original.clone();
                    box_blur_down(&mut by_rows, clip, radius, beyond);
                    for (i, (p, q)) in by_columns.pixels.iter().zip(&by_rows.pixels).enumerate() {
                        assert!(
                            [p.r, p.g, p.b, p.a]
                                .iter()
                                .zip([q.r, q.g, q.b, q.a])
                                .all(|(a, b)| a.to_bits() == b.to_bits()),
                            "radius {radius} over {clip:?}: pixel {i} is {q:?} a row at a time \
                             and {p:?} a column at a time"
                        );
                    }
                }
            }
        }
    }

    /// A pass along the rows with its middle unchecked is the pass that
    /// checks every read: the same picture to the bit, for windows wider
    /// and narrower than the line, repeating the edge and reading nothing.
    #[test]
    fn a_blur_in_three_runs_is_the_same_blur() {
        let (w, h) = (41u32, 9u32);
        let mut seed = 0x51ed_270bu32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 1000) as f32 / 999.0
        };
        let mut original = Surface::new(w, h);
        for p in &mut original.pixels {
            let a = next();
            *p = LinearRgba {
                r: next() * a,
                g: next() * a,
                b: next() * a,
                a,
            };
        }
        let reference = |surface: &mut Surface, clip: ClipRect, radius: i32, beyond: Beyond| {
            let len = (clip.x1 - clip.x0) as i32;
            let radius = radius.min(len);
            let norm = 1.0 / (2 * radius + 1) as f32;
            for y in clip.y0..clip.y1 {
                let line: Vec<LinearRgba> = (clip.x0..clip.x1)
                    .map(|x| surface.pixels[(y * w + x) as usize])
                    .collect();
                let at = |i: i32| {
                    if beyond == Beyond::Nothing && (i < 0 || i >= len) {
                        return LinearRgba::TRANSPARENT;
                    }
                    line[i.clamp(0, len - 1) as usize]
                };
                let mut sum = LinearRgba::TRANSPARENT;
                for i in -radius..=radius {
                    sum = add(sum, at(i));
                }
                for i in 0..len {
                    surface.pixels[(y * w + clip.x0 + i as u32) as usize] = scale(sum, norm);
                    sum = add(sum, at(i + radius + 1));
                    sum = sub(sum, at(i - radius));
                }
            }
        };
        for clip in [
            ClipRect {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
            ClipRect {
                x0: 4,
                y0: 1,
                x1: 33,
                y1: 8,
            },
            ClipRect {
                x0: 10,
                y0: 2,
                x1: 13,
                y1: 4,
            },
        ] {
            for radius in [1, 3, 9, 20, 60] {
                for beyond in [Beyond::Edge, Beyond::Nothing] {
                    let mut ours = original.clone();
                    box_blur_axis(&mut ours, clip, radius, true, beyond);
                    let mut theirs = original.clone();
                    reference(&mut theirs, clip, radius, beyond);
                    for (i, (p, q)) in ours.pixels.iter().zip(&theirs.pixels).enumerate() {
                        assert!(
                            [p.r, p.g, p.b, p.a]
                                .iter()
                                .zip([q.r, q.g, q.b, q.a])
                                .all(|(a, b)| a.to_bits() == b.to_bits()),
                            "radius {radius} over {clip:?}: pixel {i}"
                        );
                    }
                }
            }
        }
    }

    /// The same for a plane of coverage: [`blur_plane`]'s pass down the
    /// columns, a row at a time, against walking each column to its end.
    #[test]
    fn a_plane_blurred_a_row_at_a_time_is_the_same_plane() {
        let (w, h) = (29usize, 41usize);
        let mut seed = 0x9e37_79b9u32;
        let original: Vec<f32> = (0..w * h)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed % 1000) as f32 / 999.0
            })
            .collect();
        for radius in [1usize, 3, 8, 40] {
            let norm = 1.0 / (2 * radius + 1) as f32;
            let mut by_rows = original.clone();
            plane_down(&mut by_rows, w, h, radius, norm);
            // A column at a time, as `blur_plane` walked it.
            let mut by_columns = original.clone();
            let r = radius as isize;
            for x in 0..w {
                let line: Vec<f32> = (0..h).map(|y| by_columns[y * w + x]).collect();
                let read = |i: isize| line[i.clamp(0, h as isize - 1) as usize];
                let mut sum: f32 = (-r..=r).map(read).sum();
                for y in 0..h {
                    by_columns[y * w + x] = sum * norm;
                    sum += read(y as isize + r + 1) - read(y as isize - r);
                }
            }
            for (i, (p, q)) in by_columns.iter().zip(&by_rows).enumerate() {
                assert_eq!(p.to_bits(), q.to_bits(), "radius {radius}: element {i}");
            }
        }
    }

    fn full(surface: &Surface) -> ClipRect {
        ClipRect {
            x0: 0,
            y0: 0,
            x1: surface.width,
            y1: surface.height,
        }
    }

    #[test]
    fn blur_spreads_energy_and_roughly_conserves_it() {
        let mut s = Surface::new(31, 31);
        s.pixels[(15 * 31 + 15) as usize] = LinearRgba {
            r: 1.0,
            g: 0.5,
            b: 0.0,
            a: 1.0,
        };
        let clip = full(&s);
        gaussian_blur(&mut s, clip, 2.0, Beyond::Edge);

        assert!(s.get(15, 15).r < 1.0, "peak flattened");
        assert!(s.get(17, 15).r > 0.0, "energy spread to neighbors");
        let total: f32 = s.pixels.iter().map(|p| p.r).sum();
        assert!(
            (total - 1.0).abs() < 0.02,
            "energy roughly conserved, got {total}"
        );
    }

    #[test]
    fn zero_sigma_is_identity() {
        let mut s = Surface::new(4, 4);
        s.pixels[5] = LinearRgba {
            r: 0.3,
            g: 0.6,
            b: 0.9,
            a: 1.0,
        };
        let before = s.pixels.clone();
        let clip = full(&s);
        gaussian_blur(&mut s, clip, 0.0, Beyond::Edge);
        assert_eq!(s.pixels, before);
    }

    #[test]
    fn uniform_region_is_unchanged_by_blur() {
        let mut s = Surface::new(9, 9);
        let px = LinearRgba {
            r: 0.25,
            g: 0.5,
            b: 0.75,
            a: 1.0,
        };
        s.pixels.fill(px);
        let clip = full(&s);
        gaussian_blur(&mut s, clip, 3.0, Beyond::Edge);
        for p in &s.pixels {
            assert!((p.r - px.r).abs() < 1e-5 && (p.a - px.a).abs() < 1e-5);
        }
    }

    /// A blur is as wide as the sigma asked for.
    ///
    /// Everything else here says a blur *blurs*: the peak flattens,
    /// neighbours light up, the total is conserved, nothing happens at
    /// zero and a flat field is untouched. Every one of those holds for a
    /// blur at sixty per cent of the width it was asked for — and that
    /// mutation passes the whole workspace once the GPU crate is left
    /// out, which is to say the width of a blur was pinned by a test that
    /// self-skips.
    ///
    /// What pins a width is the second moment. Blur a single lit pixel
    /// and the result is a distribution; its variance is the square of
    /// the blur's effective standard deviation, and no amount of
    /// flattening or spreading gets that right by accident.
    ///
    /// The number to hold it to is not sigma squared, and the reason is
    /// worth writing down rather than discovering again. This is the W3C
    /// filter construction: three box passes of size
    /// `floor(sigma * 3 * sqrt(2*pi) / 4 + 0.5)`, which is about 1.88
    /// sigma. Three boxes of width `w` have variance `3 * (w^2 - 1) / 12`,
    /// so the width that construction actually delivers is about
    /// `0.88 * sigma^2` — a blur some six per cent narrower than its name,
    /// by design and by the spec, since the formula matches an equivalent
    /// width rather than a variance. Matching the spec is the point: an
    /// SVG that says `stdDeviation="4"` should soften here the way it
    /// softens in a browser.
    ///
    /// Small sigmas are left out and that is the honest part: the box
    /// radius is an integer halved, so under about five the quantisation
    /// is coarser than the thing being measured — at sigma one the
    /// delivered variance is twice what the name suggests. Held to a
    /// tenth from five up, where the steps are fine enough to mean
    /// something.
    #[test]
    fn a_blur_is_as_wide_as_the_sigma_it_was_given() {
        for sigma in [5.0f32, 6.0, 8.0, 10.0, 12.0] {
            let n = 241u32;
            let mid = n / 2;
            let mut s = Surface::new(n, n);
            s.pixels[(mid * n + mid) as usize] = LinearRgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            };
            let clip = full(&s);
            gaussian_blur(&mut s, clip, sigma, Beyond::Edge);
            let (mut mass, mut second) = (0.0f64, 0.0f64);
            for y in 0..n {
                for x in 0..n {
                    let w = s.get(x, y).r as f64;
                    let dx = x as f64 - mid as f64;
                    mass += w;
                    second += w * dx * dx;
                }
            }
            assert!(
                (mass - 1.0).abs() < 1e-3,
                "the light is all still there at sigma {sigma} ({mass})"
            );
            let variance = second / mass;
            let want = 0.88 * (sigma * sigma) as f64;
            assert!(
                (variance - want).abs() < want * 0.1,
                "at sigma {sigma} the blur's variance is {variance:.2}, \
                 against the {want:.2} the three-box construction delivers"
            );
        }
    }
}
