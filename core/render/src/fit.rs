//! A dense outline said again in few anchors: straight where it runs
//! straight, curves with handles where it bends, corners kept as corners.
//!
//! Outlines made here by tracing, combining or offsetting arrive as
//! hundreds of short straight steps, which draw well and edit badly —
//! nobody can reshape a curve held by two hundred anchors. This finds the
//! corners first (a turn sharper than [`CORNER_TURN`] at one point), then
//! fits each stretch between two corners with as few cubic Béziers as
//! stay within a distance of it: Schneider's method, from *Graphics Gems*
//! — a least-squares fit for the two handle lengths along tangents taken
//! from the ends, the points' parameters refined by Newton's method, and
//! a split at the worst point when that is not close enough. A stretch
//! that lies along its own chord is a straight segment, with no handles,
//! so a polygon comes back a polygon.

type P = [f32; 2];

/// How sharply an outline must turn at a single point to be a corner, as
/// the cosine of the turn: anything sharper than about fifty degrees.
const CORNER_TURN: f32 = 0.64;

/// The anchors and handles (`[in_x, in_y, out_x, out_y]` from each
/// anchor, as a path holds them) that follow `points` to within
/// `tolerance`. A closed outline's last point is not repeated.
pub fn fit_curves(points: &[P], closed: bool, tolerance: f32) -> (Vec<P>, Vec<[f32; 4]>) {
    let tolerance = if tolerance.is_finite() {
        tolerance.max(0.01)
    } else {
        0.5
    };
    // Steps of no length say nothing about direction.
    let mut pts: Vec<P> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last().is_none_or(|q| dist(*q, p) > 1e-4) {
            pts.push(p);
        }
    }
    if closed && pts.len() > 1 && dist(pts[0], *pts.last().unwrap()) <= 1e-4 {
        pts.pop();
    }
    let n = pts.len();
    if n < 3 {
        return (pts.clone(), Vec::new());
    }
    // Where the outline turns sharply, it is cut: each stretch between
    // two cuts is fitted on its own and meets the next at a corner.
    let turn_at = |i: usize| -> bool {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (u, v) = (unit(sub(b, a)), unit(sub(c, b)));
        dot(u, v) < CORNER_TURN
    };
    let mut cuts: Vec<usize> = if closed {
        (0..n).filter(|&i| turn_at(i)).collect()
    } else {
        std::iter::once(0)
            .chain((1..n - 1).filter(|&i| turn_at(i)))
            .chain(std::iter::once(n - 1))
            .collect()
    };
    let mut segments: Vec<[P; 4]> = Vec::new();
    if closed && cuts.is_empty() {
        // Round and round with no corner: one stretch from a point back
        // to itself, its ends' tangent the same one.
        let mut ring = pts.clone();
        ring.push(pts[0]);
        let t = unit(sub(pts[1], pts[n - 1]));
        fit_stretch(&ring, t, [-t[0], -t[1]], tolerance, &mut segments);
    } else {
        if closed {
            cuts.push(cuts[0] + n);
        }
        for w in cuts.windows(2) {
            let stretch: Vec<P> = (w[0]..=w[1]).map(|i| pts[i % n]).collect();
            let m = stretch.len();
            let start = unit(sub(stretch[1], stretch[0]));
            let end = unit(sub(stretch[m - 2], stretch[m - 1]));
            fit_stretch(&stretch, start, end, tolerance, &mut segments);
        }
    }
    // Each segment's start is an anchor; its handles are the offsets of
    // its two control points from the anchors either side.
    let count = segments.len();
    let mut anchors: Vec<P> = segments.iter().map(|s| s[0]).collect();
    let mut handles = vec![[0.0f32; 4]; count];
    for (i, s) in segments.iter().enumerate() {
        handles[i][2] = s[1][0] - s[0][0];
        handles[i][3] = s[1][1] - s[0][1];
        let j = i + 1;
        if j < count {
            handles[j][0] = s[2][0] - s[3][0];
            handles[j][1] = s[2][1] - s[3][1];
        } else if closed {
            handles[0][0] = s[2][0] - s[3][0];
            handles[0][1] = s[2][1] - s[3][1];
        }
    }
    if !closed {
        let last = segments[count - 1];
        anchors.push(last[3]);
        handles.push([last[2][0] - last[3][0], last[2][1] - last[3][1], 0.0, 0.0]);
    }
    // A polygon needs no handles at all.
    if handles.iter().all(|h| h.iter().all(|v| v.abs() <= 1e-6)) {
        handles.clear();
    }
    (anchors, handles)
}

/// Fit one stretch, whose end tangents point into it, as few cubics as
/// stay within `tolerance` of every point.
fn fit_stretch(pts: &[P], t0: P, t1: P, tolerance: f32, out: &mut Vec<[P; 4]>) {
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    // Along its own chord: a straight segment.
    if pts.iter().all(|&p| segment_distance(p, a, b) <= tolerance) && dist(a, b) > 1e-4 {
        out.push([a, a, b, b]);
        return;
    }
    if pts.len() == 2 {
        let d = dist(a, b) / 3.0;
        out.push([a, add(a, scale(t0, d)), add(b, scale(t1, d)), b]);
        return;
    }
    let mut u = chord_lengths(pts);
    let mut bez = generate(pts, &u, t0, t1);
    let (mut worst, mut at) = max_error(pts, &bez, &u);
    if worst <= tolerance {
        out.push(bez);
        return;
    }
    // Near enough that better parameters may do it.
    if worst <= tolerance * 4.0 {
        for _ in 0..4 {
            u = reparameterize(pts, &u, &bez);
            bez = generate(pts, &u, t0, t1);
            (worst, at) = max_error(pts, &bez, &u);
            if worst <= tolerance {
                out.push(bez);
                return;
            }
        }
    }
    // Split at the worst point, the tangent there the one through its
    // neighbours, and fit each half.
    let at = at.clamp(1, pts.len() - 2);
    let centre = unit(sub(pts[at - 1], pts[at + 1]));
    fit_stretch(&pts[..=at], t0, centre, tolerance, out);
    fit_stretch(&pts[at..], [-centre[0], -centre[1]], t1, tolerance, out);
}

/// The cubic from the first point to the last along the two tangents,
/// with the handle lengths that fit the points best at parameters `u`.
fn generate(pts: &[P], u: &[f32], t0: P, t1: P) -> [P; 4] {
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &p) in pts.iter().enumerate() {
        let t = u[i];
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let a0 = scale(t0, b1);
        let a1 = scale(t1, b2);
        c00 += dot(a0, a0);
        c01 += dot(a0, a1);
        c11 += dot(a1, a1);
        let rest = sub(p, add(scale(first, b0 + b1), scale(last, b2 + b3)));
        x0 += dot(a0, rest);
        x1 += dot(a1, rest);
    }
    let det = c00 * c11 - c01 * c01;
    let chord = dist(first, last);
    let (mut l0, mut l1) = if det.abs() > 1e-9 {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (0.0, 0.0)
    };
    // A handle of no length, or pointing backwards, is a fit gone wrong;
    // a third of the chord is the usual answer then.
    let least = chord * 1e-4;
    if l0 < least || l1 < least {
        l0 = chord / 3.0;
        l1 = chord / 3.0;
    }
    [
        first,
        add(first, scale(t0, l0)),
        add(last, scale(t1, l1)),
        last,
    ]
}

/// How far the cubic strays from the points at their parameters, and at
/// which point it strays most.
fn max_error(pts: &[P], bez: &[P; 4], u: &[f32]) -> (f32, usize) {
    let (mut worst, mut at) = (0.0f32, pts.len() / 2);
    for i in 1..pts.len() - 1 {
        let d = dist(at_t(bez, u[i]), pts[i]);
        if d > worst {
            worst = d;
            at = i;
        }
    }
    (worst, at)
}

/// Each point's parameter moved to where the cubic comes nearest it, by
/// one step of Newton's method.
fn reparameterize(pts: &[P], u: &[f32], bez: &[P; 4]) -> Vec<f32> {
    pts.iter()
        .zip(u)
        .map(|(&p, &t)| {
            let q = at_t(bez, t);
            let d1 = first_derivative(bez, t);
            let d2 = second_derivative(bez, t);
            let diff = sub(q, p);
            let num = dot(diff, d1);
            let den = dot(d1, d1) + dot(diff, d2);
            if den.abs() < 1e-9 {
                t
            } else {
                (t - num / den).clamp(0.0, 1.0)
            }
        })
        .collect()
}

fn chord_lengths(pts: &[P]) -> Vec<f32> {
    let mut u = vec![0.0f32; pts.len()];
    for i in 1..pts.len() {
        u[i] = u[i - 1] + dist(pts[i - 1], pts[i]);
    }
    let total = u[pts.len() - 1].max(1e-9);
    u.iter_mut().for_each(|v| *v /= total);
    u
}

fn at_t(b: &[P; 4], t: f32) -> P {
    let s = 1.0 - t;
    let (w0, w1, w2, w3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
    [
        w0 * b[0][0] + w1 * b[1][0] + w2 * b[2][0] + w3 * b[3][0],
        w0 * b[0][1] + w1 * b[1][1] + w2 * b[2][1] + w3 * b[3][1],
    ]
}

fn first_derivative(b: &[P; 4], t: f32) -> P {
    let s = 1.0 - t;
    let d = |i: usize| {
        3.0 * s * s * (b[1][i] - b[0][i])
            + 6.0 * s * t * (b[2][i] - b[1][i])
            + 3.0 * t * t * (b[3][i] - b[2][i])
    };
    [d(0), d(1)]
}

fn second_derivative(b: &[P; 4], t: f32) -> P {
    let s = 1.0 - t;
    let d = |i: usize| {
        6.0 * s * (b[2][i] - 2.0 * b[1][i] + b[0][i])
            + 6.0 * t * (b[3][i] - 2.0 * b[2][i] + b[1][i])
    };
    [d(0), d(1)]
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn scale(a: P, k: f32) -> P {
    [a[0] * k, a[1] * k]
}
fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn dist(a: P, b: P) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn unit(a: P) -> P {
    let l = a[0].hypot(a[1]);
    if l <= 1e-12 {
        [1.0, 0.0]
    } else {
        [a[0] / l, a[1] / l]
    }
}
fn segment_distance(p: P, a: P, b: P) -> f32 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 <= 1e-12 {
        0.0
    } else {
        (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
    };
    dist(p, add(a, scale(ab, t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fitted outline, drawn out as the renderer draws a path with
    /// handles: each segment in short steps.
    fn drawn(anchors: &[P], handles: &[[f32; 4]], closed: bool) -> Vec<P> {
        let n = anchors.len();
        let segs = if closed { n } else { n - 1 };
        let mut out = Vec::new();
        for i in 0..segs {
            let j = (i + 1) % n;
            let h = |k: usize| handles.get(k).copied().unwrap_or([0.0; 4]);
            let bez = [
                anchors[i],
                [anchors[i][0] + h(i)[2], anchors[i][1] + h(i)[3]],
                [anchors[j][0] + h(j)[0], anchors[j][1] + h(j)[1]],
                anchors[j],
            ];
            for s in 0..=32 {
                out.push(at_t(&bez, s as f32 / 32.0));
            }
        }
        out
    }

    /// The furthest any point is from the polyline `line`.
    fn strays(points: &[P], line: &[P]) -> f32 {
        points
            .iter()
            .map(|&p| {
                line.windows(2)
                    .map(|w| segment_distance(p, w[0], w[1]))
                    .fold(f32::MAX, f32::min)
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn a_circle_in_four_hundred_steps_is_a_few_curves() {
        let ring: Vec<P> = (0..400)
            .map(|i| {
                let a = i as f32 / 400.0 * std::f32::consts::TAU;
                [50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin()]
            })
            .collect();
        let (anchors, handles) = fit_curves(&ring, true, 0.25);
        assert!(anchors.len() <= 8, "{} anchors", anchors.len());
        assert_eq!(handles.len(), anchors.len());
        let line = drawn(&anchors, &handles, true);
        assert!(
            strays(&ring, &line) < 0.3,
            "it strays {}",
            strays(&ring, &line)
        );
        // And the curve does not stray between the points either: every
        // drawn point is on the circle.
        for p in &line {
            let r = (p[0] - 50.0).hypot(p[1] - 50.0);
            assert!((r - 40.0).abs() < 0.3, "{r}");
        }
    }

    #[test]
    fn a_square_in_many_steps_is_four_corners_and_no_handles() {
        let mut ring: Vec<P> = Vec::new();
        for (a, b) in [
            ([0.0, 0.0], [50.0, 0.0]),
            ([50.0, 0.0], [50.0, 30.0]),
            ([50.0, 30.0], [0.0, 30.0]),
            ([0.0, 30.0], [0.0, 0.0]),
        ] {
            for s in 0..25 {
                let t = s as f32 / 25.0;
                ring.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
        let (anchors, handles) = fit_curves(&ring, true, 0.25);
        assert_eq!(anchors.len(), 4, "{anchors:?}");
        assert!(handles.is_empty(), "a polygon has no handles: {handles:?}");
        for corner in [[0.0, 0.0], [50.0, 0.0], [50.0, 30.0], [0.0, 30.0]] {
            assert!(
                anchors.iter().any(|a| dist(*a, corner) < 1e-3),
                "{corner:?} in {anchors:?}"
            );
        }
    }

    #[test]
    fn a_shape_with_corners_and_curves_keeps_both() {
        // A D: a straight back and a half circle, meeting at two corners.
        let mut ring: Vec<P> = (0..=60)
            .map(|i| [10.0, 90.0 - i as f32 * (80.0 / 60.0)])
            .collect();
        ring.extend((1..200).map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 200.0 * std::f32::consts::PI;
            [10.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin()]
        }));
        let (anchors, handles) = fit_curves(&ring, true, 0.25);
        // Two corners, and the half circle between them in two curves.
        // Found by splitting at the worst point instead, the corners still
        // come out as anchors, but the curve leaving each starts along the
        // wrong tangent and takes seven anchors to say.
        assert!(anchors.len() <= 4, "{} anchors", anchors.len());
        let line = drawn(&anchors, &handles, true);
        assert!(
            strays(&ring, &line) < 0.3,
            "it strays {}",
            strays(&ring, &line)
        );
        // The two corners are anchors, and sharp: the back is straight.
        for corner in [[10.0, 10.0], [10.0, 90.0]] {
            assert!(
                anchors.iter().any(|a| dist(*a, corner) < 0.01),
                "{corner:?} in {anchors:?}"
            );
        }
    }

    #[test]
    fn an_open_line_keeps_its_ends() {
        let wave: Vec<P> = (0..300)
            .map(|i| {
                let x = i as f32 / 3.0;
                [x, 20.0 + 10.0 * (x / 12.0).sin()]
            })
            .collect();
        let (anchors, handles) = fit_curves(&wave, false, 0.2);
        assert!(anchors.len() < 20, "{} anchors", anchors.len());
        assert_eq!(anchors[0], wave[0]);
        assert_eq!(*anchors.last().unwrap(), wave[299]);
        let line = drawn(&anchors, &handles, false);
        assert!(
            strays(&wave, &line) < 0.25,
            "it strays {}",
            strays(&wave, &line)
        );
    }

    #[test]
    fn random_outlines_are_followed_within_the_tolerance() {
        let mut seed = 99u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / (1u64 << 31) as f32
        };
        for case in 0..60 {
            // A wobbly ring: a circle with a few random harmonics, some
            // with a corner or two pushed into it.
            let k = 1 + (rnd() * 4.0) as usize;
            let amps: Vec<(f32, f32)> = (0..k).map(|_| (rnd() * 8.0, rnd() * 6.0)).collect();
            let n = 150 + (rnd() * 300.0) as usize;
            let ring: Vec<P> = (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    let r = 40.0
                        + amps
                            .iter()
                            .enumerate()
                            .map(|(h, (amp, ph))| amp * ((h as f32 + 2.0) * a + ph).sin())
                            .sum::<f32>();
                    [60.0 + r * a.cos(), 60.0 + r * a.sin()]
                })
                .collect();
            let tol = 0.15 + rnd() * 0.6;
            let (anchors, handles) = fit_curves(&ring, true, tol);
            assert!(
                anchors.len() < n / 3,
                "case {case}: {} anchors of {n}",
                anchors.len()
            );
            let line = drawn(&anchors, &handles, true);
            let s = strays(&ring, &line);
            assert!(s <= tol * 1.2 + 0.02, "case {case}: strays {s} past {tol}");
            // Nor does the curve bulge away from the outline between them.
            let mut closed_ring = ring.clone();
            closed_ring.push(ring[0]);
            let b = strays(&line, &closed_ring);
            assert!(b <= tol * 1.5 + 0.05, "case {case}: bulges {b} past {tol}");
        }
    }
}
