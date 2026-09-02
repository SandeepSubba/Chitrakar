//! The one crate the app shells embed.
//!
//! The UI never touches pixel buffers or the document directly: it sends
//! [`Command`]s (as values natively, as JSON over the WASM boundary) and
//! receives rendered frames. The [`wasm`] module is the wasm-bindgen surface
//! the webview UI drives.
//!
//! [`Session`] keeps a cached composite and, for every mutation, computes the
//! dirty document region from node bounds — so interactive edits (including
//! live previews) re-render only the pixels they can affect.

#[cfg(target_arch = "wasm32")]
pub mod wasm;

use serde::Serialize;

pub use chitrakar_color::ColorMode;
pub use chitrakar_doc::{Command, Document, History, Node, NodeId, NodeKind, Transform};

/// How far a duplicate or a paste is nudged from its original, in document
/// units.
const DUPLICATE_OFFSET: f32 = 12.0;

/// A copied subtree, held whole rather than serialized: the clipboard is
/// in-process state, and JSON would only cost a round trip through text.
/// Resource pixels travel with it so a paste into a *different* document
/// still has its image — resource ids are content-addressed, so re-adding
/// the same bytes there yields the id the nodes already reference.
#[derive(Clone)]
struct ClipNode {
    node: Node,
    children: Vec<ClipNode>,
}

#[derive(Clone)]
struct Clipboard {
    root: ClipNode,
    resources: Vec<(u32, u32, Vec<u8>)>,
}

thread_local! {
    /// Outlives any one Session, which is what makes copy in one document
    /// and paste in the next work at all.
    static CLIPBOARD: std::cell::RefCell<Option<Clipboard>> =
        const { std::cell::RefCell::new(None) };
}

/// Is there anything to paste?
pub fn clipboard_has_content() -> bool {
    CLIPBOARD.with(|c| c.borrow().is_some())
}
pub use chitrakar_render::{Bounds, ClipRect, Surface};

#[derive(Debug)]
pub enum EngineError {
    Doc(chitrakar_doc::DocError),
    BadCommand(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Doc(e) => write!(f, "{e}"),
            Self::BadCommand(e) => write!(f, "invalid command: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<chitrakar_doc::DocError> for EngineError {
    fn from(e: chitrakar_doc::DocError) -> Self {
        Self::Doc(e)
    }
}

/// One recorded edit: the inverse that undoes it and a human-readable label
/// for the history panel.
struct HistoryEntry {
    inverse: Command,
    label: String,
}

/// The verb a boolean operation is described by in history and in the name
/// the combined layer takes.
fn label_for(op: chitrakar_render::boolean::BoolOp) -> &'static str {
    use chitrakar_render::boolean::BoolOp;
    match op {
        BoolOp::Union => "Union",
        BoolOp::Intersect => "Intersect",
        BoolOp::Subtract => "Subtract",
        BoolOp::Exclude => "Exclude",
    }
}

/// An open document plus its edit history and cached composite.
pub struct Session {
    doc: Document,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    cache: Option<Surface>,
    /// Reused scratch surface for padded region renders under filters.
    scratch: Option<Surface>,
    /// Region of `cache` that must be recomputed before the next present,
    /// in document pixels — `render_cached` carries it into the cache's own
    /// resolution.
    stale: Option<ClipRect>,
    /// Set when the whole presented surface must be redrawn rather than a
    /// region of the page: a new or resized cache, or a viewport that has
    /// moved. The dirty region is tracked in document pixels and so cannot
    /// say anything about the part of the surface the page does not cover,
    /// which is exactly the part a pan leaves stale.
    stale_all: bool,
    /// Resolution the cache is kept at, as a multiple of document pixels.
    /// Presenting a magnified document at 1.0 and letting the display
    /// enlarge the result is what makes a zoomed-in canvas look soft; the
    /// fix is to render more pixels, not to interpolate the ones we have.
    view_scale: f32,
    /// Where the document's origin sits on the presented surface, in that
    /// surface's own pixels. Non-zero once a viewport is set: the surface
    /// is then a window onto the page rather than the page itself.
    view_origin: (f32, f32),
    /// The presented surface's size when it is a viewport rather than the
    /// whole page.
    viewport: Option<(u32, u32)>,
    /// Inverse restoring the state before the current preview gesture, with
    /// the label captured from the gesture's first command.
    preview_inverse: Option<HistoryEntry>,
    /// Total pixels re-rendered so far (observability for tests and tuning).
    pixels_recomputed: u64,
    /// Soft-proofing (display-only): round-trip presented pixels through the
    /// document's press profile, optionally marking out-of-gamut pixels.
    proof_cms: Option<chitrakar_color::cms::ProofCms>,
    soft_proof: bool,
    gamut_warn: bool,
}

impl Session {
    pub fn new(width: u32, height: u32, color_mode: ColorMode) -> Self {
        Self::from_document(Document::new(width, height, color_mode))
    }

    fn from_document(doc: Document) -> Self {
        Self {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            cache: None,
            scratch: None,
            stale: None,
            stale_all: true,
            view_scale: 1.0,
            view_origin: (0.0, 0.0),
            viewport: None,
            preview_inverse: None,
            pixels_recomputed: 0,
            proof_cms: None,
            soft_proof: false,
            gamut_warn: false,
        }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn pixels_recomputed(&self) -> u64 {
        self.pixels_recomputed
    }

    /// The node a command touches, when knowable from the command alone.
    fn command_target(cmd: &Command) -> Option<NodeId> {
        match cmd {
            // A resize touches the page and every top-level layer, so
            // there is no one node to compute a dirty region from.
            Command::AddNode { .. }
            | Command::RestoreSubtree { .. }
            | Command::Batch(_)
            | Command::ResizeCanvas { .. }
            // Guides are not artwork: nothing renders them, so nothing
            // needs repainting when they change.
            | Command::SetGuides { .. } => None,
            Command::RemoveNode { id }
            | Command::SetOpacity { id, .. }
            | Command::SetVisible { id, .. }
            | Command::SetBlendMode { id, .. }
            | Command::SetEffects { id, .. }
            | Command::SetTransform { id, .. }
            | Command::SetKind { id, .. }
            | Command::SetName { id, .. }
            | Command::SetMask { id, .. }
            | Command::MoveNode { id, .. } => Some(*id),
        }
    }

    fn bounds_of_target(&self, id: Option<NodeId>) -> Bounds {
        id.and_then(|id| chitrakar_render::node_bounds(&self.doc, id).ok())
            .unwrap_or(Bounds::None)
    }

    fn mark_dirty(&mut self, bounds: Bounds) {
        let Some(clip) = bounds.to_clip(self.doc.meta.width, self.doc.meta.height) else {
            return;
        };
        self.stale = Some(match self.stale {
            Some(prev) => prev.union(clip),
            None => clip,
        });
    }

    /// Apply a command to the document, computing the dirty region from the
    /// target's bounds before and after. Returns the inverse.
    fn apply_internal(&mut self, cmd: Command) -> Result<Command, EngineError> {
        // Both touch more than one node, so the whole canvas is the only
        // safe dirty region — and a resize changes what "the whole canvas"
        // even means.
        let batch = matches!(cmd, Command::Batch(_) | Command::ResizeCanvas { .. });
        let pre = self.bounds_of_target(Self::command_target(&cmd));
        let inverse = self.doc.apply(cmd)?;
        let post = self.bounds_of_target(Self::command_target(&inverse));
        if batch {
            self.mark_dirty(Bounds::Everything);
        } else {
            // Filters read pixel neighborhoods, so a change also affects
            // pixels within the filter stack's reach of it. Grow the dirty
            // region by that reach rather than invalidating everything;
            // render_cached additionally computes a padded margin so the
            // reported region's own values are correct.
            let mut bounds = pre.union(post);
            let reach = chitrakar_render::filter_reach(&self.doc) as f32;
            if reach > 0.0 {
                if let Bounds::Rect(x0, y0, x1, y1) = bounds {
                    bounds = Bounds::Rect(x0 - reach, y0 - reach, x1 + reach, y1 + reach);
                }
            }
            self.mark_dirty(bounds);
        }
        Ok(inverse)
    }

    /// Human-readable label for a command, read against the current document
    /// (before the command applies, so names refer to the edited node).
    fn describe(&self, cmd: &Command) -> String {
        let name = |id: &NodeId| {
            self.doc
                .node(*id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|_| "layer".into())
        };
        match cmd {
            Command::AddNode { node, .. } => format!("Add {}", node.name),
            Command::RemoveNode { id } => format!("Delete {}", name(id)),
            Command::RestoreSubtree { .. } => "Restore layer".into(),
            Command::SetOpacity { id, .. } => format!("Opacity of {}", name(id)),
            Command::SetVisible { id, visible } => {
                format!("{} {}", if *visible { "Show" } else { "Hide" }, name(id))
            }
            Command::SetBlendMode { id, .. } => format!("Blend of {}", name(id)),
            Command::SetTransform { id, .. } => format!("Transform {}", name(id)),
            Command::SetKind { id, .. } => format!("Edit {}", name(id)),
            Command::SetName { id, .. } => format!("Rename {}", name(id)),
            Command::SetMask { id, mask } => format!(
                "{} mask on {}",
                if mask.is_some() { "Set" } else { "Clear" },
                name(id)
            ),
            Command::SetEffects { id, effects } => format!(
                "{} effects on {}",
                if effects.is_empty() { "Clear" } else { "Set" },
                name(id)
            ),
            Command::MoveNode { id, .. } => format!("Move {}", name(id)),
            Command::SetGuides { guides } => {
                if guides.is_empty() {
                    "Clear guides".into()
                } else {
                    format!("{} guides", guides.len())
                }
            }
            Command::ResizeCanvas { width, height, .. } => {
                format!("Resize canvas to {width}x{height}")
            }
            Command::Batch(_) => "Multiple edits".into(),
        }
    }

    pub fn apply(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.apply_labeled(cmd, None)
    }

    fn apply_labeled(&mut self, cmd: Command, label: Option<String>) -> Result<(), EngineError> {
        self.commit_preview(); // a stray preview must not leak into this edit
        let label = label.unwrap_or_else(|| self.describe(&cmd));
        let inverse = self.apply_internal(cmd)?;
        self.undo.push(HistoryEntry { inverse, label });
        self.redo.clear();
        Ok(())
    }

    /// Apply a JSON-encoded command — the transport used across the WASM/IPC
    /// boundary.
    pub fn apply_json(&mut self, json: &str) -> Result<(), EngineError> {
        self.apply(parse_command(json)?)
    }

    /// Apply a command as part of an in-flight gesture (drag preview). The
    /// document updates and re-renders, but history records nothing until
    /// [`commit_preview`](Self::commit_preview); the first preview of a
    /// gesture captures the inverse that undoes the whole gesture.
    pub fn preview(&mut self, cmd: Command) -> Result<(), EngineError> {
        let label = self.describe(&cmd);
        let inverse = self.apply_internal(cmd)?;
        if self.preview_inverse.is_none() {
            self.preview_inverse = Some(HistoryEntry { inverse, label });
        }
        Ok(())
    }

    pub fn preview_json(&mut self, json: &str) -> Result<(), EngineError> {
        self.preview(parse_command(json)?)
    }

    /// End the gesture, recording it as one undo step. Returns false if no
    /// preview was active.
    pub fn commit_preview(&mut self) -> bool {
        match self.preview_inverse.take() {
            Some(entry) => {
                self.undo.push(entry);
                self.redo.clear();
                true
            }
            None => false,
        }
    }

    /// Abort the gesture, restoring the pre-gesture state. Returns false if
    /// no preview was active.
    pub fn cancel_preview(&mut self) -> Result<bool, EngineError> {
        match self.preview_inverse.take() {
            Some(entry) => {
                self.apply_internal(entry.inverse)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn undo(&mut self) -> Result<bool, EngineError> {
        self.commit_preview();
        match self.undo.pop() {
            None => Ok(false),
            Some(entry) => {
                let redo = self.apply_internal(entry.inverse)?;
                self.redo.push(HistoryEntry {
                    inverse: redo,
                    label: entry.label,
                });
                Ok(true)
            }
        }
    }

    pub fn redo(&mut self) -> Result<bool, EngineError> {
        match self.redo.pop() {
            None => Ok(false),
            Some(entry) => {
                let inverse = self.apply_internal(entry.inverse)?;
                self.undo.push(HistoryEntry {
                    inverse,
                    label: entry.label,
                });
                Ok(true)
            }
        }
    }

    /// History labels for the panel: past edits oldest-first, then future
    /// (undone) edits nearest-first. `undo_depth` marks the current position.
    pub fn history_labels(&self) -> (Vec<String>, Vec<String>) {
        (
            self.undo.iter().map(|e| e.label.clone()).collect(),
            self.redo.iter().rev().map(|e| e.label.clone()).collect(),
        )
    }

    /// Move through history: negative = undo that many steps, positive =
    /// redo. Stops early at either end.
    pub fn jump(&mut self, delta: i32) -> Result<(), EngineError> {
        for _ in 0..delta.unsigned_abs() {
            let moved = if delta < 0 {
                self.undo()?
            } else {
                self.redo()?
            };
            if !moved {
                break;
            }
        }
        Ok(())
    }

    /// Group same-parent nodes into a new group at the topmost member's
    /// position, as one undo step. Returns the group's id.
    pub fn group_nodes(&mut self, ids: &[NodeId], name: &str) -> Result<NodeId, EngineError> {
        if ids.is_empty() {
            return Err(EngineError::BadCommand("nothing selected".into()));
        }
        let parent = self
            .doc
            .parent_of(ids[0])
            .ok_or_else(|| EngineError::BadCommand("cannot group the root".into()))?;
        // All members must share the parent; order them bottom-to-top.
        let siblings = self.doc.children_of(parent)?;
        let mut ordered: Vec<NodeId> = siblings
            .iter()
            .copied()
            .filter(|s| ids.contains(s))
            .collect();
        if ordered.len() != ids.len() {
            return Err(EngineError::BadCommand(
                "grouped layers must share a parent".into(),
            ));
        }
        let top_index = siblings
            .iter()
            .position(|s| *s == *ordered.last().unwrap())
            .unwrap();

        let group_id = self.doc.peek_next_id();
        let mut cmds = vec![Command::AddNode {
            parent,
            index: top_index + 1,
            node: Box::new(Node::group(name)),
        }];
        cmds.extend(
            ordered
                .drain(..)
                .enumerate()
                .map(|(i, id)| Command::MoveNode {
                    id,
                    parent: group_id,
                    index: i,
                }),
        );
        self.apply_labeled(Command::Batch(cmds), Some(format!("Group into {name}")))?;
        Ok(group_id)
    }

    /// Combine two or more shape layers into one compound path.
    ///
    /// Operands are taken bottom-to-top in the stack, which is what makes
    /// "subtract" mean what it looks like: the shape underneath, with the
    /// ones above cut out of it. The result keeps the bottom-most layer's
    /// fill and stroke, because that is the shape the eye reads as the one
    /// being operated on.
    pub fn boolean_nodes(&mut self, ids: &[NodeId], op: &str) -> Result<NodeId, EngineError> {
        let op = chitrakar_render::boolean::BoolOp::from_name(op)
            .ok_or_else(|| EngineError::BadCommand(format!("unknown operation {op:?}")))?;
        if ids.len() < 2 {
            return Err(EngineError::BadCommand(
                "combining needs at least two shapes".into(),
            ));
        }
        let parent = self
            .doc
            .parent_of(ids[0])
            .ok_or_else(|| EngineError::BadCommand("cannot combine the root".into()))?;
        let siblings = self.doc.children_of(parent)?.to_vec();
        let ordered: Vec<NodeId> = siblings
            .iter()
            .copied()
            .filter(|s| ids.contains(s))
            .collect();
        if ordered.len() != ids.len() {
            return Err(EngineError::BadCommand(
                "combined layers must share a parent".into(),
            ));
        }
        // Every operand's outline, carried into the space they all share.
        let mut rings: Vec<Vec<Vec<[f32; 2]>>> = Vec::new();
        for id in &ordered {
            let node = self.doc.node(*id)?;
            let NodeKind::Vector { shape, .. } = &node.kind else {
                return Err(EngineError::BadCommand(
                    "only shapes can be combined".into(),
                ));
            };
            let t = node.transform;
            rings.push(
                chitrakar_render::shape_rings(shape)
                    .into_iter()
                    .map(|ring| {
                        ring.into_iter()
                            .map(|p| [t.a * p[0] + t.c * p[1] + t.e, t.b * p[0] + t.d * p[1] + t.f])
                            .collect()
                    })
                    .collect(),
            );
        }
        let mut acc = rings.remove(0);
        for next in rings {
            acc = chitrakar_render::boolean::combine(&acc, &next, op).ok_or_else(|| {
                EngineError::BadCommand(
                    "these outlines cannot be combined — they touch or overlap exactly".into(),
                )
            })?;
        }
        // Anchors are stored relative to the node's own origin, like every
        // other path, so the transform carries the position.
        let (mut x0, mut y0) = (f32::MAX, f32::MAX);
        for p in acc.iter().flatten() {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
        }
        if !x0.is_finite() {
            return Err(EngineError::BadCommand("the result is empty".into()));
        }
        let shift = |ring: Vec<[f32; 2]>| -> Vec<[f32; 2]> {
            ring.into_iter().map(|p| [p[0] - x0, p[1] - y0]).collect()
        };
        let mut acc = acc.into_iter();
        let points = shift(acc.next().unwrap());
        let subpaths: Vec<Vec<[f32; 2]>> = acc.map(shift).collect();

        let bottom = self.doc.node(ordered[0])?;
        let (fill, stroke, gradient) = match &bottom.kind {
            NodeKind::Vector {
                fill,
                stroke,
                gradient,
                ..
            } => (*fill, stroke.clone(), gradient.clone()),
            _ => unreachable!("checked above"),
        };
        let name = format!("{} {}", bottom.name.clone(), label_for(op));
        let mut node = Node::vector(
            &name,
            chitrakar_doc::VectorShape::Path {
                points,
                closed: true,
                smooth: false,
                handles: Vec::new(),
                subpaths,
            },
        );
        node.transform = Transform::translation(x0, y0);
        if let NodeKind::Vector {
            fill: f,
            stroke: s,
            gradient: g,
            ..
        } = &mut node.kind
        {
            *f = fill;
            *s = stroke;
            *g = gradient;
        }
        let index = siblings
            .iter()
            .position(|s| *s == ordered[0])
            .unwrap_or(siblings.len());
        let new_id = self.doc.peek_next_id();
        let mut cmds = vec![Command::AddNode {
            parent,
            index,
            node: Box::new(node),
        }];
        cmds.extend(ordered.iter().map(|id| Command::RemoveNode { id: *id }));
        self.apply_labeled(
            Command::Batch(cmds),
            Some(format!("{} shapes", label_for(op))),
        )?;
        Ok(new_id)
    }

    /// Copy a node and everything under it, placed just above the original.
    /// One undo step.
    ///
    /// A node carries no children of its own — the tree lives in the
    /// document — so the copy is a walk that emits an AddNode per node.
    /// Ids are allocated in order as the batch applies, which is what lets
    /// each child name the parent that will exist by the time it is added.
    /// Rasters keep pointing at the same resource: content is immutable and
    /// shared, so a copy costs no pixels.
    pub fn duplicate_node(&mut self, id: NodeId) -> Result<NodeId, EngineError> {
        let parent = self
            .doc
            .parent_of(id)
            .ok_or_else(|| EngineError::BadCommand("cannot duplicate the root".into()))?;
        let index = self
            .doc
            .children_of(parent)?
            .iter()
            .position(|s| *s == id)
            .unwrap_or(0)
            + 1;
        let mut next = self.doc.peek_next_id().0;
        let mut cmds = Vec::new();
        let copy_id = self.emit_copy(id, parent, index, true, &mut next, &mut cmds)?;
        let label = format!("Duplicate {}", self.doc.node(id)?.name);
        self.apply_labeled(Command::Batch(cmds), Some(label))?;
        Ok(copy_id)
    }

    fn emit_copy(
        &self,
        src: NodeId,
        parent: NodeId,
        index: usize,
        rename: bool,
        next: &mut u64,
        cmds: &mut Vec<Command>,
    ) -> Result<NodeId, EngineError> {
        let mut node = self.doc.node(src)?.clone();
        if rename {
            node.name = format!("{} copy", node.name);
            // Offset the copy so it is visible rather than hiding exactly
            // behind the original.
            node.transform.e += DUPLICATE_OFFSET;
            node.transform.f += DUPLICATE_OFFSET;
        }
        let new_id = NodeId(*next);
        *next += 1;
        cmds.push(Command::AddNode {
            parent,
            index,
            node: Box::new(node),
        });
        for (i, child) in self.doc.children_of(src)?.to_vec().iter().enumerate() {
            self.emit_copy(*child, new_id, i, false, next, cmds)?;
        }
        Ok(new_id)
    }

    /// Put a node and everything under it on the clipboard, pixels included.
    pub fn copy_node(&self, id: NodeId) -> Result<(), EngineError> {
        let root = self.clip_of(id)?;
        let mut resources = Vec::new();
        self.collect_resources(&root, &mut resources);
        CLIPBOARD.with(|c| *c.borrow_mut() = Some(Clipboard { root, resources }));
        Ok(())
    }

    fn clip_of(&self, id: NodeId) -> Result<ClipNode, EngineError> {
        Ok(ClipNode {
            node: self.doc.node(id)?.clone(),
            children: self
                .doc
                .children_of(id)?
                .to_vec()
                .iter()
                .map(|c| self.clip_of(*c))
                .collect::<Result<_, _>>()?,
        })
    }

    fn collect_resources(&self, clip: &ClipNode, out: &mut Vec<(u32, u32, Vec<u8>)>) {
        let mut take = |rid: &str| {
            if let Some(r) = self.doc.resource(rid) {
                if !r.rgba8.is_empty() {
                    out.push((r.width, r.height, r.rgba8.clone()));
                }
            }
        };
        if let NodeKind::Raster(r) = &clip.node.kind {
            take(&r.resource_id);
        }
        if let Some(m) = &clip.node.mask {
            if let chitrakar_doc::MaskKind::Raster { resource_id, .. } = &m.kind {
                take(resource_id);
            }
        }
        for child in &clip.children {
            self.collect_resources(child, out);
        }
    }

    /// Paste the clipboard into `parent` (the root when None), nudged clear
    /// of wherever it was copied from. One undo step; `Ok(None)` when there
    /// is nothing to paste.
    pub fn paste(&mut self, parent: Option<NodeId>) -> Result<Option<NodeId>, EngineError> {
        let Some(clip) = CLIPBOARD.with(|c| c.borrow().clone()) else {
            return Ok(None);
        };
        // Restore pixels first: content-addressed ids mean this is a no-op
        // when pasting back into the document the copy came from.
        for (w, h, bytes) in &clip.resources {
            self.doc.add_resource(*w, *h, bytes.clone());
        }
        let parent = parent.unwrap_or_else(|| self.doc.root());
        let index = self.doc.children_of(parent)?.len();
        let mut next = self.doc.peek_next_id().0;
        let mut cmds = Vec::new();
        let id = Self::emit_clip(&clip.root, parent, index, true, &mut next, &mut cmds);
        let label = format!("Paste {}", clip.root.node.name);
        self.apply_labeled(Command::Batch(cmds), Some(label))?;
        Ok(Some(id))
    }

    fn emit_clip(
        clip: &ClipNode,
        parent: NodeId,
        index: usize,
        offset: bool,
        next: &mut u64,
        cmds: &mut Vec<Command>,
    ) -> NodeId {
        let mut node = clip.node.clone();
        if offset {
            node.transform.e += DUPLICATE_OFFSET;
            node.transform.f += DUPLICATE_OFFSET;
        }
        let new_id = NodeId(*next);
        *next += 1;
        cmds.push(Command::AddNode {
            parent,
            index,
            node: Box::new(node),
        });
        for (i, child) in clip.children.iter().enumerate() {
            Self::emit_clip(child, new_id, i, false, next, cmds);
        }
        new_id
    }

    /// Line up or space out several layers, as one undo step.
    ///
    /// `mode` is one of `left`, `center-h`, `right`, `top`, `middle-v`,
    /// `bottom`, `distribute-h`, `distribute-v`. Alignment is measured in
    /// document space — that is where "lined up" means anything — but a
    /// node's transform is written in its parent's, so each move is carried
    /// back through the ancestors before it is applied. Nodes may therefore
    /// come from different groups.
    pub fn align_nodes(&mut self, ids: &[NodeId], mode: &str) -> Result<(), EngineError> {
        if ids.len() < 2 {
            return Err(EngineError::BadCommand(
                "aligning needs at least two layers".into(),
            ));
        }
        // [x0, y0, x1, y1] per node, in document space.
        let mut boxes: Vec<(NodeId, [f32; 4])> = Vec::new();
        for id in ids {
            if let Some(b) = self.bounds_of(*id) {
                boxes.push((*id, [b[0], b[1], b[0] + b[2], b[1] + b[3]]));
            }
        }
        if boxes.len() < 2 {
            return Err(EngineError::BadCommand("nothing to align".into()));
        }
        let union = boxes
            .iter()
            .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |acc, (_, b)| {
                [
                    acc[0].min(b[0]),
                    acc[1].min(b[1]),
                    acc[2].max(b[2]),
                    acc[3].max(b[3]),
                ]
            });

        // Wanted document-space movement per node.
        let mut deltas: Vec<(NodeId, f32, f32)> = Vec::new();
        match mode {
            "left" | "center-h" | "right" | "top" | "middle-v" | "bottom" => {
                for (id, b) in &boxes {
                    let (dx, dy) = match mode {
                        "left" => (union[0] - b[0], 0.0),
                        "right" => (union[2] - b[2], 0.0),
                        "center-h" => ((union[0] + union[2] - b[0] - b[2]) / 2.0, 0.0),
                        "top" => (0.0, union[1] - b[1]),
                        "bottom" => (0.0, union[3] - b[3]),
                        _ => (0.0, (union[1] + union[3] - b[1] - b[3]) / 2.0),
                    };
                    deltas.push((*id, dx, dy));
                }
            }
            "distribute-h" | "distribute-v" => {
                let horizontal = mode == "distribute-h";
                let centre = |b: &[f32; 4]| {
                    if horizontal {
                        (b[0] + b[2]) / 2.0
                    } else {
                        (b[1] + b[3]) / 2.0
                    }
                };
                boxes.sort_by(|a, b| {
                    centre(&a.1)
                        .partial_cmp(&centre(&b.1))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                // The outermost two stay put and the rest space evenly
                // between them, which is what makes the result stable if you
                // run it twice.
                let (first, last) = (centre(&boxes[0].1), centre(&boxes[boxes.len() - 1].1));
                let step = (last - first) / (boxes.len() - 1) as f32;
                for (i, (id, b)) in boxes.iter().enumerate() {
                    let want = first + step * i as f32;
                    let d = want - centre(b);
                    deltas.push((
                        *id,
                        if horizontal { d } else { 0.0 },
                        if horizontal { 0.0 } else { d },
                    ));
                }
            }
            other => {
                return Err(EngineError::BadCommand(format!(
                    "unknown alignment: {other}"
                )))
            }
        }

        let mut cmds = Vec::new();
        for (id, dx, dy) in deltas {
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            // A displacement in document space is a vector, so only the
            // linear part of the ancestors' transform applies to it.
            let a = chitrakar_render::ancestor_space(&self.doc, id);
            let det = a.a * a.d - a.b * a.c;
            let (px, py) = if det.abs() < 1e-9 {
                (dx, dy)
            } else {
                ((a.d * dx - a.c * dy) / det, (a.a * dy - a.b * dx) / det)
            };
            let t = self.doc.node(id)?.transform;
            cmds.push(Command::SetTransform {
                id,
                transform: Transform {
                    e: t.e + px,
                    f: t.f + py,
                    ..t
                },
            });
        }
        if cmds.is_empty() {
            return Ok(());
        }
        self.apply_labeled(Command::Batch(cmds), Some(format!("Align ({mode})")))
    }

    /// Dissolve a group: its children move to its position in the parent,
    /// the empty group is removed. One undo step.
    pub fn ungroup_node(&mut self, id: NodeId) -> Result<(), EngineError> {
        if !matches!(self.doc.node(id)?.kind, NodeKind::Group) {
            return Err(EngineError::BadCommand("not a group".into()));
        }
        let label = format!("Ungroup {}", self.doc.node(id)?.name);
        let parent = self
            .doc
            .parent_of(id)
            .ok_or_else(|| EngineError::BadCommand("cannot ungroup the root".into()))?;
        let position = self
            .doc
            .children_of(parent)?
            .iter()
            .position(|s| *s == id)
            .unwrap();
        let children = self.doc.children_of(id)?.to_vec();
        // The group's transform reached its children while they were inside
        // it; once they leave, each has to carry that part itself or the
        // whole group would jump back to where it was before it was moved.
        let group_t = self.doc.node(id)?.transform;
        let mut cmds: Vec<Command> = Vec::new();
        for (i, child) in children.iter().enumerate() {
            if group_t != Transform::default() {
                cmds.push(Command::SetTransform {
                    id: *child,
                    transform: group_t.compose(self.doc.node(*child)?.transform),
                });
            }
            cmds.push(Command::MoveNode {
                id: *child,
                parent,
                index: position + i,
            });
        }
        cmds.push(Command::RemoveNode { id });
        self.apply_labeled(Command::Batch(cmds), Some(label))
    }

    /// Present the current document state, re-rendering only what changed
    /// since the last call. Returns the cached surface and the region that
    /// was just recomputed (None if the cache was already clean).
    pub fn render_cached(&mut self) -> Result<(&Surface, Option<ClipRect>), EngineError> {
        let view = self.view_transform();
        let scale = self.view_scale;
        let (w, h) = self.present_size();
        let full = ClipRect {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        };
        if self.cache.as_ref().map(|c| (c.width, c.height)) != Some((w, h)) {
            self.cache = Some(Surface::new(w, h));
            self.scratch = None;
            self.stale_all = true;
        }
        let doc_clip = self.stale.take();
        let everything = std::mem::take(&mut self.stale_all);
        match (everything, doc_clip) {
            (false, None) => Ok((self.cache.as_ref().unwrap(), None)),
            (everything, doc_clip) => {
                // The dirty region arrives in document pixels; the cache is
                // kept in the view's, so carry it through the view and
                // widen it outwards to whole pixels rather than trusting a
                // rounded edge.
                let clip = if everything {
                    full
                } else {
                    let d = doc_clip.unwrap();
                    ClipRect::from_float(
                        d.x0 as f32 * scale + view.e,
                        d.y0 as f32 * scale + view.f,
                        d.x1 as f32 * scale + view.e,
                        d.y1 as f32 * scale + view.f,
                        w,
                        h,
                    )
                    .intersect(full)
                };
                if clip.is_empty() {
                    // The change happened off-screen. Nothing to present,
                    // but the cache is still whole.
                    return Ok((self.cache.as_ref().unwrap(), None));
                }
                // Filters sample neighbors: a region render is only correct
                // deeper than the filter stack's reach inside its own edge.
                // So compute a padded region in scratch and copy back just
                // the exact region — the padding ring, whose values clamp
                // against stale surroundings, is discarded. The reach is a
                // document-space figure, so it scales with the view too.
                let pad = (chitrakar_render::filter_reach(&self.doc) as f32 * scale).ceil() as u32;
                if pad == 0 {
                    let cache = self.cache.as_mut().unwrap();
                    chitrakar_render::render_region_at(&self.doc, cache, clip, view)?;
                    self.pixels_recomputed += clip.area();
                } else {
                    let compute = ClipRect {
                        x0: clip.x0.saturating_sub(pad),
                        y0: clip.y0.saturating_sub(pad),
                        x1: (clip.x1 + pad).min(w),
                        y1: (clip.y1 + pad).min(h),
                    };
                    let scratch = self.scratch.get_or_insert_with(|| Surface::new(w, h));
                    chitrakar_render::render_region_at(&self.doc, scratch, compute, view)?;
                    self.cache.as_mut().unwrap().copy_region_from(scratch, clip);
                    self.pixels_recomputed += compute.area();
                }
                Ok((self.cache.as_ref().unwrap(), Some(clip)))
            }
        }
    }

    /// The document-to-surface mapping the cache is kept in.
    fn view_transform(&self) -> Transform {
        Transform {
            a: self.view_scale,
            d: self.view_scale,
            e: self.view_origin.0,
            f: self.view_origin.1,
            ..Default::default()
        }
    }

    /// Size of the surface [`render_cached`](Self::render_cached) presents:
    /// the viewport when one has been set, and otherwise the whole document
    /// at the view's resolution.
    pub fn present_size(&self) -> (u32, u32) {
        match self.viewport {
            Some(size) => size,
            None => {
                let (w, h) = (self.doc.meta.width, self.doc.meta.height);
                (
                    ((w as f32 * self.view_scale).round() as u32).max(1),
                    ((h as f32 * self.view_scale).round() as u32).max(1),
                )
            }
        }
    }

    /// Present only what a viewport of `(width, height)` device pixels can
    /// see, with the document's origin at `(x, y)` within it and `scale`
    /// device pixels to the document pixel.
    ///
    /// This is what stops a big document costing a big render: an A4 page
    /// at 300dpi is nine megapixels whatever the screen is, and composing
    /// all of it to show a screenful was most of the cost of showing
    /// anything. Passing a viewport also lifts the resolution cap, since
    /// the surface no longer grows with the zoom.
    pub fn set_viewport(&mut self, scale: f32, x: f32, y: f32, width: u32, height: u32) {
        let scale = scale.clamp(0.01, 64.0);
        let size = (width.max(1), height.max(1));
        let same = (scale - self.view_scale).abs() < 1e-4
            && (x - self.view_origin.0).abs() < 1e-3
            && (y - self.view_origin.1).abs() < 1e-3
            && self.viewport == Some(size);
        if same {
            return;
        }
        self.view_scale = scale;
        self.view_origin = (x, y);
        self.viewport = Some(size);
        // Everything the surface shows has moved; none of it can be reused,
        // and that includes the part beside the page, which a document-space
        // dirty region has no way to name.
        self.cache = None;
        self.scratch = None;
        self.stale = None;
        self.stale_all = true;
    }

    pub fn view_scale(&self) -> f32 {
        self.view_scale
    }

    /// Render the current document state from scratch (export path; the
    /// interactive path is [`render_cached`](Self::render_cached)).
    pub fn render(&self) -> Result<Surface, EngineError> {
        Ok(chitrakar_render::render(&self.doc)?)
    }

    /// Export the composite as a CMYK TIFF separated through — and with —
    /// the document's press profile. Requires a loaded profile.
    pub fn export_cmyk_tiff(&self) -> Result<Vec<u8>, EngineError> {
        let Some(icc) = self.doc.cmyk_profile_bytes() else {
            return Err(EngineError::BadCommand(
                "CMYK TIFF export needs a press profile: load an ICC first".into(),
            ));
        };
        let surface = self.render()?;
        chitrakar_codecs::export_cmyk_tiff(&surface.pixels, surface.width, surface.height, icc)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Export a one-page PDF of the composite. With a press profile loaded
    /// the page is separated into ink and carries that profile; otherwise it
    /// is sRGB over white.
    pub fn export_pdf(&self) -> Result<Vec<u8>, EngineError> {
        let surface = self.render()?;
        chitrakar_codecs::export_pdf(
            &surface.pixels,
            surface.width,
            surface.height,
            self.doc.meta.dpi,
            self.doc.cmyk_profile_bytes(),
        )
        .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Export vector layers (and embedded rasters/text) as SVG.
    pub fn export_svg(&self) -> Result<String, EngineError> {
        Ok(chitrakar_codecs::export_svg(&self.doc)?)
    }

    /// Render and encode as PNG — used by export and tests.
    pub fn render_png(&self) -> Result<Vec<u8>, EngineError> {
        let surface = self.render()?;
        chitrakar_codecs::encode_png(surface.width, surface.height, &surface.to_srgb8())
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Render and encode as JPEG. Transparency flattens onto white, since
    /// JPEG carries no alpha.
    pub fn export_jpeg(&self, quality: u8) -> Result<Vec<u8>, EngineError> {
        let surface = self.render()?;
        chitrakar_codecs::encode_jpeg(surface.width, surface.height, &surface.pixels, quality)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Flattened layer tree (depth-first, topmost layer first) for the UI's
    /// layers panel.
    pub fn layers(&self) -> Vec<LayerInfo> {
        let mut out = Vec::new();
        self.collect_layers(self.doc.root(), 0, &mut out);
        out
    }

    fn collect_layers(&self, group: NodeId, depth: u32, out: &mut Vec<LayerInfo>) {
        let Ok(children) = self.doc.children_of(group) else {
            return;
        };
        // Children are stored bottom-to-top; panels list top-to-bottom.
        for (index, &id) in children.iter().enumerate().rev() {
            let Ok(node) = self.doc.node(id) else {
                continue;
            };
            out.push(LayerInfo {
                id: id.0,
                name: node.name.clone(),
                kind: match &node.kind {
                    NodeKind::Group => "group",
                    NodeKind::Vector { .. } => "vector",
                    NodeKind::Raster(_) => "raster",
                    NodeKind::Adjustment(_) => "adjustment",
                    NodeKind::Filter(_) => "filter",
                    NodeKind::Text(_) => "text",
                },
                visible: node.visible,
                opacity: node.opacity,
                blend: node.blend,
                has_mask: node.mask.is_some(),
                has_effects: !node.effects.is_empty(),
                depth,
                parent: group.0,
                index,
                sibling_count: children.len(),
            });
            if matches!(node.kind, NodeKind::Group) {
                self.collect_layers(id, depth + 1, out);
            }
        }
    }

    /// Decode image bytes, pool them as a resource, and add a raster object
    /// referencing them at the top of the root group (one undo step).
    pub fn place_image(&mut self, bytes: &[u8], name: &str) -> Result<(), EngineError> {
        let img =
            chitrakar_codecs::decode(bytes).map_err(|e| EngineError::BadCommand(e.to_string()))?;
        let (width, height) = (img.width, img.height);
        let resource_id = self.doc.add_resource(width, height, img.rgba8);
        let root = self.doc.root();
        let index = self.doc.children_of(root)?.len();
        self.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(Node::raster(
                name,
                chitrakar_doc::RasterRef {
                    resource_id,
                    width,
                    height,
                },
            )),
        })
    }

    /// Topmost clickable node at a document-space point.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        chitrakar_render::hit_test(&self.doc, x, y).ok().flatten()
    }

    pub fn transform_of(&self, id: NodeId) -> Result<Transform, EngineError> {
        Ok(self.doc.node(id)?.transform)
    }

    /// A node's kind (shape, fill, stroke, adjustment parameters…) as JSON —
    /// what a properties panel edits and sends back via `SetKind`.
    pub fn kind_json(&self, id: NodeId) -> Result<String, EngineError> {
        serde_json::to_string(&self.doc.node(id)?.kind)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// A node's mask as JSON (`null` when unmasked) — edited via `SetMask`.
    pub fn mask_json(&self, id: NodeId) -> Result<String, EngineError> {
        serde_json::to_string(&self.doc.node(id)?.mask)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Change the page's size, shifting top-level layers by `(dx, dy)` so
    /// the picture stays where it was. Cropping to a rectangle is this with
    /// the rectangle's size and the negative of its origin.
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        dx: f32,
        dy: f32,
    ) -> Result<(), EngineError> {
        self.apply(Command::ResizeCanvas {
            width,
            height,
            dx,
            dy,
        })
    }

    /// Force the next present to recompute the whole canvas. The surface
    /// the frame is copied into lives outside the engine, so when that is
    /// replaced — a resized canvas element, say — the engine has to be
    /// told that its idea of what is already on screen is gone.
    pub fn invalidate(&mut self) {
        self.stale_all = true;
    }

    /// The document's guides as JSON.
    pub fn guides_json(&self) -> String {
        serde_json::to_string(self.doc.guides()).unwrap_or_else(|_| "[]".into())
    }

    /// A node's effect list as JSON.
    pub fn effects_json(&self, id: NodeId) -> Result<String, EngineError> {
        serde_json::to_string(&self.doc.node(id)?.effects)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Doc-space bounds of a node as `[x, y, w, h]`, if it has any.
    pub fn bounds_of(&self, id: NodeId) -> Option<[f32; 4]> {
        match chitrakar_render::node_bounds(&self.doc, id).ok()? {
            Bounds::None => None,
            Bounds::Everything => Some([
                0.0,
                0.0,
                self.doc.meta.width as f32,
                self.doc.meta.height as f32,
            ]),
            Bounds::Rect(x0, y0, x1, y1) => Some([x0, y0, x1 - x0, y1 - y0]),
        }
    }

    /// The transform carrying a node's parent space into document space —
    /// identity for a top-level layer, the enclosing groups' transforms for
    /// a nested one. A node's own transform is written against this, so a
    /// drag has to convert the cursor through it.
    pub fn parent_space_of(&self, id: NodeId) -> [f32; 6] {
        let t = chitrakar_render::ancestor_space(&self.doc, id);
        [t.a, t.b, t.c, t.d, t.e, t.f]
    }

    /// A node's bounds in its own space, `[x0, y0, x1, y1]`, for drawing a
    /// selection box that turns with the layer.
    pub fn local_bounds_of(&self, id: NodeId) -> Option<[f32; 4]> {
        chitrakar_render::local_bounds_of(&self.doc, id)
            .ok()
            .flatten()
    }

    /// Set the CMYK press profile authored CMYK colors render through.
    /// Not a command: like resources, it is document setup, not an edit.
    pub fn set_cmyk_profile(&mut self, icc: Vec<u8>) -> Result<(), EngineError> {
        self.doc
            .set_cmyk_profile(icc)
            .map_err(EngineError::BadCommand)?;
        // An active proof must follow the new profile; otherwise rebuild lazily.
        self.proof_cms = if self.soft_proof {
            Some(
                chitrakar_color::cms::ProofCms::new(self.doc.cmyk_profile_bytes().unwrap())
                    .map_err(EngineError::BadCommand)?,
            )
        } else {
            None
        };
        self.mark_dirty(Bounds::Everything);
        Ok(())
    }

    /// Toggle display soft-proofing through the document's press profile.
    /// Fails when proofing is requested without a loaded profile.
    pub fn set_proofing(&mut self, proof: bool, gamut_warn: bool) -> Result<(), EngineError> {
        if proof && self.proof_cms.is_none() {
            let Some(bytes) = self.doc.cmyk_profile_bytes() else {
                return Err(EngineError::BadCommand(
                    "soft proofing needs a CMYK profile loaded first".into(),
                ));
            };
            self.proof_cms =
                Some(chitrakar_color::cms::ProofCms::new(bytes).map_err(EngineError::BadCommand)?);
        }
        self.soft_proof = proof;
        self.gamut_warn = gamut_warn;
        // The composite is unchanged, but everything presented must re-encode.
        self.mark_dirty(Bounds::Everything);
        Ok(())
    }

    /// Encode a region of the cached surface for display, applying the
    /// soft-proof transform when enabled. `out` is the full-frame RGBA8
    /// buffer (row stride = document width).
    pub fn encode_present_region(&self, clip: ClipRect, out: &mut [u8]) {
        let Some(surface) = &self.cache else {
            return;
        };
        surface.encode_srgb8_region(clip, out);
        if !self.soft_proof {
            return;
        }
        let Some(proof) = &self.proof_cms else {
            return;
        };
        let w = surface.width as usize;
        for y in clip.y0..clip.y1 {
            let row = (y as usize * w + clip.x0 as usize) * 4;
            let end = (y as usize * w + clip.x1 as usize) * 4;
            proof.proof_rgba8(&mut out[row..end], self.gamut_warn);
        }
    }

    pub fn has_cmyk_profile(&self) -> bool {
        self.doc.cmyk_cms().is_some()
    }

    /// Serialize to `.chitra` container bytes.
    pub fn save(&self) -> Result<Vec<u8>, EngineError> {
        chitrakar_codecs::save_chitra(&self.doc).map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Open a `.chitra` container. The loaded document starts with a fresh
    /// history (undo does not cross save boundaries for now).
    pub fn load(bytes: &[u8]) -> Result<Self, EngineError> {
        let doc = chitrakar_codecs::load_chitra(bytes)
            .map_err(|e| EngineError::BadCommand(e.to_string()))?;
        Ok(Self::from_document(doc))
    }
}

fn parse_command(json: &str) -> Result<Command, EngineError> {
    serde_json::from_str(json).map_err(|e| EngineError::BadCommand(e.to_string()))
}

/// One row of the UI layers panel. `parent`/`index`/`sibling_count` describe
/// the node's slot in its group (painter's order: index 0 = bottom) so the
/// UI can issue reorder commands without mirroring the tree.
#[derive(Debug, Clone, Serialize)]
pub struct LayerInfo {
    pub id: u64,
    pub name: String,
    pub kind: &'static str,
    pub visible: bool,
    pub opacity: f32,
    pub blend: chitrakar_doc::BlendMode,
    pub has_mask: bool,
    pub has_effects: bool,
    pub depth: u32,
    pub parent: u64,
    pub index: usize,
    pub sibling_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::AuthoredColor;
    use chitrakar_doc::VectorShape;

    fn filled_rect(name: &str, w: f32, h: f32) -> Box<Node> {
        let mut node = Node::vector(
            name,
            VectorShape::Rect {
                width: w,
                height: h,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(AuthoredColor::Srgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
        }
        Box::new(node)
    }

    fn add_rect(session: &mut Session, name: &str, w: f32, h: f32) -> NodeId {
        let root = session.document().root();
        let index = session.document().children_of(root).unwrap().len();
        session
            .apply(Command::AddNode {
                parent: root,
                index,
                node: filled_rect(name, w, h),
            })
            .unwrap();
        *session
            .document()
            .children_of(root)
            .unwrap()
            .last()
            .unwrap()
    }

    fn assert_cache_matches_fresh(session: &mut Session) {
        let fresh = session.render().unwrap().to_srgb8();
        let (cached, _) = session.render_cached().unwrap();
        assert_eq!(cached.to_srgb8(), fresh, "cache diverged from full render");
    }

    #[test]
    fn a_viewport_shows_the_part_of_the_page_it_is_pointed_at() {
        // The surface is a window onto the document now, not the document:
        // it is the size it was told, the page lands where the origin says,
        // and nothing is painted outside the page's own edge.
        let mut session = Session::new(200, 200, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 200.0, 200.0);
        let _ = id;
        session.set_viewport(1.0, 20.0, 30.0, 100, 80);
        let (s, dirty) = session.render_cached().unwrap();
        assert_eq!(
            (s.width, s.height),
            (100, 80),
            "the surface is the viewport"
        );
        assert!(dirty.is_some());
        assert_eq!(s.get(10, 10).a, 0.0, "above and left of the page: nothing");
        assert_eq!(s.get(50, 50).a, 1.0, "and the page itself is painted");
        assert_eq!(s.get(19, 40).a, 0.0, "the page's left edge is respected");
        assert_eq!(s.get(21, 40).a, 1.0, "just inside it, painted");
    }

    #[test]
    fn a_viewport_still_clips_artwork_to_the_page() {
        // With the surface no longer being the page, a layer hanging off
        // the page's edge would otherwise paint onto the desk beside it.
        let mut session = Session::new(100, 100, ColorMode::Rgb);
        let id = add_rect(&mut session, "over", 80.0, 80.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(-40.0, -40.0),
            })
            .unwrap();
        session.set_viewport(1.0, 50.0, 50.0, 200, 200);
        let (s, _) = session.render_cached().unwrap();
        assert_eq!(s.get(60, 60).a, 1.0, "the part on the page is painted");
        assert_eq!(s.get(30, 30).a, 0.0, "the part hanging off it is not");
        assert_eq!(s.get(49, 60).a, 0.0, "not even a pixel past the edge");
    }

    #[test]
    fn moving_the_viewport_presents_the_whole_surface() {
        // The caller copies the reported region and nothing else, so after
        // a pan that region has to be the whole surface: the part beside
        // the page still holds the last frame's picture of the page, and a
        // document-space dirty region cannot name it.
        let mut session = Session::new(100, 100, ColorMode::Rgb);
        add_rect(&mut session, "r", 100.0, 100.0);
        session.set_viewport(1.0, 20.0, 20.0, 200, 200);
        let (_, first) = session.render_cached().unwrap();
        assert_eq!(
            first.map(|c| (c.x1 - c.x0, c.y1 - c.y0)),
            Some((200, 200)),
            "the first frame is the whole surface"
        );
        session.render_cached().unwrap();
        session.set_viewport(1.0, 60.0, 60.0, 200, 200);
        let (_, after_pan) = session.render_cached().unwrap();
        assert_eq!(
            after_pan.map(|c| (c.x0, c.y0, c.x1, c.y1)),
            Some((0, 0, 200, 200)),
            "and so is the one after a pan"
        );
        // An ordinary edit still reports only what it touched.
        let id = *session
            .document()
            .children_of(session.document().root())
            .unwrap()
            .last()
            .unwrap();
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(10.0, 10.0),
            })
            .unwrap();
        let (_, after_edit) = session.render_cached().unwrap();
        let c = after_edit.expect("the edit dirties something");
        assert!(
            (c.x1 - c.x0) < 200 || (c.y1 - c.y0) < 200,
            "an edit is not a whole-surface repaint: {c:?}"
        );
    }

    #[test]
    fn a_viewport_can_be_zoomed_past_what_the_whole_page_would_cost() {
        // The surface no longer grows with the zoom, so the zoom is not
        // capped by what a full-page surface would cost. A print-sized
        // document at 4x is a screenful of pixels like any other.
        let mut session = Session::new(2480, 3508, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 40.0, 40.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(100.0, 100.0),
            })
            .unwrap();
        session.set_viewport(4.0, -380.0, -380.0, 400, 300);
        let (s, _) = session.render_cached().unwrap();
        assert_eq!((s.width, s.height), (400, 300));
        // The rect covers document (100,100)-(140,140), which at 4x with
        // that origin is (20,20)-(180,180) on the surface.
        assert_eq!(s.get(100, 100).a, 1.0, "the magnified rect is there");
        assert_eq!(s.get(200, 200).a, 0.0, "and stops where it should");
    }

    #[test]
    fn an_edit_inside_a_viewport_repaints_the_right_pixels() {
        // The dirty region is tracked in document pixels and has to be
        // carried through both the scale and the pan. Get either wrong and
        // an edit leaves a stale band, so compare against a full render.
        let mut session = Session::new(120, 120, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 20.0, 20.0);
        session.set_viewport(2.0, -30.0, -20.0, 150, 150);
        session.render_cached().unwrap();
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(40.0, 35.0),
            })
            .unwrap();
        let (cached, dirty) = session.render_cached().unwrap();
        assert!(dirty.is_some(), "moving a layer dirties something");
        let cached = cached.to_srgb8();
        let mut fresh = Surface::new(150, 150);
        chitrakar_render::render_region_at(
            session.document(),
            &mut fresh,
            ClipRect {
                x0: 0,
                y0: 0,
                x1: 150,
                y1: 150,
            },
            Transform {
                a: 2.0,
                d: 2.0,
                e: -30.0,
                f: -20.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cached, fresh.to_srgb8(), "incremental cache diverged");
    }

    #[test]
    fn an_edit_entirely_off_screen_presents_nothing() {
        let mut session = Session::new(400, 400, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 20.0, 20.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(300.0, 300.0),
            })
            .unwrap();
        session.set_viewport(1.0, 0.0, 0.0, 100, 100);
        session.render_cached().unwrap();
        session
            .apply(Command::SetOpacity { id, opacity: 0.5 })
            .unwrap();
        let (_, dirty) = session.render_cached().unwrap();
        assert!(dirty.is_none(), "nothing on screen changed: {dirty:?}");
    }

    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn a4_viewport_probe() {
        let mut session = Session::new(2480, 3508, ColorMode::Rgb);
        add_rect(&mut session, "bg", 2480.0, 3508.0);
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            session.invalidate();
            session.render_cached().unwrap();
        }
        println!("A4 whole page: {:?}", t0.elapsed() / 3);
        session.set_viewport(0.4, 0.0, 0.0, 1400, 900);
        let t0 = std::time::Instant::now();
        for _ in 0..3 {
            session.invalidate();
            session.render_cached().unwrap();
        }
        println!("A4 through a 1400x900 viewport: {:?}", t0.elapsed() / 3);
    }

    #[test]
    fn guides_are_document_state_that_costs_no_repaint() {
        // Guides belong to the layout, so they undo and they save; they
        // are not artwork, so changing them repaints nothing.
        use chitrakar_doc::Guide;
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        add_rect(&mut session, "r", 20.0, 20.0);
        session.render_cached().unwrap();
        let before = session.pixels_recomputed();
        session
            .apply(Command::SetGuides {
                guides: vec![Guide::Vertical(32.0), Guide::Horizontal(10.0)],
            })
            .unwrap();
        assert_eq!(session.document().guides().len(), 2);
        let (_, dirty) = session.render_cached().unwrap();
        assert!(dirty.is_none(), "placing a guide repainted something");
        assert_eq!(session.pixels_recomputed(), before, "and cost pixels");

        assert!(session.undo().unwrap());
        assert!(session.document().guides().is_empty(), "and it undoes");
        // They survive a save and reopen.
        session
            .apply(Command::SetGuides {
                guides: vec![Guide::Vertical(12.5)],
            })
            .unwrap();
        let bytes = session.save().unwrap();
        let reopened = Session::load(&bytes).unwrap();
        assert_eq!(reopened.document().guides(), &[Guide::Vertical(12.5)]);
    }

    #[test]
    fn json_command_roundtrip_drives_the_engine() {
        let mut session = Session::new(16, 16, ColorMode::Rgb);
        let root = session.document().root();
        let cmd = Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("layer 1")),
        };
        let json = serde_json::to_string(&cmd).unwrap();

        session.apply_json(&json).unwrap();
        assert_eq!(session.document().node_count(), 2);

        assert!(session.undo().unwrap());
        assert_eq!(session.document().node_count(), 1);
        assert!(session.redo().unwrap());
        assert_eq!(session.document().node_count(), 2);
    }

    #[test]
    fn render_png_produces_a_valid_png() {
        let session = Session::new(8, 8, ColorMode::Cmyk);
        let png = session.render_png().unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn bad_json_is_rejected_not_panicked() {
        let mut session = Session::new(8, 8, ColorMode::Rgb);
        assert!(matches!(
            session.apply_json("{nope"),
            Err(EngineError::BadCommand(_))
        ));
    }

    #[test]
    fn cached_render_stays_pixel_identical_through_edit_sequence() {
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        assert_cache_matches_fresh(&mut session);

        let a = add_rect(&mut session, "a", 20.0, 20.0);
        assert_cache_matches_fresh(&mut session);

        let b = add_rect(&mut session, "b", 10.0, 10.0);
        assert_cache_matches_fresh(&mut session);

        session
            .apply(Command::SetTransform {
                id: a,
                transform: Transform::translation(30.0, 30.0),
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);

        session
            .apply(Command::SetOpacity {
                id: b,
                opacity: 0.4,
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);

        session.apply(Command::RemoveNode { id: b }).unwrap();
        assert_cache_matches_fresh(&mut session);

        session.undo().unwrap();
        assert_cache_matches_fresh(&mut session);
        session.undo().unwrap();
        assert_cache_matches_fresh(&mut session);
        session.redo().unwrap();
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn small_edit_recomputes_small_region() {
        let mut session = Session::new(512, 512, ColorMode::Rgb);
        let small = add_rect(&mut session, "small", 8.0, 8.0);
        session.render_cached().unwrap();

        let before = session.pixels_recomputed();
        session
            .apply(Command::SetTransform {
                id: small,
                transform: Transform::translation(4.0, 4.0),
            })
            .unwrap();
        session.render_cached().unwrap();
        let recomputed = session.pixels_recomputed() - before;

        assert!(
            recomputed < 1500,
            "moving an 8×8 rect recomputed {recomputed} pixels on a 512×512 canvas"
        );
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn cache_stays_correct_with_a_filter_layer_present() {
        let mut session = Session::new(48, 48, ColorMode::Rgb);
        let rect = add_rect(&mut session, "r", 12.0, 12.0);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::filter(
                    "blur",
                    chitrakar_doc::Filter::GaussianBlur { sigma: 2.0 },
                )),
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);

        // An edit below the filter still produces a pixel-exact cache: the
        // dirty region grows by the filter's reach and the compute region
        // is padded further, so blur halos update correctly.
        session
            .apply(Command::SetTransform {
                id: rect,
                transform: Transform::translation(20.0, 20.0),
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);

        session.undo().unwrap();
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn filter_edits_render_incrementally_not_whole_canvas() {
        let mut session = Session::new(512, 512, ColorMode::Rgb);
        let small = add_rect(&mut session, "small", 8.0, 8.0);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::filter(
                    "blur",
                    chitrakar_doc::Filter::GaussianBlur { sigma: 3.0 },
                )),
            })
            .unwrap();
        session.render_cached().unwrap();

        let before = session.pixels_recomputed();
        session
            .apply(Command::SetTransform {
                id: small,
                transform: Transform::translation(4.0, 4.0),
            })
            .unwrap();
        session.render_cached().unwrap();
        let recomputed = session.pixels_recomputed() - before;

        // Whole canvas would be 262144; the padded region is a small
        // fraction of that even with a σ=3 blur in the stack.
        assert!(
            recomputed < 20_000,
            "moving an 8×8 rect under a blur recomputed {recomputed} px of 262144"
        );
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn preview_gesture_is_one_undo_step() {
        let mut session = Session::new(32, 32, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 4.0, 4.0);

        for step in 1..=5 {
            session
                .preview(Command::SetTransform {
                    id,
                    transform: Transform::translation(step as f32, 0.0),
                })
                .unwrap();
        }
        assert!(session.commit_preview());
        assert_eq!(session.transform_of(id).unwrap().e, 5.0);

        // One undo covers the whole gesture, back to the pre-drag transform.
        session.undo().unwrap();
        assert_eq!(session.transform_of(id).unwrap().e, 0.0);
        session.redo().unwrap();
        assert_eq!(session.transform_of(id).unwrap().e, 5.0);
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn cancelled_preview_restores_state_and_records_nothing() {
        let mut session = Session::new(32, 32, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 4.0, 4.0);

        session
            .preview(Command::SetTransform {
                id,
                transform: Transform::translation(10.0, 10.0),
            })
            .unwrap();
        assert!(session.cancel_preview().unwrap());
        assert_eq!(session.transform_of(id).unwrap().e, 0.0);

        // The only undo step is the AddNode, not the cancelled drag.
        session.undo().unwrap();
        assert_eq!(session.document().node_count(), 1);
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn group_and_ungroup_are_single_undo_steps() {
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        let a = add_rect(&mut session, "a", 10.0, 10.0);
        let b = add_rect(&mut session, "b", 10.0, 10.0);
        let root = session.document().root();

        let group = session.group_nodes(&[a, b], "duo").unwrap();
        assert_eq!(session.document().children_of(root).unwrap(), &[group]);
        assert_eq!(session.document().children_of(group).unwrap(), &[a, b]);
        assert_cache_matches_fresh(&mut session);

        // One undo dissolves the grouping entirely.
        session.undo().unwrap();
        assert_eq!(session.document().children_of(root).unwrap(), &[a, b]);
        session.redo().unwrap();
        assert_eq!(session.document().children_of(group).unwrap(), &[a, b]);

        session.ungroup_node(group).unwrap();
        assert_eq!(session.document().children_of(root).unwrap(), &[a, b]);
        assert!(session.document().node(group).is_err(), "group removed");
        session.undo().unwrap();
        assert_eq!(session.document().children_of(group).unwrap(), &[a, b]);
        assert_cache_matches_fresh(&mut session);

        // Mixed-parent grouping is refused.
        let c = add_rect(&mut session, "c", 4.0, 4.0);
        assert!(session.group_nodes(&[a, c], "bad").is_err());
    }

    #[test]
    fn ungrouping_a_moved_group_leaves_its_children_where_they_look() {
        // The group's transform reaches its children while they are inside
        // it. Dissolving the group has to fold that transform into each of
        // them, or everything springs back to where it was before the group
        // was moved.
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        let a = add_rect(&mut session, "a", 8.0, 8.0);
        let b = add_rect(&mut session, "b", 8.0, 8.0);
        let group = session.group_nodes(&[a, b], "pair").unwrap();
        session
            .apply(Command::SetTransform {
                id: group,
                transform: Transform::translation(20.0, 12.0),
            })
            .unwrap();

        let before = session.bounds_of(a).unwrap();
        session.ungroup_node(group).unwrap();
        let after = session.bounds_of(a).unwrap();
        assert!(
            before.iter().zip(after).all(|(x, y)| (x - y).abs() < 1e-3),
            "a child must not move when its group dissolves: {before:?} -> {after:?}"
        );
        assert_cache_matches_fresh(&mut session);

        // And it is still one undo step, which puts the group back.
        session.undo().unwrap();
        assert!(matches!(
            session.document().node(group).map(|n| &n.kind),
            Ok(NodeKind::Group)
        ));
    }

    #[test]
    fn duplicating_a_group_copies_everything_under_it() {
        // A node carries no children, so a duplicate is a walk. What makes
        // it worth testing is that the copy's children hang off the copy —
        // not off the original — and that the whole thing is one undo step.
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        let a = add_rect(&mut session, "a", 8.0, 8.0);
        let b = add_rect(&mut session, "b", 8.0, 8.0);
        let group = session.group_nodes(&[a, b], "pair").unwrap();

        let copy = session.duplicate_node(group).unwrap();
        assert_ne!(copy, group);
        let kids = session.document().children_of(copy).unwrap().to_vec();
        assert_eq!(kids.len(), 2, "both children were copied");
        assert!(
            !kids.contains(&a) && !kids.contains(&b),
            "the copy has its own children, not the originals"
        );
        assert_eq!(
            session.document().children_of(group).unwrap(),
            &[a, b],
            "and the original is untouched"
        );
        assert_eq!(session.document().node(copy).unwrap().name, "pair copy");
        assert_cache_matches_fresh(&mut session);

        // Sits just above the original, offset so it is visible.
        let root = session.document().root();
        let top = session.document().children_of(root).unwrap().to_vec();
        assert_eq!(
            top.iter().position(|n| *n == copy),
            top.iter().position(|n| *n == group).map(|i| i + 1),
        );
        let (ob, cb) = (
            session.bounds_of(group).unwrap(),
            session.bounds_of(copy).unwrap(),
        );
        assert!(cb[0] > ob[0] && cb[1] > ob[1], "the copy is nudged clear");

        // One undo takes the whole subtree away again.
        session.undo().unwrap();
        assert!(session.document().node(copy).is_err());
        assert_eq!(session.document().children_of(group).unwrap(), &[a, b]);
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn duplicating_a_raster_shares_its_pixels() {
        // Resources are content-addressed and immutable, so a copy costs no
        // pixels — it points at the same entry.
        let mut session = Session::new(32, 32, ColorMode::Rgb);
        let png = {
            let pixels = vec![255u8, 0, 0, 255];
            chitrakar_codecs::encode_png(1, 1, &pixels).unwrap()
        };
        session.place_image(&png, "dot.png").unwrap();
        let root = session.document().root();
        let img = session.document().children_of(root).unwrap()[0];
        let before = session.document().resources().count();

        session.duplicate_node(img).unwrap();
        assert_eq!(
            session.document().resources().count(),
            before,
            "duplicating a raster adds no new resource"
        );
    }

    #[test]
    fn the_clipboard_carries_a_subtree_into_another_document() {
        // The point of a clipboard over duplicate: it survives the document
        // it was copied from. Pixels travel with it, and because resource
        // ids are content-addressed, the pasted node's reference resolves in
        // the new document without any remapping.
        let mut a = Session::new(64, 64, ColorMode::Rgb);
        let png = chitrakar_codecs::encode_png(1, 1, &[0, 200, 0, 255]).unwrap();
        a.place_image(&png, "dot.png").unwrap();
        let root_a = a.document().root();
        let img = a.document().children_of(root_a).unwrap()[0];
        let rect = add_rect(&mut a, "r", 8.0, 8.0);
        let group = a.group_nodes(&[img, rect], "pair").unwrap();
        a.copy_node(group).unwrap();

        let mut b = Session::new(64, 64, ColorMode::Rgb);
        assert_eq!(b.document().resources().count(), 0);
        let pasted = b.paste(None).unwrap().expect("clipboard had content");
        assert_eq!(b.document().node(pasted).unwrap().name, "pair");
        assert_eq!(
            b.document().children_of(pasted).unwrap().len(),
            2,
            "the whole subtree came across"
        );
        assert_eq!(
            b.document().resources().count(),
            1,
            "and its pixels came with it"
        );
        // The pasted raster resolves to real pixels, not an empty entry.
        let kids = b.document().children_of(pasted).unwrap().to_vec();
        let raster = kids
            .iter()
            .find_map(|k| match &b.document().node(*k).unwrap().kind {
                NodeKind::Raster(r) => Some(r.resource_id.clone()),
                _ => None,
            })
            .expect("a raster child");
        assert!(!b.document().resource(&raster).unwrap().rgba8.is_empty());
        assert_cache_matches_fresh(&mut b);

        // One undo step, and the clipboard is still there to paste again.
        b.undo().unwrap();
        assert!(b.document().node(pasted).is_err());
        assert!(crate::clipboard_has_content());
        assert!(b.paste(None).unwrap().is_some(), "paste is repeatable");
    }

    #[test]
    fn aligning_lines_layers_up_and_distributing_spaces_them_evenly() {
        let mut session = Session::new(200, 100, ColorMode::Rgb);
        let place = |s: &mut Session, name: &str, x: f32| {
            let id = add_rect(s, name, 10.0, 10.0);
            s.apply(Command::SetTransform {
                id,
                transform: Transform::translation(x, 0.0),
            })
            .unwrap();
            id
        };
        let a = place(&mut session, "a", 0.0);
        let b = place(&mut session, "b", 30.0);
        let c = place(&mut session, "c", 100.0);

        // Left: everything to the leftmost edge, which does not move.
        session.align_nodes(&[a, b, c], "left").unwrap();
        for id in [a, b, c] {
            assert!((session.bounds_of(id).unwrap()[0] - 0.0).abs() < 1e-3);
        }
        assert_cache_matches_fresh(&mut session);
        session.undo().unwrap();

        // Distribute: the outermost two stay, the middle one lands halfway,
        // and running it again changes nothing.
        session.align_nodes(&[a, b, c], "distribute-h").unwrap();
        let mid = |s: &Session, id| {
            let bb = s.bounds_of(id).unwrap();
            bb[0] + bb[2] / 2.0
        };
        assert!((mid(&session, a) - 5.0).abs() < 1e-3, "the left one stays");
        assert!((mid(&session, c) - 105.0).abs() < 1e-3, "so does the right");
        assert!(
            (mid(&session, b) - 55.0).abs() < 1e-3,
            "and the middle lands halfway, got {}",
            mid(&session, b)
        );
        let before = mid(&session, b);
        session.align_nodes(&[a, b, c], "distribute-h").unwrap();
        assert!(
            (mid(&session, b) - before).abs() < 1e-3,
            "distributing twice is a no-op"
        );

        // Two layers minimum, and unknown modes are refused rather than
        // silently doing nothing.
        assert!(session.align_nodes(&[a], "left").is_err());
        assert!(session.align_nodes(&[a, b], "sideways").is_err());
    }

    #[test]
    fn aligning_a_layer_inside_a_moved_group_still_lands_where_asked() {
        // Alignment is measured in document space, but a transform is
        // written in its parent's — so a layer inside a moved group has to
        // have the movement carried back through the group.
        let mut session = Session::new(200, 100, ColorMode::Rgb);
        let loose = add_rect(&mut session, "loose", 10.0, 10.0);
        session
            .apply(Command::SetTransform {
                id: loose,
                transform: Transform::translation(120.0, 0.0),
            })
            .unwrap();
        let inner = add_rect(&mut session, "inner", 10.0, 10.0);
        let group = session.group_nodes(&[inner], "g").unwrap();
        session
            .apply(Command::SetTransform {
                id: group,
                transform: Transform::translation(40.0, 0.0),
            })
            .unwrap();

        session.align_nodes(&[loose, inner], "left").unwrap();
        let (x1, x2) = (
            session.bounds_of(loose).unwrap()[0],
            session.bounds_of(inner).unwrap()[0],
        );
        assert!(
            (x1 - x2).abs() < 1e-3,
            "both should share a left edge in document space: {x1} vs {x2}"
        );
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn history_labels_and_jump_walk_the_timeline() {
        let mut session = Session::new(32, 32, ColorMode::Rgb);
        let id = add_rect(&mut session, "hero", 8.0, 8.0);
        session
            .apply(Command::SetOpacity { id, opacity: 0.5 })
            .unwrap();
        session
            .apply(Command::SetVisible { id, visible: false })
            .unwrap();

        let (past, future) = session.history_labels();
        assert_eq!(past, vec!["Add hero", "Opacity of hero", "Hide hero"]);
        assert!(future.is_empty());

        // Jump back two steps: opacity restored, visibility restored.
        session.jump(-2).unwrap();
        assert_eq!(session.document().node(id).unwrap().opacity, 1.0);
        assert!(session.document().node(id).unwrap().visible);
        let (past, future) = session.history_labels();
        assert_eq!(past, vec!["Add hero"]);
        assert_eq!(future, vec!["Opacity of hero", "Hide hero"]);

        // Jump forward one.
        session.jump(1).unwrap();
        assert_eq!(session.document().node(id).unwrap().opacity, 0.5);
        assert_cache_matches_fresh(&mut session);

        // Over-jumping clamps at the ends.
        session.jump(-99).unwrap();
        assert_eq!(session.document().node_count(), 1);
        session.jump(99).unwrap();
        assert!(!session.document().node(id).unwrap().visible);
    }

    #[test]
    fn layers_list_is_top_first_with_depth() {
        let mut session = Session::new(8, 8, ColorMode::Rgb);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("bottom group")),
            })
            .unwrap();
        let group = session.document().children_of(root).unwrap()[0];
        session
            .apply(Command::AddNode {
                parent: group,
                index: 0,
                node: Box::new(Node::group("nested")),
            })
            .unwrap();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::group("top")),
            })
            .unwrap();

        let layers = session.layers();
        let names: Vec<_> = layers.iter().map(|l| (l.name.as_str(), l.depth)).collect();
        assert_eq!(names, vec![("top", 0), ("bottom group", 0), ("nested", 1)]);
    }

    /// Needs a real CMYK press profile (CHITRAKAR_TEST_CMYK_ICC).
    #[test]
    fn cmyk_fill_renders_through_press_profile_when_set() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();

        let mut session = Session::new(4, 4, ColorMode::Cmyk);
        let root = session.document().root();
        let mut node = Node::vector(
            "cyan",
            chitrakar_doc::VectorShape::Rect {
                width: 4.0,
                height: 4.0,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(chitrakar_color::AuthoredColor::Cmyk {
                c: 1.0,
                m: 0.0,
                y: 0.0,
                k: 0.0,
                a: 1.0,
            });
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(node),
            })
            .unwrap();

        let naive = session.render().unwrap().get(0, 0).to_srgb8();
        session.set_cmyk_profile(icc).unwrap();
        let profiled = session.render().unwrap().get(0, 0).to_srgb8();
        assert_ne!(naive, profiled, "profile must change CMYK rendering");
        // Real presses can't print #00FFFF; profiled cyan is darker/bluer.
        assert!(profiled[1] < naive[1], "{naive:?} vs {profiled:?}");
        assert_cache_matches_fresh(&mut session);
    }

    /// Needs a real CMYK press profile (CHITRAKAR_TEST_CMYK_ICC).
    #[test]
    fn soft_proofing_changes_presented_pixels_only() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();

        let mut session = Session::new(4, 4, ColorMode::Rgb);
        let root = session.document().root();
        let mut node = Node::vector(
            "blue",
            chitrakar_doc::VectorShape::Rect {
                width: 4.0,
                height: 4.0,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            });
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(node),
            })
            .unwrap();

        // Proofing without a profile is refused.
        assert!(session.set_proofing(true, false).is_err());
        session.set_cmyk_profile(icc).unwrap();

        let full = ClipRect {
            x0: 0,
            y0: 0,
            x1: 4,
            y1: 4,
        };
        let mut plain = vec![0u8; 4 * 4 * 4];
        session.render_cached().unwrap();
        session.encode_present_region(full, &mut plain);
        assert_eq!(&plain[0..3], &[0, 0, 255]);

        session.set_proofing(true, false).unwrap();
        let mut proofed = vec![0u8; 4 * 4 * 4];
        session.render_cached().unwrap();
        session.encode_present_region(full, &mut proofed);
        assert_ne!(&proofed[0..3], &[0, 0, 255], "press can't print pure blue");

        session.set_proofing(true, true).unwrap();
        let mut marked = vec![0u8; 4 * 4 * 4];
        session.render_cached().unwrap();
        session.encode_present_region(full, &mut marked);
        assert_eq!(&marked[0..3], &[128, 128, 128], "gamut warning marks it");

        // Export stays unproofed: proofing is a display transform.
        let exported = session.render().unwrap().get(0, 0).to_srgb8();
        assert_eq!(exported, [0, 0, 255, 255]);
    }

    #[test]
    fn save_load_roundtrip_via_chitra() {
        let mut session = Session::new(32, 32, ColorMode::Cmyk);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("kept")),
            })
            .unwrap();

        let bytes = session.save().unwrap();
        let mut restored = Session::load(&bytes).unwrap();
        assert_eq!(restored.document().node_count(), 2);
        assert_eq!(restored.layers()[0].name, "kept");
        assert_cache_matches_fresh(&mut restored);
    }
}
