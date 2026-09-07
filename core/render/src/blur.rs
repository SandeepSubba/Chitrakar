//! Gaussian blur, approximated by three iterated box blurs (the W3C
//! feGaussianBlur approach). Sliding-window passes make it O(pixels) per
//! pass regardless of sigma — fast enough for interactive CPU use. Operates
//! on premultiplied linear pixels, which is the correct space to blur in.

use crate::{ClipRect, Surface};
use chitrakar_color::LinearRgba;

/// Blur a region of the surface in place with Gaussian standard deviation
/// `sigma` (document pixels). Samples are clamped at the region edges.
pub fn gaussian_blur(surface: &mut Surface, clip: ClipRect, sigma: f32) {
    if sigma <= 0.01 || clip.is_empty() {
        return;
    }
    // W3C: box size approximating a Gaussian after three passes.
    let d = ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as i32;
    let radius = (d.max(1) / 2).max(1);
    for _ in 0..3 {
        box_blur_axis(surface, clip, radius, true);
        box_blur_axis(surface, clip, radius, false);
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
pub fn blur_plane(cover: &mut [f32], width: u32, height: u32, sigma: f32) {
    let (w, h) = (width as usize, height as usize);
    if sigma <= 0.01 || w == 0 || h == 0 || cover.len() < w * h {
        return;
    }
    let d = ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as i32;
    let radius = ((d.max(1) / 2).max(1)) as usize;
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
fn box_blur_axis(surface: &mut Surface, clip: ClipRect, radius: i32, horizontal: bool) {
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
        let at = |i: i32| line[i.clamp(0, len - 1) as usize];
        // Prime the window centered on i = 0.
        let mut sum = LinearRgba::TRANSPARENT;
        for i in -radius..=radius {
            sum = add(sum, at(i));
        }
        for i in 0..len {
            surface.pixels[index(lane, i)] = scale(sum, norm);
            sum = add(sum, at(i + radius + 1));
            sum = sub(sum, at(i - radius));
        }
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
        gaussian_blur(&mut s, clip, 2.0);

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
        gaussian_blur(&mut s, clip, 0.0);
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
        gaussian_blur(&mut s, clip, 3.0);
        for p in &s.pixels {
            assert!((p.r - px.r).abs() < 1e-5 && (p.a - px.a).abs() < 1e-5);
        }
    }
}
