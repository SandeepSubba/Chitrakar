//! Rendering for Chitrakar.
//!
//! Current state: a scalar CPU reference renderer that evaluates the scene
//! graph bottom-up into a linear-float pixel buffer. It exists to pin down
//! *correct* output; the wgpu/vello GPU backends (Phase 1) are validated
//! against it.
//!
//! Rendering is clip-aware: [`render_region`] recomputes only a rectangle of
//! the surface, which is what makes the engine's cached incremental renders
//! cheap. [`node_bounds`] reports the document-space area a node can affect,
//! so edits map to small dirty regions. Tiling ([`tiles`]) will refine the
//! invalidation granularity further.
//!
//! Transforms support translation and scale (`a`, `d`, `e`, `f`); shear and
//! rotation (`b`, `c`) arrive with the GPU rasterizer.

pub mod blur;
pub mod text;
pub mod tiles;

use chitrakar_color::{to_working, AuthoredColor, LinearRgba};
use chitrakar_doc::{
    Adjustment, BlendMode, DocError, Document, Filter, Gradient, Mask, MaskKind, NodeId, NodeKind,
    Transform, VectorShape,
};

/// A linear-light, premultiplied float pixel buffer.
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<LinearRgba>,
}

impl Surface {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![LinearRgba::TRANSPARENT; (width * height) as usize],
        }
    }

    pub fn get(&self, x: u32, y: u32) -> LinearRgba {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Encode to 8-bit sRGB RGBA (display/export edge).
    pub fn to_srgb8(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|p| p.to_srgb8()).collect()
    }

    /// Encode one clip region into an existing full-size RGBA8 buffer.
    pub fn encode_srgb8_region(&self, clip: ClipRect, out: &mut [u8]) {
        for y in clip.y0..clip.y1 {
            for x in clip.x0..clip.x1 {
                let i = (y * self.width + x) as usize;
                out[i * 4..i * 4 + 4].copy_from_slice(&self.pixels[i].to_srgb8());
            }
        }
    }

    /// Copy one region's pixels from another surface of the same size.
    pub fn copy_region_from(&mut self, src: &Surface, clip: ClipRect) {
        for y in clip.y0..clip.y1 {
            let row = (y * self.width) as usize;
            let (a, b) = (row + clip.x0 as usize, row + clip.x1 as usize);
            self.pixels[a..b].copy_from_slice(&src.pixels[a..b]);
        }
    }

    fn full_clip(&self) -> ClipRect {
        ClipRect {
            x0: 0,
            y0: 0,
            x1: self.width,
            y1: self.height,
        }
    }
}

/// Integer pixel rectangle, min inclusive / max exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl ClipRect {
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    pub fn intersect(&self, other: ClipRect) -> ClipRect {
        ClipRect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    pub fn union(&self, other: ClipRect) -> ClipRect {
        ClipRect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    pub fn area(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            (self.x1 - self.x0) as u64 * (self.y1 - self.y0) as u64
        }
    }

    /// Clamp a float rect (with padding for seam safety) onto a surface.
    pub fn from_float(x0: f32, y0: f32, x1: f32, y1: f32, width: u32, height: u32) -> ClipRect {
        ClipRect {
            x0: (x0.floor() - 1.0).max(0.0) as u32,
            y0: (y0.floor() - 1.0).max(0.0) as u32,
            x1: ((x1.ceil() + 1.0).max(0.0) as u32).min(width),
            y1: ((y1.ceil() + 1.0).max(0.0) as u32).min(height),
        }
    }
}

/// Document-space extent a node can affect when anything about it changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bounds {
    /// Nothing on screen (e.g. an empty group).
    None,
    /// Float doc-space rect (x0, y0, x1, y1).
    Rect(f32, f32, f32, f32),
    /// The whole canvas (adjustment layers act on everything below).
    Everything,
}

impl Bounds {
    pub fn union(self, other: Bounds) -> Bounds {
        match (self, other) {
            (Bounds::Everything, _) | (_, Bounds::Everything) => Bounds::Everything,
            (Bounds::None, b) | (b, Bounds::None) => b,
            (Bounds::Rect(ax0, ay0, ax1, ay1), Bounds::Rect(bx0, by0, bx1, by1)) => {
                Bounds::Rect(ax0.min(bx0), ay0.min(by0), ax1.max(bx1), ay1.max(by1))
            }
        }
    }

    pub fn to_clip(self, width: u32, height: u32) -> Option<ClipRect> {
        match self {
            Bounds::None => None,
            Bounds::Everything => Some(ClipRect {
                x0: 0,
                y0: 0,
                x1: width,
                y1: height,
            }),
            Bounds::Rect(x0, y0, x1, y1) => {
                let c = ClipRect::from_float(x0, y0, x1, y1, width, height);
                (!c.is_empty()).then_some(c)
            }
        }
    }
}

/// Local size (width, height) of a node's own content, before its transform.
fn local_size(kind: &NodeKind) -> Option<(f32, f32)> {
    match kind {
        NodeKind::Vector { shape, .. } => Some(shape_size(shape)),
        NodeKind::Raster(r) => Some((r.width as f32, r.height as f32)),
        NodeKind::Text(spec) => Some(text::measure(spec)),
        NodeKind::Group | NodeKind::Adjustment(_) | NodeKind::Filter(_) => None,
    }
}

fn shape_size(shape: &VectorShape) -> (f32, f32) {
    match shape {
        VectorShape::Rect { width, height } => (*width, *height),
        VectorShape::Ellipse { rx, ry } => (rx * 2.0, ry * 2.0),
        // Path anchors are normalized to a (0,0) origin on creation; local
        // size is their extent.
        VectorShape::Path { points, .. } => points
            .iter()
            .fold((0.0f32, 0.0f32), |(w, h), p| (w.max(p[0]), h.max(p[1]))),
    }
}

/// Local bounding box (min x, min y, max x, max y). Unlike [`shape_size`]
/// this keeps a negative min — a smooth path's spline can overshoot the
/// anchors, including past the origin.
fn local_bounds(shape: &VectorShape) -> (f32, f32, f32, f32) {
    match shape {
        VectorShape::Path { points, .. } => points.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(x0, y0, x1, y1), p| (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])),
        ),
        _ => {
            let (w, h) = shape_size(shape);
            (0.0, 0.0, w, h)
        }
    }
}

/// Transformed doc-space bounds of a local box.
fn transformed_local_bounds(t: Transform, lb: (f32, f32, f32, f32)) -> Bounds {
    let xs = [t.a * lb.0 + t.e, t.a * lb.2 + t.e];
    let ys = [t.d * lb.1 + t.f, t.d * lb.3 + t.f];
    Bounds::Rect(
        xs[0].min(xs[1]),
        ys[0].min(ys[1]),
        xs[0].max(xs[1]),
        ys[0].max(ys[1]),
    )
}

/// Transformed doc-space bounds of a local (0,0)-(w,h) box.
fn transformed_bounds(t: Transform, w: f32, h: f32) -> Bounds {
    let xs = [t.e, t.a * w + t.e];
    let ys = [t.f, t.d * h + t.f];
    Bounds::Rect(
        xs[0].min(xs[1]),
        ys[0].min(ys[1]),
        xs[0].max(xs[1]),
        ys[0].max(ys[1]),
    )
}

/// Doc-space extent of a node: leaf bounds through its transform; groups are
/// the union of their children. Any adjustment layer in the subtree makes
/// the answer [`Bounds::Everything`], because it acts on all content below.
/// Visibility is ignored on purpose — toggling it dirties the same region.
pub fn node_bounds(doc: &Document, id: NodeId) -> Result<Bounds, DocError> {
    let node = doc.node(id)?;
    Ok(match &node.kind {
        NodeKind::Adjustment(_) | NodeKind::Filter(_) => Bounds::Everything,
        NodeKind::Group => {
            let mut acc = Bounds::None;
            for &child in doc.children_of(id)? {
                acc = acc.union(node_bounds(doc, child)?);
                if acc == Bounds::Everything {
                    break;
                }
            }
            acc
        }
        NodeKind::Vector { shape, stroke, .. } => {
            let flat = flatten_shape(shape);
            let mut bounds = transformed_local_bounds(node.transform, local_bounds(flat.as_ref()));
            // Path strokes are centered on the line, so they overhang the
            // anchor bounds (rect/ellipse strokes are inner bands and don't).
            if let (VectorShape::Path { .. }, Some(stroke)) = (shape, stroke) {
                let pad = stroke.width * node.transform.a.abs().max(node.transform.d.abs());
                if let Bounds::Rect(x0, y0, x1, y1) = bounds {
                    bounds = Bounds::Rect(x0 - pad, y0 - pad, x1 + pad, y1 + pad);
                }
            }
            bounds
        }
        kind => {
            let (w, h) = local_size(kind).unwrap();
            transformed_bounds(node.transform, w, h)
        }
    })
}

/// How far, in pixels, the document's filter stack can carry a change:
/// the summed sample reach of every filter layer (sequential filters
/// compound). A region render whose clip is padded by this much computes
/// correct values for the unpadded interior even next to stale surroundings.
pub fn filter_reach(doc: &Document) -> u32 {
    doc.nodes()
        .map(|(_, node)| match &node.kind {
            NodeKind::Filter(Filter::GaussianBlur { sigma })
            | NodeKind::Filter(Filter::Sharpen { sigma, .. }) => {
                // Three iterated box blurs reach ~3 * box radius ≈ 2.9σ;
                // round up generously.
                (sigma * 3.0).ceil() as u32 + 2
            }
            _ => 0,
        })
        .sum()
}

/// Render a document to a new full-size surface.
pub fn render(doc: &Document) -> Result<Surface, DocError> {
    let mut surface = Surface::new(doc.meta.width, doc.meta.height);
    let clip = surface.full_clip();
    render_region(doc, &mut surface, clip)?;
    Ok(surface)
}

/// Recompute one region of a surface from scratch (clears it first). Pixels
/// outside `clip` are untouched.
pub fn render_region(
    doc: &Document,
    surface: &mut Surface,
    clip: ClipRect,
) -> Result<(), DocError> {
    if clip.is_empty() {
        return Ok(());
    }
    for y in clip.y0..clip.y1 {
        let row = (y * surface.width) as usize;
        surface.pixels[row + clip.x0 as usize..row + clip.x1 as usize]
            .fill(LinearRgba::TRANSPARENT);
    }
    render_group(doc, doc.root(), surface, clip)
}

fn render_group(
    doc: &Document,
    group: NodeId,
    dst: &mut Surface,
    clip: ClipRect,
) -> Result<(), DocError> {
    // Children are stored bottom-to-top (painter's order).
    for &child in doc.children_of(group)? {
        let node = doc.node(child)?;
        if !node.visible || node.opacity <= 0.0 {
            continue;
        }
        let mask = node.mask.as_ref();
        match &node.kind {
            NodeKind::Group => {
                // Isolate the group on its own surface so group opacity,
                // blend, and mask apply to the composite, not per child.
                let mut sub = Surface::new(dst.width, dst.height);
                render_group(doc, child, &mut sub, clip)?;
                if let Some(mask) = mask {
                    apply_mask(doc, mask, &mut sub, clip);
                }
                composite(dst, &sub, node.opacity, node.blend, clip);
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                gradient,
            } => {
                // A gradient paints in place of the flat fill.
                let fill_paint = match gradient {
                    Some(g) => Paint::from_gradient(doc, g, &flatten_shape(shape), node.opacity),
                    None => {
                        fill.map(|c| Paint::Solid(scale_alpha(resolve_color(doc, c), node.opacity)))
                    }
                };
                if let Some(paint) = fill_paint {
                    paint_shape(
                        dst,
                        doc,
                        shape,
                        node.transform,
                        &paint,
                        node.blend,
                        clip,
                        None,
                        mask,
                    );
                }
                if let Some(stroke) = stroke {
                    let color = scale_alpha(resolve_color(doc, stroke.color), node.opacity);
                    paint_shape(
                        dst,
                        doc,
                        shape,
                        node.transform,
                        &Paint::Solid(color),
                        node.blend,
                        clip,
                        Some(stroke.width),
                        mask,
                    );
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = doc.resource(&raster.resource_id) {
                    if !res.rgba8.is_empty() {
                        draw_raster(
                            dst,
                            doc,
                            res,
                            node.transform,
                            node.opacity,
                            node.blend,
                            clip,
                            mask,
                        );
                    }
                }
            }
            NodeKind::Adjustment(adj) => {
                // An adjustment layer transforms everything composited below
                // it, weighted by its opacity and mask coverage.
                for y in clip.y0..clip.y1 {
                    for x in clip.x0..clip.x1 {
                        let weight = node.opacity * coverage_at(doc, mask, x, y);
                        if weight <= 0.0 {
                            continue;
                        }
                        let i = (y * dst.width + x) as usize;
                        let adjusted = apply_adjustment(adj, dst.pixels[i]);
                        dst.pixels[i] = lerp(dst.pixels[i], adjusted, weight);
                    }
                }
            }
            NodeKind::Filter(filter) => apply_filter(doc, filter, node.opacity, mask, dst, clip),
            NodeKind::Text(spec) => draw_text(
                dst,
                doc,
                spec,
                node.transform,
                node.opacity,
                node.blend,
                clip,
                mask,
            ),
        }
    }
    Ok(())
}

/// Authored color → working space: CMYK goes through the document's press
/// profile when one is set; everything else (and profileless CMYK) uses the
/// device formulas in `chitrakar_color`.
fn resolve_color(doc: &Document, color: AuthoredColor) -> LinearRgba {
    if let (AuthoredColor::Cmyk { c, m, y, k, a }, Some(cms)) = (color, doc.cmyk_cms()) {
        return cms.to_working(c, m, y, k, a);
    }
    to_working(color)
}

fn scale_alpha(px: LinearRgba, s: f32) -> LinearRgba {
    LinearRgba {
        r: px.r * s,
        g: px.g * s,
        b: px.b * s,
        a: px.a * s,
    }
}

fn lerp(a: LinearRgba, b: LinearRgba, t: f32) -> LinearRgba {
    LinearRgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn blend_pixel(src: LinearRgba, dst: LinearRgba, mode: BlendMode) -> LinearRgba {
    match mode {
        BlendMode::Normal => src.over(dst),
        // Premultiplied separable blend: B(src,dst) mixed by coverage.
        BlendMode::Multiply => separable(src, dst, |s, d| s * d),
        BlendMode::Screen => separable(src, dst, |s, d| s + d - s * d),
    }
}

fn separable(src: LinearRgba, dst: LinearRgba, f: impl Fn(f32, f32) -> f32) -> LinearRgba {
    let un = |v: f32, a: f32| if a > 0.0 { v / a } else { 0.0 };
    let mix = |s: f32, d: f32| {
        let (sa, da) = (src.a, dst.a);
        let blended = f(un(s, sa), un(d, da));
        // W3C compositing: result = (1-da)*s + (1-sa)*d + sa*da*B
        (1.0 - da) * s + (1.0 - sa) * d + sa * da * blended
    };
    LinearRgba {
        r: mix(src.r, dst.r),
        g: mix(src.g, dst.g),
        b: mix(src.b, dst.b),
        a: src.a + dst.a * (1.0 - src.a),
    }
}

fn composite(dst: &mut Surface, src: &Surface, opacity: f32, mode: BlendMode, clip: ClipRect) {
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let i = (y * dst.width + x) as usize;
            dst.pixels[i] = blend_pixel(scale_alpha(src.pixels[i], opacity), dst.pixels[i], mode);
        }
    }
}

/// Map a doc-space point into a node's local space (inverse of its
/// scale+translate transform). Degenerate scales map nowhere.
fn to_local(t: Transform, x: f32, y: f32) -> Option<(f32, f32)> {
    if t.a.abs() < 1e-6 || t.d.abs() < 1e-6 {
        return None;
    }
    Some(((x - t.e) / t.a, (y - t.f) / t.d))
}

/// Expand a curved path into the polyline everything else works on — paint,
/// hit test, bounds all run on the result, so curves need no special cases
/// downstream. Bezier handles win when present because they are authored;
/// `smooth` infers a Catmull-Rom spline through the anchors instead.
/// Everything else (and paths already polyline) borrows unchanged. Call once
/// per operation, not per pixel.
fn flatten_shape(shape: &VectorShape) -> std::borrow::Cow<'_, VectorShape> {
    use std::borrow::Cow;
    const STEPS: usize = 12;
    let VectorShape::Path {
        points,
        closed,
        smooth,
        handles,
    } = shape
    else {
        return Cow::Borrowed(shape);
    };
    let n = points.len();
    let curved = handles.len() == n && handles.iter().any(|h| h.iter().any(|v| v.abs() > 1e-6));
    if curved && n >= 2 {
        let segments = if *closed { n } else { n - 1 };
        let mut out = Vec::with_capacity(segments * STEPS + 1);
        for i in 0..segments {
            let j = (i + 1) % n;
            let (a, b) = (points[i], points[j]);
            // Control points are offsets from their anchors: the outgoing
            // handle of this anchor and the incoming one of the next.
            let c1 = [a[0] + handles[i][2], a[1] + handles[i][3]];
            let c2 = [b[0] + handles[j][0], b[1] + handles[j][1]];
            for s in 0..STEPS {
                let t = s as f32 / STEPS as f32;
                let u = 1.0 - t;
                let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                out.push([
                    w0 * a[0] + w1 * c1[0] + w2 * c2[0] + w3 * b[0],
                    w0 * a[1] + w1 * c1[1] + w2 * c2[1] + w3 * b[1],
                ]);
            }
        }
        if !closed {
            out.push(points[n - 1]);
        }
        return Cow::Owned(VectorShape::Path {
            points: out,
            closed: *closed,
            smooth: false,
            handles: Vec::new(),
        });
    }
    if !smooth || n < 3 {
        return Cow::Borrowed(shape);
    }
    let get = |i: isize| -> [f32; 2] {
        if *closed {
            points[i.rem_euclid(n as isize) as usize]
        } else {
            points[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let segments = if *closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(segments * STEPS + 1);
    for i in 0..segments {
        let (p0, p1, p2, p3) = (
            get(i as isize - 1),
            get(i as isize),
            get(i as isize + 1),
            get(i as isize + 2),
        );
        for s in 0..STEPS {
            let t = s as f32 / STEPS as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    if !closed {
        out.push(points[n - 1]);
    }
    Cow::Owned(VectorShape::Path {
        points: out,
        closed: *closed,
        smooth: false,
        handles: Vec::new(),
    })
}

/// Local-space coverage test for a shape (shape origin at 0,0). Paths fill
/// by the even-odd rule over their anchor polygon (open paths close
/// implicitly, the SVG convention).
fn shape_covers(shape: &VectorShape, x: f32, y: f32) -> bool {
    match shape {
        VectorShape::Rect { width, height } => x >= 0.0 && y >= 0.0 && x < *width && y < *height,
        VectorShape::Ellipse { rx, ry } => {
            let (nx, ny) = ((x - rx) / rx, (y - ry) / ry);
            nx * nx + ny * ny <= 1.0
        }
        VectorShape::Path { points, .. } => {
            if points.len() < 3 {
                return false;
            }
            let mut inside = false;
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                if (a[1] > y) != (b[1] > y) {
                    let t = (y - a[1]) / (b[1] - a[1]);
                    if x < a[0] + t * (b[0] - a[0]) {
                        inside = !inside;
                    }
                }
            }
            inside
        }
    }
}

/// Distance from a point to a line segment.
fn segment_distance(px: f32, py: f32, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 1e-12 {
        0.0
    } else {
        (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a[0] + t * dx, a[1] + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Stroke coverage. Rects and ellipses use an inner band of the given width
/// (bounds stay stable); paths use a stroke centered on the line so open
/// paths render as line art.
fn stroke_covers(shape: &VectorShape, width: f32, x: f32, y: f32) -> bool {
    if let VectorShape::Path { points, closed, .. } = shape {
        if points.len() < 2 {
            return false;
        }
        let segments = if *closed {
            points.len()
        } else {
            points.len() - 1
        };
        let half = width / 2.0;
        return (0..segments)
            .any(|i| segment_distance(x, y, points[i], points[(i + 1) % points.len()]) <= half);
    }
    if !shape_covers(shape, x, y) {
        return false;
    }
    match shape {
        VectorShape::Rect {
            width: w,
            height: h,
        } => x < width || y < width || x >= w - width || y >= h - width,
        VectorShape::Ellipse { rx, ry } => {
            let (irx, iry) = ((rx - width).max(0.0), (ry - width).max(0.0));
            if irx <= 0.0 || iry <= 0.0 {
                return true;
            }
            let (nx, ny) = ((x - rx) / irx, (y - ry) / iry);
            nx * nx + ny * ny > 1.0
        }
        VectorShape::Path { .. } => unreachable!(),
    }
}

/// What a shape is painted with. Gradient stops are resolved to
/// premultiplied linear (and pre-scaled by the node's opacity) once per
/// paint, so the per-pixel cost is a ramp lookup and a lerp.
///
/// Interpolation therefore happens in linear light, like every other blend
/// in the engine: the midpoint of a black-to-white ramp is linear 0.5, which
/// encodes to sRGB ~188, not 128.
enum Paint {
    Solid(LinearRgba),
    Gradient {
        kind: GradientGeom,
        /// Sorted by offset.
        stops: Vec<(f32, LinearRgba)>,
        /// Local-space box the normalized gradient coordinates map onto.
        box_: (f32, f32, f32, f32),
    },
}

enum GradientGeom {
    Linear { from: [f32; 2], to: [f32; 2] },
    Radial { center: [f32; 2], radius: f32 },
}

impl Paint {
    /// Resolve an authored gradient against the shape it fills. Returns
    /// `None` when there is nothing to paint (no stops).
    fn from_gradient(
        doc: &Document,
        g: &Gradient,
        shape: &VectorShape,
        opacity: f32,
    ) -> Option<Paint> {
        let mut stops: Vec<(f32, LinearRgba)> = g
            .stops()
            .iter()
            .map(|s| (s.offset, scale_alpha(resolve_color(doc, s.color), opacity)))
            .collect();
        if stops.is_empty() {
            return None;
        }
        stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let kind = match g {
            Gradient::Linear { from, to, .. } => GradientGeom::Linear {
                from: *from,
                to: *to,
            },
            Gradient::Radial { center, radius, .. } => GradientGeom::Radial {
                center: *center,
                radius: *radius,
            },
        };
        Some(Paint::Gradient {
            kind,
            stops,
            box_: local_bounds(shape),
        })
    }

    /// Colour at a point in the shape's local space.
    fn at(&self, lx: f32, ly: f32) -> LinearRgba {
        let (kind, stops, (x0, y0, x1, y1)) = match self {
            Paint::Solid(c) => return *c,
            Paint::Gradient { kind, stops, box_ } => (kind, stops, *box_),
        };
        // Normalized box coordinates, SVG's objectBoundingBox units: a
        // radial gradient is therefore an ellipse in a non-square shape,
        // which is what makes it follow the shape.
        let norm = |v: f32, lo: f32, hi: f32| {
            if (hi - lo).abs() < 1e-6 {
                0.0
            } else {
                (v - lo) / (hi - lo)
            }
        };
        let (u, v) = (norm(lx, x0, x1), norm(ly, y0, y1));
        let t = match kind {
            GradientGeom::Linear { from, to } => {
                let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
                let len2 = dx * dx + dy * dy;
                if len2 < 1e-12 {
                    0.0
                } else {
                    ((u - from[0]) * dx + (v - from[1]) * dy) / len2
                }
            }
            GradientGeom::Radial { center, radius } => {
                if *radius < 1e-6 {
                    1.0
                } else {
                    ((u - center[0]).powi(2) + (v - center[1]).powi(2)).sqrt() / radius
                }
            }
        }
        .clamp(0.0, 1.0);
        ramp(stops, t)
    }
}

/// Colour at `t` along a sorted ramp, clamped past either end.
fn ramp(stops: &[(f32, LinearRgba)], t: f32) -> LinearRgba {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for w in stops.windows(2) {
        if t <= w[1].0 {
            let span = w[1].0 - w[0].0;
            let k = if span.abs() < 1e-6 {
                0.0
            } else {
                (t - w[0].0) / span
            };
            return lerp(w[0].1, w[1].1, k);
        }
    }
    last.1
}

/// Exact coverage of an axis-aligned local-space rect over one device pixel:
/// the area the pixel square and the mapped rect share. Transforms carry
/// scale and translation only, so the mapped rect stays axis-aligned and the
/// answer is a product of two 1-D overlaps.
fn rect_coverage(width: f32, height: f32, t: Transform, px: u32, py: u32) -> f32 {
    if t.a.abs() < 1e-6 || t.d.abs() < 1e-6 {
        return 0.0;
    }
    let span = |lo: f32, hi: f32, at: u32| {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        (hi.min(at as f32 + 1.0) - lo.max(at as f32)).clamp(0.0, 1.0)
    };
    span(t.e, t.e + width * t.a, px) * span(t.f, t.f + height * t.d, py)
}

/// Anti-aliased coverage of a shape over one device pixel, in 0..=1.
///
/// The corners plus the centre are tested first: when all five agree the
/// pixel lies wholly inside or outside, which is true of every pixel but the
/// boundary ones, so the common case costs five point tests. Only where they
/// disagree does the pixel pay for an NxN box of samples.
///
/// Detail finer than that first five-point probe can still slip through and
/// drop out — the same blind spot the old centre-only test had, now confined
/// to sub-pixel features.
fn pixel_coverage(
    shape: &VectorShape,
    stroke_width: Option<f32>,
    t: Transform,
    px: u32,
    py: u32,
) -> f32 {
    const N: u32 = 4;
    if t.a.abs() < 1e-6 || t.d.abs() < 1e-6 {
        return 0.0;
    }
    // An axis-aligned rect fill has an exact answer, so take it. Rect fills
    // cover the largest areas, and this is both cheaper than sampling and
    // not an approximation of it.
    if let (VectorShape::Rect { width, height }, None) = (shape, stroke_width) {
        return rect_coverage(*width, *height, t, px, py);
    }
    let covers = |sx: f32, sy: f32| match to_local(t, sx, sy) {
        Some((x, y)) => match stroke_width {
            None => shape_covers(shape, x, y),
            Some(sw) => stroke_covers(shape, sw, x, y),
        },
        None => false,
    };
    let (fx, fy) = (px as f32, py as f32);
    let inside = covers(fx + 0.5, fy + 0.5);
    let uniform = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .into_iter()
        .all(|(dx, dy)| covers(fx + dx, fy + dy) == inside);
    if uniform {
        return if inside { 1.0 } else { 0.0 };
    }
    let hits = (0..N * N)
        .filter(|k| {
            let (i, j) = (k % N, k / N);
            covers(
                fx + (i as f32 + 0.5) / N as f32,
                fy + (j as f32 + 0.5) / N as f32,
            )
        })
        .count();
    hits as f32 / (N * N) as f32
}

/// Fill a polygon row by row instead of sampling it per pixel.
///
/// The crossings of the polygon with a scanline are O(anchors) to compute
/// and serve the whole row, so the fill costs O(rows x N x anchors) rather
/// than the O(pixels x N^2 x anchors) the general sampler spends here — the
/// difference between milliseconds and half a second on a canvas-filling
/// spline. Coverage is exact horizontally (a span contributes the fraction
/// of the pixel it really covers) and N-sampled vertically, so it is also
/// better than the box of samples it replaces.
///
/// Even-odd fill, matching [`shape_covers`]: sorted crossings pair up into
/// inside spans.
#[allow(clippy::too_many_arguments)]
fn fill_path_scanlines(
    dst: &mut Surface,
    doc: &Document,
    points: &[[f32; 2]],
    t: Transform,
    paint: &Paint,
    mode: BlendMode,
    bbox: ClipRect,
    mask: Option<&Mask>,
) {
    const N: u32 = 4;
    if points.len() < 3 || bbox.is_empty() || t.a.abs() < 1e-6 || t.d.abs() < 1e-6 {
        return;
    }
    let width = (bbox.x1 - bbox.x0) as usize;
    let mut cov = vec![0f32; width];
    let mut xs: Vec<f32> = Vec::new();
    for py in bbox.y0..bbox.y1 {
        cov.fill(0.0);
        for j in 0..N {
            // Sample y at the middle of each sub-row, never on its edge.
            let ly = ((py as f32 + (j as f32 + 0.5) / N as f32) - t.f) / t.d;
            xs.clear();
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if (a[1] > ly) != (b[1] > ly) {
                    let s = (ly - a[1]) / (b[1] - a[1]);
                    xs.push((a[0] + s * (b[0] - a[0])) * t.a + t.e);
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for span in xs.as_chunks::<2>().0 {
                let lo = span[0].max(bbox.x0 as f32);
                let hi = span[1].min(bbox.x1 as f32);
                if hi <= lo {
                    continue;
                }
                let first = (lo.floor().max(0.0) as u32).max(bbox.x0);
                let last = (hi.ceil().max(0.0) as u32).min(bbox.x1);
                for px in first..last {
                    let overlap = (hi.min(px as f32 + 1.0) - lo.max(px as f32)).clamp(0.0, 1.0);
                    cov[(px - bbox.x0) as usize] += overlap / N as f32;
                }
            }
        }
        for px in bbox.x0..bbox.x1 {
            let a = cov[(px - bbox.x0) as usize].min(1.0);
            if a <= 0.0 {
                continue;
            }
            let c = a * coverage_at(doc, mask, px, py);
            if c <= 0.0 {
                continue;
            }
            let (lx, ly) = to_local(t, px as f32 + 0.5, py as f32 + 0.5).unwrap_or((0.0, 0.0));
            let i = (py * dst.width + px) as usize;
            dst.pixels[i] = blend_pixel(scale_alpha(paint.at(lx, ly), c), dst.pixels[i], mode);
        }
    }
}

fn draw_bbox(t: Transform, w: f32, h: f32, dst: &Surface, clip: ClipRect) -> ClipRect {
    match transformed_bounds(t, w, h).to_clip(dst.width, dst.height) {
        Some(b) => b.intersect(clip),
        None => ClipRect {
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
        },
    }
}

/// Paint a shape's fill (stroke_width None) or its inner stroke band.
#[allow(clippy::too_many_arguments)]
fn paint_shape(
    dst: &mut Surface,
    doc: &Document,
    shape: &VectorShape,
    t: Transform,
    paint: &Paint,
    mode: BlendMode,
    clip: ClipRect,
    stroke_width: Option<f32>,
    mask: Option<&Mask>,
) {
    // Smooth paths render as their flattened spline polyline.
    let flat = flatten_shape(shape);
    let shape = flat.as_ref();
    let mut bbox =
        match transformed_local_bounds(t, local_bounds(shape)).to_clip(dst.width, dst.height) {
            Some(b) => b.intersect(clip),
            None => return,
        };
    // Centered path strokes overhang the anchor bounds.
    if let (Some(sw), VectorShape::Path { .. }) = (stroke_width, shape) {
        let pad = (sw * t.a.abs().max(t.d.abs())).ceil() as u32 + 1;
        bbox = ClipRect {
            x0: bbox.x0.saturating_sub(pad),
            y0: bbox.y0.saturating_sub(pad),
            x1: (bbox.x1 + pad).min(dst.width),
            y1: (bbox.y1 + pad).min(dst.height),
        }
        .intersect(clip);
    }
    // A degenerate transform maps nothing; bail before walking the bbox.
    if to_local(t, 0.0, 0.0).is_none() {
        return;
    }
    // Path fills go through the scanline rasterizer; strokes stay on the
    // sampler, whose distance test has no scanline form.
    if let (VectorShape::Path { points, .. }, None) = (shape, stroke_width) {
        fill_path_scanlines(dst, doc, points, t, paint, mode, bbox, mask);
        return;
    }
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            let a = pixel_coverage(shape, stroke_width, t, px, py);
            if a <= 0.0 {
                continue;
            }
            let c = a * coverage_at(doc, mask, px, py);
            if c <= 0.0 {
                continue;
            }
            let (lx, ly) = to_local(t, px as f32 + 0.5, py as f32 + 0.5).unwrap_or((0.0, 0.0));
            let i = (py * dst.width + px) as usize;
            dst.pixels[i] = blend_pixel(scale_alpha(paint.at(lx, ly), c), dst.pixels[i], mode);
        }
    }
}

/// Blit a source image through its transform, sampled bilinearly in
/// premultiplied linear space (so edges against transparency don't halo)
/// and clamped at the image border. An identity blit lands exactly on texel
/// centres, so a 1:1 image stays pixel-exact rather than being softened.
#[allow(clippy::too_many_arguments)]
fn draw_raster(
    dst: &mut Surface,
    doc: &Document,
    res: &chitrakar_doc::Resource,
    t: Transform,
    opacity: f32,
    mode: BlendMode,
    clip: ClipRect,
    mask: Option<&Mask>,
) {
    // 8-bit sRGB → linear lookup table, built per blit (256 entries, cheap).
    let mut lut = [0f32; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        *out = chitrakar_color::srgb_to_linear(v as f32 / 255.0);
    }
    let bbox = draw_bbox(t, res.width as f32, res.height as f32, dst, clip);
    // One texel, premultiplied so interpolation never mixes colour across a
    // transparent neighbour.
    let texel = |x: u32, y: u32| -> LinearRgba {
        let s = ((y * res.width + x) * 4) as usize;
        let a = res.rgba8[s + 3] as f32 / 255.0;
        LinearRgba {
            r: lut[res.rgba8[s] as usize] * a,
            g: lut[res.rgba8[s + 1] as usize] * a,
            b: lut[res.rgba8[s + 2] as usize] * a,
            a,
        }
    };
    let (last_x, last_y) = (res.width as f32 - 1.0, res.height as f32 - 1.0);
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            // The image is a rect in local space, so its outline gets the
            // same exact coverage a rect fill does.
            let edge = rect_coverage(res.width as f32, res.height as f32, t, px, py);
            if edge <= 0.0 {
                continue;
            }
            let cov = coverage_at(doc, mask, px, py) * edge * opacity;
            if cov <= 0.0 {
                continue;
            }
            let Some((lx, ly)) = to_local(t, px as f32 + 0.5, py as f32 + 0.5) else {
                return;
            };
            // Texel centres sit at (i + 0.5), so the sample lands between
            // the four texels around (lx - 0.5, ly - 0.5).
            let (u, v) = (lx - 0.5, ly - 0.5);
            let (u0, v0) = (u.floor(), v.floor());
            let (fx, fy) = (u - u0, v - v0);
            let cx = |i: f32| i.clamp(0.0, last_x) as u32;
            let cy = |i: f32| i.clamp(0.0, last_y) as u32;
            let (x0, x1) = (cx(u0), cx(u0 + 1.0));
            let (y0, y1) = (cy(v0), cy(v0 + 1.0));
            let top = lerp(texel(x0, y0), texel(x1, y0), fx);
            let bottom = lerp(texel(x0, y1), texel(x1, y1), fx);
            let src = scale_alpha(lerp(top, bottom, fy), cov);
            let i = (py * dst.width + px) as usize;
            dst.pixels[i] = blend_pixel(src, dst.pixels[i], mode);
        }
    }
}

/// Mask coverage at a pixel center, in document space. 1.0 without a mask.
fn coverage_at(doc: &Document, mask: Option<&Mask>, x: u32, y: u32) -> f32 {
    let Some(mask) = mask else {
        return 1.0;
    };
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let c = match &mask.kind {
        MaskKind::Vector { shape, transform } => pixel_coverage(shape, None, *transform, x, y),
        MaskKind::Raster {
            resource_id,
            transform,
            ..
        } => match (doc.resource(resource_id), to_local(*transform, fx, fy)) {
            (Some(res), Some((lx, ly)))
                if !res.rgba8.is_empty()
                    && lx >= 0.0
                    && ly >= 0.0
                    && (lx as u32) < res.width
                    && (ly as u32) < res.height =>
            {
                let i = ((ly as u32 * res.width + lx as u32) * 4) as usize;
                let lum = |v: u8| chitrakar_color::srgb_to_linear(v as f32 / 255.0);
                let luma = 0.2126 * lum(res.rgba8[i])
                    + 0.7152 * lum(res.rgba8[i + 1])
                    + 0.0722 * lum(res.rgba8[i + 2]);
                luma * (res.rgba8[i + 3] as f32 / 255.0)
            }
            _ => 0.0,
        },
    };
    if mask.invert {
        1.0 - c
    } else {
        c
    }
}

/// Multiply a surface region by a mask's coverage (used for group masks).
fn apply_mask(doc: &Document, mask: &Mask, surface: &mut Surface, clip: ClipRect) {
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let c = coverage_at(doc, Some(mask), x, y);
            if c < 1.0 {
                let i = (y * surface.width + x) as usize;
                surface.pixels[i] = scale_alpha(surface.pixels[i], c);
            }
        }
    }
}

/// Run a filter layer over the accumulated composite below it, weighted by
/// the layer's opacity and mask coverage.
fn apply_filter(
    doc: &Document,
    filter: &Filter,
    opacity: f32,
    mask: Option<&Mask>,
    dst: &mut Surface,
    clip: ClipRect,
) {
    match filter {
        Filter::GaussianBlur { sigma } => {
            let needs_mix = opacity < 1.0 || mask.is_some();
            let original = needs_mix.then(|| blur::snapshot(dst, clip));
            blur::gaussian_blur(dst, clip, *sigma);
            if let Some(orig) = original {
                mix_snapshot(dst, clip, &orig, |o, f, x, y| {
                    lerp(o, f, opacity * coverage_at(doc, mask, x, y))
                });
            }
        }
        Filter::Sharpen { sigma, amount } => {
            let original = blur::snapshot(dst, clip);
            blur::gaussian_blur(dst, clip, *sigma);
            mix_snapshot(dst, clip, &original, |o, blurred, x, y| {
                let amt = amount * opacity * coverage_at(doc, mask, x, y);
                // Unsharp mask; keep alpha, clamp premultiplied channels to it.
                let un = |ov: f32, bv: f32, a: f32| (ov + amt * (ov - bv)).clamp(0.0, a.max(0.0));
                LinearRgba {
                    r: un(o.r, blurred.r, o.a),
                    g: un(o.g, blurred.g, o.a),
                    b: un(o.b, blurred.b, o.a),
                    a: o.a,
                }
            });
        }
    }
}

/// Combine each region pixel (currently holding the filtered result) with
/// its snapshot original.
fn mix_snapshot(
    dst: &mut Surface,
    clip: ClipRect,
    original: &[LinearRgba],
    f: impl Fn(LinearRgba, LinearRgba, u32, u32) -> LinearRgba,
) {
    let w = (clip.x1 - clip.x0) as usize;
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let i = (y * dst.width + x) as usize;
            let s = (y - clip.y0) as usize * w + (x - clip.x0) as usize;
            dst.pixels[i] = f(original[s], dst.pixels[i], x, y);
        }
    }
}

/// Rasterize a text block at natural size and blit its coverage through the
/// node transform (nearest sample, like rasters).
#[allow(clippy::too_many_arguments)]
fn draw_text(
    dst: &mut Surface,
    doc: &Document,
    spec: &chitrakar_doc::TextSpec,
    t: Transform,
    opacity: f32,
    mode: BlendMode,
    clip: ClipRect,
    mask: Option<&Mask>,
) {
    let raster = text::rasterize(spec);
    let color = resolve_color(doc, spec.fill);
    let bbox =
        match transformed_local_bounds(t, (0.0, 0.0, raster.width as f32, raster.height as f32))
            .to_clip(dst.width, dst.height)
        {
            Some(b) => b.intersect(clip),
            None => return,
        };
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            let Some((lx, ly)) = to_local(t, px as f32 + 0.5, py as f32 + 0.5) else {
                return;
            };
            if lx < 0.0 || ly < 0.0 {
                continue;
            }
            let c = raster.sample(lx as u32, ly as u32);
            if c <= 0.0 {
                continue;
            }
            let cov = coverage_at(doc, mask, px, py);
            if cov <= 0.0 {
                continue;
            }
            let i = (py * dst.width + px) as usize;
            dst.pixels[i] = blend_pixel(scale_alpha(color, c * opacity * cov), dst.pixels[i], mode);
        }
    }
}

fn apply_adjustment(adj: &Adjustment, px: LinearRgba) -> LinearRgba {
    if px.a <= 0.0 {
        return px;
    }
    // Work in straight alpha.
    let (r, g, b) = (px.r / px.a, px.g / px.a, px.b / px.a);
    let (r, g, b) = match adj {
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
        } => {
            let f = |v: f32| ((v + brightness - 0.5) * (1.0 + contrast) + 0.5).clamp(0.0, 1.0);
            (f(r), f(g), f(b))
        }
        Adjustment::Exposure { stops } => {
            let m = 2f32.powf(*stops);
            (r * m, g * m, b * m)
        }
        Adjustment::HueSaturation {
            hue_degrees,
            saturation,
            lightness,
        } => {
            // Hue rotation (feColorMatrix hueRotate, Rec.709-ish weights).
            let (sin, cos) = hue_degrees.to_radians().sin_cos();
            let m = [
                [
                    0.213 + cos * 0.787 - sin * 0.213,
                    0.715 - cos * 0.715 - sin * 0.715,
                    0.072 - cos * 0.072 + sin * 0.928,
                ],
                [
                    0.213 - cos * 0.213 + sin * 0.143,
                    0.715 + cos * 0.285 + sin * 0.140,
                    0.072 - cos * 0.072 - sin * 0.283,
                ],
                [
                    0.213 - cos * 0.213 - sin * 0.787,
                    0.715 - cos * 0.715 + sin * 0.715,
                    0.072 + cos * 0.928 + sin * 0.072,
                ],
            ];
            let (r, g, b) = (
                m[0][0] * r + m[0][1] * g + m[0][2] * b,
                m[1][0] * r + m[1][1] * g + m[1][2] * b,
                m[2][0] * r + m[2][1] * g + m[2][2] * b,
            );
            // Saturation: scale distance from luminance; then lightness add.
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let s = 1.0 + saturation;
            let f = |v: f32| (lum + (v - lum) * s + lightness).clamp(0.0, 1.0);
            (f(r), f(g), f(b))
        }
    };
    LinearRgba {
        r: r * px.a,
        g: g * px.a,
        b: b * px.a,
        a: px.a,
    }
}

/// Topmost visible node whose filled shape or image bounds cover the
/// document-space point — the click target. Groups are traversed top-down;
/// adjustment layers are not hit-testable.
pub fn hit_test(doc: &Document, x: f32, y: f32) -> Result<Option<NodeId>, DocError> {
    hit_in_group(doc, doc.root(), x, y)
}

fn hit_in_group(doc: &Document, group: NodeId, x: f32, y: f32) -> Result<Option<NodeId>, DocError> {
    for &child in doc.children_of(group)?.iter().rev() {
        let node = doc.node(child)?;
        if !node.visible {
            continue;
        }
        match &node.kind {
            NodeKind::Group => {
                if let Some(hit) = hit_in_group(doc, child, x, y)? {
                    return Ok(Some(hit));
                }
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                gradient,
            } => {
                if let Some((lx, ly)) = to_local(node.transform, x, y) {
                    let flat = flatten_shape(shape);
                    let shape = flat.as_ref();
                    let hit = if fill.is_some() || gradient.is_some() {
                        shape_covers(shape, lx, ly)
                    } else if let Some(s) = stroke {
                        stroke_covers(shape, s.width, lx, ly)
                    } else {
                        false
                    };
                    if hit {
                        return Ok(Some(child));
                    }
                }
            }
            NodeKind::Raster(raster) => {
                if let Some((lx, ly)) = to_local(node.transform, x, y) {
                    if lx >= 0.0
                        && ly >= 0.0
                        && lx < raster.width as f32
                        && ly < raster.height as f32
                    {
                        return Ok(Some(child));
                    }
                }
            }
            NodeKind::Text(spec) => {
                if let Some((lx, ly)) = to_local(node.transform, x, y) {
                    let (w, h) = text::measure(spec);
                    if lx >= 0.0 && ly >= 0.0 && lx < w && ly < h {
                        return Ok(Some(child));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::{AuthoredColor, ColorMode};
    use chitrakar_doc::{Command, Node};

    fn filled_rect(name: &str, w: f32, h: f32, color: AuthoredColor) -> Box<Node> {
        let mut node = Node::vector(
            name,
            VectorShape::Rect {
                width: w,
                height: h,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(color);
        }
        Box::new(node)
    }

    const RED: AuthoredColor = AuthoredColor::Srgb {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    #[test]
    fn empty_document_renders_transparent() {
        let doc = Document::new(4, 4, ColorMode::Rgb);
        let s = render(&doc).unwrap();
        assert_eq!(s.get(0, 0), LinearRgba::TRANSPARENT);
    }

    #[test]
    fn rect_covers_its_bounds_only() {
        let mut doc = Document::new(8, 8, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 4.0, 4.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(2.0, 2.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(0, 0).a, 0.0);
        assert_eq!(s.get(3, 3).to_srgb8(), [255, 0, 0, 255]);
        assert_eq!(s.get(6, 6).a, 0.0);
    }

    /// Not an assertion: a guard against the rasterizer getting slow again.
    /// Both docs should land in single-digit milliseconds — the path-heavy
    /// one (a canvas-filling 480-point spline) cost 435 ms while path fills
    /// still went through the per-pixel sampler. Run with
    /// `--release --ignored --nocapture`.
    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn render_timing_probe() {
        let mut doc = Document::new(512, 512, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("bg", 512.0, 512.0, RED),
        })
        .unwrap();
        let mut e = Node::vector(
            "e",
            VectorShape::Ellipse {
                rx: 200.0,
                ry: 200.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut e.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(e),
        })
        .unwrap();
        let pts: Vec<[f32; 2]> = (0..40)
            .map(|i| {
                let a = i as f32 / 40.0 * std::f32::consts::TAU;
                [200.0 + 180.0 * a.cos(), 200.0 + 180.0 * a.sin()]
            })
            .collect();
        let mut p = Node::vector(
            "p",
            VectorShape::Path {
                points: pts,
                closed: true,
                smooth: true,
                handles: Vec::new(),
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut p.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 2,
            node: Box::new(p),
        })
        .unwrap();
        let t0 = std::time::Instant::now();
        for _ in 0..10 {
            render(&doc).unwrap();
        }
        println!(
            "TIMING path-heavy: {:?} per 512x512 frame",
            t0.elapsed() / 10
        );

        // Same doc without the 480-point flattened spline.
        let ids = doc.children_of(root).unwrap();
        doc.apply(Command::RemoveNode { id: ids[2] }).unwrap();
        let t1 = std::time::Instant::now();
        for _ in 0..10 {
            render(&doc).unwrap();
        }
        println!(
            "TIMING rect+ellipse: {:?} per 512x512 frame",
            t1.elapsed() / 10
        );
    }

    #[test]
    fn half_pixel_offset_rect_antialiases_its_edge() {
        // A rect landing on a half-pixel covers half of the boundary column,
        // so that column must come out half opaque — the whole point of
        // coverage sampling. On integer bounds the same rect stays hard.
        let mut doc = Document::new(8, 8, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 4.0, 4.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(2.5, 2.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert!(
            (s.get(2, 3).a - 0.5).abs() < 0.08,
            "left edge column should be ~half covered, got {}",
            s.get(2, 3).a
        );
        assert_eq!(s.get(3, 3).a, 1.0, "interior stays fully opaque");
        assert!(
            (s.get(6, 3).a - 0.5).abs() < 0.08,
            "right edge column should be ~half covered, got {}",
            s.get(6, 3).a
        );
        assert_eq!(s.get(7, 3).a, 0.0, "past the shape is still empty");

        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(2.0, 2.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(2, 3).a, 1.0, "integer bounds stay hard-edged");
        assert_eq!(s.get(6, 3).a, 0.0);
    }

    #[test]
    fn path_fill_coverage_integrates_to_the_true_area() {
        // Half a 32x32 square, cut corner to corner. Summing alpha over the
        // canvas recovers the triangle's real area — the check a box of
        // samples could only approximate, and the reason the scanline fill
        // is exact horizontally.
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        let mut tri = Node::vector(
            "tri",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [32.0, 0.0], [0.0, 32.0]],
                closed: true,
                smooth: false,
                handles: Vec::new(),
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut tri.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(tri),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        let area: f32 = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .map(|(x, y)| s.get(x, y).a)
            .sum();
        let expected = 32.0 * 32.0 / 2.0;
        assert!(
            (area - expected).abs() < expected * 0.01,
            "covered area {area} should be within 1% of {expected}"
        );
        assert_eq!(s.get(1, 1).a, 1.0, "well inside is solid");
        assert_eq!(s.get(30, 30).a, 0.0, "across the diagonal is empty");
    }

    fn stop(offset: f32, r: f32, g: f32, b: f32) -> chitrakar_doc::GradientStop {
        chitrakar_doc::GradientStop {
            offset,
            color: AuthoredColor::Srgb { r, g, b, a: 1.0 },
        }
    }

    fn gradient_rect(doc: &mut Document, gradient: Gradient) -> NodeId {
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("g", 32.0, 32.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Vector {
                shape: VectorShape::Rect {
                    width: 32.0,
                    height: 32.0,
                },
                fill: Some(RED),
                stroke: None,
                gradient: Some(gradient),
            }),
        })
        .unwrap();
        id
    }

    #[test]
    fn linear_gradient_ramps_across_the_shape() {
        // Black to white left to right, in the shape's own box, so the ramp
        // has to rise monotonically and hit both ends.
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        gradient_rect(
            &mut doc,
            Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 0.0],
                stops: vec![stop(0.0, 0.0, 0.0, 0.0), stop(1.0, 1.0, 1.0, 1.0)],
            },
        );

        // Read linear values: the ramp interpolates in linear light like the
        // rest of the pipeline, so sRGB-encoded numbers are not the midpoints
        // you would expect them to be.
        let s = render(&doc).unwrap();
        let row: Vec<f32> = (0..32).map(|x| s.get(x, 16).r).collect();
        assert!(row[0] < 0.05, "starts at the first stop, got {}", row[0]);
        assert!(row[31] > 0.95, "ends at the last stop, got {}", row[31]);
        assert!(
            row.windows(2).all(|w| w[1] >= w[0]),
            "ramp must be monotonic, got {row:?}"
        );
        assert!(
            (row[16] - 0.5).abs() < 0.05,
            "halfway across is halfway along the ramp, got {}",
            row[16]
        );
        // Constant down a column: the ramp runs along x only.
        assert_eq!(s.get(8, 2).to_srgb8(), s.get(8, 29).to_srgb8());
    }

    #[test]
    fn radial_gradient_ramps_outward_from_its_centre() {
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        gradient_rect(
            &mut doc,
            Gradient::Radial {
                center: [0.5, 0.5],
                radius: 0.5,
                stops: vec![stop(0.0, 1.0, 1.0, 1.0), stop(1.0, 0.0, 0.0, 0.0)],
            },
        );

        let s = render(&doc).unwrap();
        let centre = s.get(16, 16).r;
        let mid = s.get(24, 16).r;
        let edge = s.get(31, 16).r;
        assert!(centre > 0.95, "centre takes the first stop, got {centre}");
        assert!(edge < 0.05, "the rim takes the last stop, got {edge}");
        assert!(
            centre > mid && mid > edge,
            "should fall off outward, got {centre}/{mid}/{edge}"
        );
        // Radially symmetric about the centre. Pixel centres sit at x + 0.5,
        // so the mirror of pixel 24 about the shape's centre (16.0) is 7.
        assert_eq!(s.get(24, 16).to_srgb8(), s.get(7, 16).to_srgb8());
        assert_eq!(s.get(16, 24).to_srgb8(), s.get(16, 7).to_srgb8());
    }

    #[test]
    fn gradient_follows_the_shape_when_it_moves() {
        // Stops live in the shape's own box, so translating the shape
        // carries the ramp with it rather than sliding the shape past it.
        let mut doc = Document::new(64, 32, ColorMode::Rgb);
        let id = gradient_rect(
            &mut doc,
            Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 0.0],
                stops: vec![stop(0.0, 0.0, 0.0, 0.0), stop(1.0, 1.0, 1.0, 1.0)],
            },
        );
        let before = render(&doc).unwrap().get(4, 16).to_srgb8();
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(20.0, 0.0),
        })
        .unwrap();
        let after = render(&doc).unwrap().get(24, 16).to_srgb8();
        assert_eq!(before, after, "same point of the shape, same colour");
    }

    #[test]
    fn gradient_paints_over_the_flat_fill_and_is_hit_testable() {
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let id = gradient_rect(
            &mut doc,
            Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 0.0],
                stops: vec![stop(0.0, 0.0, 1.0, 0.0), stop(1.0, 0.0, 1.0, 0.0)],
            },
        );
        // The node still carries fill: RED underneath; green must win.
        let px = render(&doc).unwrap().get(16, 16).to_srgb8();
        assert_eq!(px, [0, 255, 0, 255], "gradient paints in place of fill");
        assert_eq!(hit_test(&doc, 16.0, 16.0).unwrap(), Some(id));
    }

    #[test]
    fn bezier_handles_bow_the_edge_and_beat_smooth() {
        // Two anchors and a big outward handle: the segment between them has
        // to bulge past the straight chord. And when handles are present
        // they win over `smooth`, because they are authored rather than
        // inferred — same shape either way.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        let straight = VectorShape::Path {
            points: vec![[8.0, 32.0], [56.0, 32.0], [32.0, 56.0]],
            closed: true,
            smooth: false,
            handles: Vec::new(),
        };
        let mut node = Node::vector("p", straight.clone());
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        // A point above the straight top edge, so outside to begin with.
        let probe = (32, 24);
        let s = render(&doc).unwrap();
        assert_eq!(s.get(probe.0, probe.1).a, 0.0, "flat chord leaves it empty");

        // Pull the first segment upward with out/in handles.
        let curved = VectorShape::Path {
            points: vec![[8.0, 32.0], [56.0, 32.0], [32.0, 56.0]],
            closed: true,
            smooth: true, // ignored: explicit handles take precedence
            handles: vec![
                [0.0, 0.0, 0.0, -24.0],
                [0.0, -24.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            ],
        };
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Vector {
                shape: curved.clone(),
                fill: Some(RED),
                stroke: None,
                gradient: None,
            }),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(probe.0, probe.1).a,
            1.0,
            "the curve bulges over the probe"
        );
        assert_eq!(
            hit_test(&doc, probe.0 as f32, probe.1 as f32).unwrap(),
            Some(id),
            "hit testing follows the curve, not the chord"
        );

        // Bounds grow to contain the overshoot, or incremental rendering
        // would leave the bulge stale.
        match node_bounds(&doc, id).unwrap() {
            Bounds::Rect(_, y0, _, _) => {
                assert!(y0 < 24.0, "bounds must contain the overshoot, got {y0}")
            }
            other => panic!("expected a rect, got {other:?}"),
        }

        // Handles all zero is the same as having none.
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Vector {
                shape: VectorShape::Path {
                    points: vec![[8.0, 32.0], [56.0, 32.0], [32.0, 56.0]],
                    closed: true,
                    smooth: false,
                    handles: vec![[0.0; 4]; 3],
                },
                fill: Some(RED),
                stroke: None,
                gradient: None,
            }),
        })
        .unwrap();
        assert_eq!(
            render(&doc).unwrap().get(probe.0, probe.1).a,
            0.0,
            "zero handles leave the path straight"
        );
    }

    #[test]
    fn ellipse_rim_is_partially_covered() {
        // A curved edge cannot land on pixel boundaries, so the rim has to
        // carry intermediate coverage while the inside stays solid.
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::vector("e", VectorShape::Ellipse { rx: 12.0, ry: 12.0 });
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(4.0, 4.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(16, 16).a, 1.0, "centre is solid");
        assert_eq!(s.get(0, 0).a, 0.0, "corner is empty");
        let partial = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let a = s.get(*x, *y).a;
                a > 0.01 && a < 0.99
            })
            .count();
        assert!(
            partial > 20,
            "the rim should be a band of partial coverage, found {partial} pixels"
        );
    }

    #[test]
    fn vector_mask_edges_are_soft() {
        // Masks sample the same coverage function, so a mask edge feathers
        // instead of stair-stepping.
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("bg", 32.0, 32.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(ellipse_mask(16.0, 16.0, 10.0, 10.0, false))),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(16, 16).a, 1.0, "inside the mask is untouched");
        assert_eq!(s.get(0, 0).a, 0.0, "outside the mask is cut away");
        let soft = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let a = s.get(*x, *y).a;
                a > 0.01 && a < 0.99
            })
            .count();
        assert!(soft > 20, "mask rim should feather, found {soft} pixels");
    }

    #[test]
    fn scaled_rect_renders_and_hit_tests_scaled() {
        let mut doc = Document::new(16, 16, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 4.0, 4.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        // 2× scale, translated to (2,2): covers (2,2)-(10,10).
        doc.apply(Command::SetTransform {
            id,
            transform: Transform {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 2.0,
                e: 2.0,
                f: 2.0,
            },
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(9, 9).to_srgb8(), [255, 0, 0, 255]);
        assert_eq!(s.get(11, 11).a, 0.0);
        assert_eq!(hit_test(&doc, 9.5, 9.5).unwrap(), Some(id));
        assert_eq!(hit_test(&doc, 10.5, 10.5).unwrap(), None);
        assert_eq!(
            node_bounds(&doc, id).unwrap(),
            Bounds::Rect(2.0, 2.0, 10.0, 10.0)
        );
    }

    #[test]
    fn region_render_matches_full_render() {
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("a", 20.0, 20.0, RED),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: filled_rect(
                "b",
                10.0,
                10.0,
                AuthoredColor::Srgb {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 0.5,
                },
            ),
        })
        .unwrap();
        // A diagonal path: the scanline fill clips spans to the bbox, which
        // is exactly where a region render could seam against a full one.
        let mut tri = Node::vector(
            "tri",
            VectorShape::Path {
                points: vec![[2.0, 2.0], [26.0, 8.0], [8.0, 27.0]],
                closed: true,
                smooth: false,
                handles: Vec::new(),
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut tri.kind {
            *fill = Some(AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 0.7,
            });
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 2,
            node: Box::new(tri),
        })
        .unwrap();

        let full = render(&doc).unwrap();

        // Start from a surface with stale garbage in the region, re-render
        // just that region, and compare against the full render.
        let mut patched = render(&doc).unwrap();
        let clip = ClipRect {
            x0: 4,
            y0: 4,
            x1: 16,
            y1: 16,
        };
        for y in clip.y0..clip.y1 {
            for x in clip.x0..clip.x1 {
                patched.pixels[(y * 32 + x) as usize] = LinearRgba {
                    r: 9.0,
                    g: 9.0,
                    b: 9.0,
                    a: 1.0,
                };
            }
        }
        render_region(&doc, &mut patched, clip).unwrap();
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(patched.get(x, y), full.get(x, y), "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn hidden_nodes_do_not_render() {
        let mut doc = Document::new(4, 4, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 4.0, 4.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetVisible { id, visible: false })
            .unwrap();
        assert_eq!(render(&doc).unwrap().get(1, 1).a, 0.0);
    }

    #[test]
    fn multiply_blend_darkens() {
        let mut doc = Document::new(2, 2, ColorMode::Rgb);
        let root = doc.root();
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("a", 2.0, 2.0, grey),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: filled_rect("b", 2.0, 2.0, grey),
        })
        .unwrap();
        let top = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetBlendMode {
            id: top,
            blend: BlendMode::Multiply,
        })
        .unwrap();

        let s = render(&doc).unwrap();
        let single = to_working(grey);
        assert!(s.get(0, 0).r < single.r, "multiply must darken");
    }

    #[test]
    fn adjustment_layer_affects_content_below() {
        let mut doc = Document::new(2, 2, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect(
                "r",
                2.0,
                2.0,
                AuthoredColor::Srgb {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 1.0,
                },
            ),
        })
        .unwrap();
        let base = render(&doc).unwrap().get(0, 0);

        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment("exp", Adjustment::Exposure { stops: 1.0 })),
        })
        .unwrap();
        let adjusted = render(&doc).unwrap().get(0, 0);
        assert!(
            (adjusted.r / base.r - 2.0).abs() < 1e-4,
            "+1 stop doubles linear light"
        );

        // Bounds must report Everything once an adjustment is in the tree.
        assert_eq!(node_bounds(&doc, root).unwrap(), Bounds::Everything);
    }

    fn ellipse_mask(cx: f32, cy: f32, rx: f32, ry: f32, invert: bool) -> Mask {
        Mask {
            kind: MaskKind::Vector {
                shape: VectorShape::Ellipse { rx, ry },
                transform: Transform::translation(cx - rx, cy - ry),
            },
            invert,
        }
    }

    #[test]
    fn vector_mask_hides_shape_outside_and_invert_flips_it() {
        let mut doc = Document::new(20, 20, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 20.0, 20.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        // Ellipse mask centered on the canvas: center shows, corners hide.
        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(ellipse_mask(10.0, 10.0, 6.0, 6.0, false))),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(10, 10).to_srgb8(), [255, 0, 0, 255], "center visible");
        assert_eq!(s.get(1, 1).a, 0.0, "corner masked out");

        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(ellipse_mask(10.0, 10.0, 6.0, 6.0, true))),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(10, 10).a, 0.0, "inverted: center hidden");
        assert_eq!(s.get(1, 1).to_srgb8(), [255, 0, 0, 255], "corner visible");
    }

    #[test]
    fn masked_adjustment_applies_only_inside_mask() {
        let mut doc = Document::new(20, 20, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect(
                "bg",
                20.0,
                20.0,
                AuthoredColor::Srgb {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 1.0,
                },
            ),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment("exp", Adjustment::Exposure { stops: 1.0 })),
        })
        .unwrap();
        let adj = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetMask {
            id: adj,
            mask: Some(Box::new(ellipse_mask(10.0, 10.0, 5.0, 5.0, false))),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        let inside = s.get(10, 10);
        let outside = s.get(1, 1);
        assert!(
            (inside.r / outside.r - 2.0).abs() < 1e-3,
            "exposure only where masked in ({} vs {})",
            inside.r,
            outside.r
        );
    }

    #[test]
    fn magnified_raster_interpolates_between_texels() {
        // A black and a white texel blown up 8x. Halfway between their
        // centres the blit must land between them; nearest sampling would
        // return one or the other and stair-step the boundary.
        let mut doc = Document::new(16, 8, ColorMode::Rgb);
        let root = doc.root();
        let rgba8 = vec![0, 0, 0, 255, /**/ 255, 255, 255, 255];
        let id = doc.add_resource(2, 1, rgba8);
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: 2,
                    height: 1,
                },
            )),
        })
        .unwrap();
        let node = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: node,
            transform: Transform {
                a: 8.0,
                d: 8.0,
                ..Default::default()
            },
        })
        .unwrap();

        let s = render(&doc).unwrap();
        // Texel centres map to x = 4 and x = 12; x = 8 is the midpoint.
        let mid = s.get(8, 4).to_srgb8();
        assert!(
            mid[0] > 100 && mid[0] < 200,
            "midpoint should be a blend, got {mid:?}"
        );
        assert_eq!(s.get(0, 4).to_srgb8()[0], 0, "clamped to black at the edge");
        assert_eq!(
            s.get(15, 4).to_srgb8()[0],
            255,
            "clamped to white at the edge"
        );
        // Ramp is monotonic across the interpolated span.
        let ramp: Vec<u8> = (4..=12).map(|x| s.get(x, 4).to_srgb8()[0]).collect();
        assert!(
            ramp.windows(2).all(|w| w[1] >= w[0]),
            "ramp should rise monotonically, got {ramp:?}"
        );
    }

    #[test]
    fn raster_edge_is_antialiased_on_a_half_pixel() {
        // The image outline is a rect in local space, so it gets the same
        // exact coverage a rect fill does instead of a hard jagged border.
        let mut doc = Document::new(8, 8, ColorMode::Rgb);
        let root = doc.root();
        let id = doc.add_resource(2, 2, vec![255; 16]);
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: 2,
                    height: 2,
                },
            )),
        })
        .unwrap();
        let node = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: node,
            transform: Transform::translation(2.5, 2.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert!(
            (s.get(2, 2).a - 0.5).abs() < 0.05,
            "half-covered column, got {}",
            s.get(2, 2).a
        );
        assert_eq!(s.get(3, 2).a, 1.0, "fully covered column stays opaque");
    }

    #[test]
    fn raster_mask_uses_luminance() {
        let mut doc = Document::new(4, 4, ColorMode::Rgb);
        let root = doc.root();
        // 2×2 mask: white / black in the top row → show / hide.
        let mask_px = vec![
            255, 255, 255, 255, /**/ 0, 0, 0, 255, //
            255, 255, 255, 255, /**/ 0, 0, 0, 255,
        ];
        let mask_id = doc.add_resource(2, 2, mask_px);
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 4.0, 4.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(Mask {
                kind: MaskKind::Raster {
                    resource_id: mask_id,
                    width: 2,
                    height: 2,
                    // Scale the 2×2 mask over the 4×4 canvas.
                    transform: Transform {
                        a: 2.0,
                        b: 0.0,
                        c: 0.0,
                        d: 2.0,
                        e: 0.0,
                        f: 0.0,
                    },
                },
                invert: false,
            })),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(0, 0).to_srgb8(), [255, 0, 0, 255], "white shows");
        assert_eq!(s.get(3, 0).a, 0.0, "black hides");
    }

    #[test]
    fn blur_layer_bleeds_past_shape_edges() {
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 8.0, 8.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(12.0, 12.0),
        })
        .unwrap();

        let crisp = render(&doc).unwrap();
        assert_eq!(crisp.get(10, 16).a, 0.0, "outside the rect before blur");

        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::filter(
                "blur",
                chitrakar_doc::Filter::GaussianBlur { sigma: 3.0 },
            )),
        })
        .unwrap();
        let blurred = render(&doc).unwrap();
        assert!(blurred.get(10, 16).a > 0.0, "blur bleeds outward");
        assert!(
            blurred.get(15, 16).r < crisp.get(15, 16).r,
            "edge softened inside too"
        );
        assert_eq!(
            node_bounds(&doc, root).unwrap(),
            Bounds::Everything,
            "filters invalidate the whole canvas"
        );
    }

    #[test]
    fn sharpen_boosts_edge_contrast_without_touching_flat_areas() {
        let mut doc = Document::new(24, 24, ColorMode::Rgb);
        let root = doc.root();
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        // Full-canvas grey with a brighter square in the middle.
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("bg", 24.0, 24.0, grey),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: filled_rect(
                "sq",
                8.0,
                8.0,
                AuthoredColor::Srgb {
                    r: 0.8,
                    g: 0.8,
                    b: 0.8,
                    a: 1.0,
                },
            ),
        })
        .unwrap();
        let sq = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: sq,
            transform: Transform::translation(8.0, 8.0),
        })
        .unwrap();
        let before = render(&doc).unwrap();

        doc.apply(Command::AddNode {
            parent: root,
            index: 2,
            node: Box::new(Node::filter(
                "sharpen",
                chitrakar_doc::Filter::Sharpen {
                    sigma: 1.5,
                    amount: 1.0,
                },
            )),
        })
        .unwrap();
        let after = render(&doc).unwrap();

        // Just inside the bright square's edge gets brighter; the flat
        // far-away background stays put.
        assert!(
            after.get(9, 12).r > before.get(9, 12).r + 1e-3,
            "edge overshoot inside the square"
        );
        assert!(
            (after.get(2, 2).r - before.get(2, 2).r).abs() < 1e-4,
            "flat region unchanged"
        );
    }

    #[test]
    fn raster_object_renders_and_hit_tests() {
        let mut doc = Document::new(8, 8, ColorMode::Rgb);
        let root = doc.root();
        // 2×2 image: opaque red, semi-transparent white bottom-right.
        let rgba8 = vec![
            255, 0, 0, 255, /**/ 255, 0, 0, 255, //
            255, 0, 0, 255, /**/ 255, 255, 255, 128,
        ];
        let id = doc.add_resource(2, 2, rgba8);
        let raster = chitrakar_doc::RasterRef {
            resource_id: id,
            width: 2,
            height: 2,
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster("img", raster)),
        })
        .unwrap();
        let node = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: node,
            transform: Transform::translation(3.0, 3.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(3, 3).to_srgb8(), [255, 0, 0, 255]);
        assert_eq!(s.get(4, 4).to_srgb8()[3], 128, "alpha preserved");
        assert_eq!(s.get(0, 0).a, 0.0, "outside image untouched");

        assert_eq!(hit_test(&doc, 4.5, 4.5).unwrap(), Some(node));
        assert_eq!(hit_test(&doc, 1.0, 1.0).unwrap(), None);
    }

    #[test]
    fn closed_path_fills_by_even_odd_and_hit_tests() {
        let mut doc = Document::new(20, 20, ColorMode::Rgb);
        let root = doc.root();
        // Concave arrowhead: (0,0) (16,8) (0,16) (6,8) — the notch at the
        // left must stay empty under even-odd.
        let mut node = Node::vector(
            "arrow",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [16.0, 8.0], [0.0, 16.0], [6.0, 8.0]],
                closed: true,
                smooth: false,
                handles: Vec::new(),
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(RED);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(2.0, 2.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        // Body of the arrow (doc space): local (10,8) -> doc (12,10).
        assert_eq!(s.get(12, 10).to_srgb8(), [255, 0, 0, 255], "inside filled");
        // The concave notch: local (2,8) -> doc (4,10) is outside the fill.
        assert_eq!(s.get(4, 10).a, 0.0, "concave notch stays empty");
        assert_eq!(s.get(19, 19).a, 0.0, "outside stays empty");

        assert_eq!(hit_test(&doc, 12.0, 10.0).unwrap(), Some(id));
        assert_eq!(hit_test(&doc, 4.0, 10.0).unwrap(), None);
        assert_eq!(
            node_bounds(&doc, id).unwrap(),
            Bounds::Rect(2.0, 2.0, 18.0, 18.0)
        );
    }

    #[test]
    fn text_object_renders_ink_and_hit_tests() {
        let mut doc = Document::new(200, 80, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::text(
                "label",
                chitrakar_doc::TextSpec {
                    text: "Hi".into(),
                    size: 48.0,
                    fill: RED,
                },
            )),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(10.0, 10.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        let ink: u32 = (0..80)
            .flat_map(|y| (0..200).map(move |x| (x, y)))
            .filter(|&(x, y)| s.get(x, y).a > 0.5)
            .count() as u32;
        assert!(ink > 100, "glyphs left substantial ink, got {ink} px");
        // Ink is red (the fill), and confined to the text bounds.
        let Bounds::Rect(bx0, by0, bx1, by1) = node_bounds(&doc, id).unwrap() else {
            panic!("rect bounds");
        };
        assert!(bx0 >= 9.0 && by0 >= 9.0 && bx1 < 200.0 && by1 < 80.0);
        for y in 0..80 {
            for x in 0..200 {
                if s.get(x, y).a > 0.0 {
                    let px = s.get(x, y).to_srgb8();
                    assert!(px[0] >= px[1] && px[0] >= px[2], "ink is red");
                    assert!(
                        (x as f32) >= bx0 - 1.0
                            && (x as f32) <= bx1 + 1.0
                            && (y as f32) >= by0 - 1.0
                            && (y as f32) <= by1 + 1.0,
                        "ink inside bounds"
                    );
                }
            }
        }

        // The block's box is the click target.
        assert_eq!(hit_test(&doc, bx0 + 2.0, by0 + 2.0).unwrap(), Some(id));
        assert_eq!(hit_test(&doc, 190.0, 70.0).unwrap(), None);

        // Editing the text through SetKind grows the bounds (live object).
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Text(chitrakar_doc::TextSpec {
                text: "Hi there".into(),
                size: 48.0,
                fill: RED,
            })),
        })
        .unwrap();
        let Bounds::Rect(_, _, wider, _) = node_bounds(&doc, id).unwrap() else {
            panic!("rect bounds");
        };
        assert!(wider > bx1, "longer text widens bounds");
    }

    #[test]
    fn smooth_path_bulges_past_the_straight_chord() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let root = doc.root();
        // A wide triangle; smoothing the closed path should bow the long
        // bottom edge outward (below the straight chord).
        let make = |smooth| {
            let mut node = Node::vector(
                "tri",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [30.0, 0.0], [15.0, 20.0]],
                    closed: true,
                    smooth,
                    handles: Vec::new(),
                },
            );
            if let NodeKind::Vector { fill, .. } = &mut node.kind {
                *fill = Some(RED);
            }
            Box::new(node)
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: make(false),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(5.0, 5.0),
        })
        .unwrap();
        let sharp = render(&doc).unwrap();

        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Vector {
                shape: VectorShape::Path {
                    points: vec![[0.0, 0.0], [30.0, 0.0], [15.0, 20.0]],
                    closed: true,
                    smooth: true,
                    handles: Vec::new(),
                },
                fill: Some(RED),
                stroke: None,
                gradient: None,
            }),
        })
        .unwrap();
        let smooth = render(&doc).unwrap();

        // A probe between the straight edge (0,0)->(30,0) and outside it:
        // doc (20, 3) is above y=5 line only reachable when the top edge
        // bows upward — pick the left edge midpoint instead: chord from
        // (0,0) to (15,20) passes through local (7.5,10) = doc (12.5,15);
        // just outside it, local (5.5,10) = doc (10.5,15).
        assert_eq!(sharp.get(10, 15).a, 0.0, "outside the straight chord");
        assert!(
            smooth.get(10, 15).a > 0.0,
            "smooth spline bows past the chord"
        );
        // Both agree deep inside.
        assert_eq!(sharp.get(20, 12).to_srgb8(), smooth.get(20, 12).to_srgb8());

        // Bounds account for overshoot beyond the anchor box.
        let Bounds::Rect(_, y0, _, y1) = node_bounds(&doc, id).unwrap() else {
            panic!("rect bounds expected");
        };
        assert!(
            y0 < 5.0 || y1 > 25.0,
            "spline overshoot in bounds ({y0}..{y1})"
        );
    }

    #[test]
    fn open_path_strokes_as_centered_line_art() {
        let mut doc = Document::new(24, 24, ColorMode::Rgb);
        let root = doc.root();
        // A "V" polyline, stroke only.
        let mut node = Node::vector(
            "v",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [8.0, 16.0], [16.0, 0.0]],
                closed: false,
                smooth: false,
                handles: Vec::new(),
            },
        );
        if let NodeKind::Vector { stroke, .. } = &mut node.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: AuthoredColor::Srgb {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
                width: 4.0,
            });
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(4.0, 4.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        // Bottom vertex local (8,16) -> doc (12,20): on the line.
        assert_eq!(s.get(12, 20).to_srgb8(), [0, 255, 0, 255], "vertex stroked");
        // Interior of the V (doc (12,8)) is far from both segments: empty.
        assert_eq!(s.get(12, 8).a, 0.0, "V interior not filled");
        // No implicit closing segment across the top for open paths.
        assert_eq!(s.get(12, 4).a, 0.0, "open path has no closing edge");

        assert_eq!(
            hit_test(&doc, 12.0, 20.0).unwrap(),
            Some(id),
            "stroke clickable"
        );
        assert_eq!(hit_test(&doc, 12.0, 8.0).unwrap(), None);

        // Bounds include the centered stroke's overhang.
        let Bounds::Rect(x0, y0, _, y1) = node_bounds(&doc, id).unwrap() else {
            panic!("expected rect bounds");
        };
        assert!(
            x0 < 4.0 && y0 < 4.0 && y1 > 20.0,
            "stroke overhang in bounds"
        );
    }

    #[test]
    fn stroke_paints_border_band_and_hit_tests() {
        let mut doc = Document::new(12, 12, ColorMode::Rgb);
        let root = doc.root();
        // 10×10 rect with a 2px green inner stroke and no fill.
        let mut node = Node::vector(
            "outline",
            VectorShape::Rect {
                width: 10.0,
                height: 10.0,
            },
        );
        if let NodeKind::Vector { stroke, .. } = &mut node.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: AuthoredColor::Srgb {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
                width: 2.0,
            });
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        let s = render(&doc).unwrap();
        assert_eq!(s.get(0, 0).to_srgb8(), [0, 255, 0, 255], "border painted");
        assert_eq!(s.get(5, 5).a, 0.0, "interior stays unfilled");

        assert_eq!(
            hit_test(&doc, 1.0, 1.0).unwrap(),
            Some(id),
            "stroke is clickable"
        );
        assert_eq!(
            hit_test(&doc, 5.0, 5.0).unwrap(),
            None,
            "hollow center is not"
        );
    }

    #[test]
    fn hue_rotation_180_swaps_red_toward_cyan() {
        let mut doc = Document::new(2, 2, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 2.0, 2.0, RED),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment(
                "hue",
                Adjustment::HueSaturation {
                    hue_degrees: 180.0,
                    saturation: 0.0,
                    lightness: 0.0,
                },
            )),
        })
        .unwrap();
        let px = render(&doc).unwrap().get(0, 0).to_srgb8();
        assert!(
            px[1] > px[0] && px[2] > px[0],
            "180° hue turn makes red cyan-ish, got {px:?}"
        );

        // Saturation -1 must be fully grey (all channels equal).
        let hue = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetKind {
            id: hue,
            kind: Box::new(NodeKind::Adjustment(Adjustment::HueSaturation {
                hue_degrees: 0.0,
                saturation: -1.0,
                lightness: 0.0,
            })),
        })
        .unwrap();
        let px = render(&doc).unwrap().get(0, 0).to_srgb8();
        assert!(
            px[0] == px[1] && px[1] == px[2],
            "desaturated to grey, got {px:?}"
        );
    }

    #[test]
    fn hit_test_finds_topmost_shape() {
        let mut doc = Document::new(16, 16, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("bottom", 10.0, 10.0, RED),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: filled_rect("top", 4.0, 4.0, RED),
        })
        .unwrap();
        let (bottom, top) = {
            let kids = doc.children_of(root).unwrap();
            (kids[0], kids[1])
        };
        doc.apply(Command::SetTransform {
            id: top,
            transform: Transform::translation(6.0, 6.0),
        })
        .unwrap();

        // Overlap region hits the top shape; elsewhere the bottom one.
        assert_eq!(hit_test(&doc, 7.0, 7.0).unwrap(), Some(top));
        assert_eq!(hit_test(&doc, 1.0, 1.0).unwrap(), Some(bottom));
        assert_eq!(hit_test(&doc, 15.0, 15.0).unwrap(), None);

        // Hidden shapes are not clickable.
        doc.apply(Command::SetVisible {
            id: top,
            visible: false,
        })
        .unwrap();
        assert_eq!(hit_test(&doc, 7.0, 7.0).unwrap(), Some(bottom));
    }
}
