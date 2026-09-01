//! Rendering for Chitrakar.
//!
//! Current state: a scalar CPU reference renderer that evaluates the scene
//! graph bottom-up into a linear-float pixel buffer. It exists to pin down
//! *correct* output; the wgpu/vello GPU backends (Phase 1) are validated
//! against it. Tiling ([`tiles`]) is the invalidation granularity for the
//! cached render graph.

pub mod tiles;

use chitrakar_color::{to_working, LinearRgba};
use chitrakar_doc::{Adjustment, BlendMode, DocError, Document, NodeId, NodeKind, VectorShape};

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
}

/// Render a document to a full-size surface.
pub fn render(doc: &Document) -> Result<Surface, DocError> {
    let mut surface = Surface::new(doc.meta.width, doc.meta.height);
    render_group(doc, doc.root(), &mut surface)?;
    Ok(surface)
}

fn render_group(doc: &Document, group: NodeId, dst: &mut Surface) -> Result<(), DocError> {
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
                render_group(doc, child, &mut sub)?;
                composite(dst, &sub, node.opacity, node.blend);
            }
            NodeKind::Vector { shape, fill, .. } => {
                if let Some(fill) = fill {
                    let mut color = to_working(*fill);
                    color = scale_alpha(color, node.opacity);
                    let t = node.transform;
                    fill_shape(dst, shape, t.e, t.f, color, node.blend);
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = doc.resource(&raster.resource_id) {
                    if !res.rgba8.is_empty() {
                        let t = node.transform;
                        draw_raster(dst, res, t.e, t.f, node.opacity, node.blend);
                    }
                }
            }
            NodeKind::Adjustment(adj) => {
                // An adjustment layer transforms everything composited below
                // it, weighted by its opacity.
                for px in dst.pixels.iter_mut() {
                    let adjusted = apply_adjustment(adj, *px);
                    *px = lerp(*px, adjusted, node.opacity);
                }
            }
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

fn composite(dst: &mut Surface, src: &Surface, opacity: f32, mode: BlendMode) {
    for (d, s) in dst.pixels.iter_mut().zip(&src.pixels) {
        *d = blend_pixel(scale_alpha(*s, opacity), *d, mode);
    }
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

/// Local-space bounds (min inclusive, max exclusive) of a shape.
fn shape_bounds(shape: &VectorShape) -> (f32, f32) {
    match shape {
        VectorShape::Rect { width, height } => (*width, *height),
        VectorShape::Ellipse { rx, ry } => (rx * 2.0, ry * 2.0),
        VectorShape::Path { .. } => (0.0, 0.0),
    }
}

fn fill_shape(
    dst: &mut Surface,
    shape: &VectorShape,
    tx: f32,
    ty: f32,
    color: LinearRgba,
    mode: BlendMode,
) {
    // Only walk the pixels the shape's bounds can touch.
    let (w, h) = shape_bounds(shape);
    let x0 = (tx.floor().max(0.0)) as u32;
    let y0 = (ty.floor().max(0.0)) as u32;
    let x1 = ((tx + w).ceil().max(0.0) as u32).min(dst.width);
    let y1 = ((ty + h).ceil().max(0.0) as u32).min(dst.height);
    for py in y0..y1 {
        for px in x0..x1 {
            // Sample at pixel centers; anti-aliasing arrives with the real
            // rasterizer (vello / analytic coverage).
            let (x, y) = (px as f32 + 0.5 - tx, py as f32 + 0.5 - ty);
            if shape_covers(shape, x, y) {
                let i = (py * dst.width + px) as usize;
                dst.pixels[i] = blend_pixel(color, dst.pixels[i], mode);
            }
        }
    }
}

/// Blit a source image at an integer-ish translation (nearest sample; full
/// affine sampling arrives with the GPU path).
fn draw_raster(
    dst: &mut Surface,
    res: &chitrakar_doc::Resource,
    tx: f32,
    ty: f32,
    opacity: f32,
    mode: BlendMode,
) {
    // 8-bit sRGB → linear lookup table, built per blit (256 entries, cheap).
    let mut lut = [0f32; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        *out = chitrakar_color::srgb_to_linear(v as f32 / 255.0);
    }
    let x0 = tx.floor().max(0.0) as u32;
    let y0 = ty.floor().max(0.0) as u32;
    let x1 = ((tx + res.width as f32).ceil().max(0.0) as u32).min(dst.width);
    let y1 = ((ty + res.height as f32).ceil().max(0.0) as u32).min(dst.height);
    for py in y0..y1 {
        for px in x0..x1 {
            let sx = (px as f32 + 0.5 - tx) as u32;
            let sy = (py as f32 + 0.5 - ty) as u32;
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
            NodeKind::Vector { shape, fill, .. } if fill.is_some() => {
                let t = node.transform;
                if shape_covers(shape, x - t.e, y - t.f) {
                    return Ok(Some(child));
                }
            }
            NodeKind::Raster(raster) => {
                let t = node.transform;
                let (lx, ly) = (x - t.e, y - t.f);
                if lx >= 0.0 && ly >= 0.0 && lx < raster.width as f32 && ly < raster.height as f32 {
                    return Ok(Some(child));
                }
            }
            _ => {}
        }
    }
    Ok(None)
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
        // Full HSL math lands in Phase 2; lightness-only is enough to wire
        // the pipeline.
        Adjustment::HueSaturation { lightness, .. } => {
            let f = |v: f32| (v + lightness).clamp(0.0, 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::{AuthoredColor, ColorMode};
    use chitrakar_doc::{Command, Node, Transform};

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
