//! Node types of the scene graph. Every node is a live object described by
//! parameters — nothing here ever stores baked pixels except the immutable
//! source resource a [`RasterRef`] points at.

use crate::NodeId;
use chitrakar_color::AuthoredColor;
use serde::{Deserialize, Serialize};

/// How a layer's colour meets what is under it. The names and the
/// arithmetic are the W3C compositing spec's, which is what SVG's
/// `mix-blend-mode` and PDF's `/BM` say too — so a page blends the same
/// way in the engine and in what it exports.
///
/// Additive: the three that existed keep their names, and a document
/// written before the rest still reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    /// Multiply where the backdrop is dark and screen where it is light:
    /// contrast, judged by what is underneath.
    Overlay,
    Darken,
    Lighten,
    /// Brightens the backdrop towards the layer's colour.
    ColorDodge,
    /// Darkens it towards the layer's colour.
    ColorBurn,
    /// Overlay with the two swapped: contrast judged by the layer.
    HardLight,
    /// A gentler hard light, as if the layer were a diffused lamp.
    SoftLight,
    Difference,
    /// A softer difference, with no black at the middle.
    Exclusion,
    /// The four that take one part of a colour and leave the rest: the
    /// layer's hue on the backdrop's brightness, and so on.
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// 2D affine transform (row-major 2×3: `[a c e; b d f]` maps column vector
/// `(x, y, 1)`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }
}

impl Transform {
    /// `self` applied after `inner`: the inner transform runs first, then
    /// this one. That is what nesting means, so it is how a group's
    /// transform reaches its children — and how ungrouping folds the
    /// group's transform into them.
    pub fn compose(self, inner: Transform) -> Transform {
        // `+ 0.0` on each: it changes no value a transform can hold
        // except a negative zero, which it makes positive. Multiplying by
        // a quarter turn or a mirror produces them — a page turned right
        // and then left came back to a transform equal to the one it
        // started with and written differently, so a document nothing had
        // been done to saved to different bytes. Everything else about
        // this file is careful to save the same work as the same bytes;
        // this is the same care, one operation earlier.
        Transform {
            a: self.a * inner.a + self.c * inner.b + 0.0,
            b: self.b * inner.a + self.d * inner.b + 0.0,
            c: self.a * inner.c + self.c * inner.d + 0.0,
            d: self.b * inner.c + self.d * inner.d + 0.0,
            e: self.a * inner.e + self.c * inner.f + self.e + 0.0,
            f: self.b * inner.e + self.d * inner.f + self.f + 0.0,
        }
    }

    pub fn translation(x: f32, y: f32) -> Self {
        Self {
            e: x,
            f: y,
            ..Default::default()
        }
    }

    /// The longer of the two directions this transform stretches by.
    ///
    /// What a length written in one space is worth in the next, for the
    /// lengths that have no direction of their own to be carried along —
    /// how far a mask's edge is softened over, how far a shadow is
    /// blurred, how wide an outline is drawn. A transform that stretches
    /// the two axes by different amounts cannot be answered honestly by
    /// one number, and neither can a round blur: the longer of the two
    /// is the answer the renderer already gives when it turns those
    /// figures into device pixels, so it is the answer used when they
    /// move between spaces.
    pub fn max_scale(self) -> f32 {
        self.a.hypot(self.b).max(self.c.hypot(self.d))
    }
}

/// One stop on a colour ramp: where it sits along the gradient (0..=1) and
/// the colour there. Stops are authored colours like any other fill, so a
/// CMYK document's gradients resolve through its press profile too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: AuthoredColor,
}

/// A gradient fill. Geometry is in the shape's own local bounding box
/// normalized to 0..1 on each axis (SVG's objectBoundingBox units), so a
/// gradient follows its shape when the shape moves or is resized without
/// needing its own transform to be kept in step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gradient {
    /// Ramp along the line from `from` to `to`; past either end, what
    /// `spread` says.
    Linear {
        from: [f32; 2],
        to: [f32; 2],
        stops: Vec<GradientStop>,
        #[serde(default)]
        spread: Spread,
    },
    /// Ramp outward from `center` to `radius`, clamped past the edge —
    /// in the box's own units, 0..1 each way, so on a box that is not
    /// square the rings are ellipses that follow it.
    Radial {
        center: [f32; 2],
        radius: f32,
        stops: Vec<GradientStop>,
        /// A 2×2 matrix, `[a, b, c, d]` by rows, that the offset from
        /// the centre goes through before its length is taken: the ramp
        /// is at `|A·(q − center)| / radius`. What lets the rings be
        /// circles on the page whatever the box, or ellipses at an angle
        /// — a file's radial gradient in its own units, or turned by a
        /// transform of its own. `None`, every older file, is the
        /// identity: rings in the box's units, as above.
        #[serde(default)]
        axes: Option<[f32; 4]>,
        #[serde(default)]
        spread: Spread,
        /// Where the rings start from, in the same units as `center`:
        /// the ramp's first colour is a point here rather than at the
        /// centre, and each ring between is a circle (through the axes)
        /// whose middle has moved along from it toward the centre —
        /// SVG's `fx`/`fy`, a highlight off to one side. A focus at or
        /// past the outer ring is held just inside it (`focal_offset`),
        /// as SVG 1.1 says, so every ring still goes all the way round.
        /// `None`, every older file, is the centre itself.
        #[serde(default)]
        focus: Option<[f32; 2]>,
    },
    /// Ramp once round `center`, starting at `angle` degrees and running
    /// clockwise on the page — an angular gradient, a colour wheel, the
    /// sheen on a dial. In the box's units like the other two, so on a
    /// box that is not square the turn is measured in the box rather
    /// than on the page. A full turn has no end to spread past, so a
    /// conic one pads; the seam where the last colour meets the first is
    /// the line out from the centre at `angle`.
    Conic {
        center: [f32; 2],
        #[serde(default)]
        angle: f32,
        stops: Vec<GradientStop>,
    },
}

/// Where round a conic gradient a point of its box lies, `0..1` from
/// `angle` (degrees, clockwise on the page from pointing right) — the
/// renderers' one reading of it. A point on the centre is at the start,
/// and an angle that is no number is nought.
pub fn conic_turn(center: [f32; 2], angle: f32, u: f32, v: f32) -> f32 {
    let (dx, dy) = (u - center[0], v - center[1]);
    if dx == 0.0 && dy == 0.0 {
        return 0.0;
    }
    let start = if angle.is_finite() { angle } else { 0.0 };
    let a = dy.atan2(dx).to_degrees() - start;
    let t = (a / 360.0).rem_euclid(1.0);
    if t.is_finite() {
        t
    } else {
        0.0
    }
}

/// How far in from the outer ring a focus is held, as a share of it: on
/// the ring itself, half the plane would have no ring through it.
pub const FOCUS_REACH: f32 = 0.998;

/// A radial gradient's focus as the renderers measure it: the offset
/// from the centre through the axes, in units of the radius, held inside
/// the outer ring (`FOCUS_REACH`). Zero when there is no focus.
pub fn focal_offset(
    center: [f32; 2],
    radius: f32,
    axes: Option<[f32; 4]>,
    focus: Option<[f32; 2]>,
) -> [f32; 2] {
    let Some(f) = focus else {
        return [0.0, 0.0];
    };
    if radius.abs() < 1e-12 || !radius.is_finite() {
        return [0.0, 0.0];
    }
    let (du, dv) = (f[0] - center[0], f[1] - center[1]);
    let a = axes.unwrap_or([1.0, 0.0, 0.0, 1.0]);
    let (x, y) = (
        (a[0] * du + a[1] * dv) / radius,
        (a[2] * du + a[3] * dv) / radius,
    );
    let len = x.hypot(y);
    if !len.is_finite() {
        return [0.0, 0.0];
    }
    if len > FOCUS_REACH {
        [x / len * FOCUS_REACH, y / len * FOCUS_REACH]
    } else {
        [x, y]
    }
}

/// Where along a radial ramp a point lies, `p` and the focus `f` both
/// measured from the centre through the axes in units of the radius
/// (`focal_offset`): the `t` whose ring — a circle of radius `t` about
/// `f + t·(0 − f)` — passes through `p`. With the focus at the centre it
/// is `|p|`, the plain radial.
pub fn focal_ramp(p: [f32; 2], f: [f32; 2]) -> f32 {
    let w = [p[0] - f[0], p[1] - f[1]];
    let ww = w[0] * w[0] + w[1] * w[1];
    let wf = w[0] * f[0] + w[1] * f[1];
    let k = 1.0 - (f[0] * f[0] + f[1] * f[1]);
    // `k t² − 2 (w·f) t − |w|² = 0`, its root that is not negative; said
    // the way that does not lose the small answers behind the focus.
    let root = (wf * wf + k * ww).max(0.0).sqrt();
    if wf >= 0.0 {
        (wf + root) / k
    } else if root - wf > 1e-30 {
        ww / (root - wf)
    } else {
        0.0
    }
}

/// What a gradient does past the end of its ramp — SVG's `spreadMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Spread {
    /// The end colour carries on: every gradient before there was a
    /// choice, and SVG's default.
    #[default]
    Pad,
    /// The ramp runs back the other way, and forward again, and so on.
    Reflect,
    /// The ramp starts again from its beginning.
    Repeat,
}

impl Spread {
    /// Where along the ramp, `0..=1`, a position `t` on the gradient's
    /// line or out from its centre lands.
    pub fn place(self, t: f32) -> f32 {
        if !t.is_finite() {
            return 0.0;
        }
        match self {
            Spread::Pad => t.clamp(0.0, 1.0),
            Spread::Repeat => t.rem_euclid(1.0),
            Spread::Reflect => {
                let u = t.rem_euclid(2.0);
                if u > 1.0 {
                    2.0 - u
                } else {
                    u
                }
            }
        }
    }
}

/// The inverse of a radial gradient's axes (`Gradient::Radial::axes`),
/// by rows like them, or `None` when they flatten everything to a line.
pub fn invert_axes(a: [f32; 4]) -> Option<[f32; 4]> {
    let det = a[0] * a[3] - a[1] * a[2];
    (det.abs() > 1e-12 && det.is_finite())
        .then(|| [a[3] / det, -a[1] / det, -a[2] / det, a[0] / det])
}

impl Gradient {
    /// What the gradient does past the end of its ramp.
    pub fn spread(&self) -> Spread {
        match self {
            Gradient::Linear { spread, .. } | Gradient::Radial { spread, .. } => *spread,
            Gradient::Conic { .. } => Spread::Pad,
        }
    }

    pub fn stops(&self) -> &[GradientStop] {
        match self {
            Gradient::Linear { stops, .. }
            | Gradient::Radial { stops, .. }
            | Gradient::Conic { stops, .. } => stops,
        }
    }

    pub fn stops_mut(&mut self) -> &mut [GradientStop] {
        match self {
            Gradient::Linear { stops, .. }
            | Gradient::Radial { stops, .. }
            | Gradient::Conic { stops, .. } => stops,
        }
    }
}

/// One stroke of paint: the line it was drawn along in the layer's own
/// space, the brush radius at each of its points, the colour it lays
/// down, how far in from the rim its edge fades, and whether it paints
/// or takes paint away.
///
/// The region it covers is the union of round-capped segments between
/// consecutive points — the same shape a stroked path covers — with the
/// coverage falling off across the soft edge instead of stopping dead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaintStroke {
    pub points: Vec<[f32; 2]>,
    /// Radius at each point. A shorter list repeats its last entry, so a
    /// stroke of one radius needs only one.
    pub radii: Vec<f32>,
    pub color: AuthoredColor,
    /// How much of the radius the edge fades over: 0 is a hard edge and
    /// 1 a brush with no solid core at all. Additive, so a stroke
    /// written before softness existed reads as a hard one.
    #[serde(default)]
    pub softness: f32,
    /// Takes paint off the layer instead of laying it down.
    #[serde(default)]
    pub erase: bool,
    /// Where a clone stroke takes its pixels from, as an offset in the
    /// layer's own space: the point it paints at plus this is the point
    /// it reads. Only a clone layer reads it — a brush lays its own
    /// colour and has nowhere to read from — and it is additive, so a
    /// stroke written before cloning existed reads as no offset at all.
    #[serde(default)]
    pub source: [f32; 2],
    /// Heal rather than clone: lay down the texture from the source but
    /// the colour of the place it lands in, so a patch lifted from
    /// somewhere lighter or darker still sits into its surroundings.
    /// Only a clone layer reads it, and it is additive.
    #[serde(default)]
    pub heal: bool,
    /// The region this stroke was confined to when it was laid down, in
    /// the stroke's own space.
    ///
    /// Painting with a region picked out has to stay inside it *after*
    /// the region is let go of — that is what confining means, and a
    /// stroke held to whatever happens to be picked at the moment it is
    /// drawn would spill the instant the selection changed. So the
    /// region rides on the stroke.
    ///
    /// It is not baked, though: nothing was cut out, and taking the clip
    /// off gives back the whole stroke. Additive, so a stroke painted
    /// before selections existed is confined to nothing, which is what
    /// it was.
    #[serde(default)]
    pub clip: Option<Box<Mask>>,
}

impl PaintStroke {
    /// Every colour written on the stroke: the one it lays down, and the
    /// ones inside the region it is confined to.
    pub fn each_color_mut(&mut self, f: &mut impl FnMut(&mut AuthoredColor)) {
        f(&mut self.color);
        if let Some(held) = &mut self.clip {
            held.each_color_mut(f);
        }
    }

    /// The brush radius at point `i`, which is the last one given for
    /// every point past the end of the list.
    pub fn radius(&self, i: usize) -> f32 {
        match self.radii.len() {
            0 => 0.0,
            n => self.radii[i.min(n - 1)],
        }
    }

    /// The box the stroke covers in the layer's own space, or nothing
    /// when it covers none.
    pub fn bounds(&self) -> Option<[f32; 4]> {
        self.bounds_from(0)
    }

    /// The box the stroke covers from point `from` onward — what a
    /// stroke still being drawn adds each time it grows.
    pub fn bounds_from(&self, from: usize) -> Option<[f32; 4]> {
        let mut box_: Option<[f32; 4]> = None;
        for (i, p) in self.points.iter().enumerate().skip(from) {
            let r = self.radius(i).max(0.0);
            let b = [p[0] - r, p[1] - r, p[0] + r, p[1] + r];
            box_ = Some(match box_ {
                None => b,
                Some(a) => [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ],
            });
        }
        box_.filter(|b| b[2] > b[0] && b[3] > b[1])
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VectorShape {
    Rect {
        width: f32,
        height: f32,
        /// Corner radius, clamped at render time to half the shorter side.
        /// Additive: a document written before rounded corners existed
        /// loads with square ones, which is what it had.
        #[serde(default)]
        radius: f32,
    },
    Ellipse {
        rx: f32,
        ry: f32,
    },
    /// Anchor polyline. With `smooth`, rendering interpolates a Catmull-Rom
    /// spline through the anchors instead of straight segments.
    Path {
        points: Vec<[f32; 2]>,
        closed: bool,
        #[serde(default)]
        smooth: bool,
        /// Per-anchor bezier control offsets, `[in_x, in_y, out_x, out_y]`
        /// relative to the anchor. Empty means none, which is why old files
        /// keep loading; when present they are explicit and override
        /// `smooth`, whose curve is inferred rather than authored.
        #[serde(default)]
        handles: Vec<[f32; 4]>,
        /// Extra closed rings drawn with the same fill. Coverage is
        /// even-odd across all of them, so a ring inside another cuts a
        /// hole and a ring beside it is a second island — which is what
        /// makes a compound path, and what a boolean operation produces.
        /// Straight-sided: only the main ring carries curves. Additive.
        #[serde(default)]
        subpaths: Vec<Vec<[f32; 2]>>,
    },
    /// A regular polygon, or a star: `points` corners (3..=100) round an
    /// ellipse of `rx` by `ry`, in the same box an ellipse of that size
    /// fills (its centre at `rx, ry`), the first corner straight up. With `inner` under one it is a star, every
    /// other corner pulled in to that share of the way out — so a star of
    /// five has ten corners and `points` is still five. Live, as a
    /// rectangle's corner radius is: the count and the depth stay numbers
    /// to change rather than becoming a path nobody can turn back.
    Polygon {
        rx: f32,
        ry: f32,
        points: u32,
        #[serde(default = "whole")]
        inner: f32,
        /// Each corner rounded off by an arc of this radius, as far as
        /// the sides either side of it let it go ([`round_corners`]).
        /// Additive: a polygon written before this had sharp corners,
        /// which is what nothing means.
        #[serde(default)]
        corner: f32,
    },
}

fn whole() -> f32 {
    1.0
}

/// The most corners a polygon is drawn with, and the fewest.
pub const POLYGON_MOST: u32 = 100;
pub const POLYGON_FEWEST: u32 = 3;

impl VectorShape {
    /// A polygon's corners, in order round it from the top, in its box:
    /// `points` of them, or twice that for a star. Numbers that are no numbers are
    /// held to what can be drawn — a count to 3..=100, a depth to
    /// 0..=1, a size to nothing — rather than read as something else.
    pub fn polygon_corners(rx: f32, ry: f32, points: u32, inner: f32) -> Vec<[f32; 2]> {
        let n = points.clamp(POLYGON_FEWEST, POLYGON_MOST) as usize;
        let size = |v: f32| if v.is_finite() { v.abs() } else { 0.0 };
        let (rx, ry) = (size(rx), size(ry));
        let inner = if inner.is_finite() {
            inner.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let star = inner < 1.0;
        let count = if star { 2 * n } else { n };
        (0..count)
            .map(|i| {
                let k = if star && i % 2 == 1 { inner } else { 1.0 };
                let a = std::f32::consts::TAU * i as f32 / count as f32;
                let (s, c) = a.sin_cos();
                [rx + rx * k * s, ry - ry * k * c]
            })
            .collect()
    }

    /// The same shape as a path, where it is one a path can be: a
    /// polygon is the closed path through its corners. Everything that
    /// knows how to draw, export or test a path then knows a polygon.
    pub fn as_path(&self) -> Option<VectorShape> {
        match self {
            VectorShape::Polygon {
                rx,
                ry,
                points,
                inner,
                corner,
            } => {
                let corners = Self::polygon_corners(*rx, *ry, *points, *inner);
                // Rounded, the corners are arcs and the path carries the
                // handles that draw them.
                let (points, handles) = match round_corners(&corners, &[], true, *corner, None) {
                    Some(r) => (r.points, r.handles),
                    None => (corners, Vec::new()),
                };
                Some(VectorShape::Path {
                    points,
                    closed: true,
                    smooth: false,
                    handles,
                    subpaths: Vec::new(),
                })
            }
            VectorShape::Rect { .. } | VectorShape::Ellipse { .. } | VectorShape::Path { .. } => {
                None
            }
        }
    }
}

/// A path with corners rounded off: its anchors, their handles, and for
/// each anchor the one of the old path it came from — a corner rounded is
/// two, and anything kept per anchor, a brushed line's widths, goes with
/// both.
#[derive(Debug, Clone, PartialEq)]
pub struct Rounded {
    pub points: Vec<[f32; 2]>,
    pub handles: Vec<[f32; 4]>,
    pub from: Vec<usize>,
}

/// The sharp corners of a path rounded off by arcs of `radius` — every
/// one, or those `only` names. A corner is an anchor with no handles
/// whose sides either way are straight; it becomes two anchors where an
/// arc of the radius would touch those sides, joined by the cubic that
/// draws the arc (handles `4/3·tan(sweep/4)` of the radius long, which
/// strays from the circle by under three parts in ten thousand). A corner
/// takes no more than half of either side, so two rounded corners on one
/// side meet at its middle rather than crossing, and a radius too big for
/// the corner is the biggest that fits. An end of an open path is no
/// corner, and nor is an anchor its sides run straight through.
///
/// `None` when nothing was rounded. Handles are offsets from their
/// anchor, `[in_x, in_y, out_x, out_y]`; `handles` may be empty for a
/// path drawn with none.
pub fn round_corners(
    points: &[[f32; 2]],
    handles: &[[f32; 4]],
    closed: bool,
    radius: f32,
    only: Option<&[usize]>,
) -> Option<Rounded> {
    let n = points.len();
    if n < 3 || !radius.is_finite() || radius <= 0.0 || (!handles.is_empty() && handles.len() != n)
    {
        return None;
    }
    let handle = |i: usize| handles.get(i).copied().unwrap_or([0.0; 4]);
    let flat = |v: f32, w: f32| v.abs() <= 1e-6 && w.abs() <= 1e-6;
    let mut out = Rounded {
        points: Vec::with_capacity(n + 8),
        handles: Vec::with_capacity(n + 8),
        from: Vec::with_capacity(n + 8),
    };
    let mut any = false;
    for i in 0..n {
        let p = points[i];
        let keep = |out: &mut Rounded| {
            out.points.push(p);
            out.handles.push(handle(i));
            out.from.push(i);
        };
        let end = !closed && (i == 0 || i == n - 1);
        if end || only.is_some_and(|o| !o.contains(&i)) {
            keep(&mut out);
            continue;
        }
        let (prev, next) = ((i + n - 1) % n, (i + 1) % n);
        let (h, hp, hn) = (handle(i), handle(prev), handle(next));
        // Sharp, and straight either side: no handle here, none on the
        // neighbours' ends that face it.
        let sharp =
            flat(h[0], h[1]) && flat(h[2], h[3]) && flat(hp[2], hp[3]) && flat(hn[0], hn[1]);
        let (a, b) = (points[prev], points[next]);
        let (ua, ub) = ([a[0] - p[0], a[1] - p[1]], [b[0] - p[0], b[1] - p[1]]);
        let (la, lb) = (ua[0].hypot(ua[1]), ub[0].hypot(ub[1]));
        if !sharp || la <= 1e-6 || lb <= 1e-6 {
            keep(&mut out);
            continue;
        }
        let (u, v) = ([ua[0] / la, ua[1] / la], [ub[0] / lb, ub[1] / lb]);
        // The angle at the corner, between the two sides.
        let theta = (u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0).acos();
        if !(1e-3..std::f32::consts::PI - 1e-3).contains(&theta) {
            keep(&mut out);
            continue;
        }
        let half = (theta / 2.0).tan();
        let t = (radius / half).min(la.min(lb) / 2.0);
        if t <= 1e-4 {
            keep(&mut out);
            continue;
        }
        let r = t * half;
        let sweep = std::f32::consts::PI - theta;
        let k = 4.0 / 3.0 * (sweep / 4.0).tan() * r;
        let t1 = [p[0] + u[0] * t, p[1] + u[1] * t];
        let t2 = [p[0] + v[0] * t, p[1] + v[1] * t];
        out.points.extend([t1, t2]);
        out.handles.extend([
            [0.0, 0.0, -u[0] * k, -u[1] * k],
            [-v[0] * k, -v[1] * k, 0.0, 0.0],
        ]);
        out.from.extend([i, i]);
        any = true;
    }
    any.then_some(out)
}

/// Reference to an immutable pixel resource embedded in the document
/// (`resources/` in the .chitra container). Content-addressed so identical
/// placements share bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RasterRef {
    pub resource_id: String,
    pub width: u32,
    pub height: u32,
    /// Where the picture's four corners land in the layer's own space —
    /// top left, top right, bottom right, bottom left — when they are
    /// not where its rectangle puts them: a picture distorted, or put
    /// into perspective, as a photograph of a sign is squared up or a
    /// screen is laid onto a photograph of a laptop. `None` is the
    /// rectangle `0..width × 0..height`, which is every picture until a
    /// corner is moved, and every file written before one could be.
    ///
    /// The picture is carried across by the one projective map that
    /// takes its rectangle to these four points ([`Projective`]), so
    /// straight lines in it stay straight. A quad that is not convex
    /// has no such map that keeps the picture whole, and is drawn as if
    /// the corners were not there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<[[f32; 2]; 4]>,
    /// The camera's raw file this picture was developed from, and how —
    /// so it can be developed again. `None` for every other picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub develop: Option<Developed>,
}

/// A picture developed from a camera's raw file: the file, kept in the
/// document's originals by its content id, and the settings it was
/// developed with. Developing again makes a new picture and points the
/// layer at it; the raw file is never changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Developed {
    pub original: String,
    pub settings: RawDevelop,
}

/// How a camera's raw file is developed — what can only be done before it
/// is pixels. Every value at zero (and no white balance) is the plain
/// development: the camera's own balance, the brightest hundredth at
/// white. The tone settings run -100 to 100, as Lightroom's do.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RawDevelop {
    /// The light's temperature in kelvin and a tint (green minus,
    /// magenta plus, -150 to 150); `None` is as the camera saw it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub white: Option<[f32; 2]>,
    /// In stops, on top of the plain development's own exposure.
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub vibrance: f32,
    pub saturation: f32,
    /// Detail: an unsharp mask on the brightness, 0 to 150.
    pub sharpen: f32,
    /// Detail: edge-aware smoothing, colour hardest, 0 to 100.
    pub noise: f32,
    /// Leave off the lens corrections the camera wrote into the file
    /// (distortion, colour fringes, dark corners — a Fuji's RAF carries
    /// them), which are otherwise applied.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub uncorrected: bool,
    /// The tones a camera's own pictures have, laid under the settings
    /// above: deeper shadows and a soft roll into white about the
    /// photograph's own middle (`develop::base_curve`). Off for a picture
    /// developed before there was one, so it develops again as it was;
    /// the develop window starts new ones with it on.
    pub curve: bool,
}

impl RasterRef {
    /// The four corners in the layer's own space: the moved ones, or the
    /// rectangle's.
    pub fn quad(&self) -> [[f32; 2]; 4] {
        let (w, h) = (self.width as f32, self.height as f32);
        self.corners
            .unwrap_or([[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]])
    }

    /// The map from the picture's own pixels to the layer's space, or
    /// `None` where it is the rectangle — or where the corners make no
    /// convex quad, which is drawn as the rectangle too.
    pub fn projective(&self) -> Option<Projective> {
        self.projective_of(self.width, self.height)
    }

    /// The same map from a picture `width × height` pixels — the
    /// resource's own size, which is what a renderer samples, should a
    /// file say one thing here and the resource another.
    pub fn projective_of(&self, width: u32, height: u32) -> Option<Projective> {
        let corners = self.corners?;
        let p = Projective::onto(corners)?;
        // From texel space: the unit square first.
        Some(p.after_scale(1.0 / width.max(1) as f64, 1.0 / height.max(1) as f64))
    }
}

/// A projective map of the plane: `(x, y)` goes to
/// `((m0 x + m1 y + m2) / w, (m3 x + m4 y + m5) / w)` with
/// `w = m6 x + m7 y + m8`. The one that takes a square to any convex
/// quad; an affine transform is the case `m6 = m7 = 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projective {
    pub m: [f64; 9],
}

impl Projective {
    /// The map taking the unit square — (0,0), (1,0), (1,1), (0,1) — to
    /// the quad `q` in that order, or `None` when `q` is not strictly
    /// convex (a fold, a bow tie, three corners on a line), where any
    /// such map sends part of the square through infinity.
    pub fn onto(q: [[f32; 2]; 4]) -> Option<Self> {
        if !convex(q) {
            return None;
        }
        let [x0, y0] = q[0].map(|v| v as f64);
        let [x1, y1] = q[1].map(|v| v as f64);
        let [x2, y2] = q[2].map(|v| v as f64);
        let [x3, y3] = q[3].map(|v| v as f64);
        let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
        let (g, h) = if sx.abs() < 1e-12 && sy.abs() < 1e-12 {
            (0.0, 0.0)
        } else {
            let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
            let den = dx1 * dy2 - dx2 * dy1;
            if den.abs() < 1e-12 {
                return None;
            }
            ((sx * dy2 - dx2 * sy) / den, (dx1 * sy - sx * dy1) / den)
        };
        Some(Self {
            m: [
                x1 - x0 + g * x1,
                x3 - x0 + h * x3,
                x0,
                y1 - y0 + g * y1,
                y3 - y0 + h * y3,
                y0,
                g,
                h,
                1.0,
            ],
        })
    }

    /// This map, after `(x, y) -> (kx x, ky y)`.
    pub fn after_scale(self, kx: f64, ky: f64) -> Self {
        let m = self.m;
        Self {
            m: [
                m[0] * kx,
                m[1] * ky,
                m[2],
                m[3] * kx,
                m[4] * ky,
                m[5],
                m[6] * kx,
                m[7] * ky,
                m[8],
            ],
        }
    }

    /// Where `(x, y)` goes, or `None` past the line the map sends to
    /// infinity.
    pub fn apply(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let m = &self.m;
        let w = m[6] * x + m[7] * y + m[8];
        if w.abs() < 1e-12 {
            return None;
        }
        Some((
            (m[0] * x + m[1] * y + m[2]) / w,
            (m[3] * x + m[4] * y + m[5]) / w,
        ))
    }

    /// The map back, or `None` for one that collapses the plane.
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.m;
        let c0 = m[4] * m[8] - m[5] * m[7];
        let c1 = m[5] * m[6] - m[3] * m[8];
        let c2 = m[3] * m[7] - m[4] * m[6];
        let det = m[0] * c0 + m[1] * c1 + m[2] * c2;
        if !det.is_finite() || det.abs() < 1e-18 {
            return None;
        }
        let k = 1.0 / det;
        Some(Self {
            m: [
                c0 * k,
                (m[2] * m[7] - m[1] * m[8]) * k,
                (m[1] * m[5] - m[2] * m[4]) * k,
                c1 * k,
                (m[0] * m[8] - m[2] * m[6]) * k,
                (m[2] * m[3] - m[0] * m[5]) * k,
                c2 * k,
                (m[1] * m[6] - m[0] * m[7]) * k,
                (m[0] * m[4] - m[1] * m[3]) * k,
            ],
        })
    }

    /// This map followed by the affine `t`.
    pub fn then(&self, t: Transform) -> Self {
        let m = &self.m;
        let (a, b, c, d, e, f) = (
            t.a as f64, t.b as f64, t.c as f64, t.d as f64, t.e as f64, t.f as f64,
        );
        Self {
            m: [
                a * m[0] + c * m[3] + e * m[6],
                a * m[1] + c * m[4] + e * m[7],
                a * m[2] + c * m[5] + e * m[8],
                b * m[0] + d * m[3] + f * m[6],
                b * m[1] + d * m[4] + f * m[7],
                b * m[2] + d * m[5] + f * m[8],
                m[6],
                m[7],
                m[8],
            ],
        }
    }
}

/// Whether four points, in order, make a strictly convex quad: every turn
/// the same way round, and none of them a straight line.
pub fn convex(q: [[f32; 2]; 4]) -> bool {
    if q.iter().flatten().any(|v| !v.is_finite()) {
        return false;
    }
    let mut sign = 0.0f64;
    for i in 0..4 {
        let [ax, ay] = q[i].map(|v| v as f64);
        let [bx, by] = q[(i + 1) % 4].map(|v| v as f64);
        let [cx, cy] = q[(i + 2) % 4].map(|v| v as f64);
        let cross = (bx - ax) * (cy - by) - (by - ay) * (cx - bx);
        // Relative to the sides' own lengths, so a tiny picture is not
        // refused for being tiny.
        let scale = ((bx - ax).hypot(by - ay) * (cx - bx).hypot(cy - by)).max(1e-30);
        if cross.abs() / scale < 1e-6 {
            return false;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// Non-destructive adjustments; applied at render time to everything below
/// the layer (or to the object they're attached to).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    Exposure {
        stops: f32,
    },
    /// Hue turned, saturation scaled and lightness lifted — or, with
    /// `colorize`, every colour the one hue at `hue_degrees`, at a
    /// saturation of `(saturation + 1) / 2`, each pixel keeping the
    /// lightness it showed at: Photoshop's Colorize, the one-colour print.
    HueSaturation {
        hue_degrees: f32,
        saturation: f32,
        lightness: f32,
        #[serde(default)]
        colorize: bool,
    },
    /// Levels, in linear light like every adjustment here: the input
    /// range `in_black..in_white` is stretched to 0..1, a gamma lifts or
    /// sinks the midtones (above 1 lightens), and the result lands in
    /// `out_black..out_white`.
    Levels {
        in_black: f32,
        in_white: f32,
        gamma: f32,
        out_black: f32,
        out_white: f32,
    },
    /// A tone curve through `points` (input, output), both 0..1 in the
    /// display encoding — the way every photo editor draws one, so the
    /// middle of the graph is a middle grey rather than a linear quarter.
    /// The renderer sorts the points, interpolates a monotone cubic
    /// through them and holds the curve flat past the first and last, so
    /// two points on the diagonal (or fewer) is the identity.
    Curves {
        points: Vec<[f32; 2]>,
        /// A curve of its own for each channel, run after the master one
        /// — what colour grading is done with: lift the blue in the
        /// shadows, pull the red out of the highlights. Empty is the
        /// identity, which is why a document written before they existed
        /// still looks like itself.
        #[serde(default)]
        red: Vec<[f32; 2]>,
        #[serde(default)]
        green: Vec<[f32; 2]>,
        #[serde(default)]
        blue: Vec<[f32; 2]>,
    },
    /// White balance. `temperature` warms the picture as it rises and
    /// cools it as it falls (-1..=1, blue against amber); `tint` runs the
    /// other way a light source strays, green against magenta. Both act
    /// on the linear channels, which is where a light's colour actually
    /// lives.
    WhiteBalance {
        temperature: f32,
        tint: f32,
    },
    /// Saturation that leaves the already-vivid alone: the further a
    /// colour is from grey, the less of the change it takes. That is
    /// what keeps skin from going orange while a dull sky comes up.
    Vibrance {
        amount: f32,
    },
    /// Monochrome, mixed by hand. Every colour becomes the weighted sum
    /// of its channels, and the weights are the whole point: a picture
    /// taken to grey by one recipe is a different picture from the same
    /// one taken to grey by another — a high red weight darkens a blue
    /// sky the way a red filter on the lens did.
    ///
    /// The weights are normalized by their own total, so moving one
    /// changes the mix rather than the brightness; exposure and levels
    /// are what brightness is for. [`LUMA`] is the default and is the
    /// answer for "just make it grey".
    BlackAndWhite {
        red: f32,
        green: f32,
        blue: f32,
    },
    /// Every tone replaced by the colour at its own place along a ramp:
    /// the shadows take the first stop, the highlights the last, and
    /// everything between is read off the ramp. Duotones, split tones
    /// and the whole family of graded looks are this one adjustment.
    ///
    /// Where a tone sits along the ramp is its brightness as a device
    /// shows it, not as light measures it — the same reason a tone curve
    /// is drawn over the display encoding, and what makes the middle of
    /// the ramp land on the tones that look middling.
    GradientMap {
        stops: Vec<GradientStop>,
    },
    /// Turned inside out: what was light is dark and every colour becomes
    /// its opposite. `amount` is how far to take it, so half way is the
    /// flat grey the two sides meet at.
    ///
    /// On the values a device shows rather than on light itself. Light
    /// inverted is not what anyone means by a negative: linear 0.5 shows
    /// as 188, so inverting in light would turn a middle grey into a
    /// near-black rather than into itself.
    Invert {
        amount: f32,
    },
    /// Hue, saturation and lightness asked of one band of colour at a
    /// time — the reds, the yellows, the greens, the cyans, the blues,
    /// the magentas — which is how a sky is deepened without touching
    /// the grass, or a face warmed without warming the wall behind it.
    ///
    /// `bands` is six triples in that order: a hue shift (-1..=1, half a
    /// band's width at the ends), a saturation change and a lightness
    /// change (-1..=1 each). A shorter list is read as zeroes, so a file
    /// written before a band existed still reads.
    ///
    /// A pixel belongs to the bands its own hue falls between, by how
    /// near it is to each — the weights are a triangle a band wide, so
    /// they always add to one and no colour sits in a seam. How much of
    /// the change it takes is how much colour it has: a grey has no hue
    /// to speak to and is left alone.
    SelectiveHsl {
        #[serde(default)]
        bands: Vec<[f32; 3]>,
    },
    /// The two ends of the tone range, moved without touching the middle:
    /// `shadows` above zero opens up what is dark, `highlights` above
    /// zero pulls back what is bright, and both run to -1 for the
    /// opposite — deepening the shadows, brightening the highlights. It
    /// is the first thing asked of a photograph after exposure: a face
    /// against a window is dark because the window is bright, and no
    /// single exposure fixes both.
    ///
    /// Which end a pixel belongs to is decided by its brightness as a
    /// device shows it, and each end's pull falls off as the square of
    /// the distance from it, so the middle of the range is left alone.
    /// What moves is the pixel's brightness; its colour comes along
    /// unchanged, because a shadow lifted is the same colour it was.
    ///
    /// This is a function of the pixel and nothing else. Photoshop's
    /// version reads the neighbourhood too, which is what gives it local
    /// contrast and its halos; a per-pixel one cannot make a halo, and
    /// cannot tell a dark thing in bright surroundings from a dark thing
    /// in dark ones.
    ShadowsHighlights {
        shadows: f32,
        highlights: f32,
    },
    /// The three ranges of tone pushed along the three opponent pairs —
    /// cyan/red, magenta/green, yellow/blue — which is how a print is
    /// corrected and how a grade is given its colour: cool shadows
    /// against warm highlights, said in the terms the correction is
    /// thought in. Each triple is one number per pair, -1..1.
    ///
    /// Which range a pixel belongs to is read from its lightness, not
    /// per channel, so a shift moves a colour rather than pulling it
    /// apart; the three masks overlap and add to one, so no tone sits
    /// in a seam between them.
    ColorBalance {
        #[serde(default)]
        shadows: [f32; 3],
        #[serde(default)]
        midtones: [f32; 3],
        #[serde(default)]
        highlights: [f32; 3],
        /// Hold each pixel's brightness while its colour moves, so that
        /// correcting a cast does not also lighten the picture.
        #[serde(default)]
        preserve_luminosity: bool,
    },
    /// Each channel held to a few steps: `levels` of them from black to
    /// full, evenly spaced as a device shows them, so a picture comes
    /// out as the flat bands of a screen print. Two is every channel on
    /// or off; it is read whole and held to 2..=255.
    ///
    /// On the shown values for the reason a tone curve is drawn over
    /// them: steps even in light would crowd every band but one into the
    /// shadows.
    Posterize {
        levels: f32,
    },
    /// Every pixel white or black by whether its brightness, as a device
    /// shows it, is at `level` (0..=1) or above — the picture as a stamp
    /// or a stencil would print it.
    Threshold {
        level: f32,
    },
    /// Each channel made again from all three: the red that comes out is
    /// `red[0]` of the red that went in, `red[1]` of the green and
    /// `red[2]` of the blue, and so for the other two, with `constant`
    /// added to each after. The identity is each channel taking all of
    /// itself and none of the others. A channel swapped for another, a
    /// cast taken out by borrowing from the channel that has too much,
    /// an infrared look from a green that comes out as red: all of them
    /// are this one adjustment, which is why it is Photoshop's oldest.
    ///
    /// In light, like the black-and-white mix it is the general case of:
    /// that is where a channel is an amount of something that can be
    /// shared out. The weights are *not* normalized here, unlike that
    /// mix: a row that adds to more than one brightens on purpose, and
    /// the panel says what each row adds to.
    ///
    /// `monochrome` makes all three channels the red row's answer — one
    /// grey channel mixed by hand, with a constant the black-and-white
    /// mix does not have.
    ChannelMixer {
        red: [f32; 3],
        green: [f32; 3],
        blue: [f32; 3],
        #[serde(default)]
        constant: [f32; 3],
        #[serde(default)]
        monochrome: bool,
    },
    /// A coloured glass held in front of the lens: every colour multiplied
    /// by `color` and mixed back in by `density` (0..=1) — a warming
    /// amber, a cooling blue, the green of an old print. With
    /// `preserve_luminosity` each pixel keeps the brightness it had, so
    /// the glass changes its colour and not its exposure; without, a
    /// filter darkens as a real one does.
    ///
    /// The multiplying is on the values a device shows, where a density
    /// is judged: in light a quarter of an amber glass over white comes
    /// out all but white once its brightness is put back, because the
    /// tint lands where light is spread thinnest on the eye. What is
    /// above white goes through the glass in light. The brightness kept
    /// is light's.
    ///
    /// The colour is a colour like any other on the page — it can stand
    /// for a palette entry, and it goes through a CMYK document's press
    /// profile — and its alpha thins the glass as density does.
    PhotoFilter {
        color: AuthoredColor,
        density: f32,
        #[serde(default)]
        preserve_luminosity: bool,
    },
    /// Every colour sent where a 3D table says: a grade made in another
    /// program, a film emulation, a camera's log turned into a picture —
    /// the thing a `.cube` file holds.
    ///
    /// The table is a resource (`table`, read by [`crate::lut`]), so it is
    /// as immutable and as shared as a picture's pixels; `size` is its
    /// grid, `size`³ entries over nought to one. It is read on the values
    /// a device shows, which is the space every `.cube` that grades a
    /// picture is made in; what is above white is held at white, as it
    /// is for any table. `name` is what the file called itself, for the
    /// panel.
    ColorLookup {
        table: String,
        size: u32,
        #[serde(default)]
        name: String,
    },
}

/// How much each channel contributes to brightness — the Rec. 709
/// weights, which is what "luminance" means everywhere else in this
/// engine and the default recipe for [`Adjustment::BlackAndWhite`].
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Non-destructive convolution filters; like adjustments they apply at
/// render time to everything composited below the layer. Unlike adjustments
/// they read pixel neighborhoods, so their invalidation is whole-canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    /// `sigma` is the Gaussian standard deviation in document pixels.
    GaussianBlur { sigma: f32 },
    /// Unsharp mask: original + amount × (original − blur(sigma)).
    Sharpen { sigma: f32, amount: f32 },
    /// The detail alone: what is under it less its blur by `sigma`, on a
    /// grey of half — flat where nothing changes, lighter and darker
    /// where edges and texture are. Set to overlay or soft light over a
    /// copy of the picture it sharpens by just that detail; half is grey
    /// in the light the blends are worked in, which is what they leave
    /// alone.
    HighPass { sigma: f32 },
    /// What is under it as a printer's dot screen: a grid of cells
    /// `size` document pixels across, turned `degrees`, each a black dot
    /// on white whose area is how dark the picture is where the cell's
    /// middle falls — darkness as it looks, on the display's curve. Seen
    /// from far off the dots are the picture again; close up they are
    /// the poster's texture. What is not covered stays uncovered.
    Halftone { size: f32, degrees: f32 },
    /// What is under it pressed into grey metal and lit from one side:
    /// how light the picture is, taken as a height, and the slope of it
    /// toward a light that falls from `degrees` — measured as a motion
    /// blur's direction is, from the page's x axis turning the way its y
    /// does, so 225 is the upper left. Each pixel is a grey of half less
    /// how much lighter the picture is `height` document pixels toward
    /// the light than as far away from it, times `amount`: an edge that
    /// rises toward the light faces away from it and goes dark, one that
    /// falls toward it is lit, and flat ground stays the half grey, in
    /// the light the blends are worked in, that overlay leaves alone.
    /// The two readings are whole pixels away, the offset rounded once
    /// for the page rather than at every pixel, so a relief is an edge a
    /// pixel wide rather than a blur of one.
    Emboss {
        degrees: f32,
        height: f32,
        amount: f32,
    },
    /// A smear along one direction: each pixel becomes the average of the
    /// ones on a line through it. What a camera does to a moving thing, or
    /// to a still one while the camera moves, and what a still picture is
    /// given to say the same.
    ///
    /// `distance` is the length of that line in document pixels and
    /// `degrees` the direction it runs in, measured from the page's own x
    /// axis and turning the way the page's y does — so zero smears across
    /// and ninety smears down. The line is centred on the pixel, which is
    /// what keeps the picture where it was instead of sliding it half the
    /// distance along.
    MotionBlur { distance: f32, degrees: f32 },
    /// A smear about a point: outward from it, as the picture streaks
    /// when a lens is zoomed during the exposure, or round it, as a wheel
    /// blurs as it turns. Photoshop's radial blur, in its two kinds.
    ///
    /// `center` is where the point is, as a share of the page's width
    /// and height (a half and a half is the middle), so it stays where it
    /// was put when the page changes size. Zoomed, `amount` is how far
    /// each pixel is smeared along its line to the centre, as a share of
    /// its distance from it (0..=1) — nothing at the centre itself, the
    /// most at the corners. Spun (`spin`), `amount` is the angle each
    /// pixel is smeared through, in degrees (0..=180). Both centred on
    /// the pixel, as the motion blur is, so the picture stays where it
    /// was.
    RadialBlur {
        amount: f32,
        #[serde(default)]
        spin: bool,
        #[serde(default = "page_middle")]
        center: [f32; 2],
    },
    /// What is under it turned about a point, the most at the point and
    /// less and less out to a circle round it, where it stops: a
    /// whirlpool. `degrees` is how far the middle turns, the way the
    /// page's y turns from its x (clockwise on screen) for a positive
    /// number, easing to nothing at the circle as the square of the way
    /// still to go. `center` is a share of the page's width and height,
    /// as a radial blur's is, and `radius` a share of half the page's
    /// shorter side, so one is the largest circle the page holds. Read
    /// between pixels, so a turned edge stays smooth.
    Twirl {
        degrees: f32,
        #[serde(default = "one")]
        radius: f32,
        #[serde(default = "page_middle")]
        center: [f32; 2],
    },
    /// What is under it squeezed toward a point or swollen out from it,
    /// inside a circle round it and not past: a positive `amount` (up to
    /// one) pinches, pulling the picture in toward the middle, and a
    /// negative one bulges it, as a lens held close does. A point `r`
    /// from the middle shows what was `R · (r / R)^e` from it, `R` the
    /// circle's radius and `e` one less half the amount,
    /// so the circle's edge stays where it was and nothing jumps
    /// there. `center` and `radius` are a twirl's.
    Pinch {
        amount: f32,
        #[serde(default = "one")]
        radius: f32,
        #[serde(default = "page_middle")]
        center: [f32; 2],
    },
    /// What is under it seen through rippled glass: every point moved
    /// across by `amplitude` times a sine of how far down it is, and down
    /// by the same of how far across, both waves `wavelength` long — so
    /// an upright edge goes wavy down its length and a level one along
    /// its. Both in document pixels and anchored to the document, so the
    /// waves stay on the picture when the page is panned or zoomed.
    Ripple { amplitude: f32, wavelength: f32 },
    /// The picture's edges drawn dark on white, channel by channel:
    /// flat ground of any colour goes white, and where a channel changes
    /// that channel is taken down by how steeply, `amount` times — a
    /// step from nothing to full, at one, all the way. So a black edge
    /// is where every channel changes and a red picture's edges are
    /// cyan. The steepness is read across the pixels a document pixel
    /// away each side (a Sobel kernel), so the lines keep their weight
    /// on the picture when it is zoomed.
    FindEdges { amount: f32 },
    /// What is under it as though painted in thick strokes: flat where
    /// a stroke would be, and edged where the picture is (the Kuwahara
    /// filter). Round each pixel stand four squares `radius` document
    /// pixels a side past it, one in each corner, overlapping on the
    /// pixel's own row and column; the pixel becomes the average of the
    /// square whose light, and whose cover, vary least — so an edge is
    /// never averaged across, since a square that straddles it varies
    /// most, and the edge of what is covered is an edge too. A square
    /// wins only by a clear margin, the first of them otherwise, so two
    /// equally smooth sides of an edge are not decided by rounding.
    OilPaint { radius: f32 },
    /// A lens's bowing taken out: what a wide lens bends outward
    /// (barrel) or inward (pincushion) made straight again. A point a
    /// share `ρ` of the way from the page's middle to its corner shows
    /// what was `ρ · (1 − k ρ²)` along the same line, `k` a third of
    /// `amount` (held to −1..=1): positive draws the corners in to undo
    /// a barrel, negative pushes them out to undo a pincushion. The
    /// middle stays where it was; past the page's edge the edge is read
    /// again.
    LensCorrection { amount: f32 },
    /// Squares of one colour each, the average of what they covered:
    /// what a face or a number is taken out of a picture with. `size` is
    /// the square's side in document pixels, and the grid is anchored in
    /// the document, so a block stays the same block when the page is
    /// panned or zoomed rather than crawling under the view.
    Pixelate { size: f32 },
    /// Grain. `amount` is how far a pixel can be moved, 0..=1; `grain`
    /// is how many document pixels across one speck is, so grain gets
    /// bigger with the picture rather than staying the size of a screen
    /// pixel; `mono` moves every channel together, which is film grain,
    /// against moving each on its own, which is sensor noise.
    ///
    /// It is a function of where a speck is in the document and of
    /// `seed`, not of any state carried between pixels, so the same page
    /// grains the same way every time it is drawn — which is what lets a
    /// filter be a live layer rather than something baked once.
    Noise {
        amount: f32,
        grain: f32,
        mono: bool,
        seed: u32,
    },
    /// The corners of the page taken down (or brought up), which is what
    /// a lens does and what a photographer does on purpose afterwards to
    /// hold the eye in the middle of a picture.
    ///
    /// `amount` above zero darkens and below zero lightens; `radius` is
    /// how far out from the middle nothing happens at all, as a fraction
    /// of the way to the corner; `softness` is how gradually it comes in
    /// once it does — 0 a straight ramp, 1 an eased one that has no edge
    /// anywhere.
    ///
    /// Measured from the middle of the page in document units, so it
    /// sits on the picture rather than on the window: panning slides the
    /// picture under it and the darkening stays where it is on the page.
    Vignette {
        amount: f32,
        radius: f32,
        softness: f32,
    },
    /// Noise taken out: every channel the middle value of what is around
    /// it, within `radius` document pixels each way. A median, not an
    /// average — a speck brighter or darker than everything near it is
    /// outvoted and goes, where a blur would only spread it, and an edge
    /// between two flat colours stays an edge, since the middle value
    /// either side of it is that side's own.
    Median { radius: f32 },
    /// Smoothed where it is flat and left alone where it is not: skin,
    /// sky and sensor noise go soft while the edges between them stay.
    /// Each pixel becomes a weighted average of those within `radius`
    /// document pixels along its row, then of the result down its
    /// column. A neighbour counts by how near it is (a Gaussian of half
    /// the radius) and by how alike it looks: fully when its shown
    /// colour is the pixel's own, and not at all once any channel is
    /// `threshold` apart, in the display encoding's 0..1. A bilateral
    /// filter, taken one axis at a time so a wide one costs a line of
    /// reads a pixel rather than a square of them.
    SurfaceBlur { radius: f32, threshold: f32 },
}

/// A straight line the user placed to lay work out against. Not artwork —
/// guides never render and never export — but document state all the same,
/// because they belong to the layout rather than to the window it is being
/// viewed through.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Guide {
    /// A vertical line at this document x.
    Vertical(f32),
    /// A horizontal line at this document y.
    Horizontal(f32),
}

impl Guide {
    pub fn at(&self) -> f32 {
        match self {
            Guide::Vertical(v) | Guide::Horizontal(v) => *v,
        }
    }

    pub fn is_vertical(&self) -> bool {
        matches!(self, Guide::Vertical(_))
    }
}

/// Whether a block's paragraphs are the items of a list, and how each is
/// marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextList {
    /// Paragraphs, as text always was.
    #[default]
    None,
    /// Each item marked with a bullet.
    Bullet,
    /// Each item marked with its number, counted from one.
    Numbered,
}

/// Where each line sits inside the block's own width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    /// Set flush to both edges of the block's width: what a line falls
    /// short by is shared out after its spaces. Only a wrapping block has
    /// edges to set to, and the last line of each paragraph — which ends
    /// where the writer ended it, not where the width did — is set to the
    /// left, as is any line with no space to widen.
    Justify,
}

/// A live text object: the string and styling are the document state,
/// glyphs rasterize at render time (nothing is ever baked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    pub text: String,
    /// Font size in document pixels (ascent-to-descent scale).
    pub size: f32,
    pub fill: AuthoredColor,
    /// Additive, all three: a document written before they existed loads
    /// with the values it was rendered at, so it still looks like itself.
    #[serde(default)]
    pub align: TextAlign,
    /// Line spacing as a multiple of the font's own line height.
    #[serde(default = "one")]
    pub line_height: f32,
    /// Tracking: extra space after each glyph, in ems, the way it is
    /// normally quoted — so it scales with the size rather than fighting it.
    #[serde(default)]
    pub letter_spacing: f32,
    /// Space after each paragraph — after every line the writer ended
    /// with a newline, not every line the width wrapped — in ems as
    /// tracking is, so it scales with the size. Nought, the default, is
    /// every block written before there was a choice.
    #[serde(default)]
    pub paragraph_spacing: f32,
    /// Wrap width in document pixels. Zero is a block that fits its own
    /// text; anything else wraps words to that width and is that wide, so
    /// alignment inside it holds still while the text changes. A word
    /// longer than the width stands on its own line and overhangs.
    #[serde(default)]
    pub width: f32,
    /// Which face sets the block, by the name it was registered under.
    /// Empty — and any name nothing answers to — is the bundled face, so
    /// a document set in a font this machine lacks still renders.
    #[serde(default)]
    pub font: String,
    /// Italic: set in the face's "… Oblique" (or "… Italic") twin when one
    /// is registered, and leaned by the rasterizer when none is.
    #[serde(default)]
    pub italic: bool,
    /// Bold: set in the face's "… Bold" twin when one is registered, and
    /// thickened by the rasterizer when none is. Additive.
    #[serde(default)]
    pub bold: bool,
    /// A line under each line of text, and one through it, drawn by the
    /// rasterizer in the block's colour at sizes relative to the em.
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub strike: bool,
    /// A guide to set the text along instead of in lines: a shape in the
    /// block's own space (copied from a layer when attached, so the block
    /// stands alone), with each glyph turned to follow it. Text that runs
    /// off an open guide's end is not drawn; a closed one wraps.
    #[serde(default)]
    pub along: Option<VectorShape>,
    /// How far along the guide, in document pixels, the text starts.
    #[serde(default)]
    pub along_offset: f32,
    /// Stretches of the text set differently from the rest of it.
    /// Additive: a block written before runs existed has none, and is
    /// set the way it always was.
    #[serde(default)]
    pub runs: Vec<StyleRun>,
    /// Each paragraph an item of a list: its bullet or its number set in
    /// the margin before its first line, and every line of the block
    /// indented past the widest of them, so an item's lines hang beside
    /// its marker. An empty paragraph is a gap between items rather than
    /// an item, and is neither marked nor counted. Additive: a block
    /// written before lists were is paragraphs.
    #[serde(default)]
    pub list: TextList,
}

/// A stretch of a block set differently from the rest: the same choices
/// the block itself makes, made again over a range of its text, with
/// `None` meaning "however the block does it".
///
/// A run's size is the block's — one line of text is one line tall here,
/// and mixing sizes inside a block is a different feature — so a run
/// changes how its letters are drawn, never where the lines sit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleRun {
    /// Byte offsets into the block's text. Ranges are clipped to the
    /// text and to each other when the block is set, so text can be
    /// deleted without the runs having to be repaired first.
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub fill: Option<AuthoredColor>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub underline: Option<bool>,
    #[serde(default)]
    pub strike: Option<bool>,
    #[serde(default)]
    pub font: Option<String>,
    /// A size of its own, as the block's is: document pixels from ascent
    /// to descent. A line holding a larger run is that much taller, and
    /// every run on a line sits on the same baseline.
    #[serde(default)]
    pub size: Option<f32>,
    /// How far the run sits above its line's baseline, in its own size:
    /// a superscript is raised a third or so, a subscript lowered by a
    /// fifth (negative). The line makes room for it. Additive: a run
    /// written before this sat on the baseline.
    #[serde(default)]
    pub baseline: Option<f32>,
}

/// The furthest a run is raised or lowered, in its own size.
pub const BASELINE_MOST: f32 = 1.0;

impl StyleRun {
    /// A run over `start..end` that changes nothing yet.
    pub fn over(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            fill: None,
            bold: None,
            italic: None,
            underline: None,
            strike: None,
            font: None,
            size: None,
            baseline: None,
        }
    }

    /// How far the run is raised, in its own size: held to
    /// ±[`BASELINE_MOST`], and nothing for nothing or for what is no
    /// number.
    pub fn shift(&self) -> f32 {
        self.baseline
            .filter(|b| b.is_finite())
            .map_or(0.0, |b| b.clamp(-BASELINE_MOST, BASELINE_MOST))
    }

    /// Whether the run asks for anything at all. One that does not is
    /// dropped rather than kept as a range that changes nothing.
    pub fn says_anything(&self) -> bool {
        self.fill.is_some()
            || self.bold.is_some()
            || self.italic.is_some()
            || self.underline.is_some()
            || self.strike.is_some()
            || self.font.is_some()
            || self.size.is_some()
            || self.baseline.is_some()
    }
}

fn one() -> f32 {
    1.0
}

impl TextSpec {
    /// A block with default styling; the three additive fields have to come
    /// from somewhere when the spec is built in Rust rather than parsed.
    pub fn new(text: impl Into<String>, size: f32, fill: AuthoredColor) -> Self {
        Self {
            text: text.into(),
            size,
            fill,
            align: TextAlign::default(),
            line_height: 1.0,
            letter_spacing: 0.0,
            paragraph_spacing: 0.0,
            width: 0.0,
            font: String::new(),
            italic: false,
            bold: false,
            underline: false,
            strike: false,
            along: None,
            along_offset: 0.0,
            runs: Vec::new(),
            list: TextList::None,
        }
    }

    /// The marker of each paragraph, in order: a bullet or a number for
    /// each item of a list, nothing for an empty paragraph and nothing at
    /// all when the block is not a list.
    pub fn markers(&self) -> Vec<Option<String>> {
        let mut item = 0usize;
        self.text
            .split('\n')
            .map(|paragraph| {
                if self.list == TextList::None || paragraph.is_empty() {
                    return None;
                }
                item += 1;
                Some(match self.list {
                    TextList::Bullet => "\u{2022}".to_string(),
                    _ => format!("{item}."),
                })
            })
            .collect()
    }

    /// The line height actually used, guarded against a zero or negative
    /// multiple that would collapse every line onto one.
    pub fn line_scale(&self) -> f32 {
        self.line_height.max(0.05)
    }

    /// The space after a paragraph actually used, as a multiple of the
    /// size: none below nothing, at most ten, and none for what is not a
    /// number.
    pub fn paragraph_gap(&self) -> f32 {
        if self.paragraph_spacing.is_finite() {
            self.paragraph_spacing.clamp(0.0, 10.0)
        } else {
            0.0
        }
    }

    /// The block's text as a sequence of pieces, each with the index of
    /// the run that governs it or `None` where the block's own styling
    /// stands. Pieces are in order, cover the whole text exactly once,
    /// and always begin and end on a character boundary.
    ///
    /// This is the one place that decides which run a byte belongs to, so
    /// every part of the pipeline — measuring, shaping, rasterizing and
    /// both exporters — cuts the text the same way. Ranges are sorted,
    /// clipped to the text, and trimmed against whatever came before, so
    /// runs left overlapping or hanging past the end by an edit are read
    /// sensibly rather than refused.
    pub fn pieces(&self) -> Vec<(usize, usize, Option<usize>)> {
        let len = self.text.len();
        let mut order: Vec<usize> = (0..self.runs.len()).collect();
        order.sort_by_key(|&i| (self.runs[i].start, self.runs[i].end));
        let mut out: Vec<(usize, usize, Option<usize>)> = Vec::new();
        let mut at = 0usize;
        for i in order {
            let run = &self.runs[i];
            let start = self.boundary(run.start.max(at).min(len));
            let end = self.boundary(run.end.min(len)).max(start);
            if start >= end {
                continue;
            }
            if start > at {
                out.push((at, start, None));
            }
            out.push((start, end, Some(i)));
            at = end;
        }
        if at < len || out.is_empty() {
            out.push((at, len, None));
        }
        out
    }

    /// The nearest character boundary at or before `at`: a run whose end
    /// an edit left in the middle of a character still cuts the text
    /// somewhere it can be cut.
    fn boundary(&self, at: usize) -> usize {
        let mut at = at.min(self.text.len());
        while at > 0 && !self.text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    /// This block's styling with `run`'s choices laid over it: how one
    /// piece of the text is actually set.
    ///
    /// The text itself is left empty — the piece it applies to is named
    /// separately, and copying the whole block's text once per piece is
    /// the sort of thing that makes wrapping a long block slow.
    pub fn styling_under(&self, run: Option<&StyleRun>) -> TextSpec {
        let mut spec = self.clone();
        spec.text = String::new();
        spec.runs = Vec::new();
        let Some(run) = run else {
            return spec;
        };
        if let Some(fill) = &run.fill {
            spec.fill = fill.clone();
        }
        if let Some(bold) = run.bold {
            spec.bold = bold;
        }
        if let Some(italic) = run.italic {
            spec.italic = italic;
        }
        if let Some(underline) = run.underline {
            spec.underline = underline;
        }
        if let Some(strike) = run.strike {
            spec.strike = strike;
        }
        if let Some(font) = &run.font {
            spec.font = font.clone();
        }
        // A size that is no size is the block's.
        if let Some(size) = run.size.filter(|s| s.is_finite() && *s > 0.0) {
            spec.size = size;
        }
        spec
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Group,
    /// A layer painted with a brush: the strokes that were laid on it,
    /// in the order they were laid, still live — each one can be taken
    /// off again, and the layer is re-rendered rather than kept as
    /// pixels.
    Paint {
        strokes: Vec<PaintStroke>,
    },
    /// A layer that paints with what is already under it. Each stroke
    /// carries the offset it reads at, so the picture it lays down is
    /// whatever the page shows there *now* — retouch the source and the
    /// clone follows, which is what makes this non-destructive where a
    /// stamped copy of pixels would not be.
    Clone {
        strokes: Vec<PaintStroke>,
    },
    Vector {
        shape: VectorShape,
        fill: Option<AuthoredColor>,
        stroke: Option<Stroke>,
        /// Paints in place of `fill` when set. Additive, so documents
        /// written before gradients existed still load.
        #[serde(default)]
        gradient: Option<Gradient>,
    },
    Raster(RasterRef),
    Adjustment(Adjustment),
    Filter(Filter),
    Text(TextSpec),
    /// A live copy of another layer: it draws whatever that layer holds,
    /// wherever this one is put, so changing the original changes every
    /// copy of it. Its own transform, opacity, blend and mask are its
    /// own; only what it is a picture *of* is shared.
    ///
    /// The original's own transform is not part of what travels — an
    /// instance places the picture itself — so moving the original moves
    /// only the original.
    Instance {
        of: NodeId,
        /// Which of the original's layers this copy stands in for with
        /// one of its own: `replaces[k]` is the layer, among the
        /// original's children, whose place this copy's `k`th child
        /// takes.
        ///
        /// That is what makes a copy differ where it has to — a label
        /// with a different string, a panel in a different colour — while
        /// everything else still follows the original. A child that
        /// stands in for nothing is drawn after the original's contents,
        /// so dropping a layer into a copy adds to it rather than
        /// breaking it.
        ///
        /// The *layer* and not its position. A position is only true
        /// until the original's layers are added to, reordered or thinned
        /// out, and then it silently means a different layer: add a layer
        /// to a component and every copy's stand-in slides onto its
        /// neighbour. Files written before this said positions and are
        /// carried over on the way in
        /// ([`Document::settle_stand_ins_from_positions`]).
        #[serde(default)]
        replaces: Vec<NodeId>,
    },
    /// A frame on the page: a group with a size of its own that cuts its
    /// contents to that box, paints a ground behind them, and exports at
    /// exactly that many pixels however it is placed or scaled. What lets
    /// one document hold several pictures — three screen sizes, a set of
    /// cards, a poster and its detail — instead of one.
    Artboard {
        width: f32,
        height: f32,
        /// Painted behind the contents, inside the frame. `None` is a
        /// frame that shows what is under it on the page.
        background: Option<AuthoredColor>,
        /// How many pixels a pixel of the frame exports as. A frame is
        /// usually made at the size it is designed at and wanted at
        /// twice or three times that, and which multiple belongs to
        /// which frame is a property of the frame rather than a thing to
        /// remember at the moment of exporting. Additive: a file written
        /// before this reads as 1, and so does anything at or below
        /// zero, since a frame exporting as nothing is not a wish anyone
        /// has.
        #[serde(default = "one")]
        export_scale: f32,
    },
}

impl NodeKind {
    /// Whether the node is one that holds other layers — what a parent
    /// has to be for anything to go into it.
    pub fn holds_children(&self) -> bool {
        matches!(
            self,
            // A copy holds the layers it stands in for.
            NodeKind::Group | NodeKind::Artboard { .. } | NodeKind::Instance { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: AuthoredColor,
    pub width: f32,
    /// Per-anchor width multipliers in 0..=1, scaling `width` along the
    /// path so a stroke can swell and taper. Empty means a constant width,
    /// which is why older files keep loading; a path's own anchors index
    /// it, and the renderer interpolates between them.
    #[serde(default)]
    pub widths: Vec<f32>,
    /// A dash pattern along the outline: lengths in the shape's own
    /// units, on and off in turn and repeating, so `[6.0, 3.0]` is six
    /// on and three off. One length alone is that much on and the same
    /// off. Empty is a solid stroke, which is why older files still
    /// load. A dashed stroke is drawn at one width — a stroke that
    /// swells and one that is broken up are two different ideas about
    /// the same line, and the dashes win.
    #[serde(default)]
    pub dash: Vec<f32>,
    /// How far into the dash pattern the line starts, in the same units:
    /// the pattern is walked this far before the first dash is laid, so
    /// a positive offset starts the line partway through it. Taken round
    /// the pattern's own length, either way. Nought, the default, starts
    /// at the beginning, which is every file written before there was a
    /// choice; SVG calls it `stroke-dashoffset`, PDF the phase of a dash.
    #[serde(default)]
    pub dash_offset: f32,
    /// How the stroke ends where the line stops — including at either
    /// end of every dash. Paths only: a rect's or an ellipse's stroke is
    /// a band lying inside a closed outline, which never stops.
    #[serde(default)]
    pub cap: StrokeCap,
    /// How the stroke turns a corner. Paths only, for the same reason.
    #[serde(default)]
    pub join: StrokeJoin,
    /// Which side of the outline the stroke lies on. `None` is whatever
    /// this shape has always been stroked as — see [`stroke_align`] —
    /// which is what keeps a file written before there was a choice
    /// drawing the way it did.
    #[serde(default)]
    pub align: Option<StrokeAlign>,
    /// What sits at the line's first point, and at its last. A line has
    /// two ends and they are asked separately, since an arrow at one end
    /// and nothing at the other is what most arrows are.
    ///
    /// An open path only: a closed ring, a rect and an ellipse have no
    /// ends to put anything on. These go where the *line* stops, not
    /// where each dash does, so a dashed arrow has one head and not a
    /// dozen.
    #[serde(default)]
    pub start_marker: Marker,
    #[serde(default)]
    pub end_marker: Marker,
}

/// What a line carries at an end.
///
/// Sized from the stroke's own width rather than given a size of its
/// own, which is what SVG's `markerUnits="strokeWidth"` means and what
/// keeps a head in proportion to the line it is on when the line is made
/// thicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Marker {
    #[default]
    None,
    /// A head pointing away from the line, its tip on the last point.
    Arrow,
    /// A tick across the line, as a dimension line is finished.
    Bar,
    /// A disc centred on the last point.
    Dot,
}

/// How far an arrowhead reaches back from its tip, and how far a head or
/// a bar reaches to either side, both as multiples of the stroke's width.
pub const MARKER_LENGTH: f32 = 3.0;
pub const MARKER_REACH: f32 = 1.5;

/// Which side of the outline a stroke lies on.
///
/// Asked of a rect and an ellipse, whose outlines have a distance of
/// their own. A path is stroked down the middle of its line whatever it
/// asks for — see [`stroke_align`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeAlign {
    /// The band lies inside the outline, so thickening a border never
    /// grows the shape.
    Inside,
    /// Half in and half out, which is what SVG and PDF do by default.
    Centre,
    /// The band lies outside the outline, so a border never eats into
    /// the fill.
    Outside,
}

/// Which side of the outline this stroke actually lies on.
///
/// Where nothing was asked for, the answer is what the engine did before
/// there could be an ask: a rect's and an ellipse's stroke was a band
/// inside the edge, so thickening a border never grew the shape, and a
/// path's ran down the middle of its line. That is an inconsistency —
/// the same shape drawn two ways stroked two ways — which is the reason
/// for the ask, but a file written then still has to draw as it did.
pub fn stroke_align(shape: &VectorShape, stroke: &Stroke) -> StrokeAlign {
    match shape {
        // A rect's and an ellipse's outline has a distance of its own,
        // so a band to either side of it is exact — a band inside, one
        // straddling, one outside, and the dashes that break any of them
        // up walk the middle of the band they belong to.
        VectorShape::Rect { .. } | VectorShape::Ellipse { .. } => {
            stroke.align.unwrap_or(StrokeAlign::Inside)
        }
        // A path is stroked down the middle of its line, which is what a
        // line means and what an open one can only mean. Putting a band
        // to one side of a closed path asks for its outline offset, and
        // an offset outline is a guess where a distance is not.
        // A polygon is the closed path through its corners, stroked as
        // that path is.
        VectorShape::Path { .. } | VectorShape::Polygon { .. } => StrokeAlign::Centre,
    }
}

/// How a stroke ends where its line stops.
///
/// Round is the default because it is what the engine drew before there
/// was a choice — a stroke was a distance from the line, and a distance
/// rounds every end — so a file written then still reads the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeCap {
    /// Flat on the last point: the line stops exactly where it ends,
    /// which is what a rule, a tick and a dash want.
    Butt,
    #[default]
    Round,
    /// Flat half a width past the last point, so the line ends square.
    Square,
}

/// How a stroke turns the corner at a point where two segments meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeJoin {
    /// Carried out to the point where the two outer edges cross, which
    /// is what makes a drawn corner look drawn — falling back to a bevel
    /// where that point would run away, past [`MITER_LIMIT`] widths.
    Miter,
    #[default]
    Round,
    /// Cut straight across the corner.
    Bevel,
}

/// How far a miter may reach, as a multiple of the stroke's half-width,
/// before the corner is cut off instead. Four is what SVG and PDF both
/// take as read, and it is the angle (about 29°) below which a miter
/// stops being a corner and becomes a spike.
pub const MITER_LIMIT: f32 = 4.0;

/// A non-destructive mask attachable to any node: it modulates the node's
/// output (a group's composite, a shape's paint, an adjustment's or filter's
/// strength) by per-pixel coverage. Mask geometry lives in document space,
/// independent of the node's own transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub kind: MaskKind,
    /// Flip coverage: masked-out becomes masked-in.
    pub invert: bool,
    /// How far the edge is softened over, in the mask's own units.
    ///
    /// Zero is the hard edge a shape has. Anything else blurs the
    /// coverage, which is the one thing a mask's edge can be asked for
    /// that its shape cannot say: a region picked out of a photograph
    /// almost never wants the edge the marquee drew, and a layer masked
    /// into another wants to be let into it rather than stamped on it.
    ///
    /// Additive: a mask written before this had a hard edge, which is
    /// what zero means.
    #[serde(default)]
    pub feather: f32,
}

impl Adjustment {
    /// Every colour written inside an adjustment.
    ///
    /// Fifteen of the seventeen are read as numbers. The gradient map is
    /// not: it *is* a ramp of colours, and one of them can stand for a
    /// palette entry like any other. Nor is the photo filter, whose glass
    /// is a colour. Matched kind by kind rather than
    /// with a catch-all, so that a new adjustment holding a colour does
    /// not compile until it says so — which is the promise
    /// `Node::each_color_mut` makes and did not keep.
    pub fn each_color_mut(&mut self, f: &mut impl FnMut(&mut AuthoredColor)) {
        match self {
            Adjustment::GradientMap { stops } => {
                for stop in stops {
                    f(&mut stop.color);
                }
            }
            Adjustment::PhotoFilter { color, .. } => f(color),
            Adjustment::BrightnessContrast { .. }
            | Adjustment::Exposure { .. }
            | Adjustment::HueSaturation { .. }
            | Adjustment::Levels { .. }
            | Adjustment::Curves { .. }
            | Adjustment::WhiteBalance { .. }
            | Adjustment::Vibrance { .. }
            | Adjustment::BlackAndWhite { .. }
            | Adjustment::Invert { .. }
            | Adjustment::SelectiveHsl { .. }
            | Adjustment::ShadowsHighlights { .. }
            | Adjustment::ColorBalance { .. }
            | Adjustment::Posterize { .. }
            | Adjustment::Threshold { .. }
            | Adjustment::ChannelMixer { .. }
            | Adjustment::ColorLookup { .. } => {}
        }
    }
}

impl Filter {
    /// Every colour written inside a filter, of which there are none —
    /// said kind by kind rather than assumed, for the reason above.
    pub fn each_color_mut(&mut self, f: &mut impl FnMut(&mut AuthoredColor)) {
        let _ = f;
        match self {
            Filter::GaussianBlur { .. }
            | Filter::Sharpen { .. }
            | Filter::HighPass { .. }
            | Filter::Halftone { .. }
            | Filter::Emboss { .. }
            | Filter::Twirl { .. }
            | Filter::Pinch { .. }
            | Filter::Ripple { .. }
            | Filter::FindEdges { .. }
            | Filter::OilPaint { .. }
            | Filter::LensCorrection { .. }
            | Filter::MotionBlur { .. }
            | Filter::RadialBlur { .. }
            | Filter::Pixelate { .. }
            | Filter::Median { .. }
            | Filter::SurfaceBlur { .. }
            | Filter::Noise { .. }
            | Filter::Vignette { .. } => {}
        }
    }
}

impl Mask {
    /// Every colour written inside the mask.
    ///
    /// A mask is read for coverage and never for colour, so none of these
    /// is drawn. They are still written down, though, and something that
    /// walks every colour in a document to keep it in step — the palette
    /// does — would leave a lie behind if it skipped them.
    pub fn each_color_mut(&mut self, f: &mut impl FnMut(&mut AuthoredColor)) {
        match &mut self.kind {
            MaskKind::Vector { .. } | MaskKind::Raster { .. } => {}
            MaskKind::Painted { strokes } => {
                for stroke in strokes {
                    stroke.each_color_mut(f);
                }
            }
        }
    }

    /// The same coverage, read in a space one transform further out.
    ///
    /// A mask is written in the space its owner is *placed* in — the
    /// parent's — rather than in the owner's own, which is what lets a
    /// layer be moved behind its mask. The other side of that is that
    /// anything moving a layer between spaces has to bring the mask by
    /// hand: left behind, it goes on covering the part of the page it
    /// used to, which for a layer that has moved out from under it is
    /// the whole layer. Four things want this — a page being mapped,
    /// which carries a layer's mask, the region picked out of the page
    /// and every region kept by name; and a group being dissolved,
    /// which hands its own transform to each child and so has to hand
    /// it to each child's mask too.
    pub fn carried_through(&self, m: Transform) -> Mask {
        let mut out = self.clone();
        // How far the edge is softened over is a length in that space
        // with no direction of its own, so it goes by the scale alone.
        out.feather *= m.max_scale();
        match &mut out.kind {
            MaskKind::Vector { transform, .. } | MaskKind::Raster { transform, .. } => {
                *transform = m.compose(*transform);
            }
            MaskKind::Painted { strokes } => {
                let k = m.max_scale();
                for stroke in strokes {
                    for p in &mut stroke.points {
                        *p = [m.a * p[0] + m.c * p[1] + m.e, m.b * p[0] + m.d * p[1] + m.f];
                    }
                    // A radius is a length in that space, like the
                    // softness above: the points spread with the scale
                    // and the brush has to spread with them.
                    for r in &mut stroke.radii {
                        *r *= k;
                    }
                    // And the region the stroke was confined to is
                    // another coverage over the same space, so it goes
                    // the same way. Recursion, but only as deep as a
                    // region brushed inside a region inside a region —
                    // and serde will not read a file nested past its own
                    // limit, so what arrives is as shallow as it looks.
                    if let Some(held) = &mut stroke.clip {
                        **held = held.carried_through(m);
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaskKind {
    /// Hard-edged coverage from a shape (1 inside, 0 outside).
    Vector {
        shape: VectorShape,
        transform: Transform,
    },
    /// Greyscale coverage from an image resource: luminance × alpha
    /// (white shows, black hides), sampled through the transform.
    Raster {
        resource_id: String,
        width: u32,
        height: u32,
        transform: Transform,
    },
    /// Coverage brushed on by hand. It starts showing everything, and
    /// each stroke either hides what it covers (an eraser) or shows it
    /// again (a brush) — which is how a piece is taken out of a layer
    /// without touching the layer.
    Painted { strokes: Vec<PaintStroke> },
}

/// How a layer answers when the frame around it changes size — the same
/// question on each axis, and the reason a frame can be given a new size
/// at all rather than only ever being drawn at the one it was made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Pin {
    /// Keeps its distance from the left (or top) edge, which is what a
    /// layer does when nothing is said — and what every layer written
    /// before pinning existed did.
    #[default]
    Start,
    /// Keeps its distance from the right (or bottom) edge.
    End,
    /// Keeps its distance from the middle.
    Middle,
    /// Keeps both distances, so it grows and shrinks with the frame.
    Stretch,
}

/// A layer's answer on each axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Pinning {
    pub x: Pin,
    pub y: Pin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub transform: Transform,
    pub opacity: f32,
    pub visible: bool,
    pub blend: BlendMode,
    #[serde(default)]
    pub mask: Option<Mask>,
    /// Effects drawn around the layer, bottom-most first. Live parameters
    /// like everything else: the layer's own pixels are untouched.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// A locked layer cannot be picked or moved on the canvas; it still
    /// renders and can still be reached through the panel. Additive.
    #[serde(default)]
    pub locked: bool,
    /// Confined to the layer below it: the layer shows only where that one
    /// does, and inherits its fate — hide the layer below and this one goes
    /// with it. A run of them stacks against the same layer, which is the
    /// nearest unclipped one underneath. The bottom layer of a parent has
    /// nothing to clip to, so the flag is ignored there. Additive.
    #[serde(default)]
    pub clipped: bool,
    /// What the layer does when the frame holding it is given a new
    /// size. Read only inside a frame; additive, and its default is what
    /// every layer did before it existed.
    #[serde(default)]
    pub pinned: Pinning,
    /// The kept style the layer follows (`Document::styles`, by name):
    /// when that style is kept again with a new look, this layer takes
    /// the new look with it. Given by `Session::apply_style`, let go of
    /// by detaching it or by forgetting the style. Left out of the file
    /// when the layer follows none, so a document that never linked one
    /// writes what it always wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

/// A live effect attached to a layer. Effects are rendered from the layer's
/// own composite, so they follow it when it moves, turns, or changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// The layer's silhouette, blurred and offset, painted behind it.
    /// `dx`/`dy` and `blur` are in the layer's parent space, so a shadow
    /// scales and turns with the group it is in.
    ///
    /// `spread` hardens it: the blurred silhouette's coverage is gained
    /// by `1 / (1 - spread)` and held at full, so the shadow reaches as
    /// far as before but stays solid further out (see [`spread_gain`]).
    /// At no offset and in a light colour this is a glow.
    DropShadow {
        dx: f32,
        dy: f32,
        blur: f32,
        color: AuthoredColor,
        opacity: f32,
        #[serde(default)]
        spread: f32,
    },
    /// A band of colour hugging the layer's silhouette from outside, the
    /// way a headline is given an outline. `width` is in the layer's
    /// parent space.
    Outline {
        width: f32,
        color: AuthoredColor,
        opacity: f32,
    },
    /// The same shadow, cast inward: it darkens the inside of the
    /// silhouette against its own edge and never leaves it. `spread` is
    /// the same hardening, which an inside shadow calls a choke: the
    /// shadow stays solid further in from the edge.
    InnerShadow {
        dx: f32,
        dy: f32,
        blur: f32,
        color: AuthoredColor,
        opacity: f32,
        #[serde(default)]
        spread: f32,
    },
    /// The layer raised off the page and lit: its silhouette, blurred by
    /// half of `size`, is taken as a height, and a light from the
    /// direction `(lx, ly)` across the page and `altitude` degrees above
    /// it falls on it. Where the slope faces the light more than the flat
    /// top does it is lit in `highlight`; where it faces away, shaded in
    /// `shadow`; the top itself is left as it is. Painted over the layer
    /// and kept inside it.
    ///
    /// The lighting is SVG's `feDiffuseLighting` under a distant light,
    /// the heights `depth · size / 2` at full coverage — so it goes to
    /// SVG as itself. `size` is in the layer's parent space, and so is
    /// the direction, which is a vector toward the light with y down —
    /// `(-1, -1)` is the upper left — whose length says nothing. A vector
    /// rather than an angle for the same reason a shadow's offset is
    /// one: it goes through a transform exactly, so a page turned a
    /// quarter and back is the page it was, where an angle worked out
    /// again through an arctangent came back a hair off.
    Bevel {
        size: f32,
        depth: f32,
        lx: f32,
        ly: f32,
        altitude: f32,
        highlight: AuthoredColor,
        shadow: AuthoredColor,
        opacity: f32,
    },
    /// The layer in one colour: every pixel of it taken `opacity` of the
    /// way to `color` (its own alpha weighing in too), keeping how much
    /// of the pixel the layer covers. Done to the layer's own pixels
    /// before they meet the page, as Photoshop's colour overlay is, so a
    /// soft edge is the colour at the edge's cover rather than the colour
    /// laid over what the edge left — which, painted over, wore a fringe
    /// of the layer's own colour all the way round. Neither under the
    /// layer nor over it (`over` says false), and no reach: it is the
    /// layer.
    ColorOverlay { color: AuthoredColor, opacity: f32 },
    /// The layer itself out of focus: its own pixels blurred by `sigma`,
    /// the softening the blur filter does, in the layer's parent space so
    /// it scales with the group it is in. A filter layer blurs everything
    /// under it; this blurs the one layer, which is how an element given
    /// SVG's `feGaussianBlur` says it. Done to the layer before anything
    /// is made of it, so a shadow it wears is the shadow of the blurred
    /// layer and what is held to it is held to the blurred shape. Held
    /// itself, it is what shows of the layer that is blurred, and the
    /// blur is kept inside what it is held to, as a shadow is — so its
    /// edge there stays sharp. Neither under the layer nor over it: it is
    /// the layer.
    Blur { sigma: f32 },
}

/// What a bevel is lit by and how its heights are read, as every
/// renderer takes them: the unit vector toward the light in device
/// space with y down, and the surface scale of SVG's lighting — the
/// height, in device pixels, of a pixel the layer fully covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BevelLight {
    pub toward: [f32; 3],
    pub surface_scale: f32,
    /// The blur that turns the silhouette into heights, in device pixels.
    pub sigma: f32,
}

/// The steepest a bevel's depth is taken as.
pub const BEVEL_DEPTH_MOST: f32 = 10.0;

/// How a bevel's numbers become light and heights once its size has been
/// carried into device pixels by `scale`. `None` for a bevel asked for
/// nothing: no size, no depth, no direction, or something that is not a
/// number.
pub fn bevel_light(
    size: f32,
    depth: f32,
    [lx, ly]: [f32; 2],
    altitude: f32,
    scale: f32,
) -> Option<BevelLight> {
    let size = size * scale;
    if !(size.is_finite() && size > 0.0 && depth.is_finite() && depth > 0.0) {
        return None;
    }
    let depth = depth.min(BEVEL_DEPTH_MOST);
    let across = lx.hypot(ly);
    if !(across.is_finite() && across > 0.0) {
        return None;
    }
    // Off the page altogether the flat top would be as lit as a slope
    // can be, and straight overhead it would be lit as much as any.
    let altitude = if altitude.is_finite() {
        altitude.clamp(1.0, 89.0)
    } else {
        30.0
    };
    let e = altitude.to_radians();
    Some(BevelLight {
        toward: [lx / across * e.cos(), ly / across * e.cos(), e.sin()],
        surface_scale: depth * size * 0.5,
        sigma: size * 0.5,
    })
}

/// How much of a bevel's highlight and of its shadow a slope takes, from
/// how much light falls on it (`lit`, the cosine between the slope's
/// normal and the light) against how much falls on the flat top
/// (`flat`). The two never overlap: a slope is lit or shaded, never both.
pub fn bevel_shares(lit: f32, flat: f32) -> (f32, f32) {
    let flat = flat.clamp(1e-4, 1.0 - 1e-4);
    (
        ((lit - flat) / (1.0 - flat)).clamp(0.0, 1.0),
        ((flat - lit) / flat).clamp(0.0, 1.0),
    )
}

/// The most a shadow's spread is taken as. A spread of one would gain
/// the coverage without bound, which is a hard edge cut wherever the
/// blur left anything at all: aliased, and dependent on noise in the
/// last bit of a blur. At this much it is a gain of twenty.
pub const SPREAD_MOST: f32 = 0.95;

/// What a shadow's coverage is multiplied by before it is held at full:
/// one for no spread, rising to twenty at [`SPREAD_MOST`]. Anything that
/// is not a number is no spread.
pub fn spread_gain(spread: f32) -> f32 {
    let s = if spread.is_finite() {
        spread.clamp(0.0, SPREAD_MOST)
    } else {
        0.0
    };
    1.0 / (1.0 - s)
}

impl Effect {
    /// How far, in the space the effect is written in, it can reach beyond
    /// the layer's own bounds — what bounds and dirty regions must grow by.
    pub fn reach(&self) -> f32 {
        // Three iterated box blurs reach about 3σ — but never less than
        // three pixels once there is any blur at all, since each box is
        // held at a radius of one however small σ is. Estimated from σ
        // alone, a shadow blurred by a sixth of a pixel reached half a
        // pixel on paper and three in fact, and a rectangle repainted
        // beside it read a silhouette cut short (seed 172).
        let blurred = |blur: f32| {
            if blur.abs() > 0.01 {
                (blur.abs() * 3.0).max(3.0)
            } else {
                0.0
            }
        };
        match self {
            // Round up generously, then add the offset, which can point
            // either way.
            Effect::DropShadow { dx, dy, blur, .. } => {
                blurred(*blur) + dx.abs().max(dy.abs()) + 2.0
            }
            Effect::Outline { width, .. } => width.abs() + 2.0,
            // An inner shadow stays inside the layer, but it still needs
            // the silhouette read from a ring outside it to know where the
            // edge is.
            Effect::InnerShadow { dx, dy, blur, .. } => {
                blurred(*blur) + dx.abs().max(dy.abs()) + 2.0
            }
            // Inside as well, read from the same ring; one more for the
            // slope's taps either side.
            Effect::Bevel { size, .. } => blurred(size * 0.5) + 3.0,
            // Nowhere the layer is not.
            Effect::ColorOverlay { .. } => 0.0,
            Effect::Blur { sigma } => blurred(*sigma),
        }
    }

    /// How far a layer's effects together reach beyond its own bounds.
    ///
    /// Not the widest of them: a blur moves the edge every other effect
    /// is grown from, so a shadow under a blurred layer reaches the blur
    /// and then its own reach past that. Blurs add up for the same
    /// reason; the rest are drawn from the one edge and the widest
    /// decides.
    pub fn reach_of<'a>(effects: impl IntoIterator<Item = &'a Effect>) -> f32 {
        let (mut blurs, mut widest) = (0.0f32, 0.0f32);
        for effect in effects {
            match effect {
                Effect::Blur { .. } => blurs += effect.reach(),
                _ => widest = widest.max(effect.reach()),
            }
        }
        blurs + widest
    }

    /// How far a layer's blurs together soften it, as one sigma: two
    /// gaussians in turn are one whose variance is the sum of theirs.
    /// Nought for none, or none that softens anything.
    pub fn blur_of<'a>(effects: impl IntoIterator<Item = &'a Effect>) -> f32 {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::Blur { sigma } if sigma.is_finite() && *sigma > 0.01 => Some(sigma * sigma),
                _ => None,
            })
            .sum::<f32>()
            .sqrt()
    }

    /// The same effect, read in a space one transform further out.
    ///
    /// An offset is a vector in the space the effect is written in — the
    /// layer's parent — so where it points is the transform's to say and
    /// the transform's shift is no part of it. A page turned a quarter
    /// round with the light left where it was would light every layer
    /// from a new direction; so would a group dissolved after being
    /// turned.
    pub fn carried_through(&self, m: Transform) -> Effect {
        let mut out = self.clone();
        // An offset has a direction and goes through the transform as a
        // vector; how far the shadow is blurred and how wide the outline
        // is drawn are lengths without one, and go by the scale alone.
        let k = m.max_scale();
        match &mut out {
            Effect::DropShadow { dx, dy, blur, .. } | Effect::InnerShadow { dx, dy, blur, .. } => {
                (*dx, *dy) = (m.a * *dx + m.c * *dy, m.b * *dx + m.d * *dy);
                *blur *= k;
            }
            Effect::Outline { width, .. } => *width *= k,
            // The light comes from a direction in the space the bevel is
            // written in, so it turns — and, through a mirror, flips —
            // with the transform, as an offset does.
            Effect::Bevel { size, lx, ly, .. } => {
                *size *= k;
                (*lx, *ly) = (m.a * *lx + m.c * *ly, m.b * *lx + m.d * *ly);
            }
            Effect::ColorOverlay { .. } => {}
            Effect::Blur { sigma } => *sigma *= k,
        }
        out
    }

    /// Whether the effect is painted over the layer rather than behind it.
    /// An inner shadow is inside the silhouette, so it has to land on top
    /// of the pixels it is shading.
    pub fn over(&self) -> bool {
        matches!(self, Effect::InnerShadow { .. } | Effect::Bevel { .. })
    }

    /// Whether the effect puts anything down at all: a shadow or an
    /// outline at no opacity, an outline of no width and a bevel with no
    /// light to be lit by draw nothing, and the renderer gives them no
    /// room (`an_effect_that_draws_nothing_is_no_effect`).
    pub fn draws(&self) -> bool {
        match self {
            Effect::DropShadow { opacity, .. } | Effect::InnerShadow { opacity, .. } => {
                *opacity > 0.0
            }
            Effect::Outline { width, opacity, .. } => *width > 0.0 && *opacity > 0.0,
            Effect::Bevel {
                size,
                depth,
                lx,
                ly,
                altitude,
                opacity,
                ..
            } => *opacity > 0.0 && bevel_light(*size, *depth, [*lx, *ly], *altitude, 1.0).is_some(),
            Effect::ColorOverlay { opacity, .. } => *opacity > 0.0,
            Effect::Blur { sigma } => sigma.is_finite() && *sigma > 0.01,
        }
    }

    /// A short name for history labels and the effect list.
    pub fn label(&self) -> &'static str {
        match self {
            Effect::DropShadow { .. } => "Drop shadow",
            Effect::Outline { .. } => "Outline",
            Effect::InnerShadow { .. } => "Inner shadow",
            Effect::Bevel { .. } => "Bevel",
            Effect::ColorOverlay { .. } => "Colour overlay",
            Effect::Blur { .. } => "Blur",
        }
    }
}

impl Node {
    /// Every colour written on the layer, handed over one at a time to be
    /// read or rewritten: its fill and stroke, a gradient's stops, the
    /// paint laid on it, the text it sets and the runs styled inside it,
    /// the ground of a frame, the colours of its effects, and the ones
    /// inside its mask.
    ///
    /// Exhaustive by construction — a new kind of layer or a new effect
    /// will not compile until it says whether it holds a colour — which
    /// is the point. Anything that has to keep a document's colours in
    /// step with something else, as the palette does, is only as good as
    /// this walk is complete.
    pub fn each_color_mut(&mut self, f: &mut impl FnMut(&mut AuthoredColor)) {
        match &mut self.kind {
            NodeKind::Group | NodeKind::Raster(_) | NodeKind::Instance { .. } => {}
            // An adjustment or a filter, which have to be asked kind by
            // kind rather than waved past. The line that stood here said
            // neither holds a colour of its own, however much it changes
            // them — convincing, and wrong: a gradient map is a *ramp of
            // colours*, and a named one in it never settled. The palette
            // moved and every layer followed except that one, which kept
            // what it was authored with and went into the file that way.
            // Matching `Adjustment(_)` is what let it through: this walk's
            // promise is that a new kind will not compile until it says
            // whether it holds a colour, and that promise stopped at the
            // `NodeKind` and never reached inside.
            NodeKind::Adjustment(adjustment) => adjustment.each_color_mut(f),
            NodeKind::Filter(filter) => filter.each_color_mut(f),
            NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => {
                for stroke in strokes {
                    stroke.each_color_mut(f);
                }
            }
            NodeKind::Vector {
                fill,
                stroke,
                gradient,
                shape: _,
            } => {
                if let Some(fill) = fill {
                    f(fill);
                }
                if let Some(stroke) = stroke {
                    f(&mut stroke.color);
                }
                if let Some(gradient) = gradient {
                    for stop in gradient.stops_mut() {
                        f(&mut stop.color);
                    }
                }
            }
            NodeKind::Text(spec) => {
                f(&mut spec.fill);
                for run in &mut spec.runs {
                    if let Some(fill) = &mut run.fill {
                        f(fill);
                    }
                }
            }
            NodeKind::Artboard { background, .. } => {
                if let Some(ground) = background {
                    f(ground);
                }
            }
        }
        for effect in &mut self.effects {
            match effect {
                Effect::DropShadow { color, .. }
                | Effect::Outline { color, .. }
                | Effect::InnerShadow { color, .. }
                | Effect::ColorOverlay { color, .. } => f(color),
                Effect::Bevel {
                    highlight, shadow, ..
                } => {
                    f(highlight);
                    f(shadow);
                }
                Effect::Blur { .. } => {}
            }
        }
        if let Some(mask) = &mut self.mask {
            mask.each_color_mut(f);
        }
    }

    /// Every resource the layer names — the bytes it is drawn from that
    /// live beside the document rather than in it: a picture's pixels, a
    /// picture mask's, a colour lookup's table.
    ///
    /// Exhaustive for the same reason `each_color_mut` is: anything that
    /// carries a layer somewhere else (a clipboard into another document,
    /// an audit of what a file holds) is only as good as this walk is
    /// complete, and the walks that stood in for it were written out by
    /// hand in each place, knew about pictures and picture masks, and
    /// were silently wrong the day a table became a resource too.
    pub fn each_resource(&self, f: &mut impl FnMut(&str)) {
        match &self.kind {
            NodeKind::Raster(r) => f(&r.resource_id),
            NodeKind::Adjustment(adjustment) => match adjustment {
                Adjustment::ColorLookup { table, .. } => f(table),
                Adjustment::BrightnessContrast { .. }
                | Adjustment::Exposure { .. }
                | Adjustment::HueSaturation { .. }
                | Adjustment::Levels { .. }
                | Adjustment::Curves { .. }
                | Adjustment::WhiteBalance { .. }
                | Adjustment::Vibrance { .. }
                | Adjustment::BlackAndWhite { .. }
                | Adjustment::GradientMap { .. }
                | Adjustment::Invert { .. }
                | Adjustment::SelectiveHsl { .. }
                | Adjustment::ShadowsHighlights { .. }
                | Adjustment::ColorBalance { .. }
                | Adjustment::Posterize { .. }
                | Adjustment::Threshold { .. }
                | Adjustment::ChannelMixer { .. }
                | Adjustment::PhotoFilter { .. } => {}
            },
            NodeKind::Filter(filter) => match filter {
                Filter::GaussianBlur { .. }
                | Filter::Sharpen { .. }
                | Filter::HighPass { .. }
                | Filter::Halftone { .. }
                | Filter::Emboss { .. }
                | Filter::Twirl { .. }
                | Filter::Pinch { .. }
                | Filter::Ripple { .. }
                | Filter::FindEdges { .. }
                | Filter::OilPaint { .. }
                | Filter::LensCorrection { .. }
                | Filter::MotionBlur { .. }
                | Filter::RadialBlur { .. }
                | Filter::Pixelate { .. }
                | Filter::Median { .. }
                | Filter::SurfaceBlur { .. }
                | Filter::Noise { .. }
                | Filter::Vignette { .. } => {}
            },
            NodeKind::Group
            | NodeKind::Instance { .. }
            | NodeKind::Paint { .. }
            | NodeKind::Clone { .. }
            | NodeKind::Vector { .. }
            | NodeKind::Text(_)
            | NodeKind::Artboard { .. } => {}
        }
        if let Some(mask) = &self.mask {
            match &mask.kind {
                MaskKind::Raster { resource_id, .. } => f(resource_id),
                MaskKind::Vector { .. } | MaskKind::Painted { .. } => {}
            }
        }
    }

    fn base(name: &str, kind: NodeKind) -> Self {
        Self {
            name: name.to_string(),
            kind,
            transform: Transform::default(),
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            mask: None,
            effects: Vec::new(),
            locked: false,
            clipped: false,
            pinned: Pinning::default(),
            style: None,
        }
    }

    pub fn group(name: &str) -> Self {
        Self::base(name, NodeKind::Group)
    }

    /// A live copy of `of` — see [`NodeKind::Instance`].
    pub fn instance(name: &str, of: NodeId) -> Self {
        Self::base(
            name,
            NodeKind::Instance {
                of,
                replaces: Vec::new(),
            },
        )
    }

    pub fn artboard(
        name: &str,
        width: f32,
        height: f32,
        background: Option<AuthoredColor>,
    ) -> Self {
        Self::base(
            name,
            NodeKind::Artboard {
                width,
                height,
                background,
                export_scale: 1.0,
            },
        )
    }

    pub fn vector(name: &str, shape: VectorShape) -> Self {
        Self::base(
            name,
            NodeKind::Vector {
                shape,
                fill: None,
                stroke: None,
                gradient: None,
            },
        )
    }

    pub fn raster(name: &str, raster: RasterRef) -> Self {
        Self::base(name, NodeKind::Raster(raster))
    }

    pub fn filter(name: &str, filter: Filter) -> Self {
        Self::base(name, NodeKind::Filter(filter))
    }

    /// An empty layer to clone onto.
    pub fn clone_layer(name: &str) -> Self {
        Self::base(
            name,
            NodeKind::Clone {
                strokes: Vec::new(),
            },
        )
    }

    /// An empty layer to paint on.
    pub fn paint(name: &str) -> Self {
        Self::base(
            name,
            NodeKind::Paint {
                strokes: Vec::new(),
            },
        )
    }

    pub fn text(name: &str, spec: TextSpec) -> Self {
        Self::base(name, NodeKind::Text(spec))
    }

    pub fn adjustment(name: &str, adjustment: Adjustment) -> Self {
        Self::base(name, NodeKind::Adjustment(adjustment))
    }
}

#[cfg(test)]
mod tests {
    /// A run written before runs had a size has none.
    #[test]
    fn a_run_from_before_run_sizes_has_none() {
        let old = r#"{"start":0,"end":2,"bold":true}"#;
        let run: super::StyleRun = serde_json::from_str(old).unwrap();
        assert_eq!(run.size, None);
        let mut sized = run.clone();
        sized.size = Some(30.0);
        assert!(sized.says_anything());
        let block = super::TextSpec::new(
            "ab",
            12.0,
            super::AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
        assert_eq!(block.styling_under(Some(&sized)).size, 30.0);
        assert_eq!(block.styling_under(Some(&run)).size, 12.0);
    }

    /// A block written before paragraph spacing opens without it.
    #[test]
    fn a_block_from_before_paragraph_spacing_opens_without_it() {
        let old =
            r#"{"text":"a\nb","size":12.0,"fill":{"Srgb":{"r":0.0,"g":0.0,"b":0.0,"a":1.0}}}"#;
        let spec: super::TextSpec = serde_json::from_str(old).unwrap();
        assert_eq!(spec.paragraph_spacing, 0.0);
        assert_eq!(spec.paragraph_gap(), 0.0);
    }

    /// A shadow written before there was a spread opens soft, and the
    /// gain a spread asks for is one at none, held at twenty, and one
    /// for anything that is not a number.
    #[test]
    fn a_shadow_from_before_spread_opens_soft() {
        let old = r#"{"DropShadow":{"dx":1.0,"dy":2.0,"blur":3.0,"color":{"Srgb":{"r":0.0,"g":0.0,"b":0.0,"a":1.0}},"opacity":0.5}}"#;
        let effect: super::Effect = serde_json::from_str(old).unwrap();
        assert!(matches!(effect, super::Effect::DropShadow { spread, .. } if spread == 0.0));
        assert_eq!(super::spread_gain(0.0), 1.0);
        assert!((super::spread_gain(0.8) - 5.0).abs() < 1e-4);
        assert_eq!(super::spread_gain(-3.0), 1.0, "no spread below none");
        assert_eq!(
            super::spread_gain(f32::NAN),
            1.0,
            "nor from what is not a number"
        );
        assert!(
            (super::spread_gain(2.0) - 20.0).abs() < 1e-3,
            "and at most twenty"
        );
    }

    /// Round a conic gradient's centre clockwise on the page from its
    /// angle: right is the start, below a quarter, left a half, above
    /// three quarters; the start moves with the angle; and the centre
    /// itself, and an angle that is no number, read as the start.
    #[test]
    fn a_conic_gradient_turns_clockwise_from_its_angle() {
        let c = [0.5, 0.5];
        let at = |angle: f32, u: f32, v: f32| conic_turn(c, angle, u, v);
        for (u, v, want) in [
            (0.9, 0.5, 0.0),
            (0.5, 0.9, 0.25),
            (0.1, 0.5, 0.5),
            (0.5, 0.1, 0.75),
        ] {
            assert!(
                (at(0.0, u, v) - want).abs() < 1e-5,
                "{u},{v}: {}",
                at(0.0, u, v)
            );
        }
        // Starting from the top, the top is the start and the right a
        // quarter round.
        assert!(at(270.0, 0.5, 0.1).abs() < 1e-5 || (at(270.0, 0.5, 0.1) - 1.0).abs() < 1e-5);
        assert!((at(270.0, 0.9, 0.5) - 0.25).abs() < 1e-5);
        assert_eq!(at(0.0, 0.5, 0.5), 0.0, "the centre");
        assert!(
            (at(f32::NAN, 0.5, 0.9) - 0.25).abs() < 1e-5,
            "no number is nought"
        );
        // Every reading is on the ramp, just short of the seam included.
        let just = at(0.0, 0.9, 0.4999);
        assert!((0.0..1.0).contains(&just) && just > 0.99, "{just}");
    }

    /// A polygon's corners: as many as asked, the first straight up,
    /// round the ellipse of its box; a star's every other one pulled in;
    /// and numbers that are no numbers held to what can be drawn.
    /// A corner rounded is two anchors on its sides and the arc between
    /// them: the arc on the circle of the radius that touches both sides,
    /// a radius too big held to half the shorter side, and only corners
    /// at that — not the ends of an open path, nor an anchor its sides
    /// run straight through, nor one beside a curve.
    #[test]
    fn a_corner_rounded_is_an_arc_between_its_sides() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let r = round_corners(&square, &[], true, 2.0, None).unwrap();
        assert_eq!(r.points.len(), 8);
        assert_eq!(r.from, vec![0, 0, 1, 1, 2, 2, 3, 3]);
        assert_eq!(&r.points[..2], &[[0.0, 2.0], [2.0, 0.0]]);
        // The cubic between the first two, at its middle, is on the
        // circle about (2, 2) of radius 2.
        let (a, b) = (r.points[0], r.points[1]);
        let (c1, c2) = (
            [a[0] + r.handles[0][2], a[1] + r.handles[0][3]],
            [b[0] + r.handles[1][0], b[1] + r.handles[1][1]],
        );
        for t in [0.25f32, 0.5, 0.75] {
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let p = [
                w[0] * a[0] + w[1] * c1[0] + w[2] * c2[0] + w[3] * b[0],
                w[0] * a[1] + w[1] * c1[1] + w[2] * c2[1] + w[3] * b[1],
            ];
            let off = ((p[0] - 2.0).hypot(p[1] - 2.0) - 2.0).abs();
            assert!(off < 1e-3, "{p:?} is {off} off the circle");
        }
        // Far too big a radius: each corner takes half of each side, and
        // the square is a circle.
        let big = round_corners(&square, &[], true, 100.0, None).unwrap();
        assert_eq!(&big.points[..2], &[[0.0, 5.0], [5.0, 0.0]]);
        // One corner, asked for by itself.
        let one = round_corners(&square, &[], true, 2.0, Some(&[2])).unwrap();
        assert_eq!(one.from, vec![0, 1, 2, 2, 3]);
        // An open line: its ends are no corners, its middle is.
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let bent = round_corners(&line, &[], false, 2.0, None).unwrap();
        assert_eq!(bent.from, vec![0, 1, 1, 2]);
        // An anchor its sides run straight through is no corner.
        let straight = [[0.0, 0.0], [5.0, 0.0], [10.0, 0.0]];
        assert!(round_corners(&straight, &[], false, 2.0, None).is_none());
        // A corner with a curve on one side keeps its anchor.
        let mut curved = [[0.0f32; 4]; 4];
        curved[1] = [0.0, 0.0, 0.0, 4.0];
        let kept = round_corners(&square, &curved, true, 2.0, None).unwrap();
        assert_eq!(
            kept.from,
            vec![0, 0, 1, 2, 3, 3],
            "the corners each side of the curve stay"
        );
        // Nothing to round by, nothing rounded.
        for radius in [0.0, -1.0, f32::NAN] {
            assert!(round_corners(&square, &[], true, radius, None).is_none());
        }
        // A rounded polygon is drawn through the same arcs.
        let poly = VectorShape::Polygon {
            rx: 5.0,
            ry: 5.0,
            points: 4,
            inner: 1.0,
            corner: 1.0,
        };
        let Some(VectorShape::Path {
            points, handles, ..
        }) = poly.as_path()
        else {
            unreachable!()
        };
        assert_eq!((points.len(), handles.len()), (8, 8));
    }

    #[test]
    fn a_polygon_has_its_corners_where_a_polygon_does() {
        let five = VectorShape::polygon_corners(10.0, 8.0, 5, 1.0);
        assert_eq!(five.len(), 5);
        assert!(
            (five[0][0] - 10.0).abs() < 1e-5 && five[0][1].abs() < 1e-5,
            "top: {:?}",
            five[0]
        );
        for p in &five {
            let (nx, ny) = ((p[0] - 10.0) / 10.0, (p[1] - 8.0) / 8.0);
            assert!(
                (nx * nx + ny * ny - 1.0).abs() < 1e-4,
                "on the ellipse: {p:?}"
            );
        }
        // Clockwise on the page (y down): the second corner is right of
        // the first.
        assert!(five[1][0] > five[0][0]);
        let star = VectorShape::polygon_corners(10.0, 10.0, 5, 0.4);
        assert_eq!(star.len(), 10, "a star of five has ten corners");
        let out = |p: [f32; 2]| ((p[0] - 10.0).powi(2) + (p[1] - 10.0).powi(2)).sqrt();
        for (i, p) in star.iter().enumerate() {
            let want = if i % 2 == 0 { 10.0 } else { 4.0 };
            assert!((out(*p) - want).abs() < 1e-4, "corner {i} at {}", out(*p));
        }
        assert_eq!(VectorShape::polygon_corners(1.0, 1.0, 0, 1.0).len(), 3);
        assert_eq!(VectorShape::polygon_corners(1.0, 1.0, 5000, 1.0).len(), 100);
        assert_eq!(
            VectorShape::polygon_corners(1.0, 1.0, 6, f32::NAN).len(),
            6,
            "a depth that is no number is no star"
        );
        assert!(VectorShape::polygon_corners(f32::NAN, 2.0, 4, 1.0)
            .iter()
            .all(|p| p[0].is_finite() && p[1].is_finite()));
        // And it is the path through those corners.
        let VectorShape::Path { points, closed, .. } = (VectorShape::Polygon {
            rx: 10.0,
            ry: 8.0,
            points: 5,
            inner: 1.0,
            corner: 0.0,
        })
        .as_path()
        .unwrap() else {
            panic!("a path");
        };
        assert!(closed && points == five);
    }

    /// Rings out from a focus start at it and end on the outer ring all
    /// the way round, whichever side of the focus; with the focus at the
    /// centre they are the plain distance out; and a focus past the ring
    /// is held just inside it.
    #[test]
    fn rings_from_a_focus_start_there_and_end_on_the_outer_ring() {
        use super::{focal_offset, focal_ramp, FOCUS_REACH};

        for f in [[0.0, 0.0], [0.4, -0.3], [-0.9, 0.1], [0.0, 0.99]] {
            assert!(focal_ramp(f, f).abs() < 1e-6, "nothing at the focus {f:?}");
            for k in 0..32 {
                let a = k as f32 / 32.0 * std::f32::consts::TAU;
                let rim = [a.cos(), a.sin()];
                let t = focal_ramp(rim, f);
                assert!((t - 1.0).abs() < 2e-3, "{f:?} at {a}: {t}");
                // Twice as far out from the focus along the same line is
                // past the ring, and half way is inside it.
                let twice = [2.0 * rim[0] - f[0], 2.0 * rim[1] - f[1]];
                let half = [(rim[0] + f[0]) / 2.0, (rim[1] + f[1]) / 2.0];
                assert!(focal_ramp(twice, f) > 1.0 && focal_ramp(half, f) < 1.0);
            }
        }
        let plain = focal_ramp([0.3, 0.4], [0.0, 0.0]);
        assert!((plain - 0.5).abs() < 1e-6, "{plain}");
        // Measured through the axes and in radii; and past the ring, held.
        let f = focal_offset(
            [0.5, 0.5],
            0.25,
            Some([2.0, 0.0, 0.0, 1.0]),
            Some([0.55, 0.6]),
        );
        assert!(
            (f[0] - 0.4).abs() < 1e-6 && (f[1] - 0.4).abs() < 1e-6,
            "{f:?}"
        );
        let held = focal_offset([0.5, 0.5], 0.25, None, Some([1.5, 0.5]));
        assert!(
            (held[0] - FOCUS_REACH).abs() < 1e-6 && held[1] == 0.0,
            "{held:?}"
        );
        assert_eq!(focal_offset([0.5, 0.5], 0.25, None, None), [0.0, 0.0]);
    }

    /// A stroke written before there was a choice says nothing about how
    /// it ends or turns, and has to come back the way it was drawn then:
    /// a distance from the line, which rounds every end and every corner.
    #[test]
    fn a_stroke_from_before_the_choice_reads_as_round() {
        let old = r#"{"color":{"Srgb":{"r":1,"g":0,"b":0,"a":1}},"width":4}"#;
        let stroke: Stroke = serde_json::from_str(old).unwrap();
        assert_eq!(
            (stroke.cap, stroke.join),
            (StrokeCap::Round, StrokeJoin::Round)
        );
        assert!(stroke.dash.is_empty() && stroke.widths.is_empty());
    }
    use super::*;

    fn block(text: &str, runs: Vec<StyleRun>) -> TextSpec {
        let mut spec = TextSpec::new(
            text,
            12.0,
            AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
        spec.runs = runs;
        spec
    }

    /// The pieces cover the text once, in order, whatever the runs do:
    /// none at all, one in the middle, two that overlap, one hanging
    /// past the end, and one that starts where the last one stopped.
    #[test]
    fn the_pieces_cover_the_text_once_however_the_runs_lie() {
        let cover = |spec: &TextSpec| {
            let pieces = spec.pieces();
            let mut at = 0;
            for (start, end, _) in &pieces {
                assert_eq!(*start, at, "pieces run on from each other: {pieces:?}");
                assert!(end >= start, "and never backwards: {pieces:?}");
                at = *end;
            }
            assert_eq!(at, spec.text.len(), "to the end: {pieces:?}");
            pieces
        };

        assert_eq!(cover(&block("hello", vec![])), [(0, 5, None)]);
        assert_eq!(
            cover(&block("hello", vec![StyleRun::over(1, 3)])),
            [(0, 1, None), (1, 3, Some(0)), (3, 5, None)]
        );
        // Overlapping: the one that starts first keeps what it has and
        // the other picks up where that left off.
        assert_eq!(
            cover(&block(
                "hello",
                vec![StyleRun::over(0, 3), StyleRun::over(2, 5)]
            )),
            [(0, 3, Some(0)), (3, 5, Some(1))]
        );
        // Hanging past the end, which an edit that shortened the text
        // would leave behind.
        assert_eq!(
            cover(&block("hi", vec![StyleRun::over(1, 900)])),
            [(0, 1, None), (1, 2, Some(0))]
        );
        // Given out of order, and abutting.
        assert_eq!(
            cover(&block(
                "abcd",
                vec![StyleRun::over(2, 4), StyleRun::over(0, 2)]
            )),
            [(0, 2, Some(1)), (2, 4, Some(0))]
        );
        // Empty text still has one piece to hand back.
        assert_eq!(
            cover(&block("", vec![StyleRun::over(0, 4)])),
            [(0, 0, None)]
        );
    }

    /// A range that an edit left in the middle of a character is cut
    /// where the character starts, not through it.
    #[test]
    fn a_piece_never_cuts_a_character_in_half() {
        // "é" is two bytes, "😀" is four.
        let spec = block("aé😀b", vec![StyleRun::over(2, 6)]);
        for (start, end, _) in spec.pieces() {
            assert!(
                spec.text.is_char_boundary(start) && spec.text.is_char_boundary(end),
                "{start}..{end} of {:?}",
                spec.text
            );
        }
        let mut inside = block("aé😀b", vec![StyleRun::over(2, 5)]);
        inside.runs[0].start = 2;
        let cut: Vec<_> = inside.pieces().iter().map(|p| (p.0, p.1)).collect();
        assert_eq!(cut, [(0, 1), (1, 3), (3, 8)], "clipped back to a boundary");
    }

    /// A run says only what it changes; everything else is the block's.
    #[test]
    fn a_run_lays_its_choices_over_the_block_and_no_more() {
        let mut spec = block("hi", vec![]);
        spec.italic = true;
        spec.font = "Some Face".into();
        let mut run = StyleRun::over(0, 1);
        run.bold = Some(true);
        run.italic = Some(false);
        let under = spec.styling_under(Some(&run));
        assert!(under.bold && !under.italic, "what the run asked for");
        assert_eq!(under.font, "Some Face", "and the block's for the rest");
        assert_eq!(under.size, spec.size);
        assert!(under.runs.is_empty(), "the piece is not styled again");
        assert!(
            spec.styling_under(None).italic,
            "no run is the block itself"
        );
    }
}

#[cfg(test)]
mod projective_tests {
    use super::*;

    /// The map lands each corner of the square on its own corner of the
    /// quad, comes back by its inverse, and keeps a straight line
    /// straight — and a quad that is not convex has no map at all.
    #[test]
    fn a_picture_s_corners_go_where_they_are_put() {
        let q = [[10.0, 5.0], [90.0, 20.0], [80.0, 70.0], [15.0, 60.0]];
        let p = Projective::onto(q).unwrap();
        for (corner, (u, v)) in q
            .iter()
            .zip([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)])
        {
            let (x, y) = p.apply(u, v).unwrap();
            assert!((x - corner[0] as f64).abs() < 1e-9 && (y - corner[1] as f64).abs() < 1e-9);
        }
        let back = p.inverse().unwrap();
        for (u, v) in [(0.3, 0.7), (0.5, 0.5), (0.9, 0.1)] {
            let (x, y) = p.apply(u, v).unwrap();
            let (u2, v2) = back.apply(x, y).unwrap();
            assert!((u - u2).abs() < 1e-9 && (v - v2).abs() < 1e-9);
        }
        // Three points along one line of the square stay on one line.
        let pts: Vec<(f64, f64)> = [0.1, 0.5, 0.8]
            .iter()
            .map(|&t| p.apply(t, 0.25 + 0.5 * t).unwrap())
            .collect();
        let cross = (pts[1].0 - pts[0].0) * (pts[2].1 - pts[0].1)
            - (pts[1].1 - pts[0].1) * (pts[2].0 - pts[0].0);
        assert!(cross.abs() < 1e-9, "a line bent: {cross}");
        // A parallelogram is affine: nothing is divided by anything.
        let par = Projective::onto([[0.0, 0.0], [10.0, 0.0], [14.0, 5.0], [4.0, 5.0]]).unwrap();
        assert_eq!((par.m[6], par.m[7]), (0.0, 0.0));
        // A bow tie, a fold and three corners on a line: no map.
        assert!(Projective::onto([[0.0, 0.0], [10.0, 0.0], [0.0, 10.0], [10.0, 10.0]]).is_none());
        assert!(Projective::onto([[0.0, 0.0], [10.0, 0.0], [2.0, 2.0], [0.0, 10.0]]).is_none());
        assert!(Projective::onto([[0.0, 0.0], [5.0, 0.0], [10.0, 0.0], [0.0, 10.0]]).is_none());
        // Either way round is a quad.
        assert!(Projective::onto([[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]]).is_some());
        // From the picture's own pixels: its far corner is the quad's.
        let r = RasterRef {
            resource_id: "x".into(),
            width: 40,
            height: 20,
            corners: Some(q),
            develop: None,
        };
        let (x, y) = r.projective().unwrap().apply(40.0, 20.0).unwrap();
        assert!((x - 80.0).abs() < 1e-6 && (y - 70.0).abs() < 1e-6);
        // And a file from before corners reads as the rectangle.
        let old: RasterRef =
            serde_json::from_str(r#"{"resource_id":"x","width":4,"height":3}"#).unwrap();
        assert_eq!(old.corners, None);
        assert_eq!(old.quad()[2], [4.0, 3.0]);
        assert!(!serde_json::to_string(&old).unwrap().contains("corners"));
    }
}

/// The middle of the page, as a share of its width and height.
fn page_middle() -> [f32; 2] {
    [0.5, 0.5]
}
