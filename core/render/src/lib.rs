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

/// The box a paint layer's strokes cover in its own space, or nothing
/// when none of them covers anything.
fn painted_bounds(strokes: &[chitrakar_doc::PaintStroke]) -> Option<[f32; 4]> {
    strokes.iter().filter_map(|s| s.bounds()).reduce(|a, b| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}

/// Local size (width, height) of a node's own content, before its transform.
fn local_size(kind: &NodeKind) -> Option<(f32, f32)> {
    match kind {
        NodeKind::Vector { shape, .. } => Some(shape_size(shape)),
        NodeKind::Raster(r) => Some((r.width as f32, r.height as f32)),
        NodeKind::Text(spec) => Some(text::measure(spec)),
        // A paint layer's box is not anchored at its origin — a stroke
        // can be laid anywhere on it — so it reports its own bounds
        // instead, and callers that only want a size get nothing.
        // A frame's box is its own, whatever it holds: it cuts its
        // contents to that size, so nothing inside can enlarge it.
        NodeKind::Artboard { width, height, .. } => Some((*width, *height)),
        // A copy has no size of its own: it is whatever it is a copy of.
        NodeKind::Instance { .. }
        | NodeKind::Group
        | NodeKind::Paint { .. }
        | NodeKind::Clone { .. }
        | NodeKind::Adjustment(_)
        | NodeKind::Filter(_) => None,
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

/// The document-space box a local one occupies under a transform.
pub fn transformed_box(t: Transform, box_: [f32; 4]) -> Bounds {
    transformed_local_bounds(t, (box_[0], box_[1], box_[2], box_[3]))
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
            // A clone layer paints with what is under it, so a group
            // holding one cannot be painted straight onto the page.
            NodeKind::Adjustment(_) | NodeKind::Filter(_) | NodeKind::Clone { .. } => {
                return Ok(true)
            }
            NodeKind::Group | NodeKind::Artboard { .. } if reads_backdrop(doc, child)? => {
                return Ok(true)
            }
            // A copy sees what the original sees. The graph has no
            // cycles, so following it terminates.
            NodeKind::Instance { of, .. } => {
                let master = doc.node(*of)?;
                let reads = match &master.kind {
                    NodeKind::Adjustment(_) | NodeKind::Filter(_) | NodeKind::Clone { .. } => true,
                    NodeKind::Group | NodeKind::Artboard { .. } => reads_backdrop(doc, *of)?,
                    _ => false,
                };
                if reads || master.blend != BlendMode::Normal {
                    return Ok(true);
                }
            }
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
pub fn bounds_in_parent_space(doc: &Document, id: NodeId) -> Result<Bounds, DocError> {
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
        NodeKind::Text(spec) => {
            let [x0, y0, x1, y1] = text::bounds(spec);
            transformed_local_bounds(node.transform, (x0, y0, x1, y1))
        }
        NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => {
            match painted_bounds(strokes) {
                Some([x0, y0, x1, y1]) => {
                    transformed_local_bounds(node.transform, (x0, y0, x1, y1))
                }
                None => Bounds::None,
            }
        }
        NodeKind::Instance { of, .. } => match local_bounds_of(doc, *of) {
            Ok(Some([x0, y0, x1, y1])) => {
                transformed_local_bounds(node.transform, (x0, y0, x1, y1))
            }
            // The original is gone, or is a change to what is under it
            // rather than a picture with a box.
            Ok(None) => Bounds::None,
            Err(_) => Bounds::None,
        },
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
        NodeKind::Text(spec) => Some(text::bounds(spec)),
        NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => painted_bounds(strokes),
        // A copy's own box is the original's own box: the original's
        // placement is not part of what travels. Where the copy stands in
        // for some of the original's layers, it is the box of what is
        // actually drawn — a longer label makes a wider copy.
        NodeKind::Instance { of, .. } => {
            let stand_ins = if takes_stand_ins(doc, *of) {
                copy_children(doc, id)?
            } else {
                Vec::new()
            };
            if stand_ins.is_empty() {
                local_bounds_of(doc, *of).unwrap_or(None)
            } else {
                let mut acc = Bounds::None;
                for part in stand_ins {
                    acc = acc.union(bounds_in_parent_space_inner(doc, part, false)?);
                }
                match acc {
                    Bounds::Rect(x0, y0, x1, y1) => Some([x0, y0, x1, y1]),
                    _ => None,
                }
            }
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
            // A clone reads at an offset from where it paints, so a
            // change at the source has to repaint the clone as well —
            // which is the same problem a filter's radius poses, and
            // takes the same answer.
            NodeKind::Clone { strokes } => {
                let scale = max_scale(ancestor_space(doc, *id).compose(node.transform));
                let far = strokes
                    .iter()
                    .map(|s| s.source[0].abs().max(s.source[1].abs()))
                    .fold(0.0f32, f32::max);
                (far * scale).ceil() as u32 + 1
            }
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
    let children = doc.children_of(group)?;
    let mut i = 0;
    while i < children.len() {
        // A run of layers clipped to the one below them: the first is what
        // they are confined to, and it is drawn as it always was. The
        // bottom-most child has nothing under it, so its own flag — if it
        // carries one — has nothing to bite on and is ignored here.
        let mut end = i + 1;
        while end < children.len() && doc.node(children[end])?.clipped {
            end += 1;
        }
        let capture = (end > i + 1).then(|| {
            // The layers about to be cut by this one read pixels beyond
            // the ones they cover — a shadow's blur does — so the cut has
            // to be known that much further out than the repainted region.
            let reach = children[i + 1..end]
                .iter()
                .filter_map(|&c| doc.node(c).ok())
                .flat_map(|n| n.effects.iter().map(Effect::reach))
                .fold(0.0f32, f32::max);
            (reach * max_scale(parent)).ceil() as u32
        });
        let cover = draw_layer(doc, children[i], dst, clip, parent, None, capture)?;
        for &above in &children[i + 1..end] {
            draw_layer(doc, above, dst, clip, parent, cover.as_ref(), None)?;
        }
        i = end;
    }
    Ok(())
}

/// Where a clipped layer is allowed to show: the alpha of the layer it is
/// clipped to, over the window that layer was drawn in. Anything outside
/// the window is outside the layer too, so it shows nothing.
struct Cover {
    alpha: Vec<f32>,
    origin: (u32, u32),
    width: u32,
    height: u32,
}

impl Cover {
    /// A layer that draws nothing — hidden, fully transparent, or landing
    /// nowhere in the region being painted. Everything clipped to it
    /// disappears with it, which is what makes hiding the layer below hide
    /// the stack riding on it.
    fn nothing() -> Self {
        Cover {
            alpha: Vec::new(),
            origin: (0, 0),
            width: 0,
            height: 0,
        }
    }

    /// A cover that confines nothing over the region it spans.
    fn everywhere(rect: ClipRect) -> Self {
        let (width, height) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        Cover {
            alpha: vec![1.0; (width * height) as usize],
            origin: (rect.x0, rect.y0),
            width,
            height,
        }
    }

    fn rect(&self) -> ClipRect {
        ClipRect {
            x0: self.origin.0,
            y0: self.origin.1,
            x1: self.origin.0 + self.width,
            y1: self.origin.1 + self.height,
        }
    }
}

/// Draw one layer where its parent puts it: its own picture, its effects
/// around that, and the whole composited with its blend and opacity.
///
/// Pulled out of the walk so a caller with one layer in mind — a panel
/// wanting to show what a layer holds — can draw exactly what the page
/// would have drawn of it, effects and all.
fn render_layer(
    doc: &Document,
    child: NodeId,
    dst: &mut Surface,
    clip: ClipRect,
    parent: Transform,
) -> Result<(), DocError> {
    draw_layer(doc, child, dst, clip, parent, None, None).map(|_| ())
}

/// The same, with what clipping needs on either side of it: `cover`
/// confines the layer to another one's alpha, and `capture` asks for this
/// layer's own alpha back so the layers clipped to it can be confined in
/// turn. Its value is how far past the region being repainted that alpha
/// is still wanted — the reach of the effects hanging off the layers
/// about to be cut by it, which read pixels they do not themselves cover.
fn draw_layer(
    doc: &Document,
    child: NodeId,
    dst: &mut Surface,
    clip: ClipRect,
    parent: Transform,
    cover: Option<&Cover>,
    capture: Option<u32>,
) -> Result<Option<Cover>, DocError> {
    {
        let node = doc.node(child)?;
        if !node.visible || node.opacity <= 0.0 {
            return Ok(capture.map(|_| Cover::nothing()));
        }
        // Effects are drawn from the layer's own silhouette, so a layer
        // that has any must exist as a picture before they can be. An
        // adjustment or filter has no silhouette — it is a transformation
        // of what is below — so effects on one mean nothing and are
        // ignored rather than given a surface.
        let effected = !node.effects.is_empty()
            && !matches!(node.kind, NodeKind::Adjustment(_) | NodeKind::Filter(_));
        // Clipping needs the layer as a picture before it goes down —
        // to be cut by what is under it, or to be read as the cut — so
        // either end of it forces the same surface effects ask for.
        if !effected && cover.is_none() && capture.is_none() {
            render_child(doc, child, dst, clip, parent, node.blend)?;
            return Ok(None);
        }
        // An adjustment, a filter and a clone are transformations of what
        // is already on the page rather than pictures of their own: drawn
        // on a surface of their own they would have nothing to work on. So
        // one of these is applied where it stands, over the region it is
        // confined to, and mixed back into what was there by how much of
        // that region its cover lets through.
        if matches!(
            node.kind,
            NodeKind::Adjustment(_) | NodeKind::Filter(_) | NodeKind::Clone { .. }
        ) {
            if let Some(c) = cover {
                let region = clip.intersect(c.rect());
                if region.is_empty() {
                    return Ok(None);
                }
                let before = blur::snapshot(dst, region);
                let corner = (region.x0, region.y0);
                let stride = region.x1 - region.x0;
                render_child(doc, child, dst, region, parent, node.blend)?;
                for y in region.y0..region.y1 {
                    for x in region.x0..region.x1 {
                        let a = c.alpha[at_in(c.origin, c.width, x, y)];
                        let i = (y * dst.width + x) as usize;
                        dst.pixels[i] = lerp(before[at_in(corner, stride, x, y)], dst.pixels[i], a);
                    }
                }
                return Ok(None);
            }
            render_child(doc, child, dst, clip, parent, node.blend)?;
            // Nothing clipped to one of these is confined by it: it has no
            // shape of its own to be confined to, so it lets everything
            // through rather than nothing.
            return Ok(capture.map(|pad| Cover::everywhere(grow(clip, pad, dst.width, dst.height))));
        }
        let scale = max_scale(parent);
        let reach = node
            .effects
            .iter()
            .map(Effect::reach)
            .fold(0.0f32, f32::max);
        let pad = ((reach * scale).ceil() as u32).max(capture.unwrap_or(0));
        // The layer has to be drawn wherever it could feed a visible
        // effect pixel, which is further out than the region being
        // repainted — by exactly the effects' reach.
        let grown = grow(clip, pad, dst.width, dst.height);
        let extent = match bounds_in_parent_space(doc, child)? {
            Bounds::Rect(x0, y0, x1, y1) => transformed_local_bounds(parent, (x0, y0, x1, y1)),
            other => other,
        };
        // A clipped layer cannot show outside what it is clipped to, so
        // its surface need never be bigger than that layer's window.
        let confined = match cover {
            Some(c) => grown.intersect(c.rect()),
            None => grown,
        };
        let layer_clip = match extent.to_clip(dst.width, dst.height) {
            Some(b) => b.intersect(confined),
            None => return Ok(capture.map(|_| Cover::nothing())),
        };
        if layer_clip.is_empty() {
            return Ok(capture.map(|_| Cover::nothing()));
        }
        // The layer's own surface covers only where it can land, not the
        // whole page: at A4 one small shape with a shadow was allocating
        // and clearing the canvas three times over for it. Everything
        // drawn into it, and every field built from it, is shifted by
        // that window's corner.
        let origin = (layer_clip.x0, layer_clip.y0);
        let window = Transform::translation(-(origin.0 as f32), -(origin.1 as f32));
        let mut layer = Surface::new(layer_clip.x1 - origin.0, layer_clip.y1 - origin.1);
        // Normal into the layer's own transparent surface; the node's blend
        // belongs to the composite below, once the effects are in place.
        render_child(
            doc,
            child,
            &mut layer,
            shift_clip(layer_clip, origin),
            window.compose(parent),
            BlendMode::Normal,
        )?;
        // Cut the layer to what it is clipped to before anything is made
        // of it, so its effects grow from the shape that will actually be
        // seen rather than from the whole of it.
        if let Some(c) = cover {
            for y in layer_clip.y0..layer_clip.y1 {
                for x in layer_clip.x0..layer_clip.x1 {
                    let a = c.alpha[at_in(c.origin, c.width, x, y)];
                    let i = at_in(origin, layer.width, x, y);
                    layer.pixels[i] = scale_alpha(layer.pixels[i], a);
                }
            }
        }
        let taken = capture.map(|_| Cover {
            alpha: layer.pixels.iter().map(|p| p.a).collect(),
            origin,
            width: layer.width,
            height: layer.height,
        });
        // Effects behind the layer, then the layer, then the ones that
        // belong on top of it — an inner shadow shades the pixels it sits
        // on, so it cannot be painted before they are there.
        for effect in node.effects.iter().filter(|e| !e.over()) {
            draw_effect(
                dst,
                &layer,
                origin,
                doc,
                effect,
                scale,
                layer_clip,
                clip,
                node.blend,
                node.opacity,
            );
        }
        composite_from(
            dst,
            &layer,
            origin,
            1.0,
            node.blend,
            clip.intersect(layer_clip),
        );
        for effect in node.effects.iter().filter(|e| e.over()) {
            draw_effect(
                dst,
                &layer,
                origin,
                doc,
                effect,
                scale,
                layer_clip,
                clip,
                node.blend,
                node.opacity,
            );
        }
        Ok(taken)
    }
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
        // A painted mask is worked out over the region about to be
        // drawn before anything reads it, since reading one a pixel at a
        // time would cost the strokes' length at every pixel.
        // A group is isolated on a surface of its own, in that surface's
        // coordinates, so its mask is worked out there instead of here.
        let plane = if node.kind.holds_children() {
            None
        } else {
            MaskRef::plane_for(node.mask.as_ref(), parent, clip, (dst.width, dst.height))
        };
        let mask = MaskRef::new(node.mask.as_ref(), parent).with_plane(plane.as_ref());
        match &node.kind {
            NodeKind::Instance { of, .. } => {
                // A copy draws what the original draws, where the copy
                // is: the original's own placement is undone first, so
                // moving the original moves only the original.
                let Ok(master) = doc.node(*of) else {
                    return Ok(());
                };
                let Some(back) = invert(master.transform) else {
                    return Ok(());
                };
                let space = t.compose(back);
                // The original's own box maps to the page through the
                // copy's own space: undoing the original's transform and
                // then applying it again is exactly that space.
                // The copy's own box, not the original's: where the copy
                // stands in for one of the original's layers with a
                // different one, what it covers is its own.
                let extent = match local_bounds_of(doc, child)? {
                    Some([x0, y0, x1, y1]) => transformed_local_bounds(t, (x0, y0, x1, y1)),
                    // A copy of an adjustment or a filter reaches as far
                    // as the original would.
                    None => Bounds::Everything,
                };
                let sub_clip = match extent.to_clip(dst.width, dst.height) {
                    Some(b) => b.intersect(clip),
                    None => return Ok(()),
                };
                if sub_clip.is_empty() {
                    return Ok(());
                }
                // What the copy actually draws: the original, or the
                // original with the copy's own layers standing in for
                // some of its.
                let stand_ins = if takes_stand_ins(doc, *of) {
                    copy_children(doc, child)?
                } else {
                    Vec::new()
                };
                // Composited like the original would be: nothing of its
                // own to apply, so draw it straight in.
                if node.opacity >= 1.0 && blend == BlendMode::Normal && mask.mask.is_none() {
                    if stand_ins.is_empty() {
                        return render_layer(doc, *of, dst, sub_clip, space);
                    }
                    for &part in &stand_ins {
                        render_layer(doc, part, dst, sub_clip, t)?;
                    }
                    return Ok(());
                }
                let (ox, oy) = (sub_clip.x0, sub_clip.y0);
                let window = Transform::translation(-(ox as f32), -(oy as f32));
                let inner = ClipRect {
                    x0: 0,
                    y0: 0,
                    x1: sub_clip.x1 - ox,
                    y1: sub_clip.y1 - oy,
                };
                let mut sub = Surface::new(inner.x1, inner.y1);
                if stand_ins.is_empty() {
                    render_layer(doc, *of, &mut sub, inner, window.compose(space))?;
                } else {
                    for &part in &stand_ins {
                        render_layer(doc, part, &mut sub, inner, window.compose(t))?;
                    }
                }
                if node.mask.is_some() {
                    let shifted = window.compose(parent);
                    let plane = MaskRef::plane_for(
                        node.mask.as_ref(),
                        shifted,
                        inner,
                        (sub.width, sub.height),
                    );
                    let m = MaskRef::new(node.mask.as_ref(), shifted).with_plane(plane.as_ref());
                    apply_mask(doc, m, &mut sub, inner);
                }
                composite_from(dst, &sub, (ox, oy), node.opacity, blend, sub_clip);
            }
            NodeKind::Artboard {
                width,
                height,
                background,
            } => {
                // The frame's own box, in the space it is placed in.
                let frame = transformed_local_bounds(t, (0.0, 0.0, *width, *height));
                let Bounds::Rect(fx0, fy0, fx1, fy1) = frame else {
                    return Ok(());
                };
                // Rounded to whole pixels rather than grown outward: an
                // artboard's edge is a page edge, and a page edge is
                // crisp. A frame that has been turned takes the other
                // path, where its edge is worked out per pixel.
                let board = ClipRect {
                    x0: (fx0.round().max(0.0) as u32).min(dst.width),
                    y0: (fy0.round().max(0.0) as u32).min(dst.height),
                    x1: (fx1.round().max(0.0) as u32).min(dst.width),
                    y1: (fy1.round().max(0.0) as u32).min(dst.height),
                };
                let ground = background.map(|c| resolve_color(doc, c));
                let upright = t.b.abs() < 1e-6 && t.c.abs() < 1e-6;
                let plain =
                    node.opacity >= 1.0 && blend == BlendMode::Normal && mask.mask.is_none();
                if upright && plain {
                    // Upright and composited like its contents would be:
                    // the frame is nothing but a narrower region to paint
                    // in, so paint in it. Everything inside — an
                    // adjustment included — then sees the page below the
                    // frame, which is what being on the page means.
                    let inside = board.intersect(clip);
                    if inside.is_empty() {
                        return Ok(());
                    }
                    if let Some(color) = ground {
                        fill_region(dst, inside, color, blend);
                    }
                    return render_group(doc, child, dst, inside, t);
                }
                // Turned, or composited as a whole: the frame is drawn on
                // a surface of its own and cut to shape by how much of
                // each pixel it covers, so its edge is as smooth as any
                // other edge in the picture.
                let sub_clip = match frame.to_clip(dst.width, dst.height) {
                    Some(b) => b.intersect(clip),
                    None => return Ok(()),
                };
                if sub_clip.is_empty() {
                    return Ok(());
                }
                let (ox, oy) = (sub_clip.x0, sub_clip.y0);
                let window = Transform::translation(-(ox as f32), -(oy as f32));
                let inner = ClipRect {
                    x0: 0,
                    y0: 0,
                    x1: sub_clip.x1 - ox,
                    y1: sub_clip.y1 - oy,
                };
                let mut sub = Surface::new(inner.x1, inner.y1);
                let shifted = window.compose(t);
                if let Some(color) = ground {
                    // Over the whole window, not just the frame: the cut
                    // below gives the ground the frame's own edge, and a
                    // shape painted here would give it a second one.
                    fill_region(&mut sub, inner, color, BlendMode::Normal);
                }
                render_group(doc, child, &mut sub, inner, shifted)?;
                if let Some(inv) = Inverse::of(shifted) {
                    for y in inner.y0..inner.y1 {
                        for x in inner.x0..inner.x1 {
                            let cov = rect_coverage(*width, *height, shifted, inv, x, y);
                            if cov < 1.0 {
                                let i = (y * sub.width + x) as usize;
                                sub.pixels[i] = scale_alpha(sub.pixels[i], cov);
                            }
                        }
                    }
                }
                if node.mask.is_some() {
                    let shifted_parent = window.compose(parent);
                    let plane = MaskRef::plane_for(
                        node.mask.as_ref(),
                        shifted_parent,
                        inner,
                        (sub.width, sub.height),
                    );
                    let m =
                        MaskRef::new(node.mask.as_ref(), shifted_parent).with_plane(plane.as_ref());
                    apply_mask(doc, m, &mut sub, inner);
                }
                composite_from(dst, &sub, (ox, oy), node.opacity, blend, sub_clip);
            }
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
                // The surface it is isolated on covers only where the
                // group can land rather than the whole page: at A4 a
                // group holding one small shape was allocating and
                // clearing the canvas for it. Everything drawn into it
                // is shifted by that window's own corner.
                let (ox, oy) = (sub_clip.x0, sub_clip.y0);
                let window = Transform::translation(-(ox as f32), -(oy as f32));
                let inner = ClipRect {
                    x0: 0,
                    y0: 0,
                    x1: sub_clip.x1 - ox,
                    y1: sub_clip.y1 - oy,
                };
                let mut sub = Surface::new(inner.x1, inner.y1);
                render_group(doc, child, &mut sub, inner, window.compose(t))?;
                if node.mask.is_some() {
                    // The mask is read in the window's coordinates too.
                    let shifted = window.compose(parent);
                    let plane = MaskRef::plane_for(
                        node.mask.as_ref(),
                        shifted,
                        inner,
                        (sub.width, sub.height),
                    );
                    let m = MaskRef::new(node.mask.as_ref(), shifted).with_plane(plane.as_ref());
                    apply_mask(doc, m, &mut sub, inner);
                }
                composite_from(dst, &sub, (ox, oy), node.opacity, blend, sub_clip);
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
                // it, weighted by its opacity and mask coverage. A curve is
                // tabulated once here rather than solved per pixel.
                let luts = curve_luts(adj);
                for y in clip.y0..clip.y1 {
                    for x in clip.x0..clip.x1 {
                        let weight = node.opacity * coverage_at(doc, mask, x, y);
                        if weight <= 0.0 {
                            continue;
                        }
                        let i = (y * dst.width + x) as usize;
                        let adjusted = apply_adjustment(adj, luts.as_ref(), dst.pixels[i]);
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
            NodeKind::Paint { strokes } => {
                draw_paint(dst, doc, strokes, t, node.opacity, blend, clip, mask)
            }
            NodeKind::Clone { strokes } => {
                draw_clone(dst, doc, strokes, t, node.opacity, blend, clip, mask)
            }
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
    origin: (u32, u32),
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
            let field = silhouette(layer, origin, layer_clip, tint, false, blur * scale);
            stamp(
                dst,
                &field,
                origin,
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
            let field = silhouette(layer, origin, layer_clip, tint, true, blur * scale);
            stamp(
                dst,
                &field,
                origin,
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
            let field = outline_band(layer, origin, layer_clip, tint, w, layer_opacity);
            stamp(
                dst,
                &field,
                origin,
                (0.0, 0.0),
                layer_clip,
                write,
                blend,
                None,
            );
        }
    }
}

/// The layer's silhouette (or, inverted, the hole around it) in one flat
/// colour, blurred. Tinting before the blur rather than after is the same
/// answer — the tint is constant and the blur is linear — and it means one
/// surface instead of two.
fn silhouette(
    layer: &Surface,
    origin: (u32, u32),
    clip: ClipRect,
    tint: LinearRgba,
    invert: bool,
    sigma: f32,
) -> Surface {
    let mut out = Surface::new(layer.width, layer.height);
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let i = at_in(origin, layer.width, x, y);
            let a = layer.pixels[i].a;
            out.pixels[i] = scale_alpha(tint, if invert { 1.0 - a } else { a });
        }
    }
    blur::gaussian_blur(&mut out, shift_clip(clip, origin), sigma);
    out
}

/// A region reaching `pad` pixels further out on every side, kept inside
/// the surface it indexes.
fn grow(clip: ClipRect, pad: u32, width: u32, height: u32) -> ClipRect {
    ClipRect {
        x0: clip.x0.saturating_sub(pad),
        y0: clip.y0.saturating_sub(pad),
        x1: (clip.x1 + pad).min(width),
        y1: (clip.y1 + pad).min(height),
    }
}

/// A device-space region in the coordinates of a surface holding the
/// window that starts at `origin`.
fn shift_clip(clip: ClipRect, origin: (u32, u32)) -> ClipRect {
    ClipRect {
        x0: clip.x0 - origin.0,
        y0: clip.y0 - origin.1,
        x1: clip.x1 - origin.0,
        y1: clip.y1 - origin.1,
    }
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
    layer: &Surface,
    origin: (u32, u32),
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
            let i = at_in(origin, layer.width, x as u32 + clip.x0, y as u32 + clip.y0);
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
    let mut out = Surface::new(layer.width, layer.height);
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
            let i = at_in(origin, layer.width, x as u32 + clip.x0, y as u32 + clip.y0);
            out.pixels[i] = scale_alpha(tint, cover);
        }
    }
    out
}

/// Paint a prepared effect field into the destination, displaced by
/// `offset` device pixels and sampled between pixels so a sub-pixel offset
/// (or a fractional zoom) does not jump. `keep_inside`, when given, limits
/// what lands to that surface's own alpha — how an inner shadow stays in.
#[allow(clippy::too_many_arguments)]
fn stamp(
    dst: &mut Surface,
    field: &Surface,
    origin: (u32, u32),
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
                field.pixels[at_in(origin, field.width, px, py)]
            };
            let top = lerp(at(fx, fy), at(fx + 1.0, fy), tx);
            let bottom = lerp(at(fx, fy + 1.0), at(fx + 1.0, fy + 1.0), tx);
            let mut src = lerp(top, bottom, ty);
            let i = (y * dst.width + x) as usize;
            if let Some(inside) = keep_inside {
                src = scale_alpha(src, inside.pixels[at_in(origin, inside.width, x, y)].a);
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

/// Composite `src` over `dst` with a blend mode.
///
/// The compositing arithmetic is done in the premultiplied linear pixels
/// the whole renderer works in, which is where source-over belongs. The
/// *blend function* is not: the W3C spec defines it over the values a
/// device shows, and so do SVG's `mix-blend-mode` and PDF's `/BM`. Doing
/// it in linear light would put Overlay's pivot at what reads as a light
/// grey and would make the engine disagree with its own exports, so each
/// channel crosses into the display encoding for the blend and back.
fn blend_pixel(src: LinearRgba, dst: LinearRgba, mode: BlendMode) -> LinearRgba {
    match mode {
        BlendMode::Normal => src.over(dst),
        BlendMode::Multiply => separable(src, dst, |s, d| s * d),
        BlendMode::Screen => separable(src, dst, screen),
        BlendMode::Overlay => separable(src, dst, |s, d| hard_light(d, s)),
        BlendMode::Darken => separable(src, dst, f32::min),
        BlendMode::Lighten => separable(src, dst, f32::max),
        BlendMode::ColorDodge => separable(src, dst, |s, d| {
            if d <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (d / (1.0 - s)).min(1.0)
            }
        }),
        BlendMode::ColorBurn => separable(src, dst, |s, d| {
            if d >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - d) / s).min(1.0)
            }
        }),
        BlendMode::HardLight => separable(src, dst, hard_light),
        BlendMode::SoftLight => separable(src, dst, soft_light),
        BlendMode::Difference => separable(src, dst, |s, d| (s - d).abs()),
        BlendMode::Exclusion => separable(src, dst, |s, d| s + d - 2.0 * s * d),
        // The four that take one part of a colour and leave the rest have
        // to see all three channels at once.
        BlendMode::Hue => non_separable(src, dst, |s, b| set_lum(set_sat(s, sat(b)), lum(b))),
        BlendMode::Saturation => {
            non_separable(src, dst, |s, b| set_lum(set_sat(b, sat(s)), lum(b)))
        }
        BlendMode::Color => non_separable(src, dst, |s, b| set_lum(s, lum(b))),
        BlendMode::Luminosity => non_separable(src, dst, |s, b| set_lum(b, lum(s))),
    }
}

fn screen(s: f32, d: f32) -> f32 {
    s + d - s * d
}

fn hard_light(s: f32, d: f32) -> f32 {
    if s <= 0.5 {
        d * 2.0 * s
    } else {
        screen(2.0 * s - 1.0, d)
    }
}

/// W3C soft light: a gentler hard light, with the backdrop steering how
/// far the layer can push it.
fn soft_light(s: f32, d: f32) -> f32 {
    let dd = if d <= 0.25 {
        ((16.0 * d - 12.0) * d + 4.0) * d
    } else {
        d.sqrt()
    };
    if s <= 0.5 {
        d - (1.0 - 2.0 * s) * d * (1.0 - d)
    } else {
        d + (2.0 * s - 1.0) * (dd - d)
    }
}

/// How finely the display transfer curve is tabulated for blending.
///
/// Crossing that curve means a `powf`, and a blend crosses it nine times
/// a pixel — six going in and three coming back — which was four fifths
/// of what a blended page cost. A table with a straight line between
/// entries is well inside a part in a hundred thousand at this many
/// steps, which is far finer than the eight bits the answer is ever
/// shown at, and the tests hold it to that.
const CURVE_STEPS: usize = 4096;

/// The display transfer curve, tabulated both ways.
struct Transfer {
    /// Linear light to what a device shows.
    to_shown: Box<[f32]>,
    /// And back.
    to_linear: Box<[f32]>,
}

fn transfer() -> &'static Transfer {
    static TABLES: std::sync::OnceLock<Transfer> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let at = |i: usize| i as f32 / CURVE_STEPS as f32;
        Transfer {
            to_shown: (0..=CURVE_STEPS)
                .map(|i| chitrakar_color::linear_to_srgb(at(i)))
                .collect(),
            to_linear: (0..=CURVE_STEPS)
                .map(|i| chitrakar_color::srgb_to_linear(at(i)))
                .collect(),
        }
    })
}

fn on_curve(table: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * CURVE_STEPS as f32;
    let i = x as usize;
    let f = x - i as f32;
    let a = table[i];
    let b = table[(i + 1).min(CURVE_STEPS)];
    a + (b - a) * f
}

/// The channels a blend function sees: unpremultiplied and in the
/// display encoding, which is the space the spec is written in.
fn shown(table: &[f32], v: f32, a: f32) -> f32 {
    if a > 0.0 {
        on_curve(table, (v / a).clamp(0.0, 1.0))
    } else {
        0.0
    }
}

fn separable(src: LinearRgba, dst: LinearRgba, f: impl Fn(f32, f32) -> f32) -> LinearRgba {
    let (sa, da) = (src.a, dst.a);
    // Once per pixel rather than once per crossing: consulting the lock
    // the tables live behind costs more than reading from them.
    let t = transfer();
    let (to, from) = (&t.to_shown, &t.to_linear);
    let mix = |s: f32, d: f32| {
        let blended = on_curve(from, f(shown(to, s, sa), shown(to, d, da)).clamp(0.0, 1.0));
        // W3C compositing: result = (1-da)*s + (1-sa)*d + sa*da*B
        (1.0 - da) * s + (1.0 - sa) * d + sa * da * blended
    };
    LinearRgba {
        r: mix(src.r, dst.r),
        g: mix(src.g, dst.g),
        b: mix(src.b, dst.b),
        a: sa + da * (1.0 - sa),
    }
}

/// The same compositing, for a blend that reads all three channels at
/// once rather than one at a time.
fn non_separable(
    src: LinearRgba,
    dst: LinearRgba,
    f: impl Fn([f32; 3], [f32; 3]) -> [f32; 3],
) -> LinearRgba {
    let (sa, da) = (src.a, dst.a);
    let t = transfer();
    let (to, from) = (&t.to_shown, &t.to_linear);
    let s = [
        shown(to, src.r, sa),
        shown(to, src.g, sa),
        shown(to, src.b, sa),
    ];
    let d = [
        shown(to, dst.r, da),
        shown(to, dst.g, da),
        shown(to, dst.b, da),
    ];
    let b = f(s, d);
    let mix = |i: usize, s: f32, d: f32| {
        let blended = on_curve(from, b[i].clamp(0.0, 1.0));
        (1.0 - da) * s + (1.0 - sa) * d + sa * da * blended
    };
    LinearRgba {
        r: mix(0, src.r, dst.r),
        g: mix(1, src.g, dst.g),
        b: mix(2, src.b, dst.b),
        a: sa + da * (1.0 - sa),
    }
}

/// W3C's luminosity for the non-separable blends — its own weights, not
/// the renderer's luminance, because the spec says so and matching it is
/// what makes the engine agree with SVG and PDF.
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 && l - n > 1e-6 {
        for v in &mut out {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 && x - l > 1e-6 {
        for v in &mut out {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// Stretch a colour's channels to a given saturation, keeping which
/// channel is which — the middle one lands where it sat between the two
/// others.
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let (mut lo, mut mid, mut hi) = (0usize, 1usize, 2usize);
    // Sort the indices by value, three comparisons.
    if c[lo] > c[mid] {
        std::mem::swap(&mut lo, &mut mid);
    }
    if c[mid] > c[hi] {
        std::mem::swap(&mut mid, &mut hi);
    }
    if c[lo] > c[mid] {
        std::mem::swap(&mut lo, &mut mid);
    }
    let mut out = [0.0f32; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out[lo] = 0.0;
    out
}

/// Where a device pixel sits in a surface holding only the window that
/// starts at `origin`. The caller has already established that the
/// pixel is inside that window.
fn at_in(origin: (u32, u32), width: u32, x: u32, y: u32) -> usize {
    ((y - origin.1) * width + (x - origin.0)) as usize
}

/// Lay one flat paint over a whole region, with no edge to work out —
/// what a frame's ground is, since the frame's own edge is applied to
/// everything in it at once.
fn fill_region(dst: &mut Surface, clip: ClipRect, color: LinearRgba, mode: BlendMode) {
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let i = (y * dst.width + x) as usize;
            dst.pixels[i] = blend_pixel(color, dst.pixels[i], mode);
        }
    }
}

/// Composite a window of source pixels: `src` holds the destination's
/// region starting at `origin`, so a layer isolated on a surface the
/// size of the box it can land in composites back where it belongs.
fn composite_from(
    dst: &mut Surface,
    src: &Surface,
    origin: (u32, u32),
    opacity: f32,
    mode: BlendMode,
    clip: ClipRect,
) {
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let s = ((y - origin.1) * src.width + (x - origin.0)) as usize;
            let i = (y * dst.width + x) as usize;
            dst.pixels[i] = blend_pixel(scale_alpha(src.pixels[s], opacity), dst.pixels[i], mode);
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

/// The space a brush writes into.
///
/// Painting a layer, that is the layer's own space. Painting its mask,
/// it is the space the mask is authored in — the layer's parent's —
/// because a mask describes the document as the layer sees it rather
/// than as the layer is drawn, and so does not turn or scale with it.
pub fn brush_space(doc: &Document, id: NodeId, on_mask: bool) -> Result<Transform, DocError> {
    Ok(if on_mask {
        ancestor_space(doc, id)
    } else {
        world_transform(doc, id)?
    })
}

/// A document-space point in the space a brush writes into: where on
/// the layer, or on its mask, the pointer is. `None` when that space is
/// collapsed, mapping every document point nowhere.
pub fn point_in_layer(
    doc: &Document,
    id: NodeId,
    on_mask: bool,
    x: f32,
    y: f32,
) -> Result<Option<(f32, f32)>, DocError> {
    Ok(to_local(brush_space(doc, id, on_mask)?, x, y))
}

/// A layer's own transform with every group it sits inside composed
/// onto it: where the layer's space sits on the page. The same chain
/// [`node_bounds`] measures a layer against, so a tool that writes into
/// a layer's space and the box drawn around that layer agree.
pub fn world_transform(doc: &Document, id: NodeId) -> Result<Transform, DocError> {
    Ok(ancestor_space(doc, id).compose(doc.node(id)?.transform))
}

/// How much the space a brush writes into is stretched on the page —
/// what a length written in it is worth in document pixels.
pub fn layer_scale(doc: &Document, id: NodeId, on_mask: bool) -> Result<f32, DocError> {
    Ok(max_scale(brush_space(doc, id, on_mask)?))
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

/// One ring of a path's stroke skeleton: the points of the flattened
/// ring, the half-width at each of them, and whether the ring closes.
pub struct StrokeRing {
    pub points: Vec<[f32; 2]>,
    /// Half-width at each point, one entry per point.
    pub half: Vec<f32>,
    pub closed: bool,
}

/// The skeleton a path's stroke covers, in the shape's local space.
///
/// The stroke is the union of the round-capped segments between
/// consecutive points of these rings, the half-width running linearly
/// from one point to the next. That is the region [`stroke_covers`]
/// tests one sample at a time; this states it as geometry, for a
/// renderer that draws the region instead of sampling it. The two must
/// agree, and what keeps them honest is the GPU backend's test, which
/// draws a stroked page both ways and compares the pixels.
///
/// Empty for anything but a path, or a path with fewer than two points
/// — which is what the sampler answers `false` to everywhere.
pub fn stroke_skeleton(shape: &VectorShape, width: f32, widths: &[f32]) -> Vec<StrokeRing> {
    let flat = flatten_shape(shape);
    let VectorShape::Path {
        points,
        closed,
        subpaths,
        ..
    } = flat.as_ref()
    else {
        return Vec::new();
    };
    if points.len() < 2 {
        return Vec::new();
    }
    let half = width / 2.0;
    // The same widths the sampler is handed: resampled onto the
    // flattened polyline, and absent when the stroke does not vary.
    let flat_widths = flatten_widths(shape, widths);
    let varying = !flat_widths.is_empty();
    let at = |i: usize| flat_widths.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0);
    let mut rings = vec![StrokeRing {
        half: (0..points.len())
            .map(|i| if varying { half * at(i) } else { half })
            .collect(),
        points: points.clone(),
        closed: *closed,
    }];
    // Extra rings are always closed and never carry varying widths.
    for ring in subpaths {
        if ring.len() >= 2 {
            rings.push(StrokeRing {
                half: vec![half; ring.len()],
                points: ring.clone(),
                closed: true,
            });
        }
    }
    rings
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
    /// A painted mask's coverage, worked out over the region being drawn
    /// before any of it is read.
    plane: Option<&'a MaskPlane>,
}

impl<'a> MaskRef<'a> {
    /// `parent` is the space the mask is authored in — its owner's parent,
    /// since a mask describes the document as the layer sees it.
    fn new(mask: Option<&'a Mask>, parent: Transform) -> MaskRef<'a> {
        let t = match mask.map(|m| &m.kind) {
            Some(MaskKind::Vector { transform, .. } | MaskKind::Raster { transform, .. }) => {
                parent.compose(*transform)
            }
            // A painted mask's strokes are written in the space the mask
            // is authored in, so there is nothing more to compose.
            Some(MaskKind::Painted { .. }) | None => parent,
        };
        MaskRef {
            mask,
            t,
            inv: Inverse::of(t),
            plane: None,
        }
    }

    /// The coverage a painted mask was rasterized into, which is what
    /// reading one comes down to.
    fn with_plane(self, plane: Option<&'a MaskPlane>) -> MaskRef<'a> {
        MaskRef { plane, ..self }
    }

    /// The plane a painted mask needs before it can be read, over the
    /// region about to be drawn.
    fn plane_for(
        mask: Option<&Mask>,
        parent: Transform,
        clip: ClipRect,
        surface: (u32, u32),
    ) -> Option<MaskPlane> {
        match mask.map(|m| &m.kind) {
            Some(MaskKind::Painted { strokes }) => {
                Some(paint_plane(strokes, parent, clip, surface))
            }
            _ => None,
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
        // Read off the plane it was worked out into; without one there
        // is nothing to read, and a mask that shows everything is the
        // harmless answer.
        MaskKind::Painted { .. } => m.plane.map_or(1.0, |p| p.at(x, y)),
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
fn apply_mask(doc: &Document, m: MaskRef<'_>, surface: &mut Surface, clip: ClipRect) {
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

/// The coverage raster a text layer is drawn from, and the scale it was
/// rasterized at.
///
/// The text is rasterized at the size it will be seen at under `t`, so
/// magnifying a text layer sharpens its outlines instead of enlarging
/// their pixels; the cap keeps a wildly zoomed layer from asking for an
/// enormous bitmap, past which the glyphs are already far finer than the
/// screen. A backend that draws the raster itself rather than sampling
/// it here takes it from this function, so that what it draws is what
/// this renderer would have drawn.
pub fn text_raster(spec: &chitrakar_doc::TextSpec, t: Transform) -> (text::TextRaster, f32) {
    let [bx0, by0, bx1, by1] = text::bounds(spec);
    let natural = (bx1 - bx0, by1 - by0);
    let ceiling = (8192.0 / natural.0.max(natural.1).max(1.0)).min(64.0);
    let scale = max_scale(t).clamp(0.02, ceiling.max(0.02));
    (text::rasterize_at(spec, scale), scale)
}

/// How much paint a stroke lays at a point of the layer's own space.
///
/// The stroke covers the union of the round-capped segments between its
/// points — the shape a stroked path covers — with the coverage falling
/// off across the soft edge instead of stopping dead. The pieces are
/// combined by taking the most any of them lays rather than adding
/// them, so a stroke that doubles back over itself is not darker where
/// it crossed: within one stroke the brush lays paint once.
///
/// `band` is the narrowest that fade may be, in the layer's own units —
/// one device pixel — so even a hard brush has an antialiased edge.
fn stroke_coverage(stroke: &chitrakar_doc::PaintStroke, band: f32, x: f32, y: f32) -> f32 {
    let n = stroke.points.len();
    if n == 0 {
        return 0.0;
    }
    let softness = stroke.softness.clamp(0.0, 1.0);
    let mut most = 0.0f32;
    // One point is a single dab, which is the segment from it to itself.
    for i in 0..n.saturating_sub(1).max(1) {
        let j = (i + 1).min(n - 1);
        let (a, b) = (stroke.points[i], stroke.points[j]);
        let (ra, rb) = (stroke.radius(i), stroke.radius(j));
        let t = segment_parameter(x, y, a, b);
        let r = ra + (rb - ra) * t;
        if r <= 0.0 {
            continue;
        }
        let fade = (r * softness).max(band);
        let d = segment_distance(x, y, a, b);
        most = most.max(((r - d) / fade).clamp(0.0, 1.0));
        if most >= 1.0 {
            return 1.0;
        }
    }
    most
}

/// Coverage worked out ahead of a mask being read, for a mask that
/// cannot be answered a pixel at a time: a painted one is a stack of
/// strokes, and asking every stroke about every pixel would cost the
/// strokes' length all over again at each of them.
struct MaskPlane {
    clip: ClipRect,
    cover: Vec<f32>,
}

impl MaskPlane {
    fn at(&self, x: u32, y: u32) -> f32 {
        if x < self.clip.x0 || y < self.clip.y0 || x >= self.clip.x1 || y >= self.clip.y1 {
            // Nothing was worked out there, and a mask that shows
            // everything is the harmless answer.
            return 1.0;
        }
        let w = self.clip.x1 - self.clip.x0;
        self.cover[((y - self.clip.y0) * w + (x - self.clip.x0)) as usize]
    }
}

/// A painted mask's coverage over `clip`. It starts showing everything;
/// each stroke hides what it covers or shows it again, in the order the
/// strokes were laid.
fn paint_plane(
    strokes: &[chitrakar_doc::PaintStroke],
    t: Transform,
    clip: ClipRect,
    surface: (u32, u32),
) -> MaskPlane {
    let w = clip.x1.saturating_sub(clip.x0);
    let h = clip.y1.saturating_sub(clip.y0);
    let mut cover = vec![1.0f32; (w * h) as usize];
    if let Some(inv) = Inverse::of(t) {
        let band = 1.0 / max_scale(t).max(1e-6);
        for stroke in strokes {
            let Some(box_) = stroke.bounds() else {
                continue;
            };
            let bbox = match transformed_box(t, box_).to_clip(surface.0, surface.1) {
                Some(b) => b.intersect(clip),
                None => continue,
            };
            if bbox.is_empty() {
                continue;
            }
            let bw = bbox.x1 - bbox.x0;
            let laid = stroke_cover(stroke, t, inv, band, bbox, surface);
            for py in bbox.y0..bbox.y1 {
                for px in bbox.x0..bbox.x1 {
                    let c = laid[((py - bbox.y0) * bw + (px - bbox.x0)) as usize];
                    if c <= 0.0 {
                        continue;
                    }
                    let k = ((py - clip.y0) * w + (px - clip.x0)) as usize;
                    cover[k] = if stroke.erase {
                        cover[k] * (1.0 - c)
                    } else {
                        cover[k] + c * (1.0 - cover[k])
                    };
                }
            }
        }
    }
    MaskPlane { clip, cover }
}

/// A stroke's coverage over `bbox`, one value a pixel.
///
/// Gathered one segment at a time over that segment's own box rather
/// than asking every pixel of the stroke's box about every segment of
/// it: a long stroke has hundreds of segments and a box the size of the
/// canvas, so the difference is between costing what the stroke covers
/// and costing that times how long it is. The pieces combine by taking
/// the most any of them lays rather than adding them, so a stroke that
/// doubles back is not darker where it crossed.
fn stroke_cover(
    stroke: &chitrakar_doc::PaintStroke,
    t: Transform,
    inv: Inverse,
    band: f32,
    bbox: ClipRect,
    surface: (u32, u32),
) -> Vec<f32> {
    let w = bbox.x1 - bbox.x0;
    let mut cover = vec![0.0f32; (w * (bbox.y1 - bbox.y0)) as usize];
    let n = stroke.points.len();
    let softness = stroke.softness.clamp(0.0, 1.0);
    // One point is a single dab, which is the segment from it to itself.
    for i in 0..n.saturating_sub(1).max(1) {
        let j = (i + 1).min(n - 1);
        let (a, b) = (stroke.points[i], stroke.points[j]);
        let (ra, rb) = (stroke.radius(i), stroke.radius(j));
        let reach = ra.max(rb);
        if reach <= 0.0 {
            continue;
        }
        let seg = [
            a[0].min(b[0]) - reach,
            a[1].min(b[1]) - reach,
            a[0].max(b[0]) + reach,
            a[1].max(b[1]) + reach,
        ];
        let Some(sb) = transformed_box(t, seg)
            .to_clip(surface.0, surface.1)
            .map(|c| c.intersect(bbox))
        else {
            continue;
        };
        for py in sb.y0..sb.y1 {
            for px in sb.x0..sb.x1 {
                let (lx, ly) = inv.at(px as f32 + 0.5, py as f32 + 0.5);
                let along = segment_parameter(lx, ly, a, b);
                let r = ra + (rb - ra) * along;
                if r <= 0.0 {
                    continue;
                }
                let fade = (r * softness).max(band);
                let c = ((r - segment_distance(lx, ly, a, b)) / fade).clamp(0.0, 1.0);
                let k = ((py - bbox.y0) * w + (px - bbox.x0)) as usize;
                if c > cover[k] {
                    cover[k] = c;
                }
            }
        }
    }
    cover
}

/// Lay a paint layer's strokes onto a surface, in the order they were
/// laid: paint goes on with source-over, an eraser takes off what is
/// already there.
///
/// A stroke's coverage is gathered into a buffer of its own first, one
/// segment at a time over that segment's own box, rather than asking
/// every pixel of the stroke's box about every segment of it. A long
/// stroke has hundreds of segments and a box the size of the canvas;
/// the difference is between costing what the stroke covers and costing
/// that times how long it is.
fn lay_strokes(
    dst: &mut Surface,
    doc: &Document,
    strokes: &[chitrakar_doc::PaintStroke],
    t: Transform,
    inv: Inverse,
    band: f32,
    clip: ClipRect,
) {
    for stroke in strokes {
        let Some(box_) = stroke.bounds() else {
            continue;
        };
        let bbox = match transformed_box(t, box_).to_clip(dst.width, dst.height) {
            Some(b) => b.intersect(clip),
            None => continue,
        };
        if bbox.is_empty() {
            continue;
        }
        let w = bbox.x1 - bbox.x0;
        let cover = stroke_cover(stroke, t, inv, band, bbox, (dst.width, dst.height));
        let color = resolve_color(doc, stroke.color);
        for py in bbox.y0..bbox.y1 {
            for px in bbox.x0..bbox.x1 {
                let c = cover[((py - bbox.y0) * w + (px - bbox.x0)) as usize];
                if c <= 0.0 {
                    continue;
                }
                let i = (py * dst.width + px) as usize;
                dst.pixels[i] = if stroke.erase {
                    scale_alpha(dst.pixels[i], 1.0 - c)
                } else {
                    scale_alpha(color, c).over(dst.pixels[i])
                };
            }
        }
    }
}

/// A layer on its own, drawn into a square `size` across: its own box
/// fitted inside and centred, on nothing, at whatever scale that takes.
/// Straight-alpha sRGB bytes, `size * size * 4` of them.
///
/// What comes out is what the page would have drawn of that layer,
/// effects and all — the same walk, called with one layer rather than a
/// group's worth. `None` when the layer has no box of its own: an
/// adjustment or a filter is a change to what lies under it, and shows
/// nothing on a transparent square. A layer never scales up past its own
/// size, so a small one sits small in its square rather than going soft.
pub fn thumbnail(doc: &Document, id: NodeId, size: u32) -> Result<Option<Vec<u8>>, DocError> {
    let Some(fit) = fit_into_square(doc, id, size)? else {
        return Ok(None);
    };
    let mut surface = Surface::new(size, size);
    let clip = ClipRect {
        x0: 0,
        y0: 0,
        x1: size,
        y1: size,
    };
    render_layer(doc, id, &mut surface, clip, fit)?;
    let mut rgba8 = Vec::with_capacity((size * size) as usize * 4);
    for px in &surface.pixels {
        rgba8.extend_from_slice(&px.to_srgb8());
    }
    Ok(Some(rgba8))
}

/// Paint a clone layer: each stroke lays down what the page already
/// shows at its own offset, so the picture it puts there is whatever is
/// there *now* — retouch the source and the clone follows.
///
/// The page is snapshotted before any of it is laid, so a stroke that
/// runs over its own source reads what was there when the stroke began
/// rather than what it has just painted, which is the difference
/// between cloning a patch and smearing it.
#[allow(clippy::too_many_arguments)]
fn draw_clone(
    dst: &mut Surface,
    doc: &Document,
    strokes: &[chitrakar_doc::PaintStroke],
    t: Transform,
    opacity: f32,
    blend: BlendMode,
    clip: ClipRect,
    mask: MaskRef<'_>,
) {
    let Some(inv) = Inverse::of(t) else {
        return;
    };
    let Some(extent) = painted_bounds(strokes)
        .map(|b| transformed_box(t, b))
        .and_then(|b| b.to_clip(dst.width, dst.height))
        .map(|b| b.intersect(clip))
    else {
        return;
    };
    if extent.is_empty() {
        return;
    }
    let band = 1.0 / max_scale(t).max(1e-6);
    for stroke in strokes {
        let Some(box_) = stroke.bounds() else {
            continue;
        };
        let bbox = match transformed_box(t, box_).to_clip(dst.width, dst.height) {
            Some(b) => b.intersect(extent),
            None => continue,
        };
        if bbox.is_empty() {
            continue;
        }
        let w = bbox.x1 - bbox.x0;
        let cover = stroke_cover(stroke, t, inv, band, bbox, (dst.width, dst.height));
        // The offset is written in the layer's own space; on the page it
        // is that offset carried through the layer's transform, without
        // the translation — a direction, not a place.
        let (sx, sy) = (
            t.a * stroke.source[0] + t.c * stroke.source[1],
            t.b * stroke.source[0] + t.d * stroke.source[1],
        );
        // What is under the source, taken before this stroke lays
        // anything: a stroke that runs over its own source would
        // otherwise read what it has just painted and smear it along.
        // Taken per stroke, so a later one does see an earlier one.
        let from = ClipRect::from_float(
            bbox.x0 as f32 + sx,
            bbox.y0 as f32 + sy,
            bbox.x1 as f32 + sx,
            bbox.y1 as f32 + sy,
            dst.width,
            dst.height,
        );
        if from.is_empty() {
            continue;
        }
        let source = blur::snapshot(dst, from);
        let row = (from.x1 - from.x0) as usize;
        let read = |x: i64, y: i64| {
            if x < from.x0 as i64
                || y < from.y0 as i64
                || x >= from.x1 as i64
                || y >= from.y1 as i64
            {
                // Off the page there is nothing to clone.
                return LinearRgba::TRANSPARENT;
            }
            source[(y - from.y0 as i64) as usize * row + (x - from.x0 as i64) as usize]
        };
        let at_source = |px: u32, py: u32| {
            read(
                (px as f32 + sx).round() as i64,
                (py as f32 + sy).round() as i64,
            )
        };
        // Healing takes the texture from the source and the colour from
        // where it lands: the shift between what the two average over
        // the stroke, added to every pixel it lifts. That is what lets a
        // patch taken from somewhere lighter sit into its surroundings
        // rather than showing as a disc.
        let shift = stroke.heal.then(|| {
            let (mut lifted, mut under, mut total) = ([0.0f32; 3], [0.0f32; 3], 0.0f32);
            for py in bbox.y0..bbox.y1 {
                for px in bbox.x0..bbox.x1 {
                    let c = cover[((py - bbox.y0) * w + (px - bbox.x0)) as usize];
                    let from = at_source(px, py);
                    let to = dst.pixels[(py * dst.width + px) as usize];
                    if c <= 0.0 || from.a <= 0.0 || to.a <= 0.0 {
                        continue;
                    }
                    for (k, (f, t)) in [(from.r, to.r), (from.g, to.g), (from.b, to.b)]
                        .into_iter()
                        .enumerate()
                    {
                        lifted[k] += f / from.a * c;
                        under[k] += t / to.a * c;
                    }
                    total += c;
                }
            }
            if total <= 0.0 {
                return [0.0f32; 3];
            }
            [
                (under[0] - lifted[0]) / total,
                (under[1] - lifted[1]) / total,
                (under[2] - lifted[2]) / total,
            ]
        });
        for py in bbox.y0..bbox.y1 {
            for px in bbox.x0..bbox.x1 {
                let c = cover[((py - bbox.y0) * w + (px - bbox.x0)) as usize];
                if c <= 0.0 {
                    continue;
                }
                let weight = c * opacity * coverage_at(doc, mask, px, py);
                if weight <= 0.0 {
                    continue;
                }
                let mut lifted = at_source(px, py);
                if lifted.a <= 0.0 {
                    continue;
                }
                if let Some(shift) = shift {
                    let a = lifted.a;
                    let moved = |v: f32, d: f32| (v / a + d).max(0.0) * a;
                    lifted = LinearRgba {
                        r: moved(lifted.r, shift[0]),
                        g: moved(lifted.g, shift[1]),
                        b: moved(lifted.b, shift[2]),
                        a,
                    };
                }
                let i = (py * dst.width + px) as usize;
                dst.pixels[i] = blend_pixel(scale_alpha(lifted, weight), dst.pixels[i], blend);
            }
        }
    }
}

/// A paint layer handed over as an image: its size, where its top-left
/// sits in the layer's own space, and its pixels as straight-alpha sRGB
/// bytes.
pub struct PaintedPixels {
    pub width: u32,
    pub height: u32,
    pub origin: [f32; 2],
    pub rgba8: Vec<u8>,
}

/// Where a layer's box goes on a square `size` across: fitted inside
/// and centred, and never enlarged past its own size, so a small layer
/// sits small in its square rather than going soft. The layer's own
/// transform is applied by the walk; this only places the box it lands
/// in. `None` when the layer has no box.
fn fit_into_square(doc: &Document, id: NodeId, size: u32) -> Result<Option<Transform>, DocError> {
    if size == 0 {
        return Ok(None);
    }
    let Bounds::Rect(x0, y0, x1, y1) = bounds_in_parent_space(doc, id)? else {
        return Ok(None);
    };
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w > 0.0 && h > 0.0) {
        return Ok(None);
    }
    let scale = (size as f32 / w.max(h)).min(1.0);
    Ok(Some(Transform {
        a: scale,
        b: 0.0,
        c: 0.0,
        d: scale,
        e: (size as f32 - w * scale) / 2.0 - x0 * scale,
        f: (size as f32 - h * scale) / 2.0 - y0 * scale,
    }))
}

/// What a copy draws in place of the original's children: the original's
/// own, except where the copy stands in for one with a layer of its own.
///
/// A stand-in that names a position the original no longer has, and any
/// layer put into a copy that stands in for nothing, are drawn after the
/// rest — so dropping a layer into a copy adds to it rather than losing
/// it. Empty when the copy has nothing of its own, which is the answer
/// that lets the renderer take the plainer path.
pub fn copy_children(doc: &Document, instance: NodeId) -> Result<Vec<NodeId>, DocError> {
    let NodeKind::Instance { of, replaces } = &doc.node(instance)?.kind else {
        return Ok(Vec::new());
    };
    let mine = doc.children_of(instance)?;
    if mine.is_empty() {
        return Ok(Vec::new());
    }
    let theirs = doc.children_of(*of).unwrap_or(&[]);
    let mut out = Vec::with_capacity(theirs.len() + mine.len());
    for (i, &original) in theirs.iter().enumerate() {
        match replaces.iter().position(|&r| r == i) {
            Some(k) if k < mine.len() => out.push(mine[k]),
            _ => out.push(original),
        }
    }
    for (k, &own) in mine.iter().enumerate() {
        if replaces.get(k).is_none_or(|&r| r >= theirs.len()) {
            out.push(own);
        }
    }
    Ok(out)
}

/// Whether the original is one a copy can stand in for parts of: a plain
/// group, composited exactly as its children would be. A group that is
/// isolated for its own opacity, blend, mask or effects is drawn as a
/// whole, and swapping a layer inside it would mean drawing it twice.
pub fn takes_stand_ins(doc: &Document, master: NodeId) -> bool {
    let Ok(node) = doc.node(master) else {
        return false;
    };
    matches!(node.kind, NodeKind::Group)
        && node.opacity >= 1.0
        && node.blend == BlendMode::Normal
        && node.mask.is_none()
        && node.effects.is_empty()
}

/// A transform's inverse as a transform, for the times a caller needs the
/// map itself rather than the per-point [`Inverse`] — turning a frame's
/// placement on the page back into the frame's own space, say, or keeping
/// a layer where it is while it changes parents. `None` when the
/// transform collapses.
pub fn invert(t: Transform) -> Option<Transform> {
    let det = t.a * t.d - t.b * t.c;
    if det.abs() < 1e-12 {
        return None;
    }
    let (a, b, c, d) = (t.d / det, -t.b / det, -t.c / det, t.a / det);
    Some(Transform {
        a,
        b,
        c,
        d,
        e: -(a * t.e + c * t.f),
        f: -(b * t.e + d * t.f),
    })
}

/// The space a node's children are written in: its own transform with
/// every ancestor's on top, which is what carries a point on the page
/// into the coordinates a layer inside it uses.
pub fn own_space(doc: &Document, id: NodeId) -> Result<Transform, DocError> {
    Ok(ancestor_space(doc, id).compose(doc.node(id)?.transform))
}

/// The frame under a document point: the topmost artboard whose own box
/// covers it, searched the way a pick is. `None` off every frame.
pub fn frame_at(doc: &Document, x: f32, y: f32) -> Result<Option<NodeId>, DocError> {
    frame_in(doc, doc.root(), x, y, Transform::default())
}

fn frame_in(
    doc: &Document,
    group: NodeId,
    x: f32,
    y: f32,
    parent: Transform,
) -> Result<Option<NodeId>, DocError> {
    for &child in doc.children_of(group)?.iter().rev() {
        let node = doc.node(child)?;
        if !node.visible || node.locked {
            continue;
        }
        let t = parent.compose(node.transform);
        match &node.kind {
            NodeKind::Artboard { width, height, .. } => {
                if let Some((lx, ly)) = to_local(t, x, y) {
                    if lx >= 0.0 && ly >= 0.0 && lx < *width && ly < *height {
                        // A frame inside a frame is the one that counts.
                        return Ok(Some(frame_in(doc, child, x, y, t)?.unwrap_or(child)));
                    }
                }
            }
            NodeKind::Group => {
                if let Some(hit) = frame_in(doc, child, x, y, t)? {
                    return Ok(Some(hit));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

/// A frame rendered on its own, upright, at the size it shows on the
/// page times `scale` — what "export this artboard" means. Everything
/// outside the frame is left out, because the frame cuts to its box; a
/// frame that has been turned comes out square, since what is wanted is
/// its contents, not its angle.
///
/// `None` when the node is not a frame, or has collapsed to nothing.
pub fn artboard_pixels(
    doc: &Document,
    id: NodeId,
    scale: f32,
) -> Result<Option<Surface>, DocError> {
    let node = doc.node(id)?;
    let NodeKind::Artboard { width, height, .. } = &node.kind else {
        return Ok(None);
    };
    let world = ancestor_space(doc, id).compose(node.transform);
    // How much the page enlarges the frame, on each of its own axes.
    let (sx, sy) = (
        (world.a * world.a + world.b * world.b).sqrt(),
        (world.c * world.c + world.d * world.d).sqrt(),
    );
    let (pw, ph) = (
        (width * sx * scale).round().max(1.0),
        (height * sy * scale).round().max(1.0),
    );
    if !(pw.is_finite() && ph.is_finite() && pw <= 16384.0 && ph <= 16384.0) {
        return Ok(None);
    }
    let fit = Transform {
        a: pw / width,
        d: ph / height,
        ..Default::default()
    };
    let Some(back) = invert(node.transform) else {
        return Ok(None);
    };
    let mut surface = Surface::new(pw as u32, ph as u32);
    let clip = surface.full_clip();
    // `render_layer` composes the node's own transform onto what it is
    // given, so undo that transform first: what is left is the frame in
    // its own space, blown up to the surface.
    render_layer(doc, id, &mut surface, clip, fit.compose(back))?;
    Ok(Some(surface))
}

/// A layer's mask fitted into the same square [`thumbnail`] fits the
/// layer into, so the two sit side by side and line up: white where the
/// layer shows through and clear where it is hidden.
///
/// `None` when the layer has no mask, or no box to fit.
pub fn mask_thumbnail(doc: &Document, id: NodeId, size: u32) -> Result<Option<Vec<u8>>, DocError> {
    let node = doc.node(id)?;
    let Some(mask) = node.mask.as_ref() else {
        return Ok(None);
    };
    let Some(fit) = fit_into_square(doc, id, size)? else {
        return Ok(None);
    };
    let clip = ClipRect {
        x0: 0,
        y0: 0,
        x1: size,
        y1: size,
    };
    let plane = MaskRef::plane_for(Some(mask), fit, clip, (size, size));
    let m = MaskRef::new(Some(mask), fit).with_plane(plane.as_ref());
    let mut rgba8 = Vec::with_capacity((size * size) as usize * 4);
    for y in 0..size {
        for x in 0..size {
            let a = (coverage_at(doc, m, x, y).clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba8.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    Ok(Some(rgba8))
}

/// The layer a clipped layer is confined to: the nearest sibling below it
/// that is not itself clipped. `None` when the layer is not clipped, or
/// when nothing is under it — a run's own base carries no confinement,
/// however its flag is set.
pub fn clip_base(doc: &Document, id: NodeId) -> Result<Option<NodeId>, DocError> {
    if !doc.node(id)?.clipped {
        return Ok(None);
    }
    let Some(parent) = doc.parent_of(id) else {
        return Ok(None);
    };
    let siblings = doc.children_of(parent)?;
    let Some(mut at) = siblings.iter().position(|&s| s == id) else {
        return Ok(None);
    };
    while at > 0 {
        at -= 1;
        if !doc.node(siblings[at])?.clipped {
            return Ok(Some(siblings[at]));
        }
    }
    Ok(None)
}

/// What a clipped layer is let through by, as an image over the box of
/// the layer it is clipped to — white with that layer's own alpha, the
/// same shape [`mask_pixels`] hands a mask over in. For an exporter whose
/// format has no clipping of this kind and has to say it as a mask.
/// `None` when the layer is not clipped, or its base covers nothing.
pub fn clip_pixels(doc: &Document, id: NodeId) -> Result<Option<PaintedPixels>, DocError> {
    let Some(base) = clip_base(doc, id)? else {
        return Ok(None);
    };
    let Bounds::Rect(x0, y0, x1, y1) = bounds_in_parent_space(doc, base)? else {
        return Ok(None);
    };
    let (w, h) = (
        (x1 - x0).ceil().max(1.0) as u32,
        (y1 - y0).ceil().max(1.0) as u32,
    );
    if w > 16384 || h > 16384 {
        return Ok(None);
    }
    // The base is drawn in the space it shares with the layer clipped to
    // it, shifted so its own box starts at the image's corner.
    let space = Transform::translation(-x0, -y0);
    let mut surface = Surface::new(w, h);
    let clip = surface.full_clip();
    render_layer(doc, base, &mut surface, clip, space)?;
    let mut rgba8 = Vec::with_capacity((w * h) as usize * 4);
    for p in &surface.pixels {
        rgba8.extend_from_slice(&[255, 255, 255, (p.a.clamp(0.0, 1.0) * 255.0).round() as u8]);
    }
    Ok(Some(PaintedPixels {
        width: w,
        height: h,
        origin: [x0, y0],
        rgba8,
    }))
}

/// A layer's mask as an image: what it lets through over the layer's
/// own box, one pixel per document unit.
///
/// White throughout with the coverage in the alpha channel, which reads
/// the same whether the format takes a mask by its luminance or by its
/// alpha — white has luminance 1, so one is the other. For an exporter
/// whose format has no mask like ours and has to hand one over as a
/// picture. `None` when the layer has no mask, or no box to draw it in.
pub fn mask_pixels(doc: &Document, id: NodeId) -> Result<Option<PaintedPixels>, DocError> {
    let node = doc.node(id)?;
    let Some(mask) = node.mask.as_ref() else {
        return Ok(None);
    };
    let Bounds::Rect(x0, y0, x1, y1) = bounds_in_parent_space(doc, id)? else {
        return Ok(None);
    };
    let (w, h) = (
        (x1 - x0).ceil().max(1.0) as u32,
        (y1 - y0).ceil().max(1.0) as u32,
    );
    if w > 16384 || h > 16384 {
        return Ok(None);
    }
    // A mask is authored in the space the layer sits in, so the image's
    // top-left corner is where that box starts.
    let space = Transform::translation(-x0, -y0);
    let clip = ClipRect {
        x0: 0,
        y0: 0,
        x1: w,
        y1: h,
    };
    let plane = MaskRef::plane_for(Some(mask), space, clip, (w, h));
    let m = MaskRef::new(Some(mask), space).with_plane(plane.as_ref());
    let mut rgba8 = Vec::with_capacity((w * h) as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let a = (coverage_at(doc, m, x, y).clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba8.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    Ok(Some(PaintedPixels {
        width: w,
        height: h,
        origin: [x0, y0],
        rgba8,
    }))
}

/// A paint layer rendered on its own at one pixel per document unit,
/// for an exporter whose format has no brush in it and has to hand the
/// layer over as an image. `None` when the layer has no paint on it.
pub fn paint_pixels(doc: &Document, id: NodeId) -> Result<Option<PaintedPixels>, DocError> {
    let node = doc.node(id)?;
    let NodeKind::Paint { strokes } = &node.kind else {
        return Ok(None);
    };
    let Some([x0, y0, x1, y1]) = painted_bounds(strokes) else {
        return Ok(None);
    };
    let (w, h) = (
        (x1 - x0).ceil().max(1.0) as u32,
        (y1 - y0).ceil().max(1.0) as u32,
    );
    if w > 16384 || h > 16384 {
        return Ok(None);
    }
    let t = Transform::translation(-x0, -y0);
    let Some(inv) = Inverse::of(t) else {
        return Ok(None);
    };
    let mut surface = Surface::new(w, h);
    let clip = ClipRect {
        x0: 0,
        y0: 0,
        x1: w,
        y1: h,
    };
    lay_strokes(&mut surface, doc, strokes, t, inv, 1.0, clip);
    let mut rgba8 = Vec::with_capacity((w * h) as usize * 4);
    for px in &surface.pixels {
        rgba8.extend_from_slice(&px.to_srgb8());
    }
    Ok(Some(PaintedPixels {
        width: w,
        height: h,
        origin: [x0, y0],
        rgba8,
    }))
}

/// Paint a brush layer.
///
/// Strokes go straight onto the destination when nothing about the
/// layer needs it composited as a whole: source-over is associative, so
/// laying them one after another there gives the same picture. An
/// eraser, a layer opacity, a blend or a mask does need it — what an
/// eraser takes off is the layer's own paint, not what lies under it —
/// and then the layer is laid on a surface of its own first.
#[allow(clippy::too_many_arguments)]
fn draw_paint(
    dst: &mut Surface,
    doc: &Document,
    strokes: &[chitrakar_doc::PaintStroke],
    t: Transform,
    opacity: f32,
    blend: BlendMode,
    clip: ClipRect,
    mask: MaskRef<'_>,
) {
    let Some(inv) = Inverse::of(t) else {
        return;
    };
    // The softest fade is still one device pixel wide, so a hard brush
    // has an antialiased edge for the same reason every other edge here
    // does.
    let band = 1.0 / max_scale(t).max(1e-6);
    let alone = opacity >= 1.0
        && blend == BlendMode::Normal
        && mask.mask.is_none()
        && !strokes.iter().any(|s| s.erase);
    if alone {
        lay_strokes(dst, doc, strokes, t, inv, band, clip);
        return;
    }
    let Some(extent) = painted_bounds(strokes)
        .map(|b| transformed_local_bounds(t, (b[0], b[1], b[2], b[3])))
        .and_then(|b| b.to_clip(dst.width, dst.height))
        .map(|b| b.intersect(clip))
    else {
        return;
    };
    if extent.is_empty() {
        return;
    }
    let mut sub = Surface::new(dst.width, dst.height);
    lay_strokes(&mut sub, doc, strokes, t, inv, band, extent);
    for py in extent.y0..extent.y1 {
        for px in extent.x0..extent.x1 {
            let w = opacity * coverage_at(doc, mask, px, py);
            if w <= 0.0 {
                continue;
            }
            let i = (py * dst.width + px) as usize;
            let src = sub.pixels[i];
            if src.a <= 0.0 && src.r == 0.0 && src.g == 0.0 && src.b == 0.0 {
                continue;
            }
            dst.pixels[i] = blend_pixel(scale_alpha(src, w), dst.pixels[i], blend);
        }
    }
}

/// Rasterize a text block at the size it will be seen at and blit its
/// coverage through the node transform.
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
    let [bx0, by0, bx1, by1] = text::bounds(spec);
    let (raster, scale) = text_raster(spec, t);
    let color = resolve_color(doc, spec.fill);
    // The box is the block's natural size, not the raster's: those agree
    // only while the raster is at natural scale, and a minified one would
    // otherwise clip its own right and bottom edges away.
    let bbox =
        match transformed_local_bounds(t, (bx0, by0, bx1, by1)).to_clip(dst.width, dst.height) {
            Some(b) => b.intersect(clip),
            None => return,
        };
    let (ox, oy) = raster.origin;
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            let (lx, ly) = inv.at(px as f32 + 0.5, py as f32 + 0.5);
            let (lx, ly) = (lx - ox, ly - oy);
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

/// Tabulate a tone curve: 257 outputs for inputs 0..=1 in steps of 1/256,
/// both in the display encoding. A monotone cubic (Fritsch–Carlson)
/// through the sorted points, so the curve never overshoots the points
/// it is drawn through and a rising set of points gives a rising curve;
/// flat beyond the first and last points. Fewer than two distinct points
/// is the identity.
/// The tables a curves adjustment reads: the master curve every channel
/// goes through, and a curve of its own for each channel that has one.
/// Tabulated once per pass rather than solved per pixel.
#[derive(Default)]
pub struct CurveLuts {
    pub master: Vec<f32>,
    pub red: Option<Vec<f32>>,
    pub green: Option<Vec<f32>>,
    pub blue: Option<Vec<f32>>,
}

/// Tabulate a curves adjustment's tables, or nothing when the adjustment
/// is not one. A channel with fewer than two points is left out: its
/// curve would be the identity, and skipping it saves the lookup.
pub fn curve_luts(adj: &Adjustment) -> Option<CurveLuts> {
    let Adjustment::Curves {
        points,
        red,
        green,
        blue,
    } = adj
    else {
        return None;
    };
    let channel = |pts: &Vec<[f32; 2]>| (pts.len() >= 2).then(|| curve_lut(pts));
    Some(CurveLuts {
        master: curve_lut(points),
        red: channel(red),
        green: channel(green),
        blue: channel(blue),
    })
}

pub fn curve_lut(points: &[[f32; 2]]) -> Vec<f32> {
    let mut pts: Vec<[f32; 2]> = points
        .iter()
        .map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0)])
        .collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    // Two points on one input would make a vertical step; the later one
    // wins, as the one most recently placed there.
    pts.dedup_by(|later, earlier| {
        if (later[0] - earlier[0]).abs() < 1e-6 {
            earlier[1] = later[1];
            true
        } else {
            false
        }
    });
    let n = pts.len();
    if n < 2 {
        return (0..=256).map(|i| i as f32 / 256.0).collect();
    }
    let h: Vec<f32> = (0..n - 1).map(|i| pts[i + 1][0] - pts[i][0]).collect();
    let d: Vec<f32> = (0..n - 1)
        .map(|i| (pts[i + 1][1] - pts[i][1]) / h[i])
        .collect();
    let mut m = vec![0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] > 0.0 {
            (d[i - 1] + d[i]) / 2.0
        } else {
            0.0
        };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
        let r = a * a + b * b;
        if r > 9.0 {
            let t = 3.0 / r.sqrt();
            m[i] = t * a * d[i];
            m[i + 1] = t * b * d[i];
        }
    }
    let mut seg = 0;
    (0..=256)
        .map(|i| {
            let x = i as f32 / 256.0;
            if x <= pts[0][0] {
                return pts[0][1];
            }
            if x >= pts[n - 1][0] {
                return pts[n - 1][1];
            }
            while pts[seg + 1][0] < x {
                seg += 1;
            }
            let t = (x - pts[seg][0]) / h[seg];
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * pts[seg][1]
                + (t3 - 2.0 * t2 + t) * h[seg] * m[seg]
                + (-2.0 * t3 + 3.0 * t2) * pts[seg + 1][1]
                + (t3 - t2) * h[seg] * m[seg + 1];
            y.clamp(0.0, 1.0)
        })
        .collect()
}

/// Read a tabulated curve at `x`, between its entries.
fn curve_at(lut: &[f32], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0) * 256.0;
    let i = (x as usize).min(255);
    let t = x - i as f32;
    lut[i] + (lut[i + 1] - lut[i]) * t
}

/// `lut` is the tabulated curve when `adj` is one — built by the caller
/// once per pass; built here when the caller has not, so the function
/// stays total.
fn apply_adjustment(adj: &Adjustment, luts: Option<&CurveLuts>, px: LinearRgba) -> LinearRgba {
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
        Adjustment::Levels {
            in_black,
            in_white,
            gamma,
            out_black,
            out_white,
        } => {
            // An input range that has collapsed still has to map somewhere:
            // hold it a hair wide rather than divide by nothing.
            let span = (in_white - in_black).max(1e-3);
            let exponent = 1.0 / gamma.max(0.05);
            let f = |v: f32| {
                let v = ((v - in_black) / span).clamp(0.0, 1.0).powf(exponent);
                (out_black + v * (out_white - out_black)).clamp(0.0, 1.0)
            };
            (f(r), f(g), f(b))
        }
        Adjustment::WhiteBalance { temperature, tint } => {
            // A light's colour is a gain per channel, and that is what
            // balancing one is: warming lifts red and drops blue, and
            // the tint runs green against magenta. Half the slider's
            // travel at each end, so the extremes still hold a picture.
            let warm = temperature.clamp(-1.0, 1.0) * 0.5;
            let mag = tint.clamp(-1.0, 1.0) * 0.5;
            let f = |v: f32, gain: f32| (v * gain).clamp(0.0, 1.0);
            (f(r, 1.0 + warm), f(g, 1.0 - mag), f(b, 1.0 - warm))
        }
        Adjustment::Vibrance { amount } => {
            // Saturation weighted by how much the colour has already:
            // grey takes the whole change, a colour at full chroma takes
            // none of it. That is what keeps skin from going orange
            // while a dull sky comes up.
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            // Saturation as a fraction of the pixel's own brightness,
            // not as an absolute spread: the composite is unbounded
            // linear light, so a bright colour would otherwise read as
            // fully saturated and take none of the change merely for
            // being bright.
            let top = r.max(g).max(b);
            let sat = if top > 1e-6 {
                (top - r.min(g).min(b)) / top
            } else {
                0.0
            };
            let s = 1.0 + amount * (1.0 - sat.clamp(0.0, 1.0));
            let f = |v: f32| (lum + (v - lum) * s).clamp(0.0, 1.0);
            (f(r), f(g), f(b))
        }
        Adjustment::Curves { .. } => {
            let table;
            let luts = match luts {
                Some(luts) => luts,
                None => {
                    table = curve_luts(adj).unwrap_or_default();
                    &table
                }
            };
            // The curves are drawn over the display encoding; the pixel
            // is linear, so it crosses over and back. The master runs
            // first and each channel's own curve after it, which is the
            // order the graph is read in.
            let f = |v: f32, own: Option<&Vec<f32>>| {
                let s = curve_at(
                    &luts.master,
                    chitrakar_color::linear_to_srgb(v.clamp(0.0, 1.0)),
                );
                chitrakar_color::srgb_to_linear(match own {
                    Some(lut) => curve_at(lut, s),
                    None => s,
                })
            };
            (
                f(r, luts.red.as_ref()),
                f(g, luts.green.as_ref()),
                f(b, luts.blue.as_ref()),
            )
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
        // Locked layers are not there to be picked, their contents included.
        if !node.visible || node.locked {
            continue;
        }
        match &node.kind {
            NodeKind::Group => {
                if let Some(hit) = hit_in_group(doc, child, x, y, parent.compose(node.transform))? {
                    return Ok(Some(hit));
                }
            }
            NodeKind::Instance { of, .. } => {
                // Picked over the box the original occupies, carried into
                // the copy's own place — the same box the copy's handles
                // are drawn round, so what is picked is what is outlined.
                let Ok(Some(box_)) = local_bounds_of(doc, *of) else {
                    continue;
                };
                if let Some((lx, ly)) = to_local(parent.compose(node.transform), x, y) {
                    if lx >= box_[0] && ly >= box_[1] && lx < box_[2] && ly < box_[3] {
                        return Ok(Some(child));
                    }
                }
            }
            NodeKind::Artboard {
                width,
                height,
                background,
            } => {
                let t = parent.compose(node.transform);
                let Some((lx, ly)) = to_local(t, x, y) else {
                    continue;
                };
                // A frame cuts its contents to its box, so nothing outside
                // it can be picked through it — not even a layer that
                // reaches past the edge.
                if lx < 0.0 || ly < 0.0 || lx >= *width || ly >= *height {
                    continue;
                }
                if let Some(hit) = hit_in_group(doc, child, x, y, t)? {
                    return Ok(Some(hit));
                }
                // Its own ground picks the frame, the way clicking the
                // empty part of a frame picks the frame in every editor
                // that has them. A frame with no ground is a window onto
                // the page and lets the pick through.
                if background.is_some() {
                    return Ok(Some(child));
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
                    let [x0, y0, x1, y1] = text::bounds(spec);
                    if lx >= x0 && ly >= y0 && lx < x1 && ly < y1 {
                        return Ok(Some(child));
                    }
                }
            }
            // A paint layer is picked where it has paint, not over its
            // whole box: a brush layer is mostly empty, and an empty
            // part of it should let through what is under it. An
            // eraser's own stroke is not paint, so it picks nothing.
            NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => {
                let t = parent.compose(node.transform);
                if let Some((lx, ly)) = to_local(t, x, y) {
                    let band = 1.0 / max_scale(t).max(1e-6);
                    if strokes
                        .iter()
                        .any(|s| !s.erase && stroke_coverage(s, band, lx, ly) > 0.0)
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

    fn painted(doc: &mut Document, strokes: Vec<chitrakar_doc::PaintStroke>) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(Node::paint("paint")),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[index];
        for (i, stroke) in strokes.into_iter().enumerate() {
            doc.apply(Command::AddStroke {
                id,
                index: i,
                stroke: Box::new(stroke),
                on_mask: false,
            })
            .unwrap();
        }
        id
    }

    fn stroke(
        points: &[[f32; 2]],
        radius: f32,
        color: AuthoredColor,
    ) -> chitrakar_doc::PaintStroke {
        chitrakar_doc::PaintStroke {
            points: points.to_vec(),
            radii: vec![radius],
            color,
            softness: 0.0,
            erase: false,
            source: [0.0, 0.0],
            heal: false,
        }
    }

    fn add(doc: &mut Document, node: Box<Node>) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node,
        })
        .unwrap();
        doc.children_of(root).unwrap()[index]
    }

    const BLUE: AuthoredColor = AuthoredColor::Srgb {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    /// What a full-page blended layer costs. Blending reads the values a
    /// device shows, so each channel crosses the transfer curve twice —
    /// this is the price of that, measured rather than guessed at.
    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn blend_timing_probe() {
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let page = |mode| {
            let mut doc = Document::new(2480, 3508, ColorMode::Rgb);
            add(&mut doc, filled_rect("under", 2480.0, 3508.0, grey));
            let top = add(&mut doc, filled_rect("over", 2480.0, 3508.0, grey));
            doc.apply(Command::SetBlendMode {
                id: top,
                blend: mode,
            })
            .unwrap();
            doc
        };
        for mode in [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Overlay,
            BlendMode::Color,
        ] {
            let doc = page(mode);
            let t = std::time::Instant::now();
            let _ = render(&doc).unwrap();
            println!("A4 at 300dpi, {mode:?} over a full page: {:?}", t.elapsed());
        }
    }

    /// The tabulated transfer curve stands in for the real one on every
    /// channel of every blended pixel, so it has to agree with it far
    /// more finely than eight bits can tell.
    #[test]
    fn the_tabulated_curve_agrees_with_the_real_one() {
        let t = transfer();
        let mut worst_out = 0.0f32;
        let mut worst_back = 0.0f32;
        for i in 0..=20_000 {
            let v = i as f32 / 20_000.0;
            worst_out = worst_out
                .max((on_curve(&t.to_shown, v) - chitrakar_color::linear_to_srgb(v)).abs());
            worst_back = worst_back
                .max((on_curve(&t.to_linear, v) - chitrakar_color::srgb_to_linear(v)).abs());
        }
        assert!(
            worst_out < 1e-4 && worst_back < 1e-4,
            "off by {worst_out} going out and {worst_back} coming back"
        );
        // The ends land on the entries themselves rather than between
        // two of them, so they are whatever the real curve is — which is
        // a hair under one at the top, in both.
        for v in [0.0, 1.0] {
            assert_eq!(on_curve(&t.to_shown, v), chitrakar_color::linear_to_srgb(v));
            assert_eq!(
                on_curve(&t.to_linear, v),
                chitrakar_color::srgb_to_linear(v)
            );
        }
        // Past the ends it holds rather than running off.
        assert_eq!(on_curve(&t.to_shown, 2.0), on_curve(&t.to_shown, 1.0));
        assert_eq!(on_curve(&t.to_shown, -1.0), on_curve(&t.to_shown, 0.0));
    }

    /// A blend reads the values a device shows, not the linear light
    /// behind them — which is what the W3C spec says, what SVG's
    /// mix-blend-mode and PDF's /BM do, and so what keeps a page looking
    /// the same in the engine and in what it exports.
    ///
    /// Overlay is the sharpest way to see it: it pivots on the middle,
    /// and the middle of what is shown is a mid grey. Blended in linear
    /// light the same pixels come out far darker.
    #[test]
    fn a_blend_reads_the_values_the_page_shows() {
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let over = |mode| {
            let mut doc = Document::new(4, 4, ColorMode::Rgb);
            add(&mut doc, filled_rect("under", 4.0, 4.0, grey));
            let top = add(&mut doc, filled_rect("over", 4.0, 4.0, grey));
            doc.apply(Command::SetBlendMode {
                id: top,
                blend: mode,
            })
            .unwrap();
            render(&doc).unwrap().get(1, 1).to_srgb8()
        };
        let mid = over(BlendMode::Overlay)[0] as i32;
        assert!(
            (mid - 128).abs() <= 1,
            "a mid grey overlaid on itself is left where it is ({mid})"
        );
        // Multiply: half of what is shown, times half again.
        let dark = over(BlendMode::Multiply)[0] as i32;
        assert!((dark - 64).abs() <= 1, "half a half is a quarter ({dark})");
        let light = over(BlendMode::Screen)[0] as i32;
        assert!(
            (light - 191).abs() <= 1,
            "and screen is the other way ({light})"
        );
        assert_eq!(over(BlendMode::Darken)[0], over(BlendMode::Lighten)[0]);
        assert_eq!(
            over(BlendMode::Difference)[0],
            0,
            "a colour against itself is nothing"
        );
    }

    /// The four that take one part of a colour: Color puts the layer's
    /// hue and saturation on the backdrop's brightness, Luminosity the
    /// other way round.
    #[test]
    fn the_blends_that_take_one_part_of_a_colour() {
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let paint = |mode| {
            let mut doc = Document::new(4, 4, ColorMode::Rgb);
            add(&mut doc, filled_rect("under", 4.0, 4.0, grey));
            let top = add(&mut doc, filled_rect("over", 4.0, 4.0, RED));
            doc.apply(Command::SetBlendMode {
                id: top,
                blend: mode,
            })
            .unwrap();
            render(&doc).unwrap().get(1, 1).to_srgb8()
        };
        let coloured = paint(BlendMode::Color);
        assert!(
            coloured[0] > coloured[1] && coloured[1] == coloured[2],
            "red's hue on grey is a red ({coloured:?})"
        );
        // Pure red is dark for its hue, so taking the grey's brightness
        // lifts the other two channels rather than dropping the red —
        // the spec clips back into range about the luminosity it was
        // given, which is what puts red at the top.
        assert!(
            coloured[1] > 40,
            "carrying the grey's brightness, not red's ({coloured:?})"
        );
        // Luminosity is the other way: red's brightness, grey's colour —
        // which is a grey, since grey has no colour to keep.
        let lit = paint(BlendMode::Luminosity);
        assert!(
            lit[0] == lit[1] && lit[1] == lit[2],
            "the backdrop's grey has no hue to take ({lit:?})"
        );
        assert!(
            (lit[0] as i32) < 128,
            "and red is darker than a mid grey ({lit:?})"
        );
        // Saturation of a flat red over grey leaves grey grey too: there
        // is no hue under it to make vivid.
        let sat = paint(BlendMode::Saturation);
        assert!(sat[0] == sat[1] && sat[1] == sat[2], "{sat:?}");
    }

    /// A copy draws what the original draws, where the copy is; changing
    /// the original changes the copy, and moving the original moves only
    /// the original.
    #[test]
    fn a_copy_follows_the_original_it_was_made_from() {
        let mut doc = Document::new(80, 40, ColorMode::Rgb);
        let master = add(&mut doc, filled_rect("master", 10.0, 10.0, RED));
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::instance("copy", master)),
        })
        .unwrap();
        let copy = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: copy,
            transform: Transform::translation(40.0, 0.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(5, 5).to_srgb8(), [255, 0, 0, 255], "the original");
        assert_eq!(s.get(45, 5).to_srgb8(), [255, 0, 0, 255], "and its copy");
        assert_eq!(s.get(20, 5).a, 0.0, "and nothing in between");

        // Change the original and the copy changes with it.
        doc.apply(Command::SetKind {
            id: master,
            kind: Box::new(NodeKind::Vector {
                shape: VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                fill: Some(BLUE),
                stroke: None,
                gradient: None,
            }),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(45, 5).to_srgb8(),
            [0, 0, 255, 255],
            "the copy took it"
        );
        assert_eq!(s.get(55, 15).to_srgb8(), [0, 0, 255, 255], "size and all");

        // Moving the original leaves the copy where it was put.
        doc.apply(Command::SetTransform {
            id: master,
            transform: Transform::translation(0.0, 20.0),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(5, 5).a, 0.0, "the original moved");
        assert_eq!(
            s.get(45, 5).to_srgb8(),
            [0, 0, 255, 255],
            "the copy did not"
        );
    }

    /// A copy has its own opacity on top of what it is a copy of.
    #[test]
    fn a_copy_carries_its_own_opacity() {
        let mut doc = Document::new(40, 20, ColorMode::Rgb);
        let master = add(&mut doc, filled_rect("master", 10.0, 10.0, RED));
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::instance("copy", master)),
        })
        .unwrap();
        let copy = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: copy,
            transform: Transform::translation(20.0, 0.0),
        })
        .unwrap();
        doc.apply(Command::SetOpacity {
            id: copy,
            opacity: 0.5,
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(5, 5).a, 1.0, "the original is solid");
        assert!(
            (s.get(25, 5).a - 0.5).abs() < 0.01,
            "and the copy is half there ({})",
            s.get(25, 5).a
        );
    }

    /// A curve on one channel moves that channel and leaves the others,
    /// and the master curve runs before it rather than instead of it.
    #[test]
    fn a_curve_on_one_channel_moves_only_that_channel() {
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let mut doc = Document::new(2, 2, ColorMode::Rgb);
        add(&mut doc, filled_rect("r", 2.0, 2.0, grey));
        let lift = vec![[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]];
        let set = |doc: &mut Document, id, adj| {
            doc.apply(Command::SetKind {
                id,
                kind: Box::new(NodeKind::Adjustment(adj)),
            })
            .unwrap();
        };
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment(
                "curve",
                Adjustment::Curves {
                    points: vec![[0.0, 0.0], [1.0, 1.0]],
                    red: Vec::new(),
                    green: Vec::new(),
                    blue: Vec::new(),
                },
            )),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[1];
        let shown = |doc: &Document| render(doc).unwrap().get(0, 0).to_srgb8();

        set(
            &mut doc,
            id,
            Adjustment::Curves {
                points: vec![[0.0, 0.0], [1.0, 1.0]],
                red: lift.clone(),
                green: Vec::new(),
                blue: Vec::new(),
            },
        );
        let out = shown(&doc);
        assert!(
            (out[0] as i32 - 191).abs() <= 1,
            "red is lifted to three quarters ({out:?})"
        );
        assert_eq!(
            (out[1], out[2]),
            (128, 128),
            "green and blue are where they were ({out:?})"
        );

        // The master runs first: with both lifted, red goes past where
        // either alone would put it.
        set(
            &mut doc,
            id,
            Adjustment::Curves {
                points: lift.clone(),
                red: lift.clone(),
                green: Vec::new(),
                blue: Vec::new(),
            },
        );
        let both = shown(&doc);
        assert!(
            both[0] > 191 && (both[1] as i32 - 191).abs() <= 1,
            "the channel curve reads what the master handed it ({both:?})"
        );

        // A channel with nothing on it is the identity, so a file
        // written before per-channel curves existed looks the same.
        set(
            &mut doc,
            id,
            Adjustment::Curves {
                points: lift,
                red: Vec::new(),
                green: Vec::new(),
                blue: Vec::new(),
            },
        );
        assert_eq!(shown(&doc), [191, 191, 191, 255]);
    }

    fn artboard(doc: &mut Document, w: f32, h: f32, at: (f32, f32)) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        let mut node = Node::artboard("frame", w, h, Some(WHITE));
        node.transform = Transform::translation(at.0, at.1);
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(node),
        })
        .unwrap();
        doc.children_of(root).unwrap()[index]
    }

    const WHITE: AuthoredColor = AuthoredColor::Srgb {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// A frame paints its ground, and cuts whatever is put in it to its
    /// own box however far past the edge that thing reaches.
    #[test]
    fn a_frame_grounds_its_contents_and_cuts_them_to_its_box() {
        let mut doc = Document::new(60, 60, ColorMode::Rgb);
        let board = artboard(&mut doc, 20.0, 20.0, (10.0, 10.0));
        doc.apply(Command::AddNode {
            parent: board,
            index: 0,
            node: filled_rect("wide", 100.0, 100.0, RED),
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(15, 15).to_srgb8(),
            [255, 0, 0, 255],
            "inside the frame the rect shows"
        );
        assert_eq!(
            s.get(40, 40).a,
            0.0,
            "and past the frame's edge nothing does"
        );
        assert_eq!(s.get(5, 5).a, 0.0, "nor before it starts");

        // With the rect gone the frame's own ground is what is there.
        let inner = doc.children_of(board).unwrap()[0];
        doc.apply(Command::RemoveNode { id: inner }).unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(15, 15).to_srgb8(), [255, 255, 255, 255], "the ground");
        assert_eq!(s.get(40, 40).a, 0.0, "and only inside the frame");
    }

    /// The frame's box is the frame's, not its contents': a layer
    /// hanging out of it cannot make the frame bigger.
    #[test]
    fn a_frames_box_is_its_own_size() {
        let mut doc = Document::new(60, 60, ColorMode::Rgb);
        let board = artboard(&mut doc, 20.0, 20.0, (10.0, 10.0));
        doc.apply(Command::AddNode {
            parent: board,
            index: 0,
            node: filled_rect("wide", 100.0, 100.0, RED),
        })
        .unwrap();
        assert_eq!(
            node_visual_bounds(&doc, board).unwrap(),
            Bounds::Rect(10.0, 10.0, 30.0, 30.0)
        );
    }

    /// Exporting a frame gives the frame at its own size, upright, with
    /// nothing of the page around it.
    #[test]
    fn a_frame_exports_at_its_own_size() {
        let mut doc = Document::new(200, 200, ColorMode::Rgb);
        // A layer on the page that overlaps the frame but is not in it.
        add(&mut doc, filled_rect("loose", 200.0, 200.0, RED));
        let board = artboard(&mut doc, 40.0, 30.0, (20.0, 25.0));
        doc.apply(Command::AddNode {
            parent: board,
            index: 0,
            node: filled_rect("inside", 10.0, 10.0, BLUE),
        })
        .unwrap();
        let out = artboard_pixels(&doc, board, 1.0).unwrap().unwrap();
        assert_eq!((out.width, out.height), (40, 30));
        assert_eq!(
            out.get(5, 5).to_srgb8(),
            [0, 0, 255, 255],
            "what is in the frame, at the frame's own origin"
        );
        assert_eq!(
            out.get(30, 20).to_srgb8(),
            [255, 255, 255, 255],
            "the frame's ground, not the page's red"
        );
        // Twice the size is twice the pixels and the same picture.
        let big = artboard_pixels(&doc, board, 2.0).unwrap().unwrap();
        assert_eq!((big.width, big.height), (80, 60));
        assert_eq!(big.get(10, 10).to_srgb8(), [0, 0, 255, 255]);
    }

    /// A frame turned on the page is still cut to its box — with a
    /// smooth edge, since a turned edge cannot be a row of pixels.
    #[test]
    fn a_turned_frame_keeps_a_smooth_edge() {
        let mut doc = Document::new(80, 80, ColorMode::Rgb);
        let board = artboard(&mut doc, 30.0, 30.0, (25.0, 25.0));
        let angle = 0.4f32;
        doc.apply(Command::SetTransform {
            id: board,
            transform: Transform {
                a: angle.cos(),
                b: angle.sin(),
                c: -angle.sin(),
                d: angle.cos(),
                e: 30.0,
                f: 10.0,
            },
        })
        .unwrap();
        let s = render(&doc).unwrap();
        let rim: Vec<f32> = (0..80).map(|x| s.get(x, 20).a).collect();
        assert!(
            rim.iter().any(|a| *a > 0.01 && *a < 0.99),
            "the turned edge is feathered, not a step: {rim:?}"
        );
        assert!(rim.iter().any(|a| *a > 0.99), "and solid inside it");
    }

    /// A layer clipped to the one below shows where that one does and
    /// nowhere else — which is the whole of what clipping means."""
    #[test]
    fn a_clipped_layer_shows_only_where_the_one_below_it_does() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        add(&mut doc, filled_rect("under", 20.0, 20.0, RED));
        let over = add(&mut doc, filled_rect("over", 40.0, 40.0, BLUE));
        // Before it is clipped the upper layer covers the page.
        let s = render(&doc).unwrap();
        assert_eq!(s.get(30, 30).to_srgb8(), [0, 0, 255, 255]);

        doc.apply(Command::SetClipped {
            id: over,
            clipped: true,
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(10, 10).to_srgb8(),
            [0, 0, 255, 255],
            "it still covers what it is clipped to"
        );
        assert_eq!(
            s.get(30, 30).a,
            0.0,
            "and shows nothing at all past that layer's edge"
        );
    }

    /// It inherits the fate of the layer it rides on: hide that one and
    /// the clipped layer goes with it.
    #[test]
    fn hiding_the_layer_below_hides_what_is_clipped_to_it() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let under = add(&mut doc, filled_rect("under", 20.0, 20.0, RED));
        let over = add(&mut doc, filled_rect("over", 40.0, 40.0, BLUE));
        doc.apply(Command::SetClipped {
            id: over,
            clipped: true,
        })
        .unwrap();
        doc.apply(Command::SetVisible {
            id: under,
            visible: false,
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(10, 10).a, 0.0, "nothing is left of either");
    }

    /// An adjustment cannot be drawn on a surface of its own and cut
    /// afterwards — it is a change to what is under it. Clipped, it has
    /// to change that only where the layer below reaches.
    #[test]
    fn an_adjustment_clipped_to_a_layer_leaves_the_rest_of_the_page_alone() {
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        add(&mut doc, filled_rect("page", 40.0, 40.0, grey));
        add(&mut doc, filled_rect("patch", 20.0, 20.0, grey));
        let plain = render(&doc).unwrap();
        let adj = add(
            &mut doc,
            Box::new(Node::adjustment(
                "brighter",
                Adjustment::Exposure { stops: 1.0 },
            )),
        );
        let before = render(&doc).unwrap();
        assert!(
            before.get(30, 30).r > plain.get(30, 30).r + 0.1,
            "unclipped, it lifts the whole page"
        );

        doc.apply(Command::SetClipped {
            id: adj,
            clipped: true,
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(10, 10).to_srgb8(),
            before.get(10, 10).to_srgb8(),
            "over the patch it does what it always did"
        );
        assert!(
            (s.get(30, 30).r - plain.get(30, 30).r).abs() < 1e-5,
            "off it, the page is left exactly as it was"
        );
    }

    /// Repainting a piece of the page has to give the same pixels as
    /// repainting all of it: the cut a clipped layer is made with is
    /// worked out per region, and a region boundary must not show.
    #[test]
    fn a_clipped_layer_repaints_the_same_by_the_piece_as_whole() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        add(&mut doc, filled_rect("under", 24.0, 24.0, RED));
        let over = add(&mut doc, filled_rect("over", 40.0, 40.0, BLUE));
        doc.apply(Command::SetTransform {
            id: over,
            transform: Transform::translation(6.0, 6.0),
        })
        .unwrap();
        doc.apply(Command::SetClipped {
            id: over,
            clipped: true,
        })
        .unwrap();
        let whole = render(&doc).unwrap();
        let mut piecemeal = Surface::new(40, 40);
        for band in 0..4 {
            render_region(
                &doc,
                &mut piecemeal,
                ClipRect {
                    x0: 0,
                    y0: band * 10,
                    x1: 40,
                    y1: band * 10 + 10,
                },
            )
            .unwrap();
        }
        for (i, (a, b)) in whole.pixels.iter().zip(&piecemeal.pixels).enumerate() {
            assert!(
                (a.r - b.r).abs() < 1e-5 && (a.a - b.a).abs() < 1e-5,
                "pixel {} of {}: {a:?} vs {b:?}",
                i,
                whole.pixels.len()
            );
        }
    }

    /// A brush lays paint along the line it was drawn, with a round end
    /// at either stop, and only there.
    #[test]
    fn a_stroke_paints_along_its_line_and_nowhere_else() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        painted(
            &mut doc,
            vec![stroke(&[[8.0, 20.0], [32.0, 20.0]], 5.0, RED)],
        );
        let s = render(&doc).unwrap();
        assert_eq!(s.get(20, 20).to_srgb8(), [255, 0, 0, 255], "on the line");
        assert!(s.get(20, 16).a > 0.9, "and out to the brush's radius");
        assert_eq!(s.get(20, 30).a, 0.0, "but not past it");
        // The ends are round: half a radius beyond the last point is
        // still paint, a whole radius past it is not.
        assert!(s.get(34, 20).a > 0.5, "the cap reaches past the end");
        assert_eq!(s.get(38, 20).a, 0.0, "but only by the radius");
        // The edge is not a step.
        let rim: Vec<f32> = (23..27).map(|y| s.get(20, y).a).collect();
        assert!(
            rim.iter().any(|a| *a > 0.01 && *a < 0.99),
            "a soft rim: {rim:?}"
        );
    }

    /// A stroke that doubles back over itself is not darker where it
    /// crossed: within one stroke the brush lays paint once.
    #[test]
    fn a_stroke_does_not_paint_itself_twice() {
        let half = AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 0.5,
        };
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        painted(
            &mut doc,
            vec![stroke(
                &[[10.0, 20.0], [30.0, 20.0], [10.0, 20.0]],
                4.0,
                half,
            )],
        );
        let crossed = render(&doc).unwrap().get(20, 20).a;
        // Two strokes over each other do double up, which is what makes
        // the one above worth checking.
        let mut twice = Document::new(40, 40, ColorMode::Rgb);
        painted(
            &mut twice,
            vec![
                stroke(&[[10.0, 20.0], [30.0, 20.0]], 4.0, half),
                stroke(&[[10.0, 20.0], [30.0, 20.0]], 4.0, half),
            ],
        );
        let stacked = render(&twice).unwrap().get(20, 20).a;
        assert!((crossed - 0.5).abs() < 0.01, "one coat: {crossed}");
        assert!(stacked > crossed + 0.2, "two coats: {stacked}");
    }

    /// An eraser takes off the layer's own paint and leaves what is
    /// under the layer alone.
    #[test]
    fn an_eraser_takes_off_this_layer_only() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("under", 40.0, 40.0, RED),
        })
        .unwrap();
        let blue = AuthoredColor::Srgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        let mut rub = stroke(&[[20.0, 20.0]], 6.0, RED);
        rub.erase = true;
        painted(
            &mut doc,
            vec![stroke(&[[6.0, 20.0], [34.0, 20.0]], 8.0, blue), rub],
        );
        let s = render(&doc).unwrap();
        assert_eq!(
            s.get(20, 20).to_srgb8(),
            [255, 0, 0, 255],
            "the rubbed-out spot shows the layer beneath, not a hole"
        );
        assert_eq!(
            s.get(10, 20).to_srgb8(),
            [0, 0, 255, 255],
            "paint elsewhere"
        );
    }

    /// A paint layer is picked where it has paint, so the empty part of
    /// one lets through what is under it.
    #[test]
    fn a_paint_layer_is_picked_only_where_it_has_paint() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("under", 40.0, 40.0, RED),
        })
        .unwrap();
        let under = doc.children_of(root).unwrap()[0];
        let layer = painted(&mut doc, vec![stroke(&[[20.0, 20.0]], 5.0, RED)]);
        assert_eq!(hit_test(&doc, 20.0, 20.0).unwrap(), Some(layer));
        assert_eq!(hit_test(&doc, 4.0, 4.0).unwrap(), Some(under));
    }

    /// A layer's opacity applies to the picture the strokes make, not to
    /// each of them, so overlapping strokes do not show through one
    /// another when the layer is faded.
    #[test]
    fn layer_opacity_fades_the_painting_not_each_stroke() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let id = painted(
            &mut doc,
            vec![
                stroke(&[[6.0, 20.0], [34.0, 20.0]], 8.0, RED),
                stroke(&[[6.0, 20.0], [34.0, 20.0]], 8.0, RED),
            ],
        );
        doc.apply(Command::SetOpacity { id, opacity: 0.5 }).unwrap();
        let a = render(&doc).unwrap().get(20, 20).a;
        assert!((a - 0.5).abs() < 0.01, "half of one opaque coat: {a}");
    }

    /// Every stroke can be taken back off, and the layer is what is left.
    #[test]
    fn strokes_come_off_the_way_they_went_on() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let id = painted(
            &mut doc,
            vec![
                stroke(&[[6.0, 10.0], [34.0, 10.0]], 4.0, RED),
                stroke(&[[6.0, 30.0], [34.0, 30.0]], 4.0, RED),
            ],
        );
        let inverse = doc
            .apply(Command::RemoveStroke {
                id,
                index: 0,
                on_mask: false,
            })
            .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(20, 10).a, 0.0, "the first is gone");
        assert!(s.get(20, 30).a > 0.9, "the second is not");
        doc.apply(inverse).unwrap();
        assert!(render(&doc).unwrap().get(20, 10).a > 0.9, "and comes back");
        // Nothing else is a paint layer.
        let root = doc.root();
        assert!(matches!(
            doc.apply(Command::RemoveStroke {
                id: root,
                index: 0,
                on_mask: false
            }),
            Err(DocError::NotAPaintLayer(_))
        ));
        assert!(matches!(
            doc.apply(Command::RemoveStroke {
                id,
                index: 9,
                on_mask: false
            }),
            Err(DocError::NoSuchStroke { .. })
        ));
    }

    /// Not an assertion: what a painting of long strokes costs to draw
    /// whole, which is what an export pays.
    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn painting_timing_probe() {
        let mut doc = Document::new(2480, 3508, ColorMode::Rgb);
        let strokes: Vec<chitrakar_doc::PaintStroke> = (0..40)
            .map(|i| {
                let y = 40.0 + i as f32 * 80.0;
                chitrakar_doc::PaintStroke {
                    points: (0..40)
                        .map(|k| [60.0 + k as f32 * 60.0, y + (k % 7) as f32 * 4.0])
                        .collect(),
                    radii: vec![14.0],
                    color: RED,
                    softness: 0.5,
                    erase: false,
                    source: [0.0, 0.0],
                    heal: false,
                }
            })
            .collect();
        painted(&mut doc, strokes);
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let _ = render(&doc).unwrap();
            eprintln!("A4 painting, 40 long strokes: {:?}", t.elapsed());
        }
    }

    /// A clone layer paints with what the page already shows at its own
    /// offset — and because it reads at render time rather than keeping
    /// a copy, changing the source changes what the clone lays down.
    #[test]
    fn a_clone_lays_down_what_is_under_its_source_now() {
        let blue = AuthoredColor::Srgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        let build = |patch: AuthoredColor| {
            let mut doc = Document::new(80, 80, ColorMode::Rgb);
            let root = doc.root();
            // A patch of colour in one corner to clone from.
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: filled_rect("patch", 20.0, 20.0, patch),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[0];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(10.0, 10.0),
            })
            .unwrap();
            // A clone layer that reads 40 pixels up and left of where it
            // paints, so painting at (60, 60) lifts from (20, 20).
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::clone_layer("clone")),
            })
            .unwrap();
            let clone = doc.children_of(root).unwrap()[1];
            let mut stroke = stroke(&[[60.0, 60.0]], 6.0, patch);
            stroke.source = [-40.0, -40.0];
            doc.apply(Command::AddStroke {
                id: clone,
                index: 0,
                stroke: Box::new(stroke),
                on_mask: false,
            })
            .unwrap();
            render(&doc).unwrap()
        };

        let red = build(RED);
        assert_eq!(
            red.get(60, 60).to_srgb8(),
            [255, 0, 0, 255],
            "the clone laid down what its source shows"
        );
        assert_eq!(red.get(60, 20).a, 0.0, "and nothing where it did not paint");
        assert_eq!(
            red.get(20, 20).to_srgb8(),
            [255, 0, 0, 255],
            "the source itself is untouched"
        );

        // Recolour the source, and the clone follows: it kept no copy.
        let recoloured = build(blue);
        assert_eq!(
            recoloured.get(60, 60).to_srgb8(),
            [0, 0, 255, 255],
            "the clone follows its source rather than keeping a copy"
        );
    }

    /// Healing lays the source's texture down in the colour of the place
    /// it lands, so a patch lifted from somewhere darker does not show
    /// as a disc of the wrong colour.
    #[test]
    fn healing_takes_the_texture_from_there_and_the_colour_from_here() {
        let dark = AuthoredColor::Srgb {
            r: 0.2,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        };
        let light = AuthoredColor::Srgb {
            r: 0.8,
            g: 0.8,
            b: 0.8,
            a: 1.0,
        };
        let build = |heal: bool| {
            let mut doc = Document::new(80, 80, ColorMode::Rgb);
            let root = doc.root();
            // A light field with a dark patch in one corner to lift from.
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: filled_rect("field", 80.0, 80.0, light),
            })
            .unwrap();
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: filled_rect("patch", 24.0, 24.0, dark),
            })
            .unwrap();
            let patch = doc.children_of(root).unwrap()[1];
            doc.apply(Command::SetTransform {
                id: patch,
                transform: Transform::translation(4.0, 4.0),
            })
            .unwrap();
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::clone_layer("clone")),
            })
            .unwrap();
            let clone = doc.children_of(root).unwrap()[2];
            let mut s = stroke(&[[60.0, 60.0]], 8.0, light);
            s.source = [-44.0, -44.0]; // reads the dark patch
            s.heal = heal;
            doc.apply(Command::AddStroke {
                id: clone,
                index: 0,
                stroke: Box::new(s),
                on_mask: false,
            })
            .unwrap();
            render(&doc).unwrap()
        };

        let cloned = build(false).get(60, 60).to_srgb8();
        let healed = build(true).get(60, 60).to_srgb8();
        let around = build(true).get(20, 60).to_srgb8();
        assert!(
            cloned[0] < 100,
            "cloning brings the dark patch over as it is ({cloned:?})"
        );
        assert!(
            (healed[0] as i32 - around[0] as i32).abs() < 12,
            "healing lands in the colour it was dropped into ({healed:?} against {around:?})"
        );
    }

    /// A stroke that runs over what it is reading takes what was there
    /// when it began, rather than what it has just laid down.
    #[test]
    fn a_clone_does_not_smear_itself() {
        let mut doc = Document::new(60, 60, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("patch", 12.0, 60.0, RED),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::clone_layer("clone")),
        })
        .unwrap();
        let clone = doc.children_of(root).unwrap()[1];
        // A long stroke moving right, reading four pixels to its left:
        // without a snapshot each step would read what the step before
        // it painted and drag the patch the whole way across.
        let mut smear = stroke(&[[14.0, 30.0], [50.0, 30.0]], 5.0, RED);
        smear.source = [-4.0, 0.0];
        doc.apply(Command::AddStroke {
            id: clone,
            index: 0,
            stroke: Box::new(smear),
            on_mask: false,
        })
        .unwrap();
        let s = render(&doc).unwrap();
        assert!(
            s.get(14, 30).a > 0.5,
            "it lifted the patch where it started"
        );
        assert_eq!(
            s.get(45, 30).a,
            0.0,
            "and dragged nothing along with it, having read what was there"
        );
    }

    /// A painted mask starts showing the whole layer, and an eraser
    /// stroke takes a piece out of it — without touching the layer, so
    /// undoing the stroke brings the piece back.
    #[test]
    fn a_painted_mask_takes_a_piece_out_of_a_layer() {
        let mut doc = Document::new(60, 60, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("photo", 60.0, 60.0, RED),
        })
        .unwrap();
        let photo = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetMask {
            id: photo,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Painted {
                    strokes: Vec::new(),
                },
                invert: false,
            })),
        })
        .unwrap();
        // An empty painted mask hides nothing.
        assert_eq!(
            render(&doc).unwrap().get(30, 30).to_srgb8(),
            [255, 0, 0, 255]
        );

        let mut rub = stroke(&[[30.0, 30.0]], 8.0, RED);
        rub.erase = true;
        let inverse = doc
            .apply(Command::AddStroke {
                id: photo,
                index: 0,
                stroke: Box::new(rub),
                on_mask: true,
            })
            .unwrap();
        let s = render(&doc).unwrap();
        assert_eq!(s.get(30, 30).a, 0.0, "the eraser took a piece out");
        assert_eq!(s.get(5, 5).to_srgb8(), [255, 0, 0, 255], "the rest stayed");

        // Painting over the hole shows the layer again there.
        doc.apply(Command::AddStroke {
            id: photo,
            index: 1,
            stroke: Box::new(stroke(&[[30.0, 30.0]], 4.0, RED)),
            on_mask: true,
        })
        .unwrap();
        assert!(
            render(&doc).unwrap().get(30, 30).a > 0.9,
            "and a brush over it puts the layer back"
        );

        // The layer itself was never touched: undoing the strokes is all
        // it takes to have it whole again.
        doc.apply(Command::RemoveStroke {
            id: photo,
            index: 1,
            on_mask: true,
        })
        .unwrap();
        doc.apply(inverse).unwrap();
        assert_eq!(
            render(&doc).unwrap().get(30, 30).to_srgb8(),
            [255, 0, 0, 255]
        );
    }

    /// White balance is a gain per channel: warming lifts the red and
    /// drops the blue, and the tint runs green against magenta. Grey
    /// says it plainest, since it has all three in equal measure.
    #[test]
    fn white_balance_warms_and_cools_a_grey() {
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let under = to_working(grey);
        let warm = apply_adjustment(
            &chitrakar_doc::Adjustment::WhiteBalance {
                temperature: 0.5,
                tint: 0.0,
            },
            None,
            under,
        );
        assert!(warm.r > under.r && warm.b < under.b, "{warm:?}");
        assert!((warm.g - under.g).abs() < 1e-6, "the green is left alone");
        let cool = apply_adjustment(
            &chitrakar_doc::Adjustment::WhiteBalance {
                temperature: -0.5,
                tint: 0.0,
            },
            None,
            under,
        );
        assert!(cool.r < under.r && cool.b > under.b, "{cool:?}");
        let magenta = apply_adjustment(
            &chitrakar_doc::Adjustment::WhiteBalance {
                temperature: 0.0,
                tint: 0.6,
            },
            None,
            under,
        );
        assert!(
            magenta.g < under.g && (magenta.r - under.r).abs() < 1e-6,
            "the tint takes green out and leaves the rest: {magenta:?}"
        );
        // Nothing at all is nothing at all.
        let same = apply_adjustment(
            &chitrakar_doc::Adjustment::WhiteBalance {
                temperature: 0.0,
                tint: 0.0,
            },
            None,
            under,
        );
        assert!((same.r - under.r).abs() < 1e-6 && (same.b - under.b).abs() < 1e-6);
    }

    /// Vibrance lifts what is dull and leaves what is already vivid, so
    /// a near-grey moves further than a saturated colour does.
    #[test]
    fn vibrance_lifts_the_dull_and_spares_the_vivid() {
        let lift = chitrakar_doc::Adjustment::Vibrance { amount: 1.0 };
        let moved = |c: AuthoredColor| {
            let before = to_working(c);
            let after = apply_adjustment(&lift, None, before);
            let chroma = |p: LinearRgba| p.r.max(p.g).max(p.b) - p.r.min(p.g).min(p.b);
            chroma(after) - chroma(before)
        };
        let dull = moved(AuthoredColor::Srgb {
            r: 0.52,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        });
        let vivid = moved(AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        assert!(dull > 0.0, "a dull colour comes up: {dull}");
        assert!(
            vivid < dull,
            "and a vivid one moves less than it does ({vivid} against {dull})"
        );
        // Grey has no colour to lift, so it stays grey.
        let grey = to_working(AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        });
        let after = apply_adjustment(&lift, None, grey);
        assert!(
            (after.r - after.g).abs() < 1e-6 && (after.g - after.b).abs() < 1e-6,
            "grey stays grey: {after:?}"
        );
    }

    /// A layer's mask gets a picture of its own, fitted the same way the
    /// layer's is: white where the layer shows through, clear where it
    /// is hidden.
    #[test]
    fn a_mask_gets_a_picture_of_what_it_lets_through() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("photo", 60.0, 60.0, RED),
        })
        .unwrap();
        let photo = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: photo,
            transform: Transform::translation(20.0, 20.0),
        })
        .unwrap();
        assert!(
            mask_thumbnail(&doc, photo, 24).unwrap().is_none(),
            "no mask, no picture of one"
        );

        let mut rub = stroke(&[[50.0, 50.0]], 20.0, RED);
        rub.erase = true;
        doc.apply(Command::SetMask {
            id: photo,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Painted { strokes: vec![rub] },
                invert: false,
            })),
        })
        .unwrap();
        let thumb = mask_thumbnail(&doc, photo, 24).unwrap().unwrap();
        assert_eq!(thumb.len(), 24 * 24 * 4);
        let at = |x: usize, y: usize| thumb[(y * 24 + x) * 4 + 3];
        // The rub is at the middle of the layer's box, so it is at the
        // middle of the square too.
        assert_eq!(at(12, 12), 0, "clear where the mask hides the layer");
        assert_eq!(at(1, 1), 255, "and white where it lets it through");
        assert_eq!(
            &thumb[0..3],
            &[255, 255, 255],
            "white throughout, with the coverage in the alpha"
        );
    }

    /// A thumbnail is what the page draws of one layer, fitted into a
    /// square of its own: the layer's ink is in it, nothing else is, and
    /// a layer that is only a change to what is under it has none.
    #[test]
    fn a_thumbnail_shows_one_layer_and_only_that_layer() {
        let mut doc = Document::new(200, 200, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("under", 200.0, 200.0, RED),
        })
        .unwrap();
        let blue = AuthoredColor::Srgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: filled_rect("small", 20.0, 20.0, blue),
        })
        .unwrap();
        let small = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: small,
            transform: Transform::translation(150.0, 150.0),
        })
        .unwrap();

        let thumb = thumbnail(&doc, small, 32).unwrap().unwrap();
        assert_eq!(thumb.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| {
            let i = (y * 32 + x) * 4;
            [thumb[i], thumb[i + 1], thumb[i + 2], thumb[i + 3]]
        };
        // A 20-pixel layer never scales up, so it sits centred in the
        // square with bare corners around it.
        assert_eq!(at(16, 16), [0, 0, 255, 255], "the layer is in the middle");
        assert_eq!(at(1, 1)[3], 0, "the layer below it is not");

        // The layer under it fills the page, so its thumbnail is solid.
        let under = doc.children_of(root).unwrap()[0];
        let full = thumbnail(&doc, under, 32).unwrap().unwrap();
        assert_eq!(&full[0..4], &[255, 0, 0, 255], "corner to corner");

        // An adjustment layer is a change to what is under it and has no
        // square of its own.
        doc.apply(Command::AddNode {
            parent: root,
            index: 2,
            node: Box::new(Node::adjustment(
                "exposure",
                chitrakar_doc::Adjustment::Exposure { stops: 1.0 },
            )),
        })
        .unwrap();
        let adj = doc.children_of(root).unwrap()[2];
        assert!(thumbnail(&doc, adj, 32).unwrap().is_none());
    }

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
    fn a4_effect_probe() {
        let mut doc = Document::new(2480, 3508, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 100.0, 100.0, RED),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetEffects {
            id,
            effects: vec![chitrakar_doc::Effect::DropShadow {
                dx: 6.0,
                dy: 6.0,
                blur: 8.0,
                color: RED,
                opacity: 0.6,
            }],
        })
        .unwrap();
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            render(&doc).unwrap();
        }
        eprintln!(
            "GROUP one small layer with a shadow: {:?}",
            t0.elapsed() / 3
        );
    }

    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn a4_isolated_group_probe() {
        let mut doc = Document::new(2480, 3508, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let gid = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: gid,
            index: 0,
            node: filled_rect("r", 100.0, 100.0, RED),
        })
        .unwrap();
        doc.apply(Command::SetOpacity {
            id: gid,
            opacity: 0.5,
        })
        .unwrap();
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            render(&doc).unwrap();
        }
        eprintln!(
            "GROUP one isolated group, tiny child: {:?}",
            t0.elapsed() / 3
        );
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
    fn a_shadow_inside_an_isolated_group_lands_where_it_would_alone() {
        // Both the group and the layer are drawn on surfaces of their own,
        // each a window onto the page, so this layer's shadow is placed
        // through two of them at once. Getting either corner wrong moves
        // the shadow, and nothing else in the suite stacks them.
        let shadow = chitrakar_doc::Effect::DropShadow {
            dx: 7.0,
            dy: 5.0,
            blur: 1.0,
            color: AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            opacity: 1.0,
        };
        let alone = {
            let mut doc = Document::new(80, 80, ColorMode::Rgb);
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
                transform: Transform::translation(25.0, 25.0),
            })
            .unwrap();
            doc.apply(Command::SetEffects {
                id,
                effects: vec![shadow.clone()],
            })
            .unwrap();
            render(&doc).unwrap()
        };

        let mut doc = Document::new(80, 80, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let g = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: g,
            index: 0,
            node: filled_rect("r", 20.0, 20.0, RED),
        })
        .unwrap();
        let id = doc.children_of(g).unwrap()[0];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(25.0, 25.0),
        })
        .unwrap();
        doc.apply(Command::SetEffects {
            id,
            effects: vec![shadow],
        })
        .unwrap();
        // Opacity 1 would let the group skip its own surface, which is
        // the case this test exists to avoid.
        doc.apply(Command::SetOpacity {
            id: g,
            opacity: 0.5,
        })
        .unwrap();
        let nested = render(&doc).unwrap();

        for y in 0..80 {
            for x in 0..80 {
                let (a, b) = (alone.get(x, y), nested.get(x, y));
                // Half the opacity, so half the alpha — everywhere.
                assert!(
                    (b.a - a.a * 0.5).abs() < 0.01,
                    "at ({x}, {y}): {b:?} against half of {a:?}"
                );
            }
        }
        assert!(
            nested.get(38, 36).a > 0.1,
            "and there really is a shadow to compare"
        );
    }

    #[test]
    fn a_group_casts_its_shadow_from_what_is_in_it() {
        // The other way the two windows nest: the effect is on the group
        // rather than inside it, so the layer's window is opened over
        // the group's. A shadow cast from a group is cast from what the
        // group holds, and lands where that holds it.
        let shadow = chitrakar_doc::Effect::DropShadow {
            dx: 8.0,
            dy: 6.0,
            blur: 1.0,
            color: AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            opacity: 1.0,
        };
        let build = |grouped: bool| {
            let mut doc = Document::new(80, 80, ColorMode::Rgb);
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
            doc.apply(Command::AddNode {
                parent,
                index: 0,
                node: filled_rect("r", 20.0, 20.0, RED),
            })
            .unwrap();
            let id = doc.children_of(parent).unwrap()[0];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(25.0, 25.0),
            })
            .unwrap();
            // On the group when there is one, on the layer when there
            // is not: the same silhouette either way.
            doc.apply(Command::SetEffects {
                id: if grouped { parent } else { id },
                effects: vec![shadow.clone()],
            })
            .unwrap();
            render(&doc).unwrap()
        };
        let (alone, grouped) = (build(false), build(true));
        for y in 0..80 {
            for x in 0..80 {
                let (a, b) = (alone.get(x, y), grouped.get(x, y));
                assert!(
                    (a.a - b.a).abs() < 0.01 && (a.r - b.r).abs() < 0.01,
                    "at ({x}, {y}): {b:?} against {a:?}"
                );
            }
        }
        assert!(
            grouped.get(40, 38).a > 0.1,
            "and there really is a shadow to compare"
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
        // multiply into the grey below and come out at half of what is
        // shown — a blend reads the values a device shows, so red times
        // a half-grey backdrop is a half-grey red.
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
    fn levels_stretch_the_input_range_lift_the_midtones_and_land_in_the_output_range() {
        // A mid grey — 0.25 linear — under a rect, so one pixel says it all.
        let grey = AuthoredColor::Srgb {
            r: 0.5371,
            g: 0.5371,
            b: 0.5371,
            a: 1.0,
        };
        let mut doc = Document::new(2, 2, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 2.0, 2.0, grey),
        })
        .unwrap();
        let levels = |in_black, in_white, gamma, out_black, out_white| {
            Box::new(NodeKind::Adjustment(Adjustment::Levels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            }))
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment(
                "levels",
                Adjustment::Levels {
                    in_black: 0.0,
                    in_white: 1.0,
                    gamma: 1.0,
                    out_black: 0.0,
                    out_white: 1.0,
                },
            )),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[1];
        let linear = |doc: &Document| render(doc).unwrap().get(0, 0).r;
        let plain = linear(&doc);
        assert!(
            (plain - 0.25).abs() < 0.01,
            "neutral levels leave the grey ({plain})"
        );

        let set = |doc: &mut Document, kind| doc.apply(Command::SetKind { id, kind }).unwrap();
        set(&mut doc, levels(0.0, 0.5, 1.0, 0.0, 1.0));
        assert!(
            (linear(&doc) - 0.5).abs() < 0.01,
            "an input white at the grey's double stretches it to half ({})",
            linear(&doc)
        );
        set(&mut doc, levels(0.25, 1.0, 1.0, 0.0, 1.0));
        assert!(
            linear(&doc) < 1e-3,
            "an input black at the grey sinks it to black ({})",
            linear(&doc)
        );
        set(&mut doc, levels(0.0, 1.0, 2.0, 0.0, 1.0));
        assert!(
            (linear(&doc) - 0.5).abs() < 0.01,
            "gamma 2 lifts a quarter to a half ({})",
            linear(&doc)
        );
        set(&mut doc, levels(0.0, 1.0, 1.0, 0.5, 1.0));
        assert!(
            (linear(&doc) - 0.625).abs() < 0.01,
            "an output black at half lands the grey at 0.5 + 0.25 × 0.5 ({})",
            linear(&doc)
        );
        set(&mut doc, levels(0.0, 1.0, 1.0, 0.0, 0.0));
        assert_eq!(linear(&doc), 0.0, "an output range of nothing is all black");
        set(&mut doc, levels(0.5, 0.5, 1.0, 0.0, 1.0));
        let collapsed = linear(&doc);
        assert!(
            collapsed.is_finite() && (collapsed == 0.0 || collapsed == 1.0),
            "a collapsed input range thresholds rather than blowing up ({collapsed})"
        );
        // Alpha is untouched: the adjustment is on colour, not coverage.
        assert_eq!(render(&doc).unwrap().get(0, 0).a, 1.0);
    }

    #[test]
    fn a_curve_is_monotone_through_its_points_and_flat_past_them() {
        let identity = curve_lut(&[[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!(identity.len(), 257);
        assert!((identity[128] - 0.5).abs() < 1e-6 && identity[256] == 1.0);
        assert_eq!(curve_lut(&[]), identity, "no points is the identity");
        assert_eq!(curve_lut(&[[0.3, 0.9]]), identity, "and so is one");

        // Points given out of order, with the middle lifted: the curve
        // passes through each, rises the whole way and never leaves 0..1.
        let lut = curve_lut(&[[1.0, 1.0], [0.5, 0.8], [0.0, 0.0]]);
        assert!(
            (lut[128] - 0.8).abs() < 1e-5,
            "through the lifted point ({})",
            lut[128]
        );
        assert!(lut.windows(2).all(|w| w[1] >= w[0]), "monotone");
        assert!(lut.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(
            lut[64] > 0.25 && lut[64] < 0.8,
            "lifted between, not overshooting"
        );

        // Flat beyond the outer points: a curve starting at (0.25, 0)
        // clips the low quarter to black and one ending at (0.75, 1)
        // clips the high quarter to white.
        let clipped = curve_lut(&[[0.25, 0.0], [0.75, 1.0]]);
        assert!(clipped[..=64].iter().all(|&v| v == 0.0));
        assert!(clipped[192..].iter().all(|&v| v == 1.0));

        // Two points on one input: the later one placed wins, no step.
        let twice = curve_lut(&[[0.0, 0.0], [0.5, 0.2], [0.5, 0.9], [1.0, 1.0]]);
        assert!((twice[128] - 0.9).abs() < 1e-5);
        assert!(twice.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn curves_are_drawn_over_the_display_encoding() {
        // A mid grey — 0.5 as displayed, 0.214 linear — under a rect.
        let grey = AuthoredColor::Srgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let mut doc = Document::new(2, 2, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: filled_rect("r", 2.0, 2.0, grey),
        })
        .unwrap();
        let curve = |points: &[[f32; 2]]| {
            Box::new(Node::adjustment(
                "curve",
                Adjustment::Curves {
                    points: points.to_vec(),
                    red: Vec::new(),
                    green: Vec::new(),
                    blue: Vec::new(),
                },
            ))
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: curve(&[[0.0, 0.0], [1.0, 1.0]]),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[1];
        let shown = |doc: &Document| render(doc).unwrap().get(0, 0).to_srgb8()[0];
        assert_eq!(
            shown(&doc),
            128,
            "the diagonal leaves the grey where it was"
        );

        // The graph's middle is that grey: lifting it to 0.75 shows 191.
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Adjustment(Adjustment::Curves {
                points: vec![[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]],
                red: Vec::new(),
                green: Vec::new(),
                blue: Vec::new(),
            })),
        })
        .unwrap();
        let lifted = shown(&doc);
        assert!(
            (lifted as i32 - 191).abs() <= 1,
            "the lifted middle lands the grey at three quarters as displayed ({lifted})"
        );
        // An inverted curve makes a negative; alpha stays put.
        doc.apply(Command::SetKind {
            id,
            kind: Box::new(NodeKind::Adjustment(Adjustment::Curves {
                points: vec![[0.0, 1.0], [1.0, 0.0]],
                red: Vec::new(),
                green: Vec::new(),
                blue: Vec::new(),
            })),
        })
        .unwrap();
        let px = render(&doc).unwrap().get(0, 0);
        assert!((px.to_srgb8()[0] as i32 - 127).abs() <= 1 && px.a == 1.0);
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
