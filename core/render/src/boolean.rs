//! Boolean operations on filled shapes.
//!
//! Both operands are reduced to rings of straight segments — curves are
//! flattened first, so a boolean result is an approximation of curved input
//! the way it is in every editor that offers one — and then combined by
//! classifying edges rather than by walking a winding number:
//!
//! 1. every edge is split at each crossing with the other shape,
//! 2. each fragment is asked whether its midpoint lies inside the other
//!    shape, which is a plain even-odd test,
//! 3. the operation says which answers to keep, and
//! 4. the kept fragments are chained back into closed rings.
//!
//! It is the least clever of the standard methods and the easiest to be
//! sure of, which is what matters when the result becomes a document the
//! user then edits. Where it cannot be sure — a chain that will not close,
//! usually because the two outlines share an edge exactly — it says so
//! rather than returning something plausible.

/// Which combination to take. Named as the buttons are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    /// Everything either shape covers.
    Union,
    /// Only what both cover.
    Intersect,
    /// The first shape with the second taken out of it.
    Subtract,
    /// Everything exactly one of them covers.
    Exclude,
}

impl BoolOp {
    pub fn from_name(name: &str) -> Option<BoolOp> {
        match name {
            "union" => Some(BoolOp::Union),
            "intersect" => Some(BoolOp::Intersect),
            "subtract" => Some(BoolOp::Subtract),
            "exclude" => Some(BoolOp::Exclude),
            _ => None,
        }
    }
}

type Point = [f32; 2];
type Ring = Vec<Point>;

/// How close two coordinates must be to count as the same point when
/// fragments are chained back together. Coordinates are document pixels,
/// so this is far below anything visible and far above the error of the
/// intersection arithmetic.
const WELD: f32 = 1e-3;

/// Even-odd containment against a set of rings.
fn covers(rings: &[Ring], p: Point) -> bool {
    let mut inside = false;
    for ring in rings {
        if ring.len() < 3 {
            continue;
        }
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let t = (p[1] - a[1]) / (b[1] - a[1]);
                if p[0] < a[0] + t * (b[0] - a[0]) {
                    inside = !inside;
                }
            }
        }
    }
    inside
}

/// Where two segments cross, as the parameter along each. Parallel or
/// touching-at-an-endpoint pairs report nothing: an endpoint crossing is
/// already a vertex, and splitting there would only make a zero-length
/// fragment.
fn crossing(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<(f32, f32)> {
    let (rx, ry) = (a1[0] - a0[0], a1[1] - a0[1]);
    let (sx, sy) = (b1[0] - b0[0], b1[1] - b0[1]);
    let denom = rx * sy - ry * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let (qpx, qpy) = (b0[0] - a0[0], b0[1] - a0[1]);
    let t = (qpx * sy - qpy * sx) / denom;
    let u = (qpx * ry - qpy * rx) / denom;
    const EPS: f32 = 1e-6;
    if (EPS..=1.0 - EPS).contains(&t) && (EPS..=1.0 - EPS).contains(&u) {
        Some((t, u))
    } else {
        None
    }
}

/// One ring's edges, each split at every crossing with `other`.
fn split_ring(ring: &Ring, other: &[Ring]) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    for i in 0..ring.len() {
        let (a0, a1) = (ring[i], ring[(i + 1) % ring.len()]);
        let mut cuts: Vec<f32> = Vec::new();
        for o in other {
            for j in 0..o.len() {
                let (b0, b1) = (o[j], o[(j + 1) % o.len()]);
                if let Some((t, _)) = crossing(a0, a1, b0, b1) {
                    cuts.push(t);
                }
            }
        }
        cuts.push(0.0);
        cuts.push(1.0);
        cuts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        let at = |t: f32| [a0[0] + (a1[0] - a0[0]) * t, a0[1] + (a1[1] - a0[1]) * t];
        for w in cuts.windows(2) {
            if w[1] - w[0] < 1e-6 {
                continue;
            }
            out.push((at(w[0]), at(w[1])));
        }
    }
    out
}

/// Chain directed fragments into closed rings, following each fragment to
/// whichever unused one starts where it ended. `None` when a chain runs out
/// of continuations before closing, which means the fragment set was not a
/// set of closed loops and no honest ring can be built from it.
fn chain(fragments: Vec<(Point, Point)>) -> Option<Vec<Ring>> {
    let key = |p: Point| ((p[0] / WELD).round() as i64, (p[1] / WELD).round() as i64);
    let mut starts: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, f) in fragments.iter().enumerate() {
        starts.entry(key(f.0)).or_default().push(i);
    }
    let mut used = vec![false; fragments.len()];
    let mut rings = Vec::new();
    for seed in 0..fragments.len() {
        if used[seed] {
            continue;
        }
        let mut ring: Ring = vec![fragments[seed].0];
        let mut at = seed;
        used[at] = true;
        loop {
            let end = fragments[at].1;
            if key(end) == key(ring[0]) {
                break;
            }
            ring.push(end);
            let next = starts.get(&key(end))?.iter().copied().find(|i| !used[*i])?;
            used[next] = true;
            at = next;
            // A ring longer than the whole fragment set is a cycle that is
            // eating its own tail; give up rather than spin.
            if ring.len() > fragments.len() + 1 {
                return None;
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    (!rings.is_empty()).then_some(rings)
}

/// Combine two sets of rings. Returns the result as rings, or `None` when
/// the outlines are degenerate enough that no closed result can be traced —
/// shapes that only touch, or share an edge exactly.
pub fn combine(a: &[Ring], b: &[Ring], op: BoolOp) -> Option<Vec<Ring>> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    // Which side of the other shape each operand contributes, and whether
    // that contribution runs backwards. Reversing is what turns the inside
    // of the subtracted shape into the wall of the hole it leaves.
    let (keep_a_inside, keep_b_inside, flip_b) = match op {
        BoolOp::Union => (false, false, false),
        BoolOp::Intersect => (true, true, false),
        BoolOp::Subtract => (false, true, true),
        BoolOp::Exclude => (false, false, false),
    };
    let mut fragments = Vec::new();
    let mid = |f: &(Point, Point)| [(f.0[0] + f.1[0]) / 2.0, (f.0[1] + f.1[1]) / 2.0];
    for f in split_ring_all(a, b) {
        if covers(b, mid(&f)) == keep_a_inside {
            fragments.push(f);
        }
    }
    for f in split_ring_all(b, a) {
        if covers(a, mid(&f)) == keep_b_inside {
            fragments.push(if flip_b { (f.1, f.0) } else { f });
        }
    }
    if op == BoolOp::Exclude {
        // Exclude is the one operation whose answer is not a selection of
        // edges: it is everything either shape covers minus everything both
        // do, which even-odd already means. So keep both outlines whole.
        let mut rings: Vec<Ring> = a.to_vec();
        rings.extend(b.iter().cloned());
        return Some(rings);
    }
    if fragments.is_empty() {
        return None;
    }
    chain(fragments)
}

fn split_ring_all(rings: &[Ring], other: &[Ring]) -> Vec<(Point, Point)> {
    rings.iter().flat_map(|r| split_ring(r, other)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, s: f32) -> Ring {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    /// How much the rings actually cover, measured the way the renderer
    /// reads them: even-odd, sampled on a fine grid. A signed or unsigned
    /// shoelace sum would not do — with even-odd rings, one inside another
    /// cancels rather than adds, and that cancelling is the answer.
    fn covered_area(rings: &[Ring]) -> f32 {
        const STEP: f32 = 0.25;
        let mut n = 0;
        let mut y = -5.0;
        while y < 30.0 {
            let mut x = -5.0;
            while x < 30.0 {
                if covers(rings, [x, y]) {
                    n += 1;
                }
                x += STEP;
            }
            y += STEP;
        }
        n as f32 * STEP * STEP
    }

    #[test]
    fn overlapping_squares_combine_by_area() {
        // Two 10x10 squares overlapping in a 5x5 corner: the arithmetic of
        // the four operations is what the areas have to say.
        let a = vec![square(0.0, 0.0, 10.0)];
        let b = vec![square(5.0, 5.0, 10.0)];
        let got = |op| covered_area(&combine(&a, &b, op).expect("combines"));
        assert!(
            (got(BoolOp::Union) - 175.0).abs() < 0.5,
            "{}",
            got(BoolOp::Union)
        );
        assert!(
            (got(BoolOp::Intersect) - 25.0).abs() < 0.5,
            "{}",
            got(BoolOp::Intersect)
        );
        assert!(
            (got(BoolOp::Subtract) - 75.0).abs() < 0.5,
            "{}",
            got(BoolOp::Subtract)
        );
        assert!(
            (got(BoolOp::Exclude) - 150.0).abs() < 0.5,
            "{}",
            got(BoolOp::Exclude)
        );
    }

    #[test]
    fn subtracting_an_enclosed_square_leaves_a_hole() {
        // Nothing crosses, so there is nothing to chain: the answer is both
        // outlines, which even-odd already reads as a shape with a hole.
        let a = vec![square(0.0, 0.0, 20.0)];
        let b = vec![square(5.0, 5.0, 5.0)];
        let out = combine(&a, &b, BoolOp::Subtract).expect("combines");
        assert_eq!(out.len(), 2, "the outline and the hole");
        assert!(
            (covered_area(&out) - 375.0).abs() < 2.0,
            "{}",
            covered_area(&out)
        );
        assert!(covers(&out, [1.0, 1.0]), "inside the shape");
        assert!(!covers(&out, [7.0, 7.0]), "and out again inside the hole");
    }

    #[test]
    fn disjoint_shapes_union_into_two_islands() {
        let a = vec![square(0.0, 0.0, 5.0)];
        let b = vec![square(20.0, 20.0, 5.0)];
        let out = combine(&a, &b, BoolOp::Union).expect("combines");
        assert!(covers(&out, [2.0, 2.0]) && covers(&out, [22.0, 22.0]));
        assert!(!covers(&out, [12.0, 12.0]), "and nothing between them");
        // Intersecting them covers nothing, which is not a shape.
        assert!(combine(&a, &b, BoolOp::Intersect).is_none());
    }
}
