//! The Chitrakar document model.
//!
//! A document is a tree of live, non-destructive nodes (see docs/PLAN.md §2).
//! All mutation goes through [`Command`]s applied via [`Document::apply`],
//! which returns the inverse command — undo/redo falls out of that in
//! [`History`].

#[cfg(any(test, feature = "fixture"))]
pub mod fixture;
mod node;

pub use node::{
    stroke_align, Adjustment, BlendMode, Effect, Filter, Gradient, GradientStop, Guide, Marker,
    Mask, MaskKind, Node, NodeKind, PaintStroke, Pin, Pinning, RasterRef, Stroke, StrokeAlign,
    StrokeCap, StrokeJoin, StyleRun, TextAlign, TextSpec, Transform, VectorShape, LUMA,
    MARKER_LENGTH, MARKER_REACH, MITER_LIMIT,
};

use chitrakar_color::ColorMode;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Write a map with its keys in order.
///
/// A `HashMap` hands out its entries in whatever order its seed gives,
/// and that seed is fresh every run — so a document serialized twice
/// came out with its nodes in a different order each time, and a
/// `.chitra` saved twice was two different files holding the same work.
/// Nothing read it wrongly, but nothing could compare two saves either.
/// The map stays a hash map, which is what the lookups want; only what
/// goes on the page is ordered.
fn in_order<K, V, S>(map: &HashMap<K, V>, out: S) -> Result<S::Ok, S::Error>
where
    K: Ord + std::hash::Hash + serde::Serialize,
    V: serde::Serialize,
    S: serde::Serializer,
{
    let mut pairs: Vec<(&K, &V)> = map.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    out.collect_map(pairs)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocError {
    #[error("unknown node id {0:?}")]
    UnknownNode(NodeId),
    #[error("node {0:?} is not a group")]
    NotAGroup(NodeId),
    #[error("node {0:?} is not a paint layer")]
    NotAPaintLayer(NodeId),
    #[error("no stroke {index} on paint layer {id:?}, which has {len}")]
    NoSuchStroke {
        id: NodeId,
        index: usize,
        len: usize,
    },
    #[error("index {index} out of bounds for group {group:?} with {len} children")]
    IndexOutOfBounds {
        group: NodeId,
        index: usize,
        len: usize,
    },
    #[error("cannot remove the root group")]
    CannotRemoveRoot,
    #[error("cannot move {0:?} into its own subtree")]
    MoveIntoOwnSubtree(NodeId),
    #[error("a document cannot be {0}x{1}")]
    BadCanvasSize(u32, u32),
    #[error("that would make a copy of itself")]
    InstanceCycle,
}

/// How large a page may be. The renderer's surface is sixteen bytes a
/// pixel, so the ceiling is what can actually be drawn rather than a
/// round number: a hundred million pixels is already a gigabyte and a
/// half of surface, and past that the wasm heap has nowhere to put it.
/// The per-side limit only keeps a page from being a thread.
pub const MAX_CANVAS_PIXELS: u64 = 100_000_000;
pub const MAX_CANVAS_SIDE: u32 = 30_000;

/// Whether a page of this size is one the engine could draw.
pub fn canvas_fits(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_CANVAS_SIDE
        && height <= MAX_CANVAS_SIDE
        && width as u64 * height as u64 <= MAX_CANVAS_PIXELS
}

/// The page a straighten leaves behind: the largest rectangle of the
/// page's own proportions that still fits inside it once it has been
/// turned. Straightening a photograph is turning it a little and then
/// cropping away the wedges of nothing the turn brings in at the
/// corners, and this is where that crop stops.
///
/// Both sides have to fit: turned by `t`, a rectangle `w` by `h` needs
/// `w·cos t + h·sin t` of room across and `w·sin t + h·cos t` down, so
/// the scale is whichever of the two the page can least afford.
pub fn straightened_size(width: u32, height: u32, degrees: f32) -> (u32, u32) {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let t = degrees.to_radians().abs();
    let (s, c) = (t.sin().abs(), t.cos().abs());
    let k = (w / (w * c + h * s))
        .min(h / (w * s + h * c))
        .clamp(0.0, 1.0);
    (
        ((w * k).round() as u32).max(1),
        ((h * k).round() as u32).max(1),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
    pub color_mode: ColorMode,
}

/// The scene graph. Nodes live in a flat arena keyed by [`NodeId`]; groups
/// hold ordered child-id lists (topmost child last, i.e. painter's order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub meta: DocumentMeta,
    root: NodeId,
    #[serde(serialize_with = "in_order")]
    nodes: HashMap<NodeId, Node>,
    #[serde(serialize_with = "in_order")]
    children: HashMap<NodeId, Vec<NodeId>>,
    next_id: u64,
    /// Immutable, content-addressed pixel sources referenced by RasterRef
    /// nodes. Dimensions serialize with the manifest; the bytes live as
    /// separate files in the .chitra container and are restored on load.
    ///
    /// Ordered rather than hashed: the container writes one file per
    /// resource, in the order this hands them out, and a file written in
    /// a different order every run is a different file every run.
    #[serde(default)]
    resources: BTreeMap<String, Resource>,
    /// CMYK press profile bytes (stored as profiles/cmyk.icc in the
    /// container) and the parsed transform. Authored CMYK values render
    /// through this when set; the naive preview formula otherwise.
    #[serde(skip)]
    cmyk_profile_bytes: Option<Vec<u8>>,
    #[serde(skip)]
    cmyk_cms: Option<chitrakar_color::CmykCms>,
    /// Straight lines the user placed to lay work out against. Document
    /// state, but not artwork: nothing renders or exports them. Additive.
    #[serde(default)]
    guides: Vec<Guide>,
    /// The colours this document is drawn in, kept by name. Not artwork
    /// either — a palette to pick from, so a page's colours are chosen
    /// once and reached for rather than typed again. Additive.
    #[serde(default)]
    swatches: Vec<Swatch>,
    /// The part of the page that is picked out: a region in page
    /// coordinates, not a layer and not artwork.
    ///
    /// It is a [`Mask`] because that is exactly what it is — a coverage
    /// over the page — and because that is what it is *for*. This editor
    /// is non-destructive: a selection here is not a stencil that pixels
    /// are cut through, it is a region to hand to a layer, so "mask this
    /// layer with what I picked out" is the same value moved rather than
    /// a conversion. `invert` gives the inverse selection for nothing,
    /// and a painted selection — a region brushed by hand — is a mask
    /// kind that already exists.
    ///
    /// Additive: a document written before selections had one loads with
    /// nothing picked out, which is what it had.
    #[serde(default)]
    selection: Option<Box<Mask>>,
    /// Regions kept by name, to be picked out again later.
    ///
    /// A region is often the expensive thing on a page — a sky wanded
    /// out between branches, a lasso drawn round somebody's hair — and
    /// until now the only place to put one was the layer it was handed
    /// to. These are the same [`Mask`] the selection is, so keeping one
    /// and picking it up again are a copy each way rather than a
    /// conversion. Additive, like the guides and the swatches.
    #[serde(default)]
    regions: Vec<KeptRegion>,
}

/// A region kept by name, ready to be picked out again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeptRegion {
    pub name: String,
    pub mask: Mask,
}

/// One colour in the document's palette. Authored like any other colour,
/// so a CMYK document's swatches are ink and resolve through its press
/// profile exactly as a fill does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub color: chitrakar_color::AuthoredColor,
}

/// An immutable source image (8-bit sRGB RGBA). The original bytes a raster
/// object points at — never edited, only referenced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub rgba8: Vec<u8>,
}

impl Document {
    pub fn new(width: u32, height: u32, color_mode: ColorMode) -> Self {
        let root = NodeId(0);
        let mut nodes = HashMap::new();
        nodes.insert(root, Node::group("root"));
        let mut children = HashMap::new();
        children.insert(root, Vec::new());
        Self {
            meta: DocumentMeta {
                width,
                height,
                dpi: 72.0,
                color_mode,
            },
            root,
            nodes,
            children,
            next_id: 1,
            resources: BTreeMap::new(),
            cmyk_profile_bytes: None,
            cmyk_cms: None,
            guides: Vec::new(),
            swatches: Vec::new(),
            regions: Vec::new(),
            selection: None,
        }
    }

    /// Set the document's CMYK press profile. Fails (leaving the previous
    /// profile in place) unless the bytes parse as a CMYK ICC profile.
    pub fn set_cmyk_profile(&mut self, icc: Vec<u8>) -> Result<(), String> {
        let cms = chitrakar_color::CmykCms::new(&icc)?;
        self.cmyk_profile_bytes = Some(icc);
        self.cmyk_cms = Some(cms);
        Ok(())
    }

    pub fn clear_cmyk_profile(&mut self) {
        self.cmyk_profile_bytes = None;
        self.cmyk_cms = None;
    }

    pub fn cmyk_profile_bytes(&self) -> Option<&[u8]> {
        self.cmyk_profile_bytes.as_deref()
    }

    pub fn cmyk_cms(&self) -> Option<&chitrakar_color::CmykCms> {
        self.cmyk_cms.as_ref()
    }

    /// Add pixel bytes to the resource pool, returning their content id.
    /// Identical content shares one entry. Not a [`Command`]: the pool is an
    /// immutable store, only node references to it are document state.
    pub fn add_resource(&mut self, width: u32, height: u32, rgba8: Vec<u8>) -> String {
        let id = content_id(width, height, &rgba8);
        self.resources.entry(id.clone()).or_insert(Resource {
            width,
            height,
            rgba8,
        });
        id
    }

    pub fn resource(&self, id: &str) -> Option<&Resource> {
        self.resources.get(id)
    }

    pub fn resources(&self) -> impl Iterator<Item = (&String, &Resource)> {
        self.resources.iter()
    }

    /// Re-attach pixel bytes to a resource whose metadata came from a
    /// deserialized manifest (bytes are stored outside the manifest).
    pub fn restore_resource_bytes(&mut self, id: &str, rgba8: Vec<u8>) -> bool {
        match self.resources.get_mut(id) {
            // Counted in a width that holds it: the sizes come out of a
            // file's manifest and the bytes out of the file beside it,
            // so this is the one place they are made to agree — and a
            // pair whose pixels do not fit a `u32` would have failed
            // here as arithmetic rather than as a mismatch, which in a
            // release build is worse: the product wraps, and a handful
            // of bytes can be accepted for an enormous picture.
            Some(r) if rgba8.len() as u64 == r.width as u64 * r.height as u64 * 4 => {
                r.rgba8 = rgba8;
                true
            }
            _ => false,
        }
    }

    /// Whether `maybe_descendant` is inside the subtree rooted at `ancestor`
    /// (a node counts as its own descendant).
    fn is_descendant(&self, ancestor: NodeId, maybe_descendant: NodeId) -> bool {
        let mut stack = vec![ancestor];
        while let Some(n) = stack.pop() {
            if n == maybe_descendant {
                return true;
            }
            if let Some(kids) = self.children.get(&n) {
                stack.extend(kids.iter().copied());
            }
        }
        false
    }

    fn parent_and_index(&self, id: NodeId) -> Option<(NodeId, usize)> {
        self.children
            .iter()
            .find_map(|(p, kids)| kids.iter().position(|k| *k == id).map(|i| (*p, i)))
    }

    /// The group containing a node; None for the root (or an unknown id).
    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.parent_and_index(id).map(|(p, _)| p)
    }

    pub fn guides(&self) -> &[Guide] {
        &self.guides
    }

    pub fn swatches(&self) -> &[Swatch] {
        &self.swatches
    }

    /// The regions this document has kept by name.
    pub fn regions(&self) -> &[KeptRegion] {
        &self.regions
    }

    /// The region picked out of the page, if any.
    pub fn selection(&self) -> Option<&Mask> {
        self.selection.as_deref()
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, DocError> {
        self.nodes.get(&id).ok_or(DocError::UnknownNode(id))
    }

    pub fn children_of(&self, id: NodeId) -> Result<&[NodeId], DocError> {
        match self.children.get(&id) {
            Some(c) => Ok(c),
            None if self.nodes.contains_key(&id) => Ok(&[]),
            None => Err(DocError::UnknownNode(id)),
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether any filter layer exists (filters read pixel neighborhoods,
    /// which constrains incremental invalidation — see the engine).
    pub fn has_filter(&self) -> bool {
        self.nodes
            .values()
            .any(|n| matches!(n.kind, NodeKind::Filter(_)))
    }

    /// Iterate all nodes (unordered).
    pub fn nodes(&self) -> impl Iterator<Item = (&NodeId, &Node)> {
        self.nodes.iter()
    }

    /// The strokes of a layer a brush works on — one painted or one
    /// cloned — or of a node's painted mask, to add to or take from.
    fn strokes_mut(
        &mut self,
        id: NodeId,
        on_mask: bool,
    ) -> Result<&mut Vec<PaintStroke>, DocError> {
        let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
        if on_mask {
            return match node.mask.as_mut().map(|m| &mut m.kind) {
                Some(MaskKind::Painted { strokes }) => Ok(strokes),
                _ => Err(DocError::NotAPaintLayer(id)),
            };
        }
        match &mut node.kind {
            NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => Ok(strokes),
            _ => Err(DocError::NotAPaintLayer(id)),
        }
    }

    /// Put the whole page through a transform of the page's own space:
    /// what turning it and mirroring it both come down to.
    ///
    /// Top-level layers are placed against the page, so the map goes in
    /// front of what each already does; anything deeper is placed
    /// against its parent and travels with it. Guides travel too — and a
    /// map like these either moves a guide or stands it on its end,
    /// which falls out of where two of the guide's own points land
    /// rather than out of a table of which becomes which.
    fn map_page(&mut self, m: Transform) {
        /// A coverage carried through a page transform. Written once
        /// because three things want it: a layer's mask, the region
        /// picked out of the page, and every region the page has kept
        /// by name — all masks over the page in exactly the same
        /// sense.
        fn carry_mask(mask: &mut Mask, m: Transform) {
            match &mut mask.kind {
                MaskKind::Vector { transform, .. } | MaskKind::Raster { transform, .. } => {
                    *transform = m.compose(*transform);
                }
                MaskKind::Painted { strokes } => {
                    for stroke in strokes {
                        for p in &mut stroke.points {
                            *p = [m.a * p[0] + m.c * p[1] + m.e, m.b * p[0] + m.d * p[1] + m.f];
                        }
                    }
                }
            }
        }

        // A vector rather than a point: how far a thing reaches, not
        // where it is, so the map's shift is no part of it.
        let along = |dx: f32, dy: f32| (m.a * dx + m.c * dy, m.b * dx + m.d * dy);
        let top: Vec<NodeId> = self.children.get(&self.root).cloned().unwrap_or_default();
        for id in top {
            let Some(node) = self.nodes.get_mut(&id) else {
                continue;
            };
            node.transform = m.compose(node.transform);
            // A mask is written in the space its owner is placed in, so
            // it does not travel with the layer's own transform and has
            // to be taken along by hand. Left behind, it goes on hiding
            // the part of the page it used to cover — which, for a page
            // that moved out from under it, is the whole layer.
            if let Some(mask) = &mut node.mask {
                carry_mask(mask, m);
            }
            // An effect's offset is written in that same space. A page
            // turned a quarter round with the light left where it was
            // would light every layer from a new direction.
            for effect in &mut node.effects {
                match effect {
                    Effect::DropShadow { dx, dy, .. } | Effect::InnerShadow { dx, dy, .. } => {
                        (*dx, *dy) = along(*dx, *dy);
                    }
                    Effect::Outline { .. } => {}
                }
            }
        }
        // A guide is a line, so it maps as a line: a point on it and the
        // direction it runs in. Which way it comes out is the direction's
        // to say — anything the map turns by less than half a right angle
        // stays on the axis it was on — and where it comes out is where
        // it crosses the middle of the page it now belongs to, which is
        // as near as an axis-aligned line can stay to a slanted one.
        //
        // Asking whether the two ends of a mapped guide share an x was
        // the same question for a quarter turn and a mirror, and the
        // wrong one for anything else: a page straightened by seven
        // degrees turned every vertical guide into a horizontal one, at a
        // position that meant nothing.
        // What is picked out of the page is written in the page's own
        // space, so it travels with the page: a selection left behind by
        // a quarter turn would pick out a different part of the picture
        // than the one it was drawn round.
        if let Some(selection) = &mut self.selection {
            carry_mask(selection, m);
        }
        // And so are the regions kept by name, for the same reason and
        // more so: what is picked out is on screen and would be seen to
        // be wrong, where a kept region is not looked at again until
        // the day it is picked up.
        for kept in &mut self.regions {
            carry_mask(&mut kept.mask, m);
        }
        let at = |x: f32, y: f32| (m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f);
        let (w, h) = (self.meta.width as f32 / 2.0, self.meta.height as f32 / 2.0);
        for guide in &mut self.guides {
            let (p, d) = match *guide {
                Guide::Vertical(v) => (at(v, 0.0), along(0.0, 1.0)),
                Guide::Horizontal(v) => (at(0.0, v), along(1.0, 0.0)),
            };
            *guide = if d.1.abs() >= d.0.abs() {
                // Where it crosses the middle row. A line this nearly
                // upright has a `d.1` to divide by.
                Guide::Vertical(p.0 + d.0 * (h - p.1) / d.1)
            } else {
                Guide::Horizontal(p.1 + d.1 * (w - p.0) / d.0)
            };
        }
    }

    /// Apply a command, returning its inverse (for undo).
    pub fn apply(&mut self, cmd: Command) -> Result<Command, DocError> {
        // An instance draws whatever it is a copy of, so a copy that
        // could reach itself would have nothing to draw. Only the
        // commands that can change what reaches what are worth the walk.
        let structural = matches!(
            cmd,
            Command::AddNode { .. }
                | Command::MoveNode { .. }
                | Command::SetKind { .. }
                | Command::RestoreSubtree { .. }
                | Command::Batch(_)
        );
        let inverse = self.apply_inner(cmd)?;
        if structural && self.instance_cycle() {
            // Put it back rather than leave the document in a state
            // nothing could draw.
            let _ = self.apply_inner(inverse);
            return Err(DocError::InstanceCycle);
        }
        Ok(inverse)
    }

    /// Whether any layer can reach itself through what it holds and what
    /// it is a copy of.
    fn instance_cycle(&self) -> bool {
        fn walk(doc: &Document, id: NodeId, state: &mut HashMap<NodeId, u8>) -> bool {
            match state.get(&id) {
                // Already on the way down: this is the cycle.
                Some(1) => return true,
                Some(_) => return false,
                None => {}
            }
            state.insert(id, 1);
            if let Ok(node) = doc.node(id) {
                if let NodeKind::Instance { of, .. } = node.kind {
                    if walk(doc, of, state) {
                        return true;
                    }
                }
            }
            if let Ok(children) = doc.children_of(id) {
                for &child in children {
                    if walk(doc, child, state) {
                        return true;
                    }
                }
            }
            state.insert(id, 2);
            false
        }
        let mut state = HashMap::new();
        walk(self, self.root, &mut state)
    }

    fn apply_inner(&mut self, cmd: Command) -> Result<Command, DocError> {
        match cmd {
            Command::AddNode {
                parent,
                index,
                node,
            } => {
                if !self.node(parent)?.kind.holds_children() {
                    return Err(DocError::NotAGroup(parent));
                }
                let siblings = &self.children[&parent];
                if index > siblings.len() {
                    return Err(DocError::IndexOutOfBounds {
                        group: parent,
                        index,
                        len: siblings.len(),
                    });
                }
                let id = NodeId(self.next_id);
                self.next_id += 1;
                if node.kind.holds_children() {
                    self.children.insert(id, Vec::new());
                }
                self.nodes.insert(id, *node);
                self.children.get_mut(&parent).unwrap().insert(index, id);
                Ok(Command::RemoveNode { id })
            }
            Command::RemoveNode { id } => {
                if id == self.root {
                    return Err(DocError::CannotRemoveRoot);
                }
                self.node(id)?;
                let (parent, index) = self
                    .children
                    .iter()
                    .find_map(|(p, kids)| kids.iter().position(|k| *k == id).map(|i| (*p, i)))
                    .ok_or(DocError::UnknownNode(id))?;
                self.children.get_mut(&parent).unwrap().remove(index);
                // Removing a group takes its subtree with it; restore is a
                // whole-subtree re-add.
                let subtree = self.detach_subtree(id);
                Ok(Command::RestoreSubtree {
                    parent,
                    index,
                    subtree,
                })
            }
            Command::RestoreSubtree {
                parent,
                index,
                subtree,
            } => {
                self.children
                    .get(&parent)
                    .ok_or(DocError::UnknownNode(parent))?;
                let id = subtree.root_id;
                for (nid, node) in subtree.nodes {
                    self.nodes.insert(nid, node);
                }
                for (nid, kids) in subtree.children {
                    self.children.insert(nid, kids);
                }
                self.children.get_mut(&parent).unwrap().insert(index, id);
                Ok(Command::RemoveNode { id })
            }
            Command::SetOpacity { id, opacity } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.opacity;
                node.opacity = opacity.clamp(0.0, 1.0);
                Ok(Command::SetOpacity { id, opacity: prev })
            }
            Command::SetVisible { id, visible } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.visible;
                node.visible = visible;
                Ok(Command::SetVisible { id, visible: prev })
            }
            Command::SetLocked { id, locked } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.locked;
                node.locked = locked;
                Ok(Command::SetLocked { id, locked: prev })
            }
            Command::SetClipped { id, clipped } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.clipped;
                node.clipped = clipped;
                Ok(Command::SetClipped { id, clipped: prev })
            }
            Command::SetPinning { id, pinned } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.pinned;
                node.pinned = pinned;
                Ok(Command::SetPinning { id, pinned: prev })
            }
            Command::SetBlendMode { id, blend } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.blend;
                node.blend = blend;
                Ok(Command::SetBlendMode { id, blend: prev })
            }
            Command::SetTransform { id, transform } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = node.transform;
                node.transform = transform;
                Ok(Command::SetTransform {
                    id,
                    transform: prev,
                })
            }
            Command::SetKind { id, kind } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                // Structural kinds (Group) can't be swapped with leaf kinds;
                // this command is for parameter edits on leaves.
                let prev = std::mem::replace(&mut node.kind, *kind);
                Ok(Command::SetKind {
                    id,
                    kind: Box::new(prev),
                })
            }
            Command::AddStroke {
                id,
                index,
                stroke,
                on_mask,
            } => {
                let strokes = self.strokes_mut(id, on_mask)?;
                if index > strokes.len() {
                    let len = strokes.len();
                    return Err(DocError::NoSuchStroke { id, index, len });
                }
                strokes.insert(index, *stroke);
                Ok(Command::RemoveStroke { id, index, on_mask })
            }
            Command::RemoveStroke { id, index, on_mask } => {
                let strokes = self.strokes_mut(id, on_mask)?;
                if index >= strokes.len() {
                    let len = strokes.len();
                    return Err(DocError::NoSuchStroke { id, index, len });
                }
                let stroke = strokes.remove(index);
                Ok(Command::AddStroke {
                    id,
                    index,
                    stroke: Box::new(stroke),
                    on_mask,
                })
            }
            Command::SetStroke {
                id,
                index,
                stroke,
                on_mask,
            } => {
                let strokes = self.strokes_mut(id, on_mask)?;
                if index >= strokes.len() {
                    let len = strokes.len();
                    return Err(DocError::NoSuchStroke { id, index, len });
                }
                let prev = std::mem::replace(&mut strokes[index], *stroke);
                Ok(Command::SetStroke {
                    id,
                    index,
                    stroke: Box::new(prev),
                    on_mask,
                })
            }
            Command::SetName { id, name } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = std::mem::replace(&mut node.name, name);
                Ok(Command::SetName { id, name: prev })
            }
            Command::SetMask { id, mask } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = std::mem::replace(&mut node.mask, mask.map(|m| *m));
                Ok(Command::SetMask {
                    id,
                    mask: prev.map(Box::new),
                })
            }
            Command::ResizeCanvas {
                width,
                height,
                dx,
                dy,
            } => {
                if !canvas_fits(width, height) {
                    return Err(DocError::BadCanvasSize(width, height));
                }
                let prev = (self.meta.width, self.meta.height);
                self.meta.width = width;
                self.meta.height = height;
                // The shift is a transform of the page's space like any
                // other, so it goes through the one function that carries
                // everything on the page along with it: the layers, what
                // masks them, what they cast, and the guides. A crop that
                // left any of those behind would detach it from the
                // artwork it was placed against.
                self.map_page(Transform::translation(dx, dy));
                Ok(Command::ResizeCanvas {
                    width: prev.0,
                    height: prev.1,
                    dx: -dx,
                    dy: -dy,
                })
            }
            Command::MirrorCanvas { across_x } => {
                let (w, h) = (self.meta.width as f32, self.meta.height as f32);
                self.map_page(if across_x {
                    Transform {
                        a: -1.0,
                        e: w,
                        ..Default::default()
                    }
                } else {
                    Transform {
                        d: -1.0,
                        f: h,
                        ..Default::default()
                    }
                });
                Ok(Command::MirrorCanvas { across_x })
            }
            Command::StraightenCanvas {
                degrees,
                width,
                height,
            } => {
                if !canvas_fits(width, height) {
                    return Err(DocError::BadCanvasSize(width, height));
                }
                let prev = (self.meta.width, self.meta.height);
                let (cx, cy) = (prev.0 as f32 / 2.0, prev.1 as f32 / 2.0);
                let (nx, ny) = (width as f32 / 2.0, height as f32 / 2.0);
                let (s, c) = degrees.to_radians().sin_cos();
                // Turned about the middle of the page it was, and landed
                // on the middle of the page it becomes: p' = R(p - old
                // middle) + new middle. Written out, the turn's own
                // translation is what carries the second half.
                let turn = Transform {
                    a: c,
                    b: s,
                    c: -s,
                    d: c,
                    e: nx - (c * cx - s * cy),
                    f: ny - (s * cx + c * cy),
                };
                self.meta.width = width;
                self.meta.height = height;
                self.map_page(turn);
                Ok(Command::StraightenCanvas {
                    degrees: -degrees,
                    width: prev.0,
                    height: prev.1,
                })
            }
            Command::TurnCanvas { quarters } => {
                let turns = quarters % 4;
                if turns == 0 {
                    return Ok(Command::TurnCanvas { quarters: 0 });
                }
                let (w, h) = (self.meta.width as f32, self.meta.height as f32);
                // The page's own corners decide where everything lands:
                // turned clockwise, the old page's top-left corner
                // becomes the new page's top-right one, so the turn
                // carries a shift with it that puts the page back over
                // its own origin.
                let turn = match turns {
                    1 => Transform {
                        a: 0.0,
                        b: 1.0,
                        c: -1.0,
                        d: 0.0,
                        e: h,
                        f: 0.0,
                    },
                    2 => Transform {
                        a: -1.0,
                        b: 0.0,
                        c: 0.0,
                        d: -1.0,
                        e: w,
                        f: h,
                    },
                    _ => Transform {
                        a: 0.0,
                        b: -1.0,
                        c: 1.0,
                        d: 0.0,
                        e: 0.0,
                        f: w,
                    },
                };
                if turns % 2 == 1 {
                    std::mem::swap(&mut self.meta.width, &mut self.meta.height);
                }
                self.map_page(turn);
                Ok(Command::TurnCanvas {
                    quarters: 4 - turns,
                })
            }
            Command::SetGuides { guides } => {
                let prev = std::mem::replace(&mut self.guides, guides);
                Ok(Command::SetGuides { guides: prev })
            }
            Command::SetRegions { regions } => {
                let prev = std::mem::replace(&mut self.regions, regions);
                Ok(Command::SetRegions { regions: prev })
            }
            Command::SetSwatches { swatches } => {
                let prev = std::mem::replace(&mut self.swatches, swatches);
                Ok(Command::SetSwatches { swatches: prev })
            }
            Command::SetSelection { selection } => {
                let prev = std::mem::replace(&mut self.selection, selection);
                Ok(Command::SetSelection { selection: prev })
            }
            Command::SetEffects { id, effects } => {
                let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
                let prev = std::mem::replace(&mut node.effects, effects);
                Ok(Command::SetEffects { id, effects: prev })
            }
            Command::MoveNode { id, parent, index } => {
                if id == self.root {
                    return Err(DocError::CannotRemoveRoot);
                }
                if !self.node(parent)?.kind.holds_children() {
                    return Err(DocError::NotAGroup(parent));
                }
                if self.is_descendant(id, parent) {
                    return Err(DocError::MoveIntoOwnSubtree(id));
                }
                let (old_parent, old_index) =
                    self.parent_and_index(id).ok_or(DocError::UnknownNode(id))?;
                self.children
                    .get_mut(&old_parent)
                    .unwrap()
                    .remove(old_index);
                let dest = self.children.get_mut(&parent).unwrap();
                dest.insert(index.min(dest.len()), id);
                Ok(Command::MoveNode {
                    id,
                    parent: old_parent,
                    index: old_index,
                })
            }
            Command::Batch(cmds) => {
                let mut inverses = Vec::with_capacity(cmds.len());
                for cmd in cmds {
                    match self.apply(cmd) {
                        Ok(inverse) => inverses.push(inverse),
                        Err(e) => {
                            // Roll back what already applied; these inverses
                            // came from successful applies, so they succeed.
                            for inverse in inverses.into_iter().rev() {
                                let _ = self.apply(inverse);
                            }
                            return Err(e);
                        }
                    }
                }
                inverses.reverse();
                Ok(Command::Batch(inverses))
            }
        }
    }

    /// The id the next added node will get — lets callers build a [`Batch`]
    /// that adds a node and immediately references it (e.g. grouping).
    ///
    /// [`Batch`]: Command::Batch
    pub fn peek_next_id(&self) -> NodeId {
        NodeId(self.next_id)
    }

    fn detach_subtree(&mut self, id: NodeId) -> Subtree {
        let mut subtree = Subtree {
            root_id: id,
            nodes: Vec::new(),
            children: Vec::new(),
        };
        let mut stack = vec![id];
        while let Some(nid) = stack.pop() {
            if let Some(node) = self.nodes.remove(&nid) {
                subtree.nodes.push((nid, node));
            }
            if let Some(kids) = self.children.remove(&nid) {
                stack.extend(kids.iter().copied());
                subtree.children.push((nid, kids));
            }
        }
        subtree
    }
}

/// Content id of a resource: FNV-1a over dimensions and bytes, hex-encoded.
/// Stable across sessions so identical placements dedupe in the pool and in
/// saved containers.
fn content_id(width: u32, height: u32, rgba8: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for b in width.to_le_bytes().into_iter().chain(height.to_le_bytes()) {
        eat(b);
    }
    for &b in rgba8 {
        eat(b);
    }
    format!("{hash:016x}")
}

/// A detached document fragment carried inside undo data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtree {
    root_id: NodeId,
    nodes: Vec<(NodeId, Node)>,
    children: Vec<(NodeId, Vec<NodeId>)>,
}

impl Subtree {
    /// The node this subtree restores at its top.
    pub fn root_id(&self) -> NodeId {
        self.root_id
    }
}

/// Every mutation of a [`Document`]. Serializable — the foundation for undo,
/// history persistence, and future collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    AddNode {
        parent: NodeId,
        index: usize,
        node: Box<Node>,
    },
    RemoveNode {
        id: NodeId,
    },
    RestoreSubtree {
        parent: NodeId,
        index: usize,
        subtree: Subtree,
    },
    SetOpacity {
        id: NodeId,
        opacity: f32,
    },
    SetVisible {
        id: NodeId,
        visible: bool,
    },
    /// Keep a layer from being picked or moved on the canvas.
    SetLocked {
        id: NodeId,
        locked: bool,
    },
    /// Confine a layer to the one below it, or let it out again.
    SetClipped {
        id: NodeId,
        clipped: bool,
    },
    /// Say what a layer does when the frame around it is resized.
    SetPinning {
        id: NodeId,
        pinned: Pinning,
    },
    SetBlendMode {
        id: NodeId,
        blend: BlendMode,
    },
    SetTransform {
        id: NodeId,
        transform: Transform,
    },
    SetKind {
        id: NodeId,
        kind: Box<NodeKind>,
    },
    /// Lay a stroke onto a paint layer, at `index` in the order the
    /// layer's strokes are painted in. With `on_mask`, it goes onto the
    /// node's painted mask instead of the node itself.
    AddStroke {
        id: NodeId,
        index: usize,
        stroke: Box<PaintStroke>,
        #[serde(default)]
        on_mask: bool,
    },
    /// Take one back off.
    RemoveStroke {
        id: NodeId,
        index: usize,
        #[serde(default)]
        on_mask: bool,
    },
    /// Replace one in place, which is what a brush gesture does to the
    /// stroke it is still drawing.
    SetStroke {
        id: NodeId,
        index: usize,
        stroke: Box<PaintStroke>,
        #[serde(default)]
        on_mask: bool,
    },
    SetName {
        id: NodeId,
        name: String,
    },
    /// Attach, replace, or clear (None) a node's mask.
    SetMask {
        id: NodeId,
        mask: Option<Box<Mask>>,
    },
    /// Replace a node's whole effect list. Whole rather than per-effect so
    /// adding, removing, reordering and retuning are all one command with
    /// one obvious inverse.
    SetEffects {
        id: NodeId,
        effects: Vec<Effect>,
    },
    /// Replace the document's guides. Whole-list, like effects: adding,
    /// moving and clearing one are then the same command with the same
    /// obvious inverse.
    SetGuides {
        guides: Vec<Guide>,
    },
    /// Replace the document's palette, the same whole-list way.
    SetSwatches {
        swatches: Vec<Swatch>,
    },
    /// Replace the regions kept by name, the same whole-list way:
    /// keeping one, renaming one and forgetting one are then the same
    /// command with the same obvious inverse.
    SetRegions {
        regions: Vec<KeptRegion>,
    },
    /// Replace the region picked out of the page — `None` for nothing
    /// picked out. Whole-value, so picking, adding to, taking from and
    /// dropping a selection are all the one command with the one
    /// obvious inverse.
    SetSelection {
        selection: Option<Box<Mask>>,
    },
    /// Change the page's size, shifting every top-level layer by
    /// `(dx, dy)` so a crop keeps the picture where it was. Its own
    /// inverse in shape: the old size, and the shift the other way.
    ResizeCanvas {
        width: u32,
        height: u32,
        dx: f32,
        dy: f32,
    },
    /// Mirror the page, left to right across its middle when `across_x`
    /// and top to bottom when not. The page keeps its size and
    /// everything on it — layers and guides — is reflected with it. Its
    /// own inverse: doing it twice is doing nothing.
    MirrorCanvas {
        across_x: bool,
    },
    /// Turn the page by any angle about its own middle and give it the
    /// size it should have afterwards, the middle of the new page landing
    /// on the middle of the old one. What a crooked horizon is put right
    /// with — and non-destructive here in a way it is not in a pixel
    /// editor, since a turned layer is a transform rather than resampled
    /// pixels, and the corners it takes in are off the page rather than
    /// gone.
    ///
    /// The size is asked for rather than worked out, so the command says
    /// exactly what it does: its inverse is the turn back with the old
    /// size, which lands every point where it started.
    /// [`straightened_size`] is what a caller usually asks for it.
    StraightenCanvas {
        degrees: f32,
        width: u32,
        height: u32,
    },
    /// Turn the page a quarter of the way round, `quarters` times
    /// clockwise. An odd number swaps the page's width and height, and
    /// everything on it — layers and guides alike — turns with it, so
    /// what the page holds is unchanged and only its orientation is not.
    /// Its own inverse in shape: the same command with the turn made up
    /// to a full circle. A page that fits can always be stood on its
    /// end, since what a page may be is the same either way round.
    TurnCanvas {
        quarters: u8,
    },
    /// Reparent/reorder a node. `index` is the position in the destination
    /// group's child list (painter's order: 0 = bottom).
    MoveNode {
        id: NodeId,
        parent: NodeId,
        index: usize,
    },
    /// Several commands applied atomically: all succeed, or the document is
    /// rolled back to its state before the batch. One undo step.
    Batch(Vec<Command>),
}

/// Linear undo/redo stacks of inverse commands.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl History {
    pub fn apply(&mut self, doc: &mut Document, cmd: Command) -> Result<(), DocError> {
        let inverse = doc.apply(cmd)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, DocError> {
        match self.undo.pop() {
            None => Ok(false),
            Some(inv) => {
                let redo = doc.apply(inv)?;
                self.redo.push(redo);
                Ok(true)
            }
        }
    }

    pub fn redo(&mut self, doc: &mut Document) -> Result<bool, DocError> {
        match self.redo.pop() {
            None => Ok(false),
            Some(cmd) => {
                let inv = doc.apply(cmd)?;
                self.undo.push(inv);
                Ok(true)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(name: &str) -> Box<Node> {
        Box::new(Node::vector(
            name,
            VectorShape::Rect {
                width: 10.0,
                height: 10.0,
                radius: 0.0,
            },
        ))
    }

    /// Every command's inverse puts the document back exactly as it was.
    ///
    /// That is the invariant the whole editor rests on — undo is the
    /// inverse stack, nothing else — and it was only ever tested one
    /// command at a time, which is how a new one gets added with an
    /// inverse nobody checks. Here it is asked of every kind at once,
    /// against a document with something of everything in it, and the
    /// comparison is the serialized document, so a field that quietly
    /// fails to come back is caught rather than a field somebody
    /// remembered to look at.
    #[test]
    fn every_command_undoes_to_exactly_where_it_started() {
        let f = fixture::everything();
        let each = fixture::every_command(&f);
        let doc = f.doc;

        // An id is never handed out twice — a stale reference must never
        // find a different node — so a document that has had something
        // added and taken away again is not byte for byte where it
        // started: the next id it will use has moved on. Everything else
        // has to be, and the id may only ever go forwards.
        let state = |doc: &Document| {
            let next = serde_json::to_value(doc).expect("a document serializes")["next_id"]
                .as_u64()
                .expect("a document says its next id");
            (fixture::state(doc), next)
        };
        let (before, first_id) = state(&doc);
        for cmd in each {
            let mut copy = doc.clone();
            let label = format!("{cmd:?}");
            let exact = fixture::exact(&cmd);
            let inverse = copy
                .apply(cmd)
                .unwrap_or_else(|e| panic!("{label} could not be applied: {e:?}"));
            assert!(
                state(&copy).0 != before,
                "{label} changed nothing, so its inverse proves nothing"
            );
            copy.apply(inverse)
                .unwrap_or_else(|e| panic!("{label}'s inverse would not apply: {e:?}"));
            let (after, next_id) = state(&copy);
            // A page turned by anything but a quarter cannot be turned
            // back bit for bit — a sine and its cosine do not multiply
            // out to exactly one — and an axis-aligned guide cannot
            // record a tilt at all, so it comes back on its own axis
            // where it crosses the middle of the page rather than
            // exactly where it was. Everything else is exact, and this
            // is the one command allowed either.
            if let Err(what) = fixture::came_back(&after, &before, exact) {
                panic!("{label} did not undo to where it started: {what}");
            }
            assert!(
                next_id >= first_id,
                "{label} handed an id back to be used twice"
            );
        }
    }

    /// A guide is a line, and a page that moves carries it along as one.
    /// A quarter turn stands it on its end, a mirror reflects it, and a
    /// straighten leaves it on the axis it was on — which is what asking
    /// the mapped *direction* answers, where asking whether the two ends
    /// of it still share an x turned every vertical guide horizontal the
    /// moment the page was turned by anything but a right angle.
    #[test]
    fn a_guide_keeps_the_axis_the_page_leaves_it_on() {
        let upright = |doc: &Document| match doc.guides() {
            [Guide::Vertical(v)] => *v,
            other => panic!("expected one upright guide, got {other:?}"),
        };

        // A straighten of a few degrees: still upright, still about where
        // it was.
        let mut doc = Document::new(80, 60, ColorMode::Rgb);
        doc.apply(Command::SetGuides {
            guides: vec![Guide::Vertical(12.0)],
        })
        .unwrap();
        let (w, h) = straightened_size(80, 60, 7.5);
        doc.apply(Command::StraightenCanvas {
            degrees: 7.5,
            width: w,
            height: h,
        })
        .unwrap();
        // Still upright, and still the same way over from the middle of
        // the page — which is what a turn about the middle leaves it,
        // give or take the tilt it cannot record: a line 28 across from
        // the centre, tilted by seven and a half degrees, crosses the
        // middle row 28/cos of that away.
        let after = upright(&doc);
        let across = (doc.meta.width as f32 / 2.0 - after).abs();
        let want = 28.0 / 7.5f32.to_radians().cos();
        assert!(
            (across - want).abs() < 0.5,
            "a straightened page keeps its guide where it was: {across} across, wanted {want}"
        );

        // A quarter turn lays it down, at the page's new coordinates.
        let mut turned = Document::new(80, 60, ColorMode::Rgb);
        turned
            .apply(Command::SetGuides {
                guides: vec![Guide::Vertical(12.0)],
            })
            .unwrap();
        turned.apply(Command::TurnCanvas { quarters: 1 }).unwrap();
        match turned.guides() {
            [Guide::Horizontal(v)] => assert_eq!(*v, 12.0, "and lands where the turn put it"),
            other => panic!("a quarter turn lays a guide down: {other:?}"),
        }

        // And a mirror leaves it upright, on the other side of the page.
        let mut flipped = Document::new(80, 60, ColorMode::Rgb);
        flipped
            .apply(Command::SetGuides {
                guides: vec![Guide::Vertical(12.0)],
            })
            .unwrap();
        flipped
            .apply(Command::MirrorCanvas { across_x: true })
            .unwrap();
        assert_eq!(upright(&flipped), 68.0);
    }

    /// A copy that could reach itself would have nothing to draw, so the
    /// document refuses to make one and stays exactly as it was.
    #[test]
    fn a_copy_cannot_be_made_to_hold_itself() {
        let mut doc = Document::new(80, 60, ColorMode::Rgb);
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
            node: rect("r1"),
        })
        .unwrap();
        // A copy of the group, on the page: fine.
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::instance("copy", group)),
        })
        .unwrap();
        let copy = doc.children_of(root).unwrap()[1];
        let before = doc.node_count();

        // The same copy moved inside what it copies: refused, and
        // nothing about the document changed.
        assert_eq!(
            doc.apply(Command::MoveNode {
                id: copy,
                parent: group,
                index: 1,
            })
            .err(),
            Some(DocError::InstanceCycle)
        );
        assert_eq!(doc.parent_of(copy), Some(root));
        assert_eq!(doc.children_of(group).unwrap().len(), 1);
        assert_eq!(doc.node_count(), before);

        // A copy of itself is refused the same way.
        assert_eq!(
            doc.apply(Command::AddNode {
                parent: group,
                index: 1,
                node: Box::new(Node::instance("self", group)),
            })
            .err(),
            Some(DocError::InstanceCycle)
        );
        assert_eq!(doc.node_count(), before);
    }

    /// Confining a layer to the one below it is a plain switch with a
    /// plain inverse, and a document written before it existed loads as
    /// what it was: nothing confined.
    #[test]
    fn clipping_a_layer_undoes_to_where_it_started() {
        let mut doc = Document::new(80, 60, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("r1"),
                },
            )
            .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        assert!(!doc.node(id).unwrap().clipped);
        history
            .apply(&mut doc, Command::SetClipped { id, clipped: true })
            .unwrap();
        assert!(doc.node(id).unwrap().clipped);
        assert!(history.undo(&mut doc).unwrap());
        assert!(!doc.node(id).unwrap().clipped);

        // A node written without the field reads as unconfined.
        let json = serde_json::to_value(doc.node(id).unwrap()).unwrap();
        let mut object = json.as_object().unwrap().clone();
        assert!(object.remove("clipped").is_some(), "it is written out");
        let old: Node = serde_json::from_value(object.into()).unwrap();
        assert!(!old.clipped);
    }

    #[test]
    fn add_then_undo_then_redo() {
        let mut doc = Document::new(800, 600, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();

        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("r1"),
                },
            )
            .unwrap();
        assert_eq!(doc.children_of(root).unwrap().len(), 1);

        assert!(history.undo(&mut doc).unwrap());
        assert_eq!(doc.children_of(root).unwrap().len(), 0);
        assert_eq!(doc.node_count(), 1); // just root

        assert!(history.redo(&mut doc).unwrap());
        assert_eq!(doc.children_of(root).unwrap().len(), 1);
        assert_eq!(
            doc.node(doc.children_of(root).unwrap()[0]).unwrap().name,
            "r1"
        );
    }

    #[test]
    fn removing_group_restores_whole_subtree_on_undo() {
        let mut doc = Document::new(100, 100, ColorMode::Cmyk);
        let mut history = History::default();
        let root = doc.root();

        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: Box::new(Node::group("g")),
                },
            )
            .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: group,
                    index: 0,
                    node: rect("child"),
                },
            )
            .unwrap();
        assert_eq!(doc.node_count(), 3);

        history
            .apply(&mut doc, Command::RemoveNode { id: group })
            .unwrap();
        assert_eq!(doc.node_count(), 1);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node_count(), 3);
        let restored_group = doc.children_of(root).unwrap()[0];
        assert_eq!(restored_group, group);
        assert_eq!(doc.children_of(group).unwrap().len(), 1);
    }

    #[test]
    fn set_opacity_is_invertible_and_new_edit_clears_redo() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("r"),
                },
            )
            .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        history
            .apply(&mut doc, Command::SetOpacity { id, opacity: 0.5 })
            .unwrap();
        assert_eq!(doc.node(id).unwrap().opacity, 0.5);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node(id).unwrap().opacity, 1.0);
        assert!(history.can_redo());

        history
            .apply(&mut doc, Command::SetVisible { id, visible: false })
            .unwrap();
        assert!(!history.can_redo());
    }

    #[test]
    fn cannot_add_under_leaf_or_remove_root() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: rect("r"),
        })
        .unwrap();
        let leaf = doc.children_of(root).unwrap()[0];

        let err = doc
            .apply(Command::AddNode {
                parent: leaf,
                index: 0,
                node: rect("x"),
            })
            .unwrap_err();
        assert_eq!(err, DocError::NotAGroup(leaf));
        assert_eq!(
            doc.apply(Command::RemoveNode { id: root }).unwrap_err(),
            DocError::CannotRemoveRoot
        );
    }

    #[test]
    fn move_node_reorders_and_undoes() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        for name in ["a", "b", "c"] {
            let index = doc.children_of(root).unwrap().len();
            history
                .apply(
                    &mut doc,
                    Command::AddNode {
                        parent: root,
                        index,
                        node: rect(name),
                    },
                )
                .unwrap();
        }
        let ids = doc.children_of(root).unwrap().to_vec();

        // Move bottom node "a" to the top.
        history
            .apply(
                &mut doc,
                Command::MoveNode {
                    id: ids[0],
                    parent: root,
                    index: 2,
                },
            )
            .unwrap();
        assert_eq!(doc.children_of(root).unwrap(), &[ids[1], ids[2], ids[0]]);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.children_of(root).unwrap(), &[ids[0], ids[1], ids[2]]);
    }

    #[test]
    fn move_node_rejects_own_subtree_and_reparents() {
        let mut doc = Document::new(100, 100, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: rect("r"),
        })
        .unwrap();
        let r = doc.children_of(root).unwrap()[1];

        // Reparent the rect into the group.
        doc.apply(Command::MoveNode {
            id: r,
            parent: group,
            index: 0,
        })
        .unwrap();
        assert_eq!(doc.children_of(group).unwrap(), &[r]);

        // A group can't move into itself.
        assert_eq!(
            doc.apply(Command::MoveNode {
                id: group,
                parent: group,
                index: 0,
            })
            .unwrap_err(),
            DocError::MoveIntoOwnSubtree(group)
        );
    }

    #[test]
    fn set_name_is_invertible() {
        let mut doc = Document::new(10, 10, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("old"),
                },
            )
            .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        history
            .apply(
                &mut doc,
                Command::SetName {
                    id,
                    name: "new".into(),
                },
            )
            .unwrap();
        assert_eq!(doc.node(id).unwrap().name, "new");
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node(id).unwrap().name, "old");
    }

    #[test]
    fn batch_applies_atomically_and_rolls_back_on_failure() {
        let mut doc = Document::new(50, 50, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();

        // A batch that adds a group and moves a new rect into it, using the
        // predicted ids.
        let group_id = doc.peek_next_id();
        let rect_id = NodeId(group_id.0 + 1);
        history
            .apply(
                &mut doc,
                Command::Batch(vec![
                    Command::AddNode {
                        parent: root,
                        index: 0,
                        node: Box::new(Node::group("g")),
                    },
                    Command::AddNode {
                        parent: root,
                        index: 1,
                        node: rect("r"),
                    },
                    Command::MoveNode {
                        id: rect_id,
                        parent: group_id,
                        index: 0,
                    },
                ]),
            )
            .unwrap();
        assert_eq!(doc.children_of(root).unwrap(), &[group_id]);
        assert_eq!(doc.children_of(group_id).unwrap(), &[rect_id]);

        // One undo unwinds the whole batch; redo replays it, ids intact.
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node_count(), 1);
        history.redo(&mut doc).unwrap();
        assert_eq!(doc.children_of(group_id).unwrap(), &[rect_id]);

        // A failing step rolls the earlier steps back.
        let before = doc.node_count();
        let err = doc.apply(Command::Batch(vec![
            Command::AddNode {
                parent: root,
                index: 0,
                node: rect("orphan"),
            },
            Command::RemoveNode { id: NodeId(9999) },
        ]));
        assert!(err.is_err());
        assert_eq!(doc.node_count(), before, "partial batch rolled back");
    }

    #[test]
    fn set_mask_attaches_and_undoes() {
        let mut doc = Document::new(10, 10, ColorMode::Rgb);
        let mut history = History::default();
        let root = doc.root();
        history
            .apply(
                &mut doc,
                Command::AddNode {
                    parent: root,
                    index: 0,
                    node: rect("r"),
                },
            )
            .unwrap();
        let id = doc.children_of(root).unwrap()[0];

        let mask = Mask {
            kind: MaskKind::Vector {
                shape: VectorShape::Ellipse { rx: 5.0, ry: 5.0 },
                transform: Transform::default(),
            },
            invert: false,
            feather: 0.0,
        };
        history
            .apply(
                &mut doc,
                Command::SetMask {
                    id,
                    mask: Some(Box::new(mask.clone())),
                },
            )
            .unwrap();
        assert_eq!(doc.node(id).unwrap().mask.as_ref(), Some(&mask));

        history
            .apply(&mut doc, Command::SetMask { id, mask: None })
            .unwrap();
        assert!(doc.node(id).unwrap().mask.is_none());
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.node(id).unwrap().mask.as_ref(), Some(&mask));
        history.undo(&mut doc).unwrap();
        assert!(doc.node(id).unwrap().mask.is_none());
    }

    #[test]
    fn resources_are_content_addressed_and_survive_manifest_roundtrip() {
        let mut doc = Document::new(10, 10, ColorMode::Rgb);
        let bytes = vec![7u8; 2 * 2 * 4];
        let id1 = doc.add_resource(2, 2, bytes.clone());
        let id2 = doc.add_resource(2, 2, bytes.clone());
        assert_eq!(id1, id2, "identical content shares one entry");
        assert_eq!(doc.resources().count(), 1);

        // Manifest serialization keeps dimensions but not bytes …
        let json = serde_json::to_string(&doc).unwrap();
        let mut restored: Document = serde_json::from_str(&json).unwrap();
        let res = restored.resource(&id1).unwrap();
        assert_eq!((res.width, res.height), (2, 2));
        assert!(res.rgba8.is_empty());

        // … and the container layer restores them, validating the length.
        assert!(!restored.restore_resource_bytes(&id1, vec![1u8; 3]));
        assert!(restored.restore_resource_bytes(&id1, bytes.clone()));
        assert_eq!(restored.resource(&id1).unwrap().rgba8, bytes);
    }

    #[test]
    fn a_path_written_before_bezier_handles_still_loads() {
        // Same additive contract as gradients: an older path has no handles
        // field, and must load as a plain polyline rather than failing.
        let json = r#"{ "Path": { "points": [[0.0, 0.0], [4.0, 4.0]], "closed": false } }"#;
        let shape: VectorShape = serde_json::from_str(json).unwrap();
        match shape {
            VectorShape::Path {
                handles,
                smooth,
                points,
                ..
            } => {
                assert!(handles.is_empty(), "absent handles default to none");
                assert!(!smooth, "and absent smooth defaults to off");
                assert_eq!(points.len(), 2);
            }
            other => panic!("expected a path, got {other:?}"),
        }
    }

    #[test]
    fn a_document_written_before_gradients_still_loads() {
        // File compatibility is additive: a Vector node serialized without
        // the gradient field must load, not fail the whole document.
        let json = r#"{
            "Vector": {
                "shape": { "Rect": { "width": 4.0, "height": 4.0 } },
                "fill": { "Srgb": { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 } },
                "stroke": null
            }
        }"#;
        let kind: NodeKind = serde_json::from_str(json).unwrap();
        match kind {
            NodeKind::Vector { gradient, fill, .. } => {
                assert!(gradient.is_none(), "absent gradient defaults to none");
                assert!(fill.is_some(), "the rest of the node still parses");
            }
            other => panic!("expected a vector node, got {other:?}"),
        }
    }

    #[test]
    fn document_roundtrips_through_json() {
        let mut doc = Document::new(640, 480, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: rect("r"),
        })
        .unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.node_count(), doc.node_count());
        assert_eq!(restored.meta.width, 640);
    }
}
