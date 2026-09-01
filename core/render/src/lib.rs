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
pub mod tiles;

use chitrakar_color::{to_working, LinearRgba};
use chitrakar_doc::{
    Adjustment, BlendMode, DocError, Document, Filter, NodeId, NodeKind, Transform, VectorShape,
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
        NodeKind::Group | NodeKind::Adjustment(_) | NodeKind::Filter(_) => None,
    }
}

fn shape_size(shape: &VectorShape) -> (f32, f32) {
    match shape {
        VectorShape::Rect { width, height } => (*width, *height),
        VectorShape::Ellipse { rx, ry } => (rx * 2.0, ry * 2.0),
        VectorShape::Path { .. } => (0.0, 0.0),
    }
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
        kind => {
            let (w, h) = local_size(kind).unwrap();
            transformed_bounds(node.transform, w, h)
        }
    })
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
        match &node.kind {
            NodeKind::Group => {
                // Isolate the group on its own surface so group opacity and
                // blend apply to the composite, not per child.
                let mut sub = Surface::new(dst.width, dst.height);
                render_group(doc, child, &mut sub, clip)?;
                composite(dst, &sub, node.opacity, node.blend, clip);
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
            } => {
                if let Some(fill) = fill {
                    let color = scale_alpha(to_working(*fill), node.opacity);
                    paint_shape(dst, shape, node.transform, color, node.blend, clip, None);
                }
                if let Some(stroke) = stroke {
                    let color = scale_alpha(to_working(stroke.color), node.opacity);
                    paint_shape(
                        dst,
                        shape,
                        node.transform,
                        color,
                        node.blend,
                        clip,
                        Some(stroke.width),
                    );
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = doc.resource(&raster.resource_id) {
                    if !res.rgba8.is_empty() {
                        draw_raster(dst, res, node.transform, node.opacity, node.blend, clip);
                    }
                }
            }
            NodeKind::Adjustment(adj) => {
                // An adjustment layer transforms everything composited below
                // it, weighted by its opacity.
                for y in clip.y0..clip.y1 {
                    for x in clip.x0..clip.x1 {
                        let i = (y * dst.width + x) as usize;
                        let adjusted = apply_adjustment(adj, dst.pixels[i]);
                        dst.pixels[i] = lerp(dst.pixels[i], adjusted, node.opacity);
                    }
                }
            }
            NodeKind::Filter(filter) => apply_filter(filter, node.opacity, dst, clip),
        }
    }
    Ok(())
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

/// Local-space coverage test for a shape (shape origin at 0,0).
fn shape_covers(shape: &VectorShape, x: f32, y: f32) -> bool {
    match shape {
        VectorShape::Rect { width, height } => x >= 0.0 && y >= 0.0 && x < *width && y < *height,
        VectorShape::Ellipse { rx, ry } => {
            let (nx, ny) = ((x - rx) / rx, (y - ry) / ry);
            nx * nx + ny * ny <= 1.0
        }
        // Path filling comes with the vector rasterizer proper.
        VectorShape::Path { .. } => false,
    }
}

/// Coverage of a shape's inner stroke band of the given width: inside the
/// shape but not inside the shape shrunk by the stroke width.
fn stroke_covers(shape: &VectorShape, width: f32, x: f32, y: f32) -> bool {
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
        VectorShape::Path { .. } => false,
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
    shape: &VectorShape,
    t: Transform,
    color: LinearRgba,
    mode: BlendMode,
    clip: ClipRect,
    stroke_width: Option<f32>,
) {
    let (w, h) = shape_size(shape);
    let bbox = draw_bbox(t, w, h, dst, clip);
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            // Sample at pixel centers; anti-aliasing arrives with the real
            // rasterizer (vello / analytic coverage).
            let Some((x, y)) = to_local(t, px as f32 + 0.5, py as f32 + 0.5) else {
                return;
            };
            let covered = match stroke_width {
                None => shape_covers(shape, x, y),
                Some(sw) => stroke_covers(shape, sw, x, y),
            };
            if covered {
                let i = (py * dst.width + px) as usize;
                dst.pixels[i] = blend_pixel(color, dst.pixels[i], mode);
            }
        }
    }
}

/// Blit a source image through its transform (nearest sample; filtered
/// sampling arrives with the GPU path).
fn draw_raster(
    dst: &mut Surface,
    res: &chitrakar_doc::Resource,
    t: Transform,
    opacity: f32,
    mode: BlendMode,
    clip: ClipRect,
) {
    // 8-bit sRGB → linear lookup table, built per blit (256 entries, cheap).
    let mut lut = [0f32; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        *out = chitrakar_color::srgb_to_linear(v as f32 / 255.0);
    }
    let bbox = draw_bbox(t, res.width as f32, res.height as f32, dst, clip);
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            let Some((lx, ly)) = to_local(t, px as f32 + 0.5, py as f32 + 0.5) else {
                return;
            };
            if lx < 0.0 || ly < 0.0 {
                continue;
            }
            let (sx, sy) = (lx as u32, ly as u32);
            if sx >= res.width || sy >= res.height {
                continue;
            }
            let s = ((sy * res.width + sx) * 4) as usize;
            let a = res.rgba8[s + 3] as f32 / 255.0 * opacity;
            let src = LinearRgba {
                r: lut[res.rgba8[s] as usize] * a,
                g: lut[res.rgba8[s + 1] as usize] * a,
                b: lut[res.rgba8[s + 2] as usize] * a,
                a,
            };
            let i = (py * dst.width + px) as usize;
            dst.pixels[i] = blend_pixel(src, dst.pixels[i], mode);
        }
    }
}

/// Run a filter layer over the accumulated composite below it, weighted by
/// the layer's opacity.
fn apply_filter(filter: &Filter, opacity: f32, dst: &mut Surface, clip: ClipRect) {
    match filter {
        Filter::GaussianBlur { sigma } => {
            let original = (opacity < 1.0).then(|| blur::snapshot(dst, clip));
            blur::gaussian_blur(dst, clip, *sigma);
            if let Some(orig) = original {
                mix_snapshot(dst, clip, &orig, |o, f| lerp(o, f, opacity));
            }
        }
        Filter::Sharpen { sigma, amount } => {
            let original = blur::snapshot(dst, clip);
            blur::gaussian_blur(dst, clip, *sigma);
            let amt = amount * opacity;
            mix_snapshot(dst, clip, &original, |o, blurred| {
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
    f: impl Fn(LinearRgba, LinearRgba) -> LinearRgba,
) {
    let w = (clip.x1 - clip.x0) as usize;
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let i = (y * dst.width + x) as usize;
            let s = (y - clip.y0) as usize * w + (x - clip.x0) as usize;
            dst.pixels[i] = f(original[s], dst.pixels[i]);
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
            } => {
                if let Some((lx, ly)) = to_local(node.transform, x, y) {
                    let hit = if fill.is_some() {
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
