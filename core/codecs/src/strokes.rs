//! A brush stroke's shape as outlines, for an exporter that can fill a
//! path: exactly what the renderer covers, so a stroke leaves a file as
//! the curve it is rather than as a picture of it at one resolution.
//!
//! The renderer covers, for each segment between two points, every point
//! whose distance to the segment is within the radius there — the radius
//! running straight from one end's to the other's along the segment, and
//! held at an end's past it. That is a band whose sides run straight from
//! one end's radius to the other's, with a half disc on each end, and
//! nothing else: a closed shape that a path says exactly. A stroke is the
//! union of its segments' shapes, which is one path of them all, wound
//! the same way, under the nonzero rule.
//!
//! Only a hard stroke has an edge a path can say — a soft one fades over
//! its radius — and only paint has one: an eraser takes paint off, and a
//! stroke laid inside a region is cut by it. A layer holding any of those
//! is left to go as pixels.

use chitrakar_doc::PaintStroke;

/// One piece of an outline, from wherever the one before it ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Piece {
    Line([f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
}

/// A closed outline: where it starts, and the pieces back round to there.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Outline {
    pub start: [f32; 2],
    pub pieces: Vec<Piece>,
}

/// Whether every stroke of a layer has an edge a path can say exactly.
pub(crate) fn sayable(strokes: &[PaintStroke]) -> bool {
    !strokes.is_empty()
        && strokes
            .iter()
            .all(|s| s.softness <= 0.0 && !s.erase && s.clip.is_none())
}

/// A quarter turn of a circle as a cubic, the handles this far along
/// each tangent for a radius of one. Off the true circle by less than
/// three ten-thousandths of the radius at its worst.
const QUARTER: f32 = 0.552_284_8;

/// The outlines one stroke covers, one per segment (or a single disc for
/// a stroke of one point), all wound the same way round.
pub(crate) fn outlines(stroke: &PaintStroke) -> Vec<Outline> {
    let n = stroke.points.len();
    let mut out = Vec::new();
    // One point is a single dab, which is the segment from it to itself.
    for i in 0..n.saturating_sub(1).max(1) {
        if n == 0 {
            break;
        }
        let j = (i + 1).min(n - 1);
        let (a, b) = (stroke.points[i], stroke.points[j]);
        let (ra, rb) = (stroke.radius(i).max(0.0), stroke.radius(j).max(0.0));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 1e-6 {
            // No length: a disc of the first end's radius, as the renderer
            // measures every point from `a` with `a`'s radius.
            if ra > 0.0 {
                out.push(disc(a, ra));
            }
            continue;
        }
        if ra <= 0.0 && rb <= 0.0 {
            continue;
        }
        let u = [dx / len, dy / len];
        let nrm = [-u[1], u[0]];
        let at = |c: [f32; 2], r: f32, s: [f32; 2]| [c[0] + s[0] * r, c[1] + s[1] * r];
        let neg = |v: [f32; 2]| [-v[0], -v[1]];
        // Down one side, round the far end, back up the other side, and
        // round the near end: each end's half disc on the side away from
        // the segment, its diameter the band's end.
        let mut pieces = vec![Piece::Line(at(b, rb, nrm))];
        pieces.extend(half_turn(b, rb, nrm, u));
        pieces.push(Piece::Line(at(a, ra, neg(nrm))));
        pieces.extend(half_turn(a, ra, neg(nrm), neg(u)));
        out.push(Outline {
            start: at(a, ra, nrm),
            pieces,
        });
    }
    out
}

/// Half a turn round `c` at radius `r`, from `c + r·from` through
/// `c + r·via` to `c − r·from`, as two quarter-turn cubics. `via` is a
/// quarter turn from `from` the same way round for every caller, which is
/// what keeps every outline wound alike.
fn half_turn(c: [f32; 2], r: f32, from: [f32; 2], via: [f32; 2]) -> Vec<Piece> {
    if r <= 0.0 {
        // A band that ends in a point: its two sides meet there.
        return vec![Piece::Line(c)];
    }
    let p = |v: [f32; 2]| [c[0] + v[0] * r, c[1] + v[1] * r];
    let to = [-from[0], -from[1]];
    let quarter = |s: [f32; 2], e: [f32; 2]| {
        // From `s` to `e`, a quarter turn apart: each handle runs along the
        // tangent, which at `s` points toward `e` and at `e` back to `s`.
        Piece::Cubic(
            p([s[0] + e[0] * QUARTER, s[1] + e[1] * QUARTER]),
            p([e[0] + s[0] * QUARTER, e[1] + s[1] * QUARTER]),
            p(e),
        )
    };
    vec![quarter(from, via), quarter(via, to)]
}

/// A whole circle, wound the way the bands are.
fn disc(c: [f32; 2], r: f32) -> Outline {
    let (up, ahead) = ([0.0, 1.0], [1.0, 0.0]);
    let mut pieces = half_turn(c, r, up, ahead);
    pieces.extend(half_turn(c, r, [0.0, -1.0], [-1.0, 0.0]));
    Outline {
        start: [c[0], c[1] + r],
        pieces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hard(points: &[[f32; 2]], radii: &[f32]) -> PaintStroke {
        PaintStroke {
            points: points.to_vec(),
            radii: radii.to_vec(),
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            softness: 0.0,
            erase: false,
            source: [0.0, 0.0],
            heal: false,
            clip: None,
        }
    }

    /// The signed area an outline encloses, its curves flattened finely.
    fn area(o: &Outline) -> f32 {
        let mut pts = vec![o.start];
        let mut at = o.start;
        for p in &o.pieces {
            match *p {
                Piece::Line(e) => {
                    pts.push(e);
                    at = e;
                }
                Piece::Cubic(c1, c2, e) => {
                    for k in 1..=64 {
                        let t = k as f32 / 64.0;
                        let s = 1.0 - t;
                        let w = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
                        pts.push([
                            w[0] * at[0] + w[1] * c1[0] + w[2] * c2[0] + w[3] * e[0],
                            w[0] * at[1] + w[1] * c1[1] + w[2] * c2[1] + w[3] * e[1],
                        ]);
                    }
                    at = e;
                }
            }
        }
        (0..pts.len())
            .map(|i| {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                p[0] * q[1] - p[1] * q[0]
            })
            .sum::<f32>()
            / 2.0
    }

    #[test]
    fn a_segment_is_a_band_with_a_half_disc_on_each_end() {
        let pi = std::f32::consts::PI;
        // Even: a 10 × 4 band and a whole circle's worth of ends.
        let even = outlines(&hard(&[[0.0, 0.0], [10.0, 0.0]], &[2.0]));
        assert_eq!(even.len(), 1);
        let want = 10.0 * 4.0 + pi * 4.0;
        assert!(
            (area(&even[0]).abs() - want).abs() < 0.01,
            "{}",
            area(&even[0])
        );
        // Tapering, turned: a trapezoid and two half discs.
        let taper = outlines(&hard(&[[1.0, 2.0], [7.0, 10.0]], &[3.0, 1.0]));
        let want = 10.0 * (3.0 + 1.0) + pi * (9.0 + 1.0) / 2.0;
        assert!(
            (area(&taper[0]).abs() - want).abs() < 0.02,
            "{}",
            area(&taper[0])
        );
        // A dab is a disc.
        let dab = outlines(&hard(&[[5.0, 5.0]], &[3.0]));
        assert!((area(&dab[0]).abs() - pi * 9.0).abs() < 0.01);
    }

    #[test]
    fn every_outline_is_wound_the_same_way() {
        // Segments running every which way, a dab, and a band that ends in
        // a point: nonzero unions them only if none winds against the rest.
        let s = hard(
            &[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 3.0],
                [-4.0, -6.0],
            ],
            &[2.0, 3.0, 0.0, 1.0, 2.0],
        );
        let mut all = outlines(&s);
        all.extend(outlines(&hard(&[[3.0, 3.0]], &[2.0])));
        let signs: Vec<bool> = all.iter().map(|o| area(o) > 0.0).collect();
        assert!(signs.iter().all(|s| *s == signs[0]), "{signs:?}");
    }

    #[test]
    fn only_a_hard_stroke_of_paint_is_sayable() {
        let plain = hard(&[[0.0, 0.0], [4.0, 0.0]], &[1.0]);
        assert!(sayable(std::slice::from_ref(&plain)));
        let mut soft = plain.clone();
        soft.softness = 0.3;
        let mut eraser = plain.clone();
        eraser.erase = true;
        for other in [soft, eraser] {
            assert!(!sayable(&[plain.clone(), other]));
        }
        assert!(!sayable(&[]));
    }
}
