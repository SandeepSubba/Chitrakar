//! One outline turned into another by degrees: what a blend between two
//! shapes is made of.
//!
//! Two outlines say nothing about which point of one is which point of
//! the other — a square has four corners and a circle none — so both are
//! walked at equal steps of their own length into the same number of
//! points, and then matched: wound the same way round, and the second
//! turned to start where it lines up best with the first, judged about
//! each one's own middle so that where the two shapes stand on the page
//! does not decide how they twist. Between them, a shape is each pair of
//! points the same share of the way from one to the other.

type P = [f32; 2];

/// How many points each outline is walked into. A shape between two is
/// fitted with curves afterwards, so this is how finely it follows the
/// two, not how many anchors it ends up with.
pub const MORPH_POINTS: usize = 128;

/// The two outlines as `MORPH_POINTS` points each, point for point.
pub fn matched(a: &[P], b: &[P]) -> Option<(Vec<P>, Vec<P>)> {
    if a.len() < 3 || b.len() < 3 {
        return None;
    }
    let n = MORPH_POINTS;
    let pa = walked(a, n)?;
    let mut pb = walked(b, n)?;
    if (area2(&pa) > 0.0) != (area2(&pb) > 0.0) {
        pb.reverse();
    }
    let (ca, cb) = (middle(&pa), middle(&pb));
    let off = (0..n)
        .min_by(|&j, &k| {
            let cost = |s: usize| -> f32 {
                (0..n)
                    .map(|i| {
                        let (p, q) = (pa[i], pb[(i + s) % n]);
                        let dx = (p[0] - ca[0]) - (q[0] - cb[0]);
                        let dy = (p[1] - ca[1]) - (q[1] - cb[1]);
                        dx * dx + dy * dy
                    })
                    .sum()
            };
            cost(j).total_cmp(&cost(k)).then(j.cmp(&k))
        })
        .unwrap_or(0);
    let pb = (0..n).map(|i| pb[(i + off) % n]).collect();
    Some((pa, pb))
}

/// The outline `t` of the way from one matched outline to the other.
pub fn between(a: &[P], b: &[P], t: f32) -> Vec<P> {
    a.iter()
        .zip(b)
        .map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t])
        .collect()
}

/// A closed outline walked at `n` equal steps of its own length, from its
/// first point.
fn walked(ring: &[P], n: usize) -> Option<Vec<P>> {
    let m = ring.len();
    let seg = |i: usize| {
        let (p, q) = (ring[i], ring[(i + 1) % m]);
        ((q[0] - p[0]).hypot(q[1] - p[1]), p, q)
    };
    let total: f32 = (0..m).map(|i| seg(i).0).sum();
    if !total.is_finite() || total <= 1e-6 {
        return None;
    }
    let step = total / n as f32;
    let mut out = Vec::with_capacity(n);
    let (mut i, mut along) = (0usize, 0f32);
    for k in 0..n {
        let want = k as f32 * step;
        // Forward to the segment that holds this distance.
        while i < m - 1 && along + seg(i).0 < want {
            along += seg(i).0;
            i += 1;
        }
        let (len, p, q) = seg(i);
        let f = if len > 1e-9 {
            ((want - along) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]);
    }
    Some(out)
}

fn area2(ring: &[P]) -> f32 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

fn middle(ring: &[P]) -> P {
    let n = ring.len().max(1) as f32;
    let s = ring
        .iter()
        .fold([0.0, 0.0], |s, p| [s[0] + p[0], s[1] + p[1]]);
    [s[0] / n, s[1] / n]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, side: f32) -> Vec<P> {
        vec![[x, y], [x + side, y], [x + side, y + side], [x, y + side]]
    }

    #[test]
    fn halfway_between_two_squares_is_the_square_between_them() {
        // The second square starts at another corner and runs the other
        // way round: matching undoes both.
        let a = square(0.0, 0.0, 10.0);
        let mut b = square(40.0, 20.0, 30.0);
        b.rotate_left(2);
        b.reverse();
        let (pa, pb) = matched(&a, &b).unwrap();
        assert_eq!((pa.len(), pb.len()), (MORPH_POINTS, MORPH_POINTS));
        let mid = between(&pa, &pb, 0.5);
        let (lo, hi) = mid
            .iter()
            .fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| {
                (
                    [lo[0].min(p[0]), lo[1].min(p[1])],
                    [hi[0].max(p[0]), hi[1].max(p[1])],
                )
            });
        assert!(
            (lo[0] - 20.0).abs() < 0.01
                && (lo[1] - 10.0).abs() < 0.01
                && (hi[0] - 40.0).abs() < 0.01
                && (hi[1] - 30.0).abs() < 0.01,
            "a square of twenty at (20, 10): {lo:?} {hi:?}"
        );
        // And it is a square, not a twisted bow: every point on its edge.
        for p in &mid {
            let on = (p[0] - 20.0).abs() < 0.01
                || (p[0] - 40.0).abs() < 0.01
                || (p[1] - 10.0).abs() < 0.01
                || (p[1] - 30.0).abs() < 0.01;
            assert!(on, "{p:?} is on the square's edge");
        }
        assert_eq!(between(&pa, &pb, 0.0), pa);
        assert_eq!(between(&pa, &pb, 1.0), pb);
        assert!(matched(&a[..2], &b).is_none(), "a line is no outline");
    }
}
