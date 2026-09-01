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
