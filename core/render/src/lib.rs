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
pub mod boolean;
pub mod text;
pub mod tiles;

use chitrakar_color::{to_working, AuthoredColor, LinearRgba};
use chitrakar_doc::{
    Adjustment, BlendMode, DocError, Document, Effect, Filter, Gradient, Mask, MaskKind, NodeId,
    NodeKind, Transform, VectorShape,
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
        VectorShape::Rect { width, height, .. } => (*width, *height),
        VectorShape::Ellipse { rx, ry } => (rx * 2.0, ry * 2.0),
        // Path anchors are normalized to a (0,0) origin on creation; local
        // size is their extent.
        VectorShape::Path {
            points, subpaths, ..
        } => points
            .iter()
            .chain(subpaths.iter().flatten())
            .fold((0.0f32, 0.0f32), |(w, h), p| (w.max(p[0]), h.max(p[1]))),
    }
}

/// Local bounding box (min x, min y, max x, max y). Unlike [`shape_size`]
/// this keeps a negative min — a smooth path's spline can overshoot the
/// anchors, including past the origin.
fn local_bounds(shape: &VectorShape) -> (f32, f32, f32, f32) {
    match shape {
        VectorShape::Path {
            points, subpaths, ..
        } => points.iter().chain(subpaths.iter().flatten()).fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(x0, y0, x1, y1), p| (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])),
        ),
        _ => {
            let (w, h) = shape_size(shape);
            (0.0, 0.0, w, h)
        }
    }
}

/// Doc-space bounds of a local box: the axis-aligned box around all four
/// mapped corners, since a rotated box is not one of them.
fn transformed_local_bounds(t: Transform, lb: (f32, f32, f32, f32)) -> Bounds {
    let corners = [
        to_device(t, lb.0, lb.1),
        to_device(t, lb.2, lb.1),
        to_device(t, lb.0, lb.3),
        to_device(t, lb.2, lb.3),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    Bounds::Rect(x0, y0, x1, y1)
}

/// Transformed doc-space bounds of a local (0,0)-(w,h) box.
fn transformed_bounds(t: Transform, w: f32, h: f32) -> Bounds {
    transformed_local_bounds(t, (0.0, 0.0, w, h))
}

/// Doc-space extent of a node: leaf bounds through its transform; groups are
/// the union of their children. Any adjustment layer in the subtree makes
/// the answer [`Bounds::Everything`], because it acts on all content below.
/// Visibility is ignored on purpose — toggling it dirties the same region.
/// The transform carrying a node's *parent's* space into document space:
/// every ancestor's transform, outermost first. A node's own transform is
/// written against this, which is why drags and bounds both need it.
pub fn ancestor_space(doc: &Document, id: NodeId) -> Transform {
    let mut chain = Vec::new();
    let mut cur = doc.parent_of(id);
    while let Some(p) = cur {
        if let Ok(n) = doc.node(p) {
            chain.push(n.transform);
        }
        cur = doc.parent_of(p);
    }
    chain
        .iter()
        .rev()
        .fold(Transform::default(), |acc, t| acc.compose(*t))
}

/// Whether anything inside a group can see the pixels underneath it, which
/// is what forces the group onto a surface of its own. Blend modes other
/// than Normal read the backdrop, and so do adjustment and filter layers;
/// a nested group that isolates itself does not, because it hands back a
/// composite either way.
fn reads_backdrop(doc: &Document, group: NodeId) -> Result<bool, DocError> {
    for &child in doc.children_of(group)? {
        let node = doc.node(child)?;
        if node.blend != BlendMode::Normal {
            return Ok(true);
        }
        match &node.kind {
            NodeKind::Adjustment(_) | NodeKind::Filter(_) => return Ok(true),
            NodeKind::Group if reads_backdrop(doc, child)? => return Ok(true),
            _ => {}
        }
    }
    Ok(false)
}

/// Doc-space bounds of a node, ancestors included.
pub fn node_bounds(doc: &Document, id: NodeId) -> Result<Bounds, DocError> {
    node_bounds_inner(doc, id, true)
}

/// The box the layer itself occupies — what a selection outline is drawn
/// around and what "W" in the panel means.
///
/// Distinct from [`node_bounds`], which answers what the layer can *touch*
/// and so includes the reach of its effects. A drop shadow changes where
/// pixels must be repainted; it does not change how wide the layer is, and
/// laying out against a box inflated by one would be nonsense.
pub fn node_visual_bounds(doc: &Document, id: NodeId) -> Result<Bounds, DocError> {
    node_bounds_inner(doc, id, false)
}

fn node_bounds_inner(doc: &Document, id: NodeId, effects: bool) -> Result<Bounds, DocError> {
    let local = bounds_in_parent_space_inner(doc, id, effects)?;
    Ok(match local {
        Bounds::Rect(x0, y0, x1, y1) => {
            transformed_local_bounds(ancestor_space(doc, id), (x0, y0, x1, y1))
        }
        other => other,
    })
}

/// Bounds in the space the node's transform is written against — its
/// parent's. Groups union their children here, which is why this is the
/// recursive form and `node_bounds` is the one that finishes the job.
fn bounds_in_parent_space(doc: &Document, id: NodeId) -> Result<Bounds, DocError> {
    bounds_in_parent_space_inner(doc, id, true)
}

fn bounds_in_parent_space_inner(
    doc: &Document,
    id: NodeId,
    effects: bool,
) -> Result<Bounds, DocError> {
    let node = doc.node(id)?;
    // Effects reach past the layer's own edges, in the parent space this
    // whole function answers in, so they widen the box the renderer and
    // the dirty tracking are cut from — but not the one the layer is said
    // to occupy, which is why the caller chooses.
    let effect_pad = if effects {
        node.effects
            .iter()
            .map(Effect::reach)
            .fold(0.0f32, f32::max)
    } else {
        0.0
    };
    let grow = |b: Bounds| match b {
        Bounds::Rect(x0, y0, x1, y1) if effect_pad > 0.0 => Bounds::Rect(
            x0 - effect_pad,
            y0 - effect_pad,
            x1 + effect_pad,
            y1 + effect_pad,
        ),
        other => other,
    };
    Ok(grow(match &node.kind {
        NodeKind::Adjustment(_) | NodeKind::Filter(_) => Bounds::Everything,
        NodeKind::Group => {
            let mut acc = Bounds::None;
            for &child in doc.children_of(id)? {
                acc = acc.union(bounds_in_parent_space_inner(doc, child, effects)?);
                if acc == Bounds::Everything {
                    break;
                }
            }
            // Children's bounds are in the group's space; the group's own
            // transform carries them into its parent's.
            match acc {
                Bounds::Rect(x0, y0, x1, y1) => {
                    transformed_local_bounds(node.transform, (x0, y0, x1, y1))
                }
                other => other,
            }
        }
        NodeKind::Vector { shape, stroke, .. } => {
            let flat = flatten_shape(shape);
            let mut bounds = transformed_local_bounds(node.transform, local_bounds(flat.as_ref()));
            // Path strokes are centered on the line, so they overhang the
            // anchor bounds (rect/ellipse strokes are inner bands and don't).
            if let (VectorShape::Path { .. }, Some(stroke)) = (shape, stroke) {
                let pad = stroke.width * max_scale(node.transform);
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
    }))
}

/// A node's bounds in its *own* space, before its transform — the box a
/// selection outline should be drawn around so it turns with the layer.
///
/// Groups report document space, because a group's own transform is not
/// applied to its children (their transforms are absolute), so its local
/// space and the document's are the same one.
pub fn local_bounds_of(doc: &Document, id: NodeId) -> Result<Option<[f32; 4]>, DocError> {
    let node = doc.node(id)?;
    Ok(match &node.kind {
        NodeKind::Vector { shape, .. } => {
            let flat = flatten_shape(shape);
            let (x0, y0, x1, y1) = local_bounds(flat.as_ref());
            (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
        }
        kind => match local_size(kind) {
            Some((w, h)) => Some([0.0, 0.0, w, h]),
            // A group's own box is the union of its children, which are
            // already expressed in its space — using node_bounds here would
            // apply the group's transform a second time.
            None if matches!(node.kind, NodeKind::Group) => {
                let mut acc = Bounds::None;
                for &child in doc.children_of(id)? {
                    // The box the group occupies, so without the reach of
                    // anything's effects: this is what gets an outline and
                    // handles drawn round it.
                    acc = acc.union(bounds_in_parent_space_inner(doc, child, false)?);
                }
                match acc {
                    Bounds::Rect(x0, y0, x1, y1) => Some([x0, y0, x1, y1]),
                    _ => None,
                }
            }
            // Adjustments and filters act on everything below them.
            None => match node_bounds(doc, id)? {
                Bounds::Rect(x0, y0, x1, y1) => Some([x0, y0, x1, y1]),
                _ => None,
            },
        },
    })
}

/// How far, in pixels, the document's filter stack can carry a change:
/// the summed sample reach of every filter layer (sequential filters
/// compound). A region render whose clip is padded by this much computes
/// correct values for the unpadded interior even next to stale surroundings.
pub fn filter_reach(doc: &Document) -> u32 {
    doc.nodes()
        .map(|(id, node)| match &node.kind {
            NodeKind::Filter(Filter::GaussianBlur { sigma })
            | NodeKind::Filter(Filter::Sharpen { sigma, .. }) => {
                // A filter's radius is written in the space it sits in, so
                // a blur inside a group scaled up reaches further than its
                // sigma says — and the padding has to know that or the
                // region render leaves a halo of stale pixels behind.
                let scale = max_scale(ancestor_space(doc, *id));
                // Three iterated box blurs reach ~3 * box radius ≈ 2.9σ;
                // round up generously.
                (sigma * scale * 3.0).ceil() as u32 + 2
            }
            _ => 0,
        })
        .sum()
}

/// Render a document to a new full-size surface.
pub fn render(doc: &Document) -> Result<Surface, DocError> {
    let mut surface = Surface::new(doc.meta.width, doc.meta.height);
    let clip = surface.full_clip();
    // A fresh surface is already transparent, so paint straight into it
    // rather than going through render_region, whose first act would be to
    // clear the region again — a whole-canvas write of zeroes over zeroes.
    render_group(doc, doc.root(), &mut surface, clip, Transform::default())?;
    Ok(surface)
}

/// Recompute one region of a surface from scratch (clears it first). Pixels
/// outside `clip` are untouched.
pub fn render_region(
    doc: &Document,
    surface: &mut Surface,
    clip: ClipRect,
) -> Result<(), DocError> {
    render_region_at(doc, surface, clip, Transform::default())
}

/// The same, with the document mapped through `view` on the way to the
/// surface.
///
/// This is what lets the surface stop being the page. A `view` of scale
/// two on a surface twice the size renders the document at twice the
/// resolution — outlines re-solved at that scale, not a magnified bitmap —
/// and a `view` that also translates renders whatever part of the document
/// the surface is looking at, at whatever size the surface is.
///
/// The clip is cleared in full but painted only where the page is: the
/// page's own edge is what clips the artwork, and with the surface no
/// longer being the page that has to be said rather than assumed.
pub fn render_region_at(
    doc: &Document,
    surface: &mut Surface,
    clip: ClipRect,
    view: Transform,
) -> Result<(), DocError> {
    if clip.is_empty() {
        return Ok(());
    }
    for y in clip.y0..clip.y1 {
        let row = (y * surface.width) as usize;
        surface.pixels[row + clip.x0 as usize..row + clip.x1 as usize]
            .fill(LinearRgba::TRANSPARENT);
    }
    // Rounded outward but no further: a pixel the page's edge partly
    // covers is the page's to paint, and one it does not touch is not.
    // (`Bounds::to_clip` pads by a pixel for seam safety, which here would
    // let the artwork bleed past the page.)
    let Bounds::Rect(x0, y0, x1, y1) =
        transformed_bounds(view, doc.meta.width as f32, doc.meta.height as f32)
    else {
        return Ok(());
    };
    let page = ClipRect {
        x0: x0.floor().max(0.0) as u32,
        y0: y0.floor().max(0.0) as u32,
        x1: (x1.ceil().max(0.0) as u32).min(surface.width),
        y1: (y1.ceil().max(0.0) as u32).min(surface.height),
    };
    let inside = clip.intersect(page);
    if inside.is_empty() {
        return Ok(());
    }
    render_group(doc, doc.root(), surface, inside, view)
}

fn render_group(
    doc: &Document,
    group: NodeId,
    dst: &mut Surface,
    clip: ClipRect,
    parent: Transform,
) -> Result<(), DocError> {
    // Children are stored bottom-to-top (painter's order).
    for &child in doc.children_of(group)? {
        let node = doc.node(child)?;
        if !node.visible || node.opacity <= 0.0 {
            continue;
        }
        // Effects are drawn from the layer's own silhouette, so a layer
        // that has any must exist as a picture before they can be. An
        // adjustment or filter has no silhouette — it is a transformation
        // of what is below — so effects on one mean nothing and are
        // ignored rather than given a surface.
        let effected = !node.effects.is_empty()
            && !matches!(node.kind, NodeKind::Adjustment(_) | NodeKind::Filter(_));
        if !effected {
            render_child(doc, child, dst, clip, parent, node.blend)?;
            continue;
        }
        let scale = max_scale(parent);
        let reach = node
            .effects
            .iter()
            .map(Effect::reach)
            .fold(0.0f32, f32::max);
        let pad = (reach * scale).ceil() as u32;
        // The layer has to be drawn wherever it could feed a visible
        // effect pixel, which is further out than the region being
        // repainted — by exactly the effects' reach.
        let grown = ClipRect {
            x0: clip.x0.saturating_sub(pad),
            y0: clip.y0.saturating_sub(pad),
            x1: (clip.x1 + pad).min(dst.width),
            y1: (clip.y1 + pad).min(dst.height),
        };
        let extent = match bounds_in_parent_space(doc, child)? {
            Bounds::Rect(x0, y0, x1, y1) => transformed_local_bounds(parent, (x0, y0, x1, y1)),
            other => other,
        };
        let layer_clip = match extent.to_clip(dst.width, dst.height) {
            Some(b) => b.intersect(grown),
            None => continue,
        };
        if layer_clip.is_empty() {
            continue;
        }
        let mut layer = Surface::new(dst.width, dst.height);
        // Normal into the layer's own transparent surface; the node's blend
        // belongs to the composite below, once the effects are in place.
        render_child(
            doc,
            child,
            &mut layer,
            layer_clip,
            parent,
            BlendMode::Normal,
        )?;
        // Effects behind the layer, then the layer, then the ones that
        // belong on top of it — an inner shadow shades the pixels it sits
        // on, so it cannot be painted before they are there.
        for effect in node.effects.iter().filter(|e| !e.over()) {
            draw_effect(
                dst,
                &layer,
                doc,
                effect,
                scale,
                layer_clip,
                clip,
                node.blend,
                node.opacity,
            );
        }
        composite(dst, &layer, 1.0, node.blend, clip.intersect(layer_clip));
        for effect in node.effects.iter().filter(|e| e.over()) {
            draw_effect(
                dst,
                &layer,
                doc,
                effect,
                scale,
                layer_clip,
                clip,
                node.blend,
                node.opacity,
            );
        }
    }
    Ok(())
}

/// Draw one layer, with its own blend supplied rather than read, so a layer
/// being staged for its effects can be painted plainly into its own surface
/// and blended once at the end.
fn render_child(
    doc: &Document,
    child: NodeId,
    dst: &mut Surface,
    clip: ClipRect,
    parent: Transform,
    blend: BlendMode,
) -> Result<(), DocError> {
    {
        let node = doc.node(child)?;
        // Everything below draws in the parent's space, so a node's own
        // transform is composed onto whatever its ancestors contribute; a
        // mask is authored in that same parent space.
        let t = parent.compose(node.transform);
        let mask = MaskRef::new(node.mask.as_ref(), parent);
        match &node.kind {
            NodeKind::Group => {
                // Isolate the group on its own surface so group opacity,
                // blend, and mask apply to the composite, not per child.
                //
                // Bound every pass that follows by where the group can
                // actually land. Outside those bounds its surface stays
                // transparent, and compositing a transparent source leaves
                // the destination alone under every blend mode — so the
                // work was always discarded, but at A4 a group holding one
                // small shape still paid two whole-canvas passes for it.
                let extent = match bounds_in_parent_space(doc, child)? {
                    Bounds::Rect(x0, y0, x1, y1) => {
                        transformed_local_bounds(parent, (x0, y0, x1, y1))
                    }
                    other => other,
                };
                let sub_clip = match extent.to_clip(dst.width, dst.height) {
                    Some(b) => b.intersect(clip),
                    None => return Ok(()),
                };
                if sub_clip.is_empty() {
                    return Ok(());
                }
                // A group that composites like its children individually
                // would needs no surface of its own: source-over is
                // associative, so painting straight into the destination
                // gives the same pixels for a fraction of the cost.
                if node.opacity >= 1.0
                    && blend == BlendMode::Normal
                    && mask.mask.is_none()
                    && !reads_backdrop(doc, child)?
                {
                    return render_group(doc, child, dst, sub_clip, t);
                }
                let mut sub = Surface::new(dst.width, dst.height);
                render_group(doc, child, &mut sub, sub_clip, t)?;
                if let Some(m) = mask.mask {
                    apply_mask(doc, m, &mut sub, sub_clip, parent);
                }
                composite(dst, &sub, node.opacity, blend, sub_clip);
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
                    paint_shape(dst, doc, shape, t, &paint, blend, clip, None, &[], mask);
                }
                if let Some(stroke) = stroke {
                    let color = scale_alpha(resolve_color(doc, stroke.color), node.opacity);
                    paint_shape(
                        dst,
                        doc,
                        shape,
                        t,
                        &Paint::Solid(color),
                        blend,
                        clip,
                        Some(stroke.width),
                        &flatten_widths(shape, &stroke.widths),
                        mask,
                    );
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = doc.resource(&raster.resource_id) {
                    if !res.rgba8.is_empty() {
                        draw_raster(dst, doc, res, t, node.opacity, blend, clip, mask);
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
            // A filter's radius is written in the space it lives in, so it
            // stretches with whatever scales that space — the view, or a
            // group the filter sits inside.
            NodeKind::Filter(filter) => apply_filter(
                doc,
                filter,
                node.opacity,
                mask,
                dst,
                clip,
                max_scale(parent),
            ),
            NodeKind::Text(spec) => draw_text(dst, doc, spec, t, node.opacity, blend, clip, mask),
        }
    }
    Ok(())
}

/// Paint one of a layer's effects beneath it, given the layer already
/// rendered on its own surface.
///
/// `layer_clip` is where that surface actually holds the layer (the visible
/// region grown by the effect's reach); `clip` is what may be written.
/// `scale` carries the effect's parameters — written in the layer's parent
/// space — into device pixels, so a shadow grows with the group it is in
/// and with the zoom.
#[allow(clippy::too_many_arguments)]
fn draw_effect(
    dst: &mut Surface,
    layer: &Surface,
    doc: &Document,
    effect: &Effect,
    scale: f32,
    layer_clip: ClipRect,
    clip: ClipRect,
    blend: BlendMode,
    layer_opacity: f32,
) {
    // Every effect is built from the layer's silhouette rather than its
    // picture. That is what makes a shadow of a photograph a shape, and
    // what lets all three of these share one path.
    let write = clip.intersect(layer_clip);
    if write.is_empty() {
        return;
    }
    match effect {
        Effect::DropShadow {
            dx,
            dy,
            blur,
            color,
            opacity,
        } => {
            if *opacity <= 0.0 {
                return;
            }
            let tint = scale_alpha(resolve_color(doc, *color), *opacity);
            let field = silhouette(dst, layer, layer_clip, tint, false, blur * scale);
            stamp(
                dst,
                &field,
                (dx * scale, dy * scale),
                layer_clip,
                write,
                blend,
                None,
            );
        }
        Effect::InnerShadow {
            dx,
            dy,
            blur,
            color,
            opacity,
        } => {
            if *opacity <= 0.0 {
                return;
            }
            // Cast from the hole around the layer instead of the layer, then
            // kept inside it: what shows is the part of that shadow the
            // silhouette itself covers, which is the edge, from inside.
            let tint = scale_alpha(resolve_color(doc, *color), *opacity);
            let field = silhouette(dst, layer, layer_clip, tint, true, blur * scale);
            stamp(
                dst,
                &field,
                (dx * scale, dy * scale),
                layer_clip,
                write,
                blend,
                Some(layer),
            );
        }
        Effect::Outline {
            width,
            color,
            opacity,
        } => {
            let w = width * scale;
            if *opacity <= 0.0 || w <= 0.0 {
                return;
            }
            let tint = scale_alpha(resolve_color(doc, *color), *opacity);
            let field = outline_band(dst, layer, layer_clip, tint, w, layer_opacity);
            stamp(dst, &field, (0.0, 0.0), layer_clip, write, blend, None);
        }
    }
}

/// The layer's silhouette (or, inverted, the hole around it) in one flat
/// colour, blurred. Tinting before the blur rather than after is the same
/// answer — the tint is constant and the blur is linear — and it means one
/// surface instead of two.
fn silhouette(
    dst: &Surface,
    layer: &Surface,
    clip: ClipRect,
    tint: LinearRgba,
    invert: bool,
    sigma: f32,
) -> Surface {
    let mut out = Surface::new(dst.width, dst.height);
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let i = (y * dst.width + x) as usize;
            let a = layer.pixels[i].a;
            out.pixels[i] = scale_alpha(tint, if invert { 1.0 - a } else { a });
        }
    }
    blur::gaussian_blur(&mut out, clip, sigma);
    out
}

/// A band of colour reaching `width` device pixels out from the layer's
/// edge, filled solid and feathered over the last pixel.
///
/// Blurring the silhouette and lifting the result would give a band whose
/// softness grew with its width; what an outline wants is a distance. So
/// this measures one: a two-pass chamfer transform from the silhouette,
/// which is a couple of passes over the region and rounds corners the way
/// a pen would.
fn outline_band(
    dst: &Surface,
    layer: &Surface,
    clip: ClipRect,
    tint: LinearRgba,
    width: f32,
    layer_opacity: f32,
) -> Surface {
    let (w, h) = ((clip.x1 - clip.x0) as usize, (clip.y1 - clip.y0) as usize);
    let far = width + 4.0;
    let mut dist = vec![far; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = ((y as u32 + clip.y0) * dst.width + x as u32 + clip.x0) as usize;
            // Half covered is inside. The layer was staged with its own
            // opacity already applied, so half of *that* is where its edge
            // is: a layer at a third opacity would otherwise have no
            // inside at all, and cast no outline.
            if layer.pixels[i].a >= 0.5 * layer_opacity.max(1e-3) {
                dist[y * w + x] = 0.0;
            }
        }
    }
    // Chamfer weights: a step sideways costs one, a diagonal costs root two.
    const D1: f32 = 1.0;
    const D2: f32 = std::f32::consts::SQRT_2;
    let relax = |dist: &mut Vec<f32>, at: usize, from: usize, cost: f32| {
        let candidate = dist[from] + cost;
        if candidate < dist[at] {
            dist[at] = candidate;
        }
    };
    for y in 0..h {
        for x in 0..w {
            let at = y * w + x;
            if y > 0 {
                relax(&mut dist, at, at - w, D1);
                if x > 0 {
                    relax(&mut dist, at, at - w - 1, D2);
                }
                if x + 1 < w {
                    relax(&mut dist, at, at - w + 1, D2);
                }
            }
            if x > 0 {
                relax(&mut dist, at, at - 1, D1);
            }
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let at = y * w + x;
            if y + 1 < h {
                relax(&mut dist, at, at + w, D1);
                if x > 0 {
                    relax(&mut dist, at, at + w - 1, D2);
                }
                if x + 1 < w {
                    relax(&mut dist, at, at + w + 1, D2);
                }
            }
            if x + 1 < w {
                relax(&mut dist, at, at + 1, D1);
            }
        }
    }
    let mut out = Surface::new(dst.width, dst.height);
    for y in 0..h {
        for x in 0..w {
            // The chamfer counts steps between pixel centres, and the
            // centre of an edge pixel already sits half a pixel inside the
            // shape — so the distance to the edge itself is one less half
            // at each end.
            let cover = (width + 1.0 - dist[y * w + x]).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let i = ((y as u32 + clip.y0) * dst.width + x as u32 + clip.x0) as usize;
            out.pixels[i] = scale_alpha(tint, cover);
        }
    }
    out
}

/// Paint a prepared effect field into the destination, displaced by
/// `offset` device pixels and sampled between pixels so a sub-pixel offset
/// (or a fractional zoom) does not jump. `keep_inside`, when given, limits
/// what lands to that surface's own alpha — how an inner shadow stays in.
fn stamp(
    dst: &mut Surface,
    field: &Surface,
    offset: (f32, f32),
    field_clip: ClipRect,
    write: ClipRect,
    blend: BlendMode,
    keep_inside: Option<&Surface>,
) {
    let (ox, oy) = offset;
    let (lo_x, hi_x) = (field_clip.x0 as f32, field_clip.x1 as f32 - 1.0);
    let (lo_y, hi_y) = (field_clip.y0 as f32, field_clip.y1 as f32 - 1.0);
    if hi_x < lo_x || hi_y < lo_y {
        return;
    }
    for y in write.y0..write.y1 {
        for x in write.x0..write.x1 {
            let (sx, sy) = (x as f32 - ox, y as f32 - oy);
            if sx < lo_x - 1.0 || sy < lo_y - 1.0 || sx > hi_x + 1.0 || sy > hi_y + 1.0 {
                continue;
            }
            let (fx, fy) = (sx.floor(), sy.floor());
            let (tx, ty) = (sx - fx, sy - fy);
            let at = |px: f32, py: f32| {
                let px = px.clamp(lo_x, hi_x) as u32;
                let py = py.clamp(lo_y, hi_y) as u32;
                field.pixels[(py * field.width + px) as usize]
            };
            let top = lerp(at(fx, fy), at(fx + 1.0, fy), tx);
            let bottom = lerp(at(fx, fy + 1.0), at(fx + 1.0, fy + 1.0), tx);
            let mut src = lerp(top, bottom, ty);
            let i = (y * dst.width + x) as usize;
            if let Some(inside) = keep_inside {
                src = scale_alpha(src, inside.pixels[i].a);
            }
            if src.a <= 0.0 {
                continue;
            }
            dst.pixels[i] = blend_pixel(src, dst.pixels[i], blend);
        }
    }
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
/// Map a point from a node's local space into document space. Transforms
/// are the full affine `[a c e; b d f]`, so this is where rotation and
/// shear actually take effect.
fn to_device(t: Transform, x: f32, y: f32) -> (f32, f32) {
    (t.a * x + t.c * y + t.e, t.b * x + t.d * y + t.f)
}

/// A transform's inverse, solved once and then applied as a plain multiply.
///
/// Coverage sampling asks for the local coordinate of up to twenty-one
/// points per boundary pixel; re-deriving the determinant and dividing by
/// it at each one made the inverse the hot spot of the whole renderer.
/// Inverting per shape instead leaves two multiply-adds per sample.
#[derive(Clone, Copy)]
struct Inverse {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Inverse {
    /// `None` when the transform collapses space (a zero determinant),
    /// which would make every device pixel map nowhere.
    fn of(t: Transform) -> Option<Inverse> {
        let det = t.a * t.d - t.b * t.c;
        if det.abs() < 1e-9 {
            return None;
        }
        Some(Inverse {
            a: t.d / det,
            b: -t.b / det,
            c: -t.c / det,
            d: t.a / det,
            e: t.e,
            f: t.f,
        })
    }

    /// Map a device point into the local space the transform came from.
    #[inline]
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.e, y - self.f);
        (self.a * dx + self.c * dy, self.b * dx + self.d * dy)
    }
}

/// The inverse map for a one-off point. Per-pixel callers invert once with
/// [`Inverse::of`] and call [`Inverse::at`] instead.
fn to_local(t: Transform, x: f32, y: f32) -> Option<(f32, f32)> {
    Inverse::of(t).map(|inv| inv.at(x, y))
}

/// How much the transform can stretch a length, used to pad bounds for
/// strokes: the larger of the two column norms, which bounds the true
/// largest singular value closely enough for a conservative pad.
fn max_scale(t: Transform) -> f32 {
    t.a.hypot(t.b).max(t.c.hypot(t.d))
}

/// Samples per curve segment when flattening. Shared so per-anchor data
/// can be resampled to match the polyline exactly.
const FLATTEN_STEPS: usize = 12;

/// Expand a curved path into the polyline everything else works on — paint,
/// hit test, bounds all run on the result, so curves need no special cases
/// downstream. Bezier handles win when present because they are authored;
/// `smooth` infers a Catmull-Rom spline through the anchors instead.
/// Everything else (and paths already polyline) borrows unchanged. Call once
/// per operation, not per pixel.
fn flatten_shape(shape: &VectorShape) -> std::borrow::Cow<'_, VectorShape> {
    use std::borrow::Cow;
    const STEPS: usize = FLATTEN_STEPS;
    let VectorShape::Path {
        points,
        closed,
        smooth,
        handles,
        subpaths,
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
            // Subpaths are straight-sided, so they survive flattening
            // untouched.
            subpaths: subpaths.clone(),
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
        subpaths: subpaths.clone(),
    })
}

/// A shape as closed rings of straight segments, in its own local space.
///
/// Booleans, and anything else that needs an outline rather than a
/// coverage test, work on these: curves are flattened, an ellipse becomes
/// a polygon fine enough that its own edge is the limit, and a rounded
/// rectangle's corners become arcs of segments. An open path is treated as
/// closed, since only its filled area means anything here.
pub fn shape_rings(shape: &VectorShape) -> Vec<Vec<[f32; 2]>> {
    /// Segments per quarter turn. At 16 the chord of a 1000px circle
    /// strays under a fifth of a pixel from the true arc.
    const PER_QUARTER: usize = 16;
    match shape {
        VectorShape::Ellipse { rx, ry } => {
            let n = PER_QUARTER * 4;
            vec![(0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    [rx + rx * a.cos(), ry + ry * a.sin()]
                })
                .collect()]
        }
        VectorShape::Rect {
            width,
            height,
            radius,
        } => {
            let r = corner_radius(*width, *height, *radius);
            if r <= 0.0 {
                return vec![vec![
                    [0.0, 0.0],
                    [*width, 0.0],
                    [*width, *height],
                    [0.0, *height],
                ]];
            }
            // Each corner is a quarter arc about its own centre, walked in
            // the same direction the square-cornered ring goes.
            let corners = [
                (r, r, std::f32::consts::PI, 1.5 * std::f32::consts::PI),
                (
                    width - r,
                    r,
                    1.5 * std::f32::consts::PI,
                    std::f32::consts::TAU,
                ),
                (width - r, height - r, 0.0, std::f32::consts::FRAC_PI_2),
                (
                    r,
                    height - r,
                    std::f32::consts::FRAC_PI_2,
                    std::f32::consts::PI,
                ),
            ];
            let mut ring = Vec::with_capacity(4 * (PER_QUARTER + 1));
            for (cx, cy, from, to) in corners {
                for i in 0..=PER_QUARTER {
                    let a = from + (to - from) * (i as f32 / PER_QUARTER as f32);
                    ring.push([cx + r * a.cos(), cy + r * a.sin()]);
                }
            }
            vec![ring]
        }
        VectorShape::Path { .. } => {
            let flat = flatten_shape(shape);
            let VectorShape::Path {
                points, subpaths, ..
            } = flat.as_ref()
            else {
                return Vec::new();
            };
            std::iter::once(points.clone())
                .chain(subpaths.iter().cloned())
                .filter(|r| r.len() >= 3)
                .collect()
        }
    }
}

/// Per-anchor widths resampled onto the flattened polyline, so index i of
/// the result describes point i of the flattened shape. Flattening turns
/// each segment into a fixed number of samples, so the width at a sample is
/// just the lerp between its segment's two anchors.
fn flatten_widths(shape: &VectorShape, widths: &[f32]) -> Vec<f32> {
    let VectorShape::Path {
        points,
        closed,
        smooth,
        handles,
        ..
    } = shape
    else {
        return widths.to_vec();
    };
    let n = points.len();
    if widths.len() != n || n < 2 {
        return Vec::new();
    }
    let curved = (handles.len() == n && handles.iter().any(|h| h.iter().any(|v| v.abs() > 1e-6)))
        || (*smooth && n >= 3);
    if !curved {
        return widths.to_vec();
    }
    let segments = if *closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(segments * FLATTEN_STEPS + 1);
    for i in 0..segments {
        let (a, b) = (widths[i], widths[(i + 1) % n]);
        for s in 0..FLATTEN_STEPS {
            let t = s as f32 / FLATTEN_STEPS as f32;
            out.push(a + (b - a) * t);
        }
    }
    if !closed {
        out.push(widths[n - 1]);
    }
    out
}

/// Local-space coverage test for a shape (shape origin at 0,0). Paths fill
/// by the even-odd rule over their anchor polygon (open paths close
/// implicitly, the SVG convention).
fn shape_covers(shape: &VectorShape, x: f32, y: f32) -> bool {
    match shape {
        VectorShape::Rect {
            width,
            height,
            radius,
        } => rounded_rect_distance(*width, *height, *radius, x, y) <= 0.0,
        VectorShape::Ellipse { rx, ry } => {
            let (nx, ny) = ((x - rx) / rx, (y - ry) / ry);
            nx * nx + ny * ny <= 1.0
        }
        // Even-odd across every ring, so a ring inside another cuts a
        // hole and a ring beside it is a second island.
        VectorShape::Path {
            points, subpaths, ..
        } => {
            let mut inside = false;
            for ring in std::iter::once(points).chain(subpaths) {
                if ring.len() < 3 {
                    continue;
                }
                for i in 0..ring.len() {
                    let a = ring[i];
                    let b = ring[(i + 1) % ring.len()];
                    if (a[1] > y) != (b[1] > y) {
                        let t = (y - a[1]) / (b[1] - a[1]);
                        if x < a[0] + t * (b[0] - a[0]) {
                            inside = !inside;
                        }
                    }
                }
            }
            inside
        }
    }
}

/// How round a rectangle's corners can actually be: never negative, never
/// past half the shorter side — and never asked to clamp to a range that
/// runs backwards, which a rectangle with a negative dimension (from a
/// file rather than from the editor) would otherwise do.
fn corner_radius(width: f32, height: f32, radius: f32) -> f32 {
    let limit = (width / 2.0).min(height / 2.0).max(0.0);
    radius.clamp(0.0, limit)
}

/// Signed distance from a point to a rounded rectangle's edge: negative
/// inside, zero on the edge, positive outside. One formula covers the
/// sides, the corners and a radius of zero, which is why the square case
/// needs no branch of its own.
fn rounded_rect_distance(width: f32, height: f32, radius: f32, x: f32, y: f32) -> f32 {
    let (hw, hh) = (width / 2.0, height / 2.0);
    let r = corner_radius(width, height, radius);
    let qx = (x - hw).abs() - (hw - r);
    let qy = (y - hh).abs() - (hh - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

/// Where the closest point on a segment lies along it, 0 at `a` and 1 at
/// `b` — the parameter the width is interpolated at.
fn segment_parameter(px: f32, py: f32, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 <= 1e-12 {
        return 0.0;
    }
    (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0.0, 1.0)
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
fn stroke_covers(shape: &VectorShape, width: f32, widths: &[f32], x: f32, y: f32) -> bool {
    if let VectorShape::Path {
        points,
        closed,
        subpaths,
        ..
    } = shape
    {
        if points.len() < 2 {
            return false;
        }
        let segments = if *closed {
            points.len()
        } else {
            points.len() - 1
        };
        // A varying stroke is still a distance test, just against a width
        // that changes along the segment — which keeps caps round and
        // self-crossings solid, where building an outline polygon would
        // have to decide what those mean.
        let at = |i: usize| -> f32 { widths.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0) };
        let varying = !widths.is_empty();
        let half = width / 2.0;
        let on_main = (0..segments).any(|i| {
            let j = (i + 1) % points.len();
            let (a, b) = (points[i], points[j]);
            if !varying {
                return segment_distance(x, y, a, b) <= half;
            }
            let t = segment_parameter(x, y, a, b);
            let w = half * (at(i) + (at(j) - at(i)) * t);
            segment_distance(x, y, a, b) <= w
        });
        // Extra rings are always closed and never carry varying widths, so
        // they are a plain distance test against the whole ring.
        return on_main
            || subpaths.iter().any(|ring| {
                ring.len() >= 2
                    && (0..ring.len()).any(|i| {
                        let b = ring[(i + 1) % ring.len()];
                        segment_distance(x, y, ring[i], b) <= half
                    })
            });
    }
    if !shape_covers(shape, x, y) {
        return false;
    }
    match shape {
        // Inside already; the band is the outermost `width` of that, which
        // the signed distance gives directly however round the corners are.
        VectorShape::Rect {
            width: w,
            height: h,
            radius,
        } => rounded_rect_distance(*w, *h, *radius, x, y) > -width,
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
fn rect_coverage(width: f32, height: f32, t: Transform, inv: Inverse, px: u32, py: u32) -> f32 {
    // Rotated or sheared, the mapped rect is no longer axis-aligned and the
    // area is no longer a product of two 1-D overlaps, so fall back to
    // sampling the pixel.
    if t.b.abs() > 1e-6 || t.c.abs() > 1e-6 {
        const N: u32 = 4;
        let hits = (0..N * N)
            .filter(|k| {
                let (i, j) = (k % N, k / N);
                let (x, y) = inv.at(
                    px as f32 + (i as f32 + 0.5) / N as f32,
                    py as f32 + (j as f32 + 0.5) / N as f32,
                );
                x >= 0.0 && y >= 0.0 && x < width && y < height
            })
            .count();
        return hits as f32 / (N * N) as f32;
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
    stroke_widths: &[f32],
    t: Transform,
    inv: Inverse,
    px: u32,
    py: u32,
) -> f32 {
    const N: u32 = 4;
    // An axis-aligned rect fill has an exact answer, so take it. Rect fills
    // cover the largest areas, and this is both cheaper than sampling and
    // not an approximation of it.
    // Only a square-cornered one: the exact answer is a product of two 1-D
    // overlaps, which a rounded corner is not.
    if let (
        VectorShape::Rect {
            width,
            height,
            radius,
        },
        None,
    ) = (shape, stroke_width)
    {
        if *radius <= 0.0 {
            return rect_coverage(*width, *height, t, inv, px, py);
        }
    }
    let covers = |sx: f32, sy: f32| {
        let (x, y) = inv.at(sx, sy);
        match stroke_width {
            None => shape_covers(shape, x, y),
            Some(sw) => stroke_covers(shape, sw, stroke_widths, x, y),
        }
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
    rings: &[&[[f32; 2]]],
    t: Transform,
    inv: Inverse,
    paint: &Paint,
    mode: BlendMode,
    bbox: ClipRect,
    mask: MaskRef<'_>,
) {
    const N: u32 = 4;
    if bbox.is_empty() {
        return;
    }
    // Map the rings into device space once, then scan there. Doing it the
    // other way — scanning in local space — only works while the transform
    // keeps device rows parallel to local rows, which rotation does not.
    let polys: Vec<Vec<(f32, f32)>> = rings
        .iter()
        .filter(|r| r.len() >= 3)
        .map(|r| r.iter().map(|p| to_device(t, p[0], p[1])).collect())
        .collect();
    if polys.is_empty() {
        return;
    }
    let width = (bbox.x1 - bbox.x0) as usize;
    let mut cov = vec![0f32; width];
    let mut xs: Vec<f32> = Vec::new();
    for py in bbox.y0..bbox.y1 {
        cov.fill(0.0);
        for j in 0..N {
            // Sample y at the middle of each sub-row, never on its edge.
            let sy = py as f32 + (j as f32 + 0.5) / N as f32;
            xs.clear();
            // Crossings from every ring go into one sorted list, which is
            // exactly what makes the fill even-odd across all of them.
            for poly in &polys {
                for i in 0..poly.len() {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    if (a.1 > sy) != (b.1 > sy) {
                        let s = (sy - a.1) / (b.1 - a.1);
                        xs.push(a.0 + s * (b.0 - a.0));
                    }
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
            let (lx, ly) = inv.at(px as f32 + 0.5, py as f32 + 0.5);
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
    stroke_widths: &[f32],
    mask: MaskRef<'_>,
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
        let pad = (sw * max_scale(t)).ceil() as u32 + 1;
        bbox = ClipRect {
            x0: bbox.x0.saturating_sub(pad),
            y0: bbox.y0.saturating_sub(pad),
            x1: (bbox.x1 + pad).min(dst.width),
            y1: (bbox.y1 + pad).min(dst.height),
        }
        .intersect(clip);
    }
    // A degenerate transform maps nothing; bail before walking the bbox.
    // Inverting once here is what keeps the loops below free of it.
    let Some(inv) = Inverse::of(t) else {
        return;
    };
    // Path fills go through the scanline rasterizer; strokes stay on the
    // sampler, whose distance test has no scanline form.
    if let (
        VectorShape::Path {
            points, subpaths, ..
        },
        None,
    ) = (shape, stroke_width)
    {
        let rings: Vec<&[[f32; 2]]> = std::iter::once(points.as_slice())
            .chain(subpaths.iter().map(|r| r.as_slice()))
            .collect();
        fill_path_scanlines(dst, doc, &rings, t, inv, paint, mode, bbox, mask);
        return;
    }
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            let a = pixel_coverage(shape, stroke_width, stroke_widths, t, inv, px, py);
            if a <= 0.0 {
                continue;
            }
            let c = a * coverage_at(doc, mask, px, py);
            if c <= 0.0 {
                continue;
            }
            let (lx, ly) = inv.at(px as f32 + 0.5, py as f32 + 0.5);
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
    mask: MaskRef<'_>,
) {
    // 8-bit sRGB → linear lookup table, built per blit (256 entries, cheap).
    let mut lut = [0f32; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        *out = chitrakar_color::srgb_to_linear(v as f32 / 255.0);
    }
    let bbox = draw_bbox(t, res.width as f32, res.height as f32, dst, clip);
    let Some(inv) = Inverse::of(t) else {
        return;
    };
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
    // One bilinear sample takes the four texels around a point and ignores
    // the rest, which is right when the image is being enlarged and wrong
    // when it is being shrunk: at a third of its size, two texels in three
    // never contribute and the picture crawls as it moves. So when one
    // device pixel spans more than one texel, average a grid across the
    // texels it really covers. The footprint is the inverse's own column
    // sums — how far in the source a step of one device pixel goes.
    let foot_x = (inv.a.abs() + inv.c.abs()).max(1.0);
    let foot_y = (inv.b.abs() + inv.d.abs()).max(1.0);
    // Capped: past four samples an axis the picture is already settled and
    // the cost is not.
    let taps_x = (foot_x.ceil() as u32).clamp(1, 4);
    let taps_y = (foot_y.ceil() as u32).clamp(1, 4);
    let averaging = taps_x > 1 || taps_y > 1;
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            // The image is a rect in local space, so its outline gets the
            // same exact coverage a rect fill does.
            let edge = rect_coverage(res.width as f32, res.height as f32, t, inv, px, py);
            if edge <= 0.0 {
                continue;
            }
            let cov = coverage_at(doc, mask, px, py) * edge * opacity;
            if cov <= 0.0 {
                continue;
            }
            let (lx, ly) = inv.at(px as f32 + 0.5, py as f32 + 0.5);
            let cx = |i: f32| i.clamp(0.0, last_x) as u32;
            let cy = |i: f32| i.clamp(0.0, last_y) as u32;
            // Texel centres sit at (i + 0.5), so a sample at (sx, sy) lands
            // between the four texels around (sx - 0.5, sy - 0.5).
            let bilinear = |sx: f32, sy: f32| {
                let (u, v) = (sx - 0.5, sy - 0.5);
                let (u0, v0) = (u.floor(), v.floor());
                let (fx, fy) = (u - u0, v - v0);
                let (x0, x1) = (cx(u0), cx(u0 + 1.0));
                let (y0, y1) = (cy(v0), cy(v0 + 1.0));
                let top = lerp(texel(x0, y0), texel(x1, y0), fx);
                let bottom = lerp(texel(x0, y1), texel(x1, y1), fx);
                lerp(top, bottom, fy)
            };
            let sampled = if averaging {
                let mut acc = LinearRgba::TRANSPARENT;
                for j in 0..taps_y {
                    for i in 0..taps_x {
                        let ox = (i as f32 + 0.5) / taps_x as f32 - 0.5;
                        let oy = (j as f32 + 0.5) / taps_y as f32 - 0.5;
                        let s = bilinear(lx + ox * foot_x, ly + oy * foot_y);
                        acc = LinearRgba {
                            r: acc.r + s.r,
                            g: acc.g + s.g,
                            b: acc.b + s.b,
                            a: acc.a + s.a,
                        };
                    }
                }
                scale_alpha(acc, 1.0 / (taps_x * taps_y) as f32)
            } else {
                bilinear(lx, ly)
            };
            let src = scale_alpha(sampled, cov);
            let i = (py * dst.width + px) as usize;
            dst.pixels[i] = blend_pixel(src, dst.pixels[i], mode);
        }
    }
}

/// Mask coverage at a pixel center, in document space. 1.0 without a mask.
/// A mask together with the space its transform is written in — its owner's
/// parent space, since a mask is authored against the document as the layer
/// sees it. Carried as one value so every painter keeps a single mask
/// argument instead of a second, easily mismatched, transform.
#[derive(Clone, Copy)]
struct MaskRef<'a> {
    mask: Option<&'a Mask>,
    /// The mask's own transform already composed into device space, with
    /// its inverse solved. Both were being recomputed at every pixel of
    /// every masked layer; neither varies across the region.
    t: Transform,
    inv: Option<Inverse>,
}

impl<'a> MaskRef<'a> {
    /// `parent` is the space the mask is authored in — its owner's parent,
    /// since a mask describes the document as the layer sees it.
    fn new(mask: Option<&'a Mask>, parent: Transform) -> MaskRef<'a> {
        let t = match mask.map(|m| &m.kind) {
            Some(MaskKind::Vector { transform, .. } | MaskKind::Raster { transform, .. }) => {
                parent.compose(*transform)
            }
            None => parent,
        };
        MaskRef {
            mask,
            t,
            inv: Inverse::of(t),
        }
    }
}

fn coverage_at(doc: &Document, m: MaskRef<'_>, x: u32, y: u32) -> f32 {
    let Some(mask) = m.mask else {
        return 1.0;
    };
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let c = match &mask.kind {
        MaskKind::Vector { shape, .. } => match m.inv {
            Some(inv) => pixel_coverage(shape, None, &[], m.t, inv, x, y),
            None => 0.0,
        },
        MaskKind::Raster { resource_id, .. } => {
            match (doc.resource(resource_id), m.inv.map(|inv| inv.at(fx, fy))) {
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
            }
        }
    };
    if mask.invert {
        1.0 - c
    } else {
        c
    }
}

/// Multiply a surface region by a mask's coverage (used for group masks).
fn apply_mask(
    doc: &Document,
    mask: &Mask,
    surface: &mut Surface,
    clip: ClipRect,
    parent: Transform,
) {
    let m = MaskRef::new(Some(mask), parent);
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let c = coverage_at(doc, m, x, y);
            if c < 1.0 {
                let i = (y * surface.width + x) as usize;
                surface.pixels[i] = scale_alpha(surface.pixels[i], c);
            }
        }
    }
}

/// Run a filter layer over the accumulated composite below it, weighted by
/// the layer's opacity and mask coverage.
#[allow(clippy::too_many_arguments)]
fn apply_filter(
    doc: &Document,
    filter: &Filter,
    opacity: f32,
    mask: MaskRef<'_>,
    dst: &mut Surface,
    clip: ClipRect,
    scale: f32,
) {
    match filter {
        Filter::GaussianBlur { sigma } => {
            let needs_mix = opacity < 1.0 || mask.mask.is_some();
            let original = needs_mix.then(|| blur::snapshot(dst, clip));
            blur::gaussian_blur(dst, clip, *sigma * scale);
            if let Some(orig) = original {
                mix_snapshot(dst, clip, &orig, |o, f, x, y| {
                    lerp(o, f, opacity * coverage_at(doc, mask, x, y))
                });
            }
        }
        Filter::Sharpen { sigma, amount } => {
            let original = blur::snapshot(dst, clip);
            blur::gaussian_blur(dst, clip, *sigma * scale);
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
    mask: MaskRef<'_>,
) {
    let Some(inv) = Inverse::of(t) else {
        return;
    };
    // Rasterize at the size the text will be seen at, so magnifying a text
    // layer sharpens the outlines instead of enlarging their pixels. The
    // cap keeps a wildly zoomed layer from asking for an enormous bitmap;
    // past it the glyphs are already far finer than the screen.
    let natural = text::measure(spec);
    let ceiling = (8192.0 / natural.0.max(natural.1).max(1.0)).min(64.0);
    let scale = max_scale(t).clamp(0.02, ceiling.max(0.02));
    let raster = text::rasterize_at(spec, scale);
    let color = resolve_color(doc, spec.fill);
    // The box is the block's natural size, not the raster's: those agree
    // only while the raster is at natural scale, and a minified one would
    // otherwise clip its own right and bottom edges away.
    let bbox = match transformed_local_bounds(t, (0.0, 0.0, natural.0, natural.1))
        .to_clip(dst.width, dst.height)
    {
        Some(b) => b.intersect(clip),
        None => return,
    };
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            let (lx, ly) = inv.at(px as f32 + 0.5, py as f32 + 0.5);
            if lx < 0.0 || ly < 0.0 {
                continue;
            }
            let c = raster.sample_at(lx * scale, ly * scale);
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
    hit_in_group(doc, doc.root(), x, y, Transform::default())
}

fn hit_in_group(
    doc: &Document,
    group: NodeId,
    x: f32,
    y: f32,
    parent: Transform,
) -> Result<Option<NodeId>, DocError> {
    for &child in doc.children_of(group)?.iter().rev() {
        let node = doc.node(child)?;
        if !node.visible {
            continue;
        }
        match &node.kind {
            NodeKind::Group => {
                if let Some(hit) = hit_in_group(doc, child, x, y, parent.compose(node.transform))? {
                    return Ok(Some(hit));
                }
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                gradient,
            } => {
                if let Some((lx, ly)) = to_local(parent.compose(node.transform), x, y) {
                    let flat = flatten_shape(shape);
                    let shape = flat.as_ref();
                    let hit = if fill.is_some() || gradient.is_some() {
                        shape_covers(shape, lx, ly)
                    } else if let Some(s) = stroke {
                        stroke_covers(shape, s.width, &flatten_widths(shape, &s.widths), lx, ly)
                    } else {
                        false
                    };
                    if hit {
                        return Ok(Some(child));
                    }
                }
            }
            NodeKind::Raster(raster) => {
                if let Some((lx, ly)) = to_local(parent.compose(node.transform), x, y) {
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
                if let Some((lx, ly)) = to_local(parent.compose(node.transform), x, y) {
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
                radius: 0.0,
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
    fn a4_timing_probe() {
        // A4 at 300dpi, the largest preset the app offers.
        let mut doc = Document::new(2480, 3508, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("bg", 2480.0, 3508.0, RED),
        })
        .unwrap();
        let mut e = Node::vector(
            "e",
            VectorShape::Ellipse {
                rx: 900.0,
                ry: 900.0,
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
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            render(&doc).unwrap();
        }
        println!("TIMING A4 300dpi (rect + ellipse): {:?}", t0.elapsed() / 3);
    }

    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn a4_group_probe() {
        let mut doc = Document::new(2480, 3508, ColorMode::Rgb);
        let root = doc.root();
        let g = Node::group("g");
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(g),
        })
        .unwrap();
        let gid = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: gid,
            index: 0,
            node: filled_rect("r", 100.0, 100.0, RED),
        })
        .unwrap();
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            render(&doc).unwrap();
        }
        println!("GROUP one group, tiny child: {:?}", t0.elapsed() / 3);
    }

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
                subpaths: Vec::new(),
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
                subpaths: Vec::new(),
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
                    radius: 0.0,
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
            subpaths: Vec::new(),
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
            subpaths: Vec::new(),
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
                    subpaths: Vec::new(),
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

    /// A rotation by `deg` about the local origin, then a translation.
    fn rotation(deg: f32, tx: f32, ty: f32) -> Transform {
        let r = deg.to_radians();
        Transform {
            a: r.cos(),
            b: r.sin(),
            c: -r.sin(),
            d: r.cos(),
            e: tx,
            f: ty,
        }
    }

    #[test]
    fn a_rotated_rect_paints_where_it_was_turned_to() {
        // 45 degrees turns a square into a diamond: its corners reach the
        // horizontal extremes and the old corners come off the canvas edge.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 20.0, 20.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: rotation(45.0, 32.0, 18.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        // The diamond's centre sits a half-diagonal below the pivot.
        let mid = 18.0 + 20.0 * std::f32::consts::SQRT_2 / 2.0;
        assert_eq!(s.get(32, mid as u32).a, 1.0, "inside the rotated shape");
        // Just outside the pivot corner, where an unrotated rect would be.
        assert_eq!(s.get(36, 20).a, 0.0, "the unrotated position is empty");
        assert_eq!(
            hit_test(&doc, 32.0, mid).unwrap(),
            Some(id),
            "hit testing follows the rotation"
        );
        assert_eq!(hit_test(&doc, 36.0, 20.0).unwrap(), None);

        // Bounds have to contain the turned shape, or incremental rendering
        // would leave parts of it stale.
        match node_bounds(&doc, id).unwrap() {
            Bounds::Rect(x0, y0, x1, y1) => {
                let half = 20.0 * std::f32::consts::SQRT_2;
                assert!(
                    x1 - x0 > half - 0.5 && y1 - y0 > half - 0.5,
                    "bounds must span the diagonal, got {:?}",
                    (x0, y0, x1, y1)
                );
            }
            other => panic!("expected a rect, got {other:?}"),
        }
    }

    #[test]
    fn a_rotated_path_fills_through_the_scanline_rasterizer() {
        // Paths take a different code path from rects, and it scans device
        // rows — so it has to map the polygon rather than the scanline.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::vector(
            "tri",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [24.0, 0.0], [24.0, 24.0], [0.0, 24.0]],
                closed: true,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
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

        // Unrotated: a square with its top-left at (20, 20).
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(20.0, 20.0),
        })
        .unwrap();
        let flat = render(&doc).unwrap();
        assert_eq!(flat.get(22, 22).a, 1.0);

        // Rotated 45 degrees about the same pivot, that corner is now empty
        // and the shape reaches straight down instead.
        doc.apply(Command::SetTransform {
            id,
            transform: rotation(45.0, 20.0, 20.0),
        })
        .unwrap();
        let turned = render(&doc).unwrap();
        // Clear of the diamond: (22,22) would sit exactly on the rotated
        // edge running down-right from the pivot, and read half covered.
        assert_eq!(turned.get(35, 22).a, 0.0, "the old corner is vacated");
        assert_eq!(
            turned.get(20, 20 + 12).a,
            1.0,
            "and the shape now runs down from the pivot"
        );
        // The area is unchanged by a rotation, give or take the edge pixels.
        let area = |s: &Surface| -> f32 {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .map(|(x, y)| s.get(x, y).a)
                .sum()
        };
        let (before, after) = (area(&flat), area(&turned));
        assert!(
            (before - after).abs() < before * 0.05,
            "rotation preserves area: {before} vs {after}"
        );
    }

    #[test]
    fn region_render_matches_full_render_when_rotated() {
        // The equivalence incremental rendering rests on, with a transform
        // whose device rows are not local rows.
        let mut doc = Document::new(48, 48, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 20.0, 12.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: rotation(30.0, 20.0, 14.0),
        })
        .unwrap();

        let full = render(&doc).unwrap();
        let mut patched = render(&doc).unwrap();
        let clip = ClipRect {
            x0: 8,
            y0: 8,
            x1: 40,
            y1: 40,
        };
        for y in clip.y0..clip.y1 {
            for x in clip.x0..clip.x1 {
                patched.pixels[(y * 48 + x) as usize] = LinearRgba {
                    r: 9.0,
                    g: 9.0,
                    b: 9.0,
                    a: 1.0,
                };
            }
        }
        render_region(&doc, &mut patched, clip).unwrap();
        for y in 0..48 {
            for x in 0..48 {
                assert_eq!(patched.get(x, y), full.get(x, y), "pixel ({x},{y})");
            }
        }
    }

    /// Build `doc` with two overlapping half-transparent rects, either as
    /// siblings at the root or wrapped in a group, so the two arrangements
    /// can be compared pixel for pixel.
    fn two_overlapping_rects(grouped: bool) -> Document {
        const HALF_BLUE: AuthoredColor = AuthoredColor::Srgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 0.5,
        };
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        let parent = if grouped {
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("g")),
            })
            .unwrap();
            doc.children_of(root).unwrap()[0]
        } else {
            root
        };
        for (i, (color, dx)) in [(RED, 2.0), (HALF_BLUE, 8.0)].into_iter().enumerate() {
            doc.apply(Command::AddNode {
                parent,
                index: i,
                node: filled_rect(&format!("r{i}"), 14.0, 14.0, color),
            })
            .unwrap();
            let id = doc.children_of(parent).unwrap()[i];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(dx, 4.0),
            })
            .unwrap();
        }
        doc
    }

    /// Render a one-line text layer of `size`, magnified by `zoom`.
    fn text_at(size: f32, zoom: f32) -> Surface {
        let mut doc = Document::new(96, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::text(
                "t",
                chitrakar_doc::TextSpec::new("Ag", size, RED),
            )),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform {
                a: zoom,
                d: zoom,
                ..Default::default()
            },
        })
        .unwrap();
        render(&doc).unwrap()
    }

    #[test]
    fn magnified_text_is_rasterized_at_the_size_it_is_seen_at() {
        // Type is outlines, so scaling a text layer up must re-rasterize
        // the outlines, not enlarge the pixels of a natural-size bitmap.
        // Small-and-magnified therefore has to land on the same pixels as
        // large-and-unmagnified; blowing up a bitmap would not.
        let magnified = text_at(8.0, 8.0);
        let native = text_at(64.0, 1.0);
        let n = magnified.pixels.len();
        let diff: f32 = (0..n)
            .map(|i| (magnified.pixels[i].a - native.pixels[i].a).abs())
            .sum();
        let ink: f32 = native.pixels.iter().map(|p| p.a).sum();
        assert!(ink > 20.0, "the reference actually drew something: {ink}");
        assert!(
            diff < ink * 0.1,
            "magnified text differs from native by {diff} over {ink} of ink"
        );
    }

    #[test]
    fn a_drop_shadow_falls_where_it_is_aimed_and_leaves_the_layer_alone() {
        // The shadow is the layer's silhouette in one colour, offset and
        // blurred, painted behind it: so it darkens the offset side, the
        // layer's own pixels are untouched, and nothing lands on the side
        // it was aimed away from.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 20.0, 20.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(20.0, 20.0),
        })
        .unwrap();
        let plain = render(&doc).unwrap();
        assert_eq!(
            plain.get(42, 42).a,
            0.0,
            "nothing below-right to begin with"
        );

        doc.apply(Command::SetEffects {
            id,
            effects: vec![chitrakar_doc::Effect::DropShadow {
                dx: 6.0,
                dy: 6.0,
                blur: 2.0,
                color: AuthoredColor::Srgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                opacity: 0.8,
            }],
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(30, 30),
            plain.get(30, 30),
            "the layer's own pixels are untouched"
        );
        let shadowed = s.get(42, 42);
        assert!(
            shadowed.a > 0.5 && shadowed.r < 0.05,
            "the shadow lands below-right, dark: {shadowed:?}"
        );
        assert_eq!(
            s.get(14, 14).a,
            0.0,
            "and nothing falls on the side it points away from"
        );
        // It reaches outside the layer, so what must be repainted grows —
        // but the layer is still the size it was, and the panel, the
        // selection outline and alignment all go by that.
        match node_bounds(&doc, id).unwrap() {
            Bounds::Rect(_, _, x1, y1) => {
                assert!(x1 > 46.0 && y1 > 46.0, "the repaint region grew");
            }
            other => panic!("expected a rect, got {other:?}"),
        }
        assert_eq!(
            node_visual_bounds(&doc, id).unwrap(),
            Bounds::Rect(20.0, 20.0, 40.0, 40.0),
            "the layer itself is not one pixel wider for having a shadow"
        );
    }

    /// A 20x20 red square at (20,20) on a 64x64 canvas, with `effects`.
    fn square_with(effects: Vec<chitrakar_doc::Effect>) -> Surface {
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 20.0, 20.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(20.0, 20.0),
        })
        .unwrap();
        doc.apply(Command::SetEffects { id, effects }).unwrap();
        render(&doc).unwrap()
    }

    const BLACK: AuthoredColor = AuthoredColor::Srgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    #[test]
    fn a_corner_radius_rounds_the_rectangle_off() {
        // The corner is cut away, the sides stay flush, and the cut
        // follows a circle rather than a chamfer.
        let build = |radius: f32| {
            let mut doc = Document::new(40, 40, ColorMode::Rgb);
            let root = doc.root();
            let mut node = Node::vector(
                "r",
                VectorShape::Rect {
                    width: 30.0,
                    height: 30.0,
                    radius,
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
            doc
        };
        let square = render(&build(0.0)).unwrap();
        assert_eq!(square.get(1, 1).a, 1.0, "square-cornered to begin with");

        let round = render(&build(10.0)).unwrap();
        assert_eq!(round.get(1, 1).a, 0.0, "the corner is cut away");
        assert_eq!(round.get(15, 1).a, 1.0, "the middle of the top edge stays");
        assert_eq!(round.get(1, 15).a, 1.0, "and so does the left edge");
        assert_eq!(round.get(15, 15).a, 1.0, "and the inside");
        // On the corner circle: the radius runs from (10,10) outwards, so
        // 10 - 10/sqrt2 ~ 2.93 along the diagonal is just inside it.
        assert_eq!(round.get(3, 3).a, 1.0, "the cut follows a circle");
        assert_eq!(round.get(2, 2).a, 0.0, "just outside it, nothing");
        // All four corners, and a bigger radius cuts more.
        for (x, y) in [(38, 1), (1, 38), (38, 38)] {
            assert_eq!(round.get(x, y).a, 0.0, "corner ({x},{y}) is rounded too");
        }
        let capsule = render(&build(999.0)).unwrap();
        assert_eq!(capsule.get(15, 15).a, 1.0, "an absurd radius still draws");
        assert_eq!(
            capsule.get(3, 3).a,
            0.0,
            "clamped to a circle, not inverted"
        );
        assert_eq!(capsule.get(1, 15).a, 1.0, "which still reaches the sides");
    }

    #[test]
    fn a_backwards_rectangle_renders_rather_than_panicking() {
        // Nothing in the editor makes a negative-sized rect, but a file
        // can carry one, and clamping a radius into a range that runs
        // backwards is a panic rather than a wrong picture.
        let mut doc = Document::new(16, 16, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::vector(
            "r",
            VectorShape::Rect {
                width: -20.0,
                height: 10.0,
                radius: 4.0,
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
        render(&doc).unwrap();
        assert!(hit_test(&doc, 1.0, 1.0).is_ok());
        assert!(
            shape_rings(match &doc.node(id).unwrap().kind {
                NodeKind::Vector { shape, .. } => shape,
                _ => unreachable!(),
            })
            .len()
                <= 1
        );
    }

    #[test]
    fn a_rounded_rectangle_strokes_round_the_corner() {
        // The inner band follows the rounded edge: present just inside the
        // curve, absent in the middle.
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::vector(
            "r",
            VectorShape::Rect {
                width: 30.0,
                height: 30.0,
                radius: 10.0,
            },
        );
        if let NodeKind::Vector { fill, stroke, .. } = &mut node.kind {
            *fill = None;
            *stroke = Some(chitrakar_doc::Stroke {
                color: RED,
                width: 3.0,
                widths: Vec::new(),
            });
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert!(s.get(4, 4).a > 0.5, "the band turns the corner");
        assert_eq!(s.get(15, 15).a, 0.0, "and leaves the middle empty");
        assert!(s.get(15, 1).a > 0.5, "the straight edges still carry it");
    }

    #[test]
    fn an_outline_hugs_the_shape_to_the_width_it_is_given() {
        // The band reaches the width asked for and stops: solid just
        // outside the edge, gone just past the width, and the layer itself
        // untouched underneath it.
        let s = square_with(vec![chitrakar_doc::Effect::Outline {
            width: 5.0,
            color: BLACK,
            opacity: 1.0,
        }]);
        assert_eq!(
            s.get(30, 30),
            square_with(vec![]).get(30, 30),
            "layer intact"
        );
        let just_out = s.get(17, 30);
        assert!(
            just_out.a > 0.9 && just_out.r < 0.05,
            "solid outline three pixels out: {just_out:?}"
        );
        assert!(
            s.get(15, 30).a > 0.9,
            "still solid at the far edge of the band"
        );
        assert_eq!(s.get(14, 30).a, 0.0, "and nothing beyond it");
        // It goes all the way round.
        assert!(s.get(30, 15).a > 0.9 && s.get(44, 30).a > 0.9 && s.get(30, 44).a > 0.9);
        // The corner is rounded rather than squared off: five pixels out
        // along a diagonal is further than five pixels out sideways.
        assert_eq!(
            s.get(15, 15).a,
            0.0,
            "the band turns the corner instead of filling it"
        );
        // Wider means wider.
        let wide = square_with(vec![chitrakar_doc::Effect::Outline {
            width: 10.0,
            color: BLACK,
            opacity: 1.0,
        }]);
        assert!(
            wide.get(13, 30).a > 0.9,
            "ten pixels reaches where five did not"
        );
    }

    #[test]
    fn a_filter_reaches_as_far_as_the_space_it_sits_in_stretches_it() {
        // The sigma is written in the filter's own space, so a group
        // scaled up carries the blur with it — and the padding a region
        // render needs has to say so, or it leaves a stale ring behind.
        let build = |scale: f32| {
            let mut doc = Document::new(64, 64, ColorMode::Rgb);
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("g")),
            })
            .unwrap();
            let g = doc.children_of(root).unwrap()[0];
            doc.apply(Command::SetTransform {
                id: g,
                transform: Transform {
                    a: scale,
                    d: scale,
                    ..Default::default()
                },
            })
            .unwrap();
            doc.apply(Command::AddNode {
                parent: g,
                index: 0,
                node: Box::new(Node::filter("blur", Filter::GaussianBlur { sigma: 4.0 })),
            })
            .unwrap();
            filter_reach(&doc)
        };
        let plain = build(1.0);
        assert!(plain >= 14, "a sigma of four reaches about twelve: {plain}");
        let scaled = build(3.0);
        assert!(
            scaled >= plain * 5 / 2,
            "tripling the space it sits in roughly triples its reach: {plain} -> {scaled}"
        );
    }

    #[test]
    fn an_outline_survives_a_faint_layer() {
        // The layer is staged with its own opacity already applied, so the
        // edge is at half of that rather than at half of one. Take that
        // for granted and a layer under half opacity has no inside at all,
        // and casts no outline.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 20.0, 20.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(20.0, 20.0),
        })
        .unwrap();
        doc.apply(Command::SetOpacity { id, opacity: 0.3 }).unwrap();
        doc.apply(Command::SetEffects {
            id,
            effects: vec![chitrakar_doc::Effect::Outline {
                width: 4.0,
                color: BLACK,
                opacity: 1.0,
            }],
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert!(
            s.get(18, 30).a > 0.9,
            "a faint layer still gets a solid outline: {:?}",
            s.get(18, 30)
        );
        assert_eq!(s.get(14, 30).a, 0.0, "and it still stops at its width");
    }

    #[test]
    fn an_inner_shadow_stays_inside_the_shape() {
        // Cast from the hole around the layer and kept to the silhouette:
        // it darkens the inside of the edge it is aimed from, leaves the
        // middle alone, and never spills outside.
        let plain = square_with(vec![]);
        let s = square_with(vec![chitrakar_doc::Effect::InnerShadow {
            dx: 4.0,
            dy: 4.0,
            blur: 2.0,
            color: BLACK,
            opacity: 1.0,
        }]);
        for (x, y) in [(18, 30), (30, 18), (42, 30), (30, 42)] {
            assert_eq!(s.get(x, y), plain.get(x, y), "nothing outside at ({x},{y})");
        }
        let shaded = s.get(22, 22);
        assert!(
            shaded.r < 0.5 && shaded.a >= 1.0,
            "the top-left inside edge is darkened but still opaque: {shaded:?}"
        );
        assert_eq!(
            s.get(30, 30),
            plain.get(30, 30),
            "the middle is out of its reach"
        );
        let away = s.get(38, 38);
        assert!(
            (away.r - plain.get(38, 38).r).abs() < 0.1 && away.r > shaded.r * 2.0,
            "and the side it is aimed away from is barely touched: {away:?}"
        );
    }

    #[test]
    fn effects_stack_in_order_around_the_layer() {
        // An outline goes behind, an inner shadow on top: with both, the
        // band still shows outside and the shading still shows inside.
        let both = square_with(vec![
            chitrakar_doc::Effect::Outline {
                width: 4.0,
                color: BLACK,
                opacity: 1.0,
            },
            chitrakar_doc::Effect::InnerShadow {
                dx: 4.0,
                dy: 4.0,
                blur: 2.0,
                color: BLACK,
                opacity: 1.0,
            },
        ]);
        assert!(both.get(18, 30).a > 0.9, "the outline is there");
        assert!(both.get(22, 22).r < 0.5, "and so is the inner shadow");
        assert_eq!(both.get(30, 30), square_with(vec![]).get(30, 30));
    }

    #[test]
    fn a_shadow_is_cast_by_the_silhouette_not_the_colours() {
        // Two layers of different colours cast the same shadow, and a
        // half-transparent layer casts a fainter one — that is what makes
        // it a shadow rather than a tinted copy.
        let shadow_at = |color: AuthoredColor, opacity: f32| {
            let mut doc = Document::new(64, 64, ColorMode::Rgb);
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: filled_rect("r", 20.0, 20.0, color),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[0];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(20.0, 20.0),
            })
            .unwrap();
            doc.apply(Command::SetOpacity { id, opacity }).unwrap();
            doc.apply(Command::SetEffects {
                id,
                effects: vec![chitrakar_doc::Effect::DropShadow {
                    dx: 8.0,
                    dy: 8.0,
                    blur: 0.0,
                    color: AuthoredColor::Srgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    opacity: 1.0,
                }],
            })
            .unwrap();
            render(&doc).unwrap().get(46, 46)
        };
        let red = shadow_at(RED, 1.0);
        let blue = shadow_at(
            AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            },
            1.0,
        );
        assert_eq!(red, blue, "colour does not change the silhouette");
        let faint = shadow_at(RED, 0.4);
        assert!(
            faint.a < red.a * 0.6 && faint.a > 0.0,
            "a fainter layer casts a fainter shadow: {} vs {}",
            faint.a,
            red.a
        );
    }

    #[test]
    fn a_plain_group_composites_exactly_as_its_children_would() {
        // Wrapping shapes in a folder must not change a single pixel: the
        // renderer skips the group's isolation surface when nothing inside
        // reads the backdrop, and this is what says that shortcut is
        // faithful rather than merely fast.
        let flat = render(&two_overlapping_rects(false)).unwrap();
        let grouped = render(&two_overlapping_rects(true)).unwrap();
        assert_eq!(flat.pixels, grouped.pixels);
    }

    #[test]
    fn a_group_still_isolates_a_child_that_reads_the_backdrop() {
        // Multiply inside a group sees only the group's own contents, not
        // the document underneath — that is what a group being an isolation
        // group means, and it is the case the shortcut above must decline.
        const GREY: AuthoredColor = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let mut doc = Document::new(32, 32, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("backdrop", 32.0, 32.0, GREY),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[1];
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: filled_rect("m", 16.0, 16.0, RED),
        })
        .unwrap();
        let child = doc.children_of(group).unwrap()[0];
        doc.apply(Command::SetBlendMode {
            id: child,
            blend: BlendMode::Multiply,
        })
        .unwrap();

        let s = render(&doc).unwrap();
        // Isolated, the child multiplies against nothing and simply lands
        // on top at full red. Were the group flattened away it would
        // multiply into the grey below and come out at about a fifth of
        // that (grey is 0.5 sRGB, ~0.214 linear).
        let inside = s.get(8, 8);
        assert!((inside.r - 1.0).abs() < 1e-4, "{inside:?}");
        assert!(inside.g.abs() < 1e-4 && inside.b.abs() < 1e-4, "{inside:?}");
        assert!((inside.a - 1.0).abs() < 1e-4, "{inside:?}");
        assert!(s.get(24, 24).r < 0.3, "backdrop still shows beside it");
    }

    #[test]
    fn a_group_leaves_the_canvas_outside_its_bounds_alone() {
        // The group's passes are clipped to where it can land. A shape
        // painted under it, far away, must survive that untouched.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("under", 8.0, 8.0, RED),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[1];
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: filled_rect("in", 8.0, 8.0, RED),
        })
        .unwrap();
        // Half-opaque group, so it takes the isolation path.
        doc.apply(Command::SetOpacity {
            id: group,
            opacity: 0.5,
        })
        .unwrap();
        doc.apply(Command::SetTransform {
            id: group,
            transform: Transform::translation(40.0, 40.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(4, 4).a, 1.0, "the shape below is untouched");
        assert!((s.get(44, 44).a - 0.5).abs() < 1e-4, "the group is halved");
        assert_eq!(s.get(20, 20).a, 0.0, "and nothing leaked in between");
    }

    #[test]
    fn a_group_transform_moves_and_turns_its_children() {
        // A group's transform applies to everything inside it, so grouping
        // two shapes and moving the group moves both — and the children's
        // own transforms stay untouched, which is what makes it undoable
        // and what ungrouping relies on.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: filled_rect("a", 8.0, 8.0, RED),
        })
        .unwrap();
        let child = doc.children_of(group).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: child,
            transform: Transform::translation(4.0, 4.0),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        assert_eq!(s.get(6, 6).a, 1.0, "child paints at its own position");
        assert_eq!(s.get(30, 30).a, 0.0);
        assert_eq!(hit_test(&doc, 6.0, 6.0).unwrap(), Some(child));

        // Move the group: the child moves with it.
        doc.apply(Command::SetTransform {
            id: group,
            transform: Transform::translation(24.0, 24.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(6, 6).a, 0.0, "the old position is vacated");
        assert_eq!(s.get(30, 30).a, 1.0, "and the child moved with the group");
        assert_eq!(
            hit_test(&doc, 30.0, 30.0).unwrap(),
            Some(child),
            "hit testing composes down the tree too"
        );
        match node_bounds(&doc, group).unwrap() {
            Bounds::Rect(x0, y0, _, _) => {
                assert!(
                    (x0 - 28.0).abs() < 0.5 && (y0 - 28.0).abs() < 0.5,
                    "group bounds follow its transform, got {:?}",
                    (x0, y0)
                );
            }
            other => panic!("expected a rect, got {other:?}"),
        }

        // Turning the group turns its contents.
        doc.apply(Command::SetTransform {
            id: group,
            transform: rotation(90.0, 24.0, 24.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(30, 30).a, 0.0, "a turned group moves its child");
        assert_eq!(
            s.get(24 - 6, 24 + 6).a,
            1.0,
            "the child swung a quarter turn about the group's origin"
        );
    }

    #[test]
    fn a_mask_travels_with_its_layer_inside_a_moved_group() {
        // A mask is authored in the space its layer lives in, so an
        // ancestor's transform has to reach it too. Miss that and the layer
        // slides out from under its own mask when the group moves.
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: filled_rect("r", 16.0, 16.0, RED),
        })
        .unwrap();
        let child = doc.children_of(group).unwrap()[0];
        doc.apply(Command::SetMask {
            id: child,
            // Covers the left half of the rect only.
            mask: Some(Box::new(ellipse_mask(0.0, 8.0, 8.0, 8.0, false))),
        })
        .unwrap();

        let inside = render(&doc).unwrap().get(4, 8).a;
        let outside = render(&doc).unwrap().get(13, 8).a;
        assert!(inside > 0.5 && outside < 0.5, "mask cuts the rect in half");

        doc.apply(Command::SetTransform {
            id: group,
            transform: Transform::translation(30.0, 20.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert!(
            (s.get(34, 28).a - inside).abs() < 0.05,
            "the masked half moved with the group, got {}",
            s.get(34, 28).a
        );
        assert!(
            (s.get(43, 28).a - outside).abs() < 0.05,
            "and the cut-away half stayed cut away, got {}",
            s.get(43, 28).a
        );
    }

    #[test]
    fn a_stroke_can_swell_and_taper() {
        // Per-anchor widths scale the stroke along the path, so one end can
        // be fat and the other thin — the difference between a line and a
        // brush stroke.
        let mut doc = Document::new(64, 32, ColorMode::Rgb);
        let root = doc.root();
        let mut node = Node::vector(
            "s",
            VectorShape::Path {
                points: vec![[4.0, 16.0], [60.0, 16.0]],
                closed: false,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
        );
        if let NodeKind::Vector { stroke, .. } = &mut node.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: RED,
                width: 12.0,
                widths: vec![1.0, 0.15],
            });
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(node),
        })
        .unwrap();

        let s = render(&doc).unwrap();
        let band = |x: u32| (0..32).filter(|y| s.get(x, *y).a > 0.5).count();
        let (near, far) = (band(8), band(56));
        assert!(near > 8, "the fat end is fat, got {near}");
        assert!(far < 4, "the thin end is thin, got {far}");
        assert!(
            near > far * 2,
            "and it tapers between them: {near} vs {far}"
        );

        // Hit testing follows the same width, so the thin end is not
        // clickable where the fat end would have been.
        let id = doc.children_of(root).unwrap()[0];
        assert_eq!(hit_test(&doc, 8.0, 21.0).unwrap(), Some(id));
        assert_eq!(hit_test(&doc, 56.0, 21.0).unwrap(), None);
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
                subpaths: Vec::new(),
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
    fn a_minified_raster_averages_the_texels_it_skips_over() {
        // Four-texel stripes shrunk to an eighth. Every device pixel spans
        // a whole period, so the honest answer is the average — mid grey.
        // One bilinear sample instead lands in whichever stripe the grid
        // happens to fall on and reads black or white, which is what makes
        // a shrunk photograph crawl when it moves. (Stripes rather than a
        // checkerboard: a one-texel checkerboard averages to grey under a
        // single bilinear tap too, and so would prove nothing.)
        const N: u32 = 64;
        let mut rgba8 = Vec::with_capacity((N * N * 4) as usize);
        for y in 0..N {
            for x in 0..N {
                let _ = y;
                let v = if (x / 3) % 2 == 0 { 0u8 } else { 255u8 };
                rgba8.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let mut doc = Document::new(16, 16, ColorMode::Rgb);
        let root = doc.root();
        let id = doc.add_resource(N, N, rgba8);
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: N,
                    height: N,
                },
            )),
        })
        .unwrap();
        let node = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: node,
            transform: Transform {
                a: 0.125,
                d: 0.125,
                ..Default::default()
            },
        })
        .unwrap();
        let s = render(&doc).unwrap();
        // Linear-light average of black and white is 0.5, whatever sRGB
        // makes of it at the display edge.
        let inside: Vec<f32> = (1..7)
            .flat_map(|y| (1..7).map(move |x| (x, y)))
            .map(|(x, y)| s.get(x, y).r)
            .collect();
        let lo = inside.iter().cloned().fold(f32::MAX, f32::min);
        let hi = inside.iter().cloned().fold(f32::MIN, f32::max);
        // Four taps across eight texels do not fully resolve a six-texel
        // period, so the reading wobbles a little either side of the
        // average. A single sample swings the whole way from black to
        // white, which is the difference this is about.
        assert!(
            (lo - 0.5).abs() < 0.15 && (hi - 0.5).abs() < 0.15,
            "every pixel should read near the average, got {lo}..{hi}"
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
                subpaths: Vec::new(),
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
                chitrakar_doc::TextSpec::new("Hi", 48.0, RED),
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
            kind: Box::new(NodeKind::Text(chitrakar_doc::TextSpec::new(
                "Hi there", 48.0, RED,
            ))),
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
                    subpaths: Vec::new(),
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
                    subpaths: Vec::new(),
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
                subpaths: Vec::new(),
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
                widths: Vec::new(),
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
                radius: 0.0,
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
                widths: Vec::new(),
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
