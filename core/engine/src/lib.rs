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
/// The box that differs between two versions of one stroke.
///
/// The case worth catching is the one a brush makes every few
/// milliseconds: the same stroke with more points on the end of it.
/// Then only the last segment is new, and the box that changed starts
/// at the point the shorter version ended on. Anything else — a
/// different colour, a rewritten middle — is both boxes together.
fn changed_bounds(
    a: &chitrakar_doc::PaintStroke,
    b: &chitrakar_doc::PaintStroke,
) -> Option<[f32; 4]> {
    let (long, short) = if a.points.len() >= b.points.len() {
        (a, b)
    } else {
        (b, a)
    };
    let n = short.points.len();
    let appended = n > 0
        && long.color == short.color
        && long.softness == short.softness
        && long.erase == short.erase
        && long.points[..n] == short.points[..]
        && long.radii.len() >= short.radii.len()
        && long.radii[..short.radii.len()] == short.radii[..];
    if appended {
        return long.bounds_from(n - 1);
    }
    match (a.bounds(), b.bounds()) {
        (Some(p), Some(q)) => Some([
            p[0].min(q[0]),
            p[1].min(q[1]),
            p[2].max(q[2]),
            p[3].max(q[3]),
        ]),
        (some, None) | (None, some) => some,
    }
}

/// A stroke being drawn: the layer it is going onto, where it sits in
/// that layer's order, and what has been drawn of it so far.
struct Painting {
    layer: NodeId,
    index: usize,
    stroke: chitrakar_doc::PaintStroke,
    /// Whether it is going onto the node's painted mask rather than onto
    /// the node itself.
    on_mask: bool,
}

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
    /// The stroke a brush is in the middle of drawing: which layer it is
    /// going onto, where in that layer's order, and the stroke so far.
    painting: Option<Painting>,
    /// Total pixels re-rendered so far (observability for tests and tuning).
    pixels_recomputed: u64,
    /// The node the most recent command touched, when there was one. What
    /// undo hands back so a selection can follow the layer it brings back
    /// rather than being left pointing at nothing.
    last_touched: Option<NodeId>,
    /// Soft-proofing (display-only): round-trip presented pixels through the
    /// document's press profile, optionally marking out-of-gamut pixels.
    proof_cms: Option<chitrakar_color::cms::ProofCms>,
    soft_proof: bool,
    gamut_warn: bool,
    /// Whether any layer is a live copy of another. Kept rather than
    /// looked up: the dirty region has to ask on every command, and the
    /// answer is no for almost every document.
    has_copies: bool,
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
            painting: None,
            pixels_recomputed: 0,
            last_touched: None,
            proof_cms: None,
            soft_proof: false,
            gamut_warn: false,
            has_copies: false,
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
            // A restored subtree knows its own root; an add does not know
            // its id until it has happened, which is why apply_internal
            // consults the inverse too.
            Command::RestoreSubtree { subtree, .. } => Some(subtree.root_id()),
            Command::AddNode { .. }
            | Command::Batch(_)
            | Command::ResizeCanvas { .. }
            // Guides are not artwork: nothing renders them, so nothing
            // needs repainting when they change; nor does a lock.
            | Command::SetGuides { .. }
            | Command::SetLocked { .. } => None,
            Command::RemoveNode { id }
            | Command::SetOpacity { id, .. }
            | Command::SetVisible { id, .. }
            | Command::SetClipped { id, .. }
            | Command::SetPinning { id, .. }
            | Command::SetBlendMode { id, .. }
            | Command::SetEffects { id, .. }
            | Command::SetTransform { id, .. }
            | Command::SetKind { id, .. }
            | Command::SetName { id, .. }
            | Command::SetMask { id, .. }
            | Command::MoveNode { id, .. }
            | Command::AddStroke { id, .. }
            | Command::RemoveStroke { id, .. }
            | Command::SetStroke { id, .. } => Some(*id),
        }
    }

    /// The box a command disturbs when that is narrower than the whole
    /// node's: a stroke laid on, replaced in, or taken off a paint layer
    /// touches only where that stroke is, and a painting can be large.
    fn stroke_bounds(&self, cmd: &Command) -> Option<Bounds> {
        let (id, box_, on_mask) = match cmd {
            Command::AddStroke {
                id,
                stroke,
                on_mask,
                ..
            } => (*id, stroke.bounds()?, *on_mask),
            // A brush extending a stroke changes only its tail, and
            // repainting the whole of a long one every time it grows is
            // what would make a long stroke crawl.
            Command::SetStroke {
                id,
                index,
                stroke,
                on_mask,
            } => {
                let box_ = match self.strokes_of(*id, *on_mask).and_then(|s| s.get(*index)) {
                    Some(other) => changed_bounds(stroke, other)?,
                    None => stroke.bounds()?,
                };
                (*id, box_, *on_mask)
            }
            Command::RemoveStroke { id, index, on_mask } => (
                *id,
                self.strokes_of(*id, *on_mask)?.get(*index)?.bounds()?,
                *on_mask,
            ),
            _ => return None,
        };
        // The box is written in the space the brush wrote it in, which
        // for a mask is the layer's parent's rather than the layer's own.
        let t = chitrakar_render::brush_space(&self.doc, id, on_mask).ok()?;
        Some(chitrakar_render::transformed_box(t, box_))
    }

    /// The strokes of a paint layer, or of a node's painted mask.
    fn strokes_of(&self, id: NodeId, on_mask: bool) -> Option<&[chitrakar_doc::PaintStroke]> {
        let node = self.doc.node(id).ok()?;
        if on_mask {
            return match node.mask.as_ref().map(|m| &m.kind) {
                Some(chitrakar_doc::MaskKind::Painted { strokes }) => Some(strokes),
                _ => None,
            };
        }
        match &node.kind {
            chitrakar_doc::NodeKind::Paint { strokes }
            | chitrakar_doc::NodeKind::Clone { strokes } => Some(strokes),
            _ => None,
        }
    }

    fn bounds_of_target(&self, id: Option<NodeId>) -> Bounds {
        id.and_then(|id| chitrakar_render::node_bounds(&self.doc, id).ok())
            .unwrap_or(Bounds::None)
    }

    /// Where every live copy of a layer is — directly, or through
    /// another copy. A change to the layer changes all of them, wherever
    /// they were put, so the region to repaint has to take them in.
    fn copies_bounds(&self, id: Option<NodeId>) -> Bounds {
        let Some(id) = id else {
            return Bounds::None;
        };
        // Almost every document has no copies in it, and this runs on
        // every command — a drag asks for it a hundred times a second.
        if !self.has_copies {
            return Bounds::None;
        }
        let mut order = Vec::new();
        Self::painter_order(&self.doc, self.doc.root(), &mut order);
        let mut following = vec![id];
        let mut bounds = Bounds::None;
        // A copy of a copy follows the same original, so keep going until
        // nothing new is found. The graph has no cycles, so it ends.
        let mut at = 0;
        while at < following.len() {
            let cur = following[at];
            at += 1;
            for &other in &order {
                let Ok(node) = self.doc.node(other) else {
                    continue;
                };
                if matches!(node.kind, NodeKind::Instance { of } if of == cur)
                    && !following.contains(&other)
                {
                    following.push(other);
                    if let Ok(b) = chitrakar_render::node_bounds(&self.doc, other) {
                        bounds = bounds.union(b);
                    }
                }
            }
        }
        bounds
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
    /// Re-read whether the document holds any live copies. Cheap, and
    /// only worth doing when a command could have changed the answer.
    fn note_copies(&mut self) {
        let mut order = Vec::new();
        Self::painter_order(&self.doc, self.doc.root(), &mut order);
        self.has_copies = order.iter().any(|&id| {
            matches!(
                self.doc.node(id).map(|n| &n.kind),
                Ok(NodeKind::Instance { .. })
            )
        });
    }

    fn apply_internal(&mut self, cmd: Command) -> Result<Command, EngineError> {
        // Both touch more than one node, so the whole canvas is the only
        // safe dirty region — and a resize changes what "the whole canvas"
        // even means.
        let batch = matches!(cmd, Command::Batch(_) | Command::ResizeCanvas { .. });
        let target = Self::command_target(&cmd);
        let pre = self
            .stroke_bounds(&cmd)
            .unwrap_or_else(|| self.bounds_of_target(target))
            .union(self.copies_bounds(target));
        let structural = matches!(
            cmd,
            Command::AddNode { .. }
                | Command::RemoveNode { .. }
                | Command::SetKind { .. }
                | Command::RestoreSubtree { .. }
                | Command::Batch(_)
        );
        let inverse = self.doc.apply(cmd)?;
        if structural {
            self.note_copies();
        }
        let post_target = Self::command_target(&inverse);
        let post = self
            .stroke_bounds(&inverse)
            .unwrap_or_else(|| self.bounds_of_target(post_target))
            .union(self.copies_bounds(post_target));
        self.last_touched = post_target.or(target);
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
            Command::SetLocked { id, locked } => {
                format!("{} {}", if *locked { "Lock" } else { "Unlock" }, name(id))
            }
            Command::SetClipped { id, clipped } => {
                format!("{} {}", if *clipped { "Clip" } else { "Unclip" }, name(id))
            }
            Command::SetPinning { id, .. } => format!("Pin {}", name(id)),
            Command::SetBlendMode { id, .. } => format!("Blend of {}", name(id)),
            Command::SetTransform { id, .. } => format!("Transform {}", name(id)),
            Command::SetKind { id, .. } => format!("Edit {}", name(id)),
            Command::SetName { id, .. } => format!("Rename {}", name(id)),
            Command::AddStroke { id, stroke, .. } | Command::SetStroke { id, stroke, .. } => {
                format!(
                    "{} on {}",
                    if stroke.erase { "Erase" } else { "Paint" },
                    name(id)
                )
            }
            Command::RemoveStroke { id, .. } => format!("Take a stroke off {}", name(id)),
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
        self.painting = None;
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
        self.painting = None;
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

    /// Give one layer its own adjustment or filter: wrap the layer and the
    /// new node in a group, where the group's isolation confines what the
    /// node does to what the layer paints. That is the only machinery
    /// involved — a group isolates whenever something inside it reads the
    /// backdrop — and so the result is ordinary layers that can be opened
    /// up, reordered and undone like any others. One history entry.
    pub fn adjust_node(&mut self, id: NodeId, node: Node) -> Result<NodeId, EngineError> {
        if !matches!(node.kind, NodeKind::Adjustment(_) | NodeKind::Filter(_)) {
            return Err(EngineError::BadCommand(
                "only an adjustment or a filter can be scoped to a layer".into(),
            ));
        }
        let parent = self
            .doc
            .parent_of(id)
            .ok_or_else(|| EngineError::BadCommand("cannot adjust the root".into()))?;
        let index = self
            .doc
            .children_of(parent)?
            .iter()
            .position(|s| *s == id)
            .ok_or_else(|| EngineError::BadCommand("layer is not in its parent".into()))?;
        let name = format!("{} + {}", self.doc.node(id)?.name, node.name);
        let group_id = self.doc.peek_next_id();
        let label = format!("Adjust {}", self.doc.node(id)?.name);
        self.apply_labeled(
            Command::Batch(vec![
                Command::AddNode {
                    parent,
                    index: index + 1,
                    node: Box::new(Node::group(&name)),
                },
                Command::MoveNode {
                    id,
                    parent: group_id,
                    index: 0,
                },
                Command::AddNode {
                    parent: group_id,
                    index: 1,
                    node: Box::new(node),
                },
            ]),
            Some(label),
        )?;
        Ok(group_id)
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
        let copy_id = self.emit_copy(
            id,
            parent,
            index,
            CopyStyle::Duplicate,
            &mut next,
            &mut cmds,
        )?;
        let label = format!("Duplicate {}", self.doc.node(id)?.name);
        self.apply_labeled(Command::Batch(cmds), Some(label))?;
        Ok(copy_id)
    }

    /// Add an empty layer to paint on at the top of the document.
    pub fn add_paint_layer(&mut self, name: &str) -> Result<NodeId, EngineError> {
        let parent = self.doc.root();
        let index = self.doc.children_of(parent)?.len();
        self.apply(Command::AddNode {
            parent,
            index,
            node: Box::new(chitrakar_doc::Node::paint(name)),
        })?;
        Ok(self.doc.children_of(parent)?[index])
    }

    /// Put a frame on the page: a box of its own size, at (x, y), that
    /// cuts whatever goes into it to that box and exports at exactly that
    /// many pixels. Frames go at the top level, under everything already
    /// there, so a frame added to a page of loose layers does not cover
    /// them.
    pub fn add_artboard(
        &mut self,
        name: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        background: Option<chitrakar_color::AuthoredColor>,
    ) -> Result<NodeId, EngineError> {
        if !(width > 0.0 && height > 0.0) {
            return Err(EngineError::BadCommand("a frame needs a size".into()));
        }
        let parent = self.doc.root();
        let mut node = chitrakar_doc::Node::artboard(name, width, height, background);
        node.transform = Transform::translation(x, y);
        let index = self.doc.children_of(parent)?.len();
        self.apply(Command::AddNode {
            parent,
            index,
            node: Box::new(node),
        })?;
        Ok(self.doc.children_of(parent)?[index])
    }

    /// How the page's tones are spread, as four runs of 256 counts —
    /// red, green, blue, then luminance — in the display encoding, which
    /// is the encoding the graphs that read a histogram are drawn over.
    ///
    /// `below` asks for what an adjustment layer *sees* rather than for
    /// the finished page: everything composited under it, which is what
    /// its own graph has to be read against. Transparent pixels are not
    /// counted — a histogram of a half-empty page should describe the
    /// picture on it, not the hole around it.
    ///
    /// Taken from a render shrunk to about a hundred thousand pixels: a
    /// histogram is a shape, and the shape settles long before the last
    /// pixel is counted.
    pub fn histogram(&self, below: Option<NodeId>) -> Result<Vec<u32>, EngineError> {
        let mut doc = self.doc.clone();
        if let Some(id) = below {
            let mut order = Vec::new();
            Self::painter_order(&doc, doc.root(), &mut order);
            if let Some(at) = order.iter().position(|&n| n == id) {
                for &later in &order[at..] {
                    // Already invisible layers are left alone: setting
                    // them again would only cost a command.
                    if doc.node(later).map(|n| n.visible).unwrap_or(false) {
                        doc.apply(Command::SetVisible {
                            id: later,
                            visible: false,
                        })?;
                    }
                }
            }
        }
        let (w, h) = (doc.meta.width.max(1) as f32, doc.meta.height.max(1) as f32);
        let scale = (100_000.0 / (w * h)).sqrt().min(1.0);
        let (pw, ph) = (
            (w * scale).round().max(1.0) as u32,
            (h * scale).round().max(1.0) as u32,
        );
        let mut surface = Surface::new(pw, ph);
        chitrakar_render::render_region_at(
            &doc,
            &mut surface,
            ClipRect {
                x0: 0,
                y0: 0,
                x1: pw,
                y1: ph,
            },
            Transform {
                a: scale,
                d: scale,
                ..Default::default()
            },
        )?;
        let mut bins = vec![0u32; 256 * 4];
        for px in &surface.pixels {
            if px.a <= 0.0 {
                continue;
            }
            let [r, g, b, _] = px.to_srgb8();
            bins[r as usize] += 1;
            bins[256 + g as usize] += 1;
            bins[512 + b as usize] += 1;
            // Luminance is a linear quantity, encoded for display once it
            // has been worked out — not an average of encoded channels.
            let lin = |v: f32| v / px.a;
            let y = 0.2126 * lin(px.r) + 0.7152 * lin(px.g) + 0.0722 * lin(px.b);
            let y = chitrakar_color::linear_to_srgb(y.clamp(0.0, 1.0));
            bins[768 + (y * 255.0).round() as usize] += 1;
        }
        Ok(bins)
    }

    /// The frame under a document point, if any — what a shape drawn
    /// there should go into.
    pub fn frame_at(&self, x: f32, y: f32) -> Option<NodeId> {
        chitrakar_render::frame_at(&self.doc, x, y).ok().flatten()
    }

    /// A document point in the coordinates a layer inside `id` is written
    /// in. `None` when that space has collapsed.
    pub fn point_inside(&self, id: NodeId, x: f32, y: f32) -> Option<[f32; 2]> {
        let space = chitrakar_render::own_space(&self.doc, id).ok()?;
        let back = chitrakar_render::invert(space)?;
        Some([
            back.a * x + back.c * y + back.e,
            back.b * x + back.d * y + back.f,
        ])
    }

    /// How many layers a group or frame holds.
    pub fn child_count(&self, id: NodeId) -> usize {
        self.doc.children_of(id).map(|c| c.len()).unwrap_or(0)
    }

    /// Move a layer to a new parent and slot, keeping it exactly where it
    /// is on the page: the new parent's space is undone from the layer's
    /// own transform, so dropping a layer into a frame that sits at
    /// (400, 200) does not throw it 400 pixels left. One entry in
    /// history. A move within the same parent is a plain reorder and
    /// leaves the transform alone.
    pub fn reparent(
        &mut self,
        id: NodeId,
        parent: NodeId,
        index: usize,
    ) -> Result<(), EngineError> {
        let label = format!("Move {}", self.doc.node(id)?.name);
        let move_it = Command::MoveNode { id, parent, index };
        if self.doc.parent_of(id) == Some(parent) {
            return self.apply_labeled(move_it, Some(label));
        }
        let was = chitrakar_render::ancestor_space(&self.doc, id);
        let now = chitrakar_render::own_space(&self.doc, parent)?;
        let Some(back) = chitrakar_render::invert(now) else {
            return self.apply_labeled(move_it, Some(label));
        };
        let keep = back.compose(was).compose(self.doc.node(id)?.transform);
        self.apply_labeled(
            Command::Batch(vec![
                move_it,
                Command::SetTransform {
                    id,
                    transform: keep,
                },
            ]),
            Some(label),
        )
    }

    /// Put a live copy of a layer beside it: a layer that draws whatever
    /// that one holds, wherever the copy is put, so changing the original
    /// changes every copy of it. Returns the copy's id.
    ///
    /// The copy goes in the original's own parent, directly above it, and
    /// starts exactly on top of the original — the placement is the first
    /// thing anyone changes about a copy, and starting it anywhere else
    /// would only be a guess.
    pub fn make_instance(&mut self, of: NodeId) -> Result<NodeId, EngineError> {
        let node = self.doc.node(of)?;
        // A copy of a copy is a copy of the same original: a chain of
        // them would work, but it is never what was meant and it makes
        // the original hard to find.
        let target = match node.kind {
            NodeKind::Instance { of: original } => original,
            _ => of,
        };
        let name = format!("{} copy", self.doc.node(target)?.name);
        let parent = self.doc.parent_of(of).unwrap_or_else(|| self.doc.root());
        let index = self
            .doc
            .children_of(parent)?
            .iter()
            .position(|&c| c == of)
            .map(|i| i + 1)
            .unwrap_or(0);
        let mut copy = chitrakar_doc::Node::instance(&name, target);
        // Written in the copy's parent space, which is where the
        // original's own transform is written too.
        copy.transform = self.doc.node(target)?.transform;
        self.apply(Command::AddNode {
            parent,
            index,
            node: Box::new(copy),
        })?;
        Ok(self.doc.children_of(parent)?[index])
    }

    /// The one command that gives a frame a new size and moves what is
    /// in it by how each layer is pinned — as JSON, so a corner drag can
    /// preview it every move and let go of it as a single entry.
    ///
    /// `dx`/`dy` shift the frame's own origin, in the frame's coordinates
    /// before the resize: pulling the west or north edge moves the corner
    /// everything inside is measured from, and the frame's transform has
    /// to take that up so the rest of the page does not move.
    ///
    /// The layers inside are not told about that shift at all — their own
    /// coordinates are unchanged by it, and a layer pinned to the start
    /// edge should follow that edge, which is exactly what happens when
    /// nothing is done. What the other pins need is the change in size.
    pub fn artboard_resize(
        &self,
        id: NodeId,
        width: f32,
        height: f32,
        dx: f32,
        dy: f32,
    ) -> Result<String, EngineError> {
        let node = self.doc.node(id)?;
        let NodeKind::Artboard {
            width: w0,
            height: h0,
            background,
        } = &node.kind
        else {
            return Err(EngineError::BadCommand("that layer is not a frame".into()));
        };
        if !(width > 0.0 && height > 0.0) {
            return Err(EngineError::BadCommand("a frame needs a size".into()));
        }
        let (dw, dh) = (width - w0, height - h0);
        let mut cmds = vec![
            Command::SetKind {
                id,
                kind: Box::new(NodeKind::Artboard {
                    width,
                    height,
                    background: *background,
                }),
            },
            Command::SetTransform {
                id,
                transform: node.transform.compose(Transform::translation(dx, dy)),
            },
        ];
        for &child in self.doc.children_of(id)? {
            let kid = self.doc.node(child)?;
            let Bounds::Rect(x0, y0, x1, y1) =
                chitrakar_render::bounds_in_parent_space(&self.doc, child)?
            else {
                // An adjustment or a filter fills whatever it is in and
                // has no box to pin by; it needs no moving either.
                continue;
            };
            let (sx, tx) = Self::axis(kid.pinned.x, x0, x1, dw);
            let (sy, ty) = Self::axis(kid.pinned.y, y0, y1, dh);
            if sx == 1.0 && sy == 1.0 && tx == 0.0 && ty == 0.0 {
                continue;
            }
            // The move is written in the frame's space, so it goes on the
            // outside of the layer's own transform.
            let outer = Transform {
                a: sx,
                b: 0.0,
                c: 0.0,
                d: sy,
                e: tx + x0 * (1.0 - sx),
                f: ty + y0 * (1.0 - sy),
            };
            cmds.push(Command::SetTransform {
                id: child,
                transform: outer.compose(kid.transform),
            });
        }
        serde_json::to_string(&Command::Batch(cmds))
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// A frame's contents as a PNG, at the size the frame shows on the
    /// page times `scale`. Errors when the node is not a frame.
    pub fn artboard_png(&self, id: NodeId, scale: f32) -> Result<Vec<u8>, EngineError> {
        let scale = scale.clamp(0.05, 16.0);
        let Some(surface) = chitrakar_render::artboard_pixels(&self.doc, id, scale)? else {
            return Err(EngineError::BadCommand("that layer is not a frame".into()));
        };
        chitrakar_codecs::encode_png(surface.width, surface.height, &surface.to_srgb8())
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Start a brush stroke on a paint layer. The point is in document
    /// space and is written into the layer's own; the stroke goes on as
    /// a preview, so however many times it is extended the whole of it
    /// is one entry in history when the gesture is committed.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_begin(
        &mut self,
        layer: NodeId,
        x: f32,
        y: f32,
        radius: f32,
        color: chitrakar_color::AuthoredColor,
        softness: f32,
        erase: bool,
        on_mask: bool,
    ) -> Result<(), EngineError> {
        let index = self.stroke_count(layer, on_mask)?;
        let Some((lx, ly)) = self.point_in_layer(layer, on_mask, x, y)? else {
            return Ok(());
        };
        let stroke = chitrakar_doc::PaintStroke {
            points: vec![[lx, ly]],
            radii: vec![self.radius_in_layer(layer, on_mask, radius)?],
            color,
            softness,
            erase,
            source: [0.0, 0.0],
            heal: false,
        };
        self.preview(Command::AddStroke {
            id: layer,
            index,
            stroke: Box::new(stroke.clone()),
            on_mask,
        })?;
        self.painting = Some(Painting {
            layer,
            index,
            stroke,
            on_mask,
        });
        Ok(())
    }

    /// Carry the stroke being drawn on to another point. A point that
    /// lands where the last one did is dropped: a pointer that has not
    /// moved has nothing to add, and the stroke is redrawn every time it
    /// grows.
    pub fn paint_extend(&mut self, x: f32, y: f32, radius: f32) -> Result<(), EngineError> {
        let Some(painting) = self.painting.as_ref() else {
            return Ok(());
        };
        let (layer, index, on_mask) = (painting.layer, painting.index, painting.on_mask);
        let Some((lx, ly)) = self.point_in_layer(layer, on_mask, x, y)? else {
            return Ok(());
        };
        let radius = self.radius_in_layer(layer, on_mask, radius)?;
        let painting = self.painting.as_mut().expect("checked above");
        if let Some(last) = painting.stroke.points.last() {
            if (last[0] - lx).abs() < 0.01 && (last[1] - ly).abs() < 0.01 {
                return Ok(());
            }
        }
        painting.stroke.points.push([lx, ly]);
        painting.stroke.radii.push(radius);
        let stroke = Box::new(painting.stroke.clone());
        self.preview(Command::SetStroke {
            id: layer,
            index,
            stroke,
            on_mask,
        })
    }

    /// Whether a brush stroke is being drawn.
    pub fn is_painting(&self) -> bool {
        self.painting.is_some()
    }

    /// Give a layer a mask a brush can work on, when it has not got one.
    ///
    /// `false` when there is already a mask of another kind there: that
    /// one was drawn or placed deliberately, and replacing it is not
    /// this function's decision to make.
    pub fn ensure_painted_mask(&mut self, id: NodeId) -> Result<bool, EngineError> {
        match self.doc.node(id)?.mask.as_ref().map(|m| &m.kind) {
            Some(chitrakar_doc::MaskKind::Painted { .. }) => Ok(true),
            Some(_) => Ok(false),
            None => {
                self.apply(Command::SetMask {
                    id,
                    mask: Some(Box::new(chitrakar_doc::Mask {
                        kind: chitrakar_doc::MaskKind::Painted {
                            strokes: Vec::new(),
                        },
                        invert: false,
                    })),
                })?;
                Ok(true)
            }
        }
    }

    /// A small square picture of one layer on its own, for a panel that
    /// shows what a layer holds rather than only what it is called.
    /// RGBA8, `size * size * 4` bytes — empty when the layer has no
    /// picture of its own, which an adjustment or a filter has not.
    pub fn thumbnail(&self, id: NodeId, size: u32) -> Result<Vec<u8>, EngineError> {
        Ok(chitrakar_render::thumbnail(&self.doc, id, size)?.unwrap_or_default())
    }

    /// A small square picture of a layer's mask, fitted the same way its
    /// own picture is so the two line up: white where the layer shows
    /// through and clear where it is hidden. Empty when there is no
    /// mask.
    pub fn mask_thumbnail(&self, id: NodeId, size: u32) -> Result<Vec<u8>, EngineError> {
        Ok(chitrakar_render::mask_thumbnail(&self.doc, id, size)?.unwrap_or_default())
    }

    /// Put a new anchor on a path at the point nearest a document point,
    /// splitting the segment it lands on so the path keeps the shape it
    /// had. Returns where the new anchor sits in the path's order.
    ///
    /// A curved segment is split properly rather than cut straight: the
    /// two halves take the control points de Casteljau gives them, so
    /// the curve through the new anchor is the curve that was there.
    ///
    /// `within` is how far from the outline the point may be, in the
    /// layer's own units: an anchor goes on the outline, so a point that
    /// is merely *inside* the shape is not asking for one.
    pub fn insert_anchor(
        &mut self,
        id: NodeId,
        x: f32,
        y: f32,
        within: f32,
    ) -> Result<usize, EngineError> {
        let node = self.doc.node(id)?;
        let chitrakar_doc::NodeKind::Vector { shape, .. } = &node.kind else {
            return Err(EngineError::BadCommand("not a shape layer".into()));
        };
        let chitrakar_doc::VectorShape::Path {
            points,
            closed,
            smooth,
            handles,
            subpaths,
        } = shape
        else {
            return Err(EngineError::BadCommand("not a path".into()));
        };
        if points.len() < 2 {
            return Err(EngineError::BadCommand("a path of one anchor".into()));
        }
        let Some((lx, ly)) = self.point_in_layer(id, false, x, y)? else {
            return Ok(0);
        };
        let mut hs = padded_handles(handles, points.len());
        let segments = if *closed {
            points.len()
        } else {
            points.len() - 1
        };
        // The closest point on the path, found by walking each segment.
        const STEPS: usize = 24;
        let (mut best, mut at, mut best_t) = (f32::MAX, 0usize, 0.0f32);
        for i in 0..segments {
            let j = (i + 1) % points.len();
            for k in 0..=STEPS {
                let t = k as f32 / STEPS as f32;
                let p = cubic_at(points[i], hs[i], points[j], hs[j], t);
                let d = (p[0] - lx).powi(2) + (p[1] - ly).powi(2);
                if d < best {
                    (best, at, best_t) = (d, i, t);
                }
            }
        }
        if best.sqrt() > within.max(0.0) {
            return Err(EngineError::BadCommand("no outline near that point".into()));
        }
        let j = (at + 1) % points.len();
        let (a, b) = (points[at], points[j]);
        let (c1, c2) = (
            [a[0] + hs[at][2], a[1] + hs[at][3]],
            [b[0] + hs[j][0], b[1] + hs[j][1]],
        );
        let t = best_t;
        let mix = |p: [f32; 2], q: [f32; 2]| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
        let (p01, p12, p23) = (mix(a, c1), mix(c1, c2), mix(c2, b));
        let (p012, p123) = (mix(p01, p12), mix(p12, p23));
        let m = mix(p012, p123);
        let off = |from: [f32; 2], to: [f32; 2]| [to[0] - from[0], to[1] - from[1]];

        let mut points = points.clone();
        hs[at] = [hs[at][0], hs[at][1], off(a, p01)[0], off(a, p01)[1]];
        hs[j] = [off(b, p23)[0], off(b, p23)[1], hs[j][2], hs[j][3]];
        let fresh = [
            off(m, p012)[0],
            off(m, p012)[1],
            off(m, p123)[0],
            off(m, p123)[1],
        ];
        points.insert(at + 1, m);
        hs.insert(at + 1, fresh);
        self.replace_path(
            id,
            chitrakar_doc::VectorShape::Path {
                points,
                closed: *closed,
                smooth: *smooth,
                handles: hs,
                subpaths: subpaths.clone(),
            },
            "Add anchor",
        )?;
        Ok(at + 1)
    }

    /// Take an anchor off a path. Refuses to leave one that has nothing
    /// left to be a path with.
    pub fn remove_anchor(&mut self, id: NodeId, index: usize) -> Result<(), EngineError> {
        let node = self.doc.node(id)?;
        let chitrakar_doc::NodeKind::Vector { shape, .. } = &node.kind else {
            return Err(EngineError::BadCommand("not a shape layer".into()));
        };
        let chitrakar_doc::VectorShape::Path {
            points,
            closed,
            smooth,
            handles,
            subpaths,
        } = shape
        else {
            return Err(EngineError::BadCommand("not a path".into()));
        };
        let least = if *closed { 3 } else { 2 };
        if index >= points.len() || points.len() <= least {
            return Err(EngineError::BadCommand(
                "a path needs the anchors it has left".into(),
            ));
        }
        let mut points = points.clone();
        let mut hs = padded_handles(handles, points.len());
        points.remove(index);
        hs.remove(index);
        self.replace_path(
            id,
            chitrakar_doc::VectorShape::Path {
                points,
                closed: *closed,
                smooth: *smooth,
                handles: hs,
                subpaths: subpaths.clone(),
            },
            "Remove anchor",
        )
    }

    /// Put a rewritten path back on a layer, with its anchors normalized
    /// to a (0,0) origin and the shift folded into the transform, which
    /// is the invariant every other path edit keeps.
    fn replace_path(
        &mut self,
        id: NodeId,
        shape: chitrakar_doc::VectorShape,
        label: &str,
    ) -> Result<(), EngineError> {
        let node = self.doc.node(id)?;
        let chitrakar_doc::NodeKind::Vector {
            fill,
            stroke,
            gradient,
            ..
        } = &node.kind
        else {
            return Err(EngineError::BadCommand("not a shape layer".into()));
        };
        let (fill, stroke, gradient) = (*fill, stroke.clone(), gradient.clone());
        let t = node.transform;
        let chitrakar_doc::VectorShape::Path {
            mut points,
            closed,
            smooth,
            handles,
            subpaths,
        } = shape
        else {
            return Err(EngineError::BadCommand("not a path".into()));
        };
        let (mut mx, mut my) = (f32::MAX, f32::MAX);
        for p in points.iter().chain(subpaths.iter().flatten()) {
            mx = mx.min(p[0]);
            my = my.min(p[1]);
        }
        let mut subpaths = subpaths;
        if mx.is_finite() && my.is_finite() && (mx != 0.0 || my != 0.0) {
            for p in points.iter_mut().chain(subpaths.iter_mut().flatten()) {
                p[0] -= mx;
                p[1] -= my;
            }
        } else {
            (mx, my) = (0.0, 0.0);
        }
        let moved = chitrakar_doc::Transform {
            e: t.e + t.a * mx + t.c * my,
            f: t.f + t.b * mx + t.d * my,
            ..t
        };
        self.apply_labeled(
            Command::Batch(vec![
                Command::SetKind {
                    id,
                    kind: Box::new(chitrakar_doc::NodeKind::Vector {
                        shape: chitrakar_doc::VectorShape::Path {
                            points,
                            closed,
                            smooth,
                            handles,
                            subpaths,
                        },
                        fill,
                        stroke,
                        gradient,
                    }),
                },
                Command::SetTransform {
                    id,
                    transform: moved,
                },
            ]),
            Some(label.to_string()),
        )
    }

    /// Add an empty layer to clone onto at the top of the document.
    pub fn add_clone_layer(&mut self, name: &str) -> Result<NodeId, EngineError> {
        let parent = self.doc.root();
        let index = self.doc.children_of(parent)?.len();
        self.apply(Command::AddNode {
            parent,
            index,
            node: Box::new(chitrakar_doc::Node::clone_layer(name)),
        })?;
        Ok(self.doc.children_of(parent)?[index])
    }

    /// Where the stroke being drawn reads from, as an offset in the
    /// layer's own space, and whether it heals — laying the source's
    /// texture down in the colour of the place it lands. A clone stroke
    /// set after it has begun still takes both, since the whole stroke
    /// is one preview.
    pub fn paint_source(&mut self, dx: f32, dy: f32, heal: bool) -> Result<(), EngineError> {
        let Some(painting) = self.painting.as_ref() else {
            return Ok(());
        };
        let (layer, index, on_mask) = (painting.layer, painting.index, painting.on_mask);
        // The offset arrives in document units, like the point and the
        // radius, and like them it is written in the layer's own.
        let scale = chitrakar_render::layer_scale(&self.doc, layer, on_mask)?.max(1e-6);
        let painting = self.painting.as_mut().expect("checked above");
        painting.stroke.source = [dx / scale, dy / scale];
        painting.stroke.heal = heal;
        let stroke = Box::new(painting.stroke.clone());
        self.preview(Command::SetStroke {
            id: layer,
            index,
            stroke,
            on_mask,
        })
    }

    /// How many strokes a paint layer holds, which is where the next one
    /// goes.
    pub fn stroke_count(&self, id: NodeId, on_mask: bool) -> Result<usize, EngineError> {
        self.strokes_of(id, on_mask)
            .map(|s| s.len())
            .ok_or(EngineError::Doc(chitrakar_doc::DocError::NotAPaintLayer(
                id,
            )))
    }

    /// A brush radius given in document units, in a layer's own — a
    /// stroke is written in the layer's space, so a scaled layer takes a
    /// smaller radius to paint the same width on the page.
    fn radius_in_layer(&self, id: NodeId, on_mask: bool, radius: f32) -> Result<f32, EngineError> {
        Ok(radius / chitrakar_render::layer_scale(&self.doc, id, on_mask)?.max(1e-6))
    }

    /// A document-space point in a layer's own space, which is where a
    /// brush has to write its stroke.
    pub fn point_in_layer(
        &self,
        id: NodeId,
        on_mask: bool,
        x: f32,
        y: f32,
    ) -> Result<Option<(f32, f32)>, EngineError> {
        Ok(chitrakar_render::point_in_layer(
            &self.doc, id, on_mask, x, y,
        )?)
    }

    /// Duplicate several layers as one undo step, each copy landing just
    /// above the layer it was made from and nudged clear of it when
    /// `offset`. Returns the copies in the order the originals were
    /// given.
    pub fn duplicate_nodes(
        &mut self,
        ids: &[NodeId],
        offset: bool,
    ) -> Result<Vec<NodeId>, EngineError> {
        if ids.is_empty() {
            return Err(EngineError::BadCommand("nothing to duplicate".into()));
        }
        let style = if offset {
            CopyStyle::Duplicate
        } else {
            CopyStyle::InPlace
        };
        // Where each copy goes, read from the document as it stands.
        let mut slots = Vec::with_capacity(ids.len());
        for (at, &id) in ids.iter().enumerate() {
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
            slots.push((at, id, parent, index));
        }
        // Topmost first: an insertion shifts everything above it, so the
        // slots read a moment ago stay true if they are filled downwards.
        slots.sort_by(|a, b| b.3.cmp(&a.3));
        let mut next = self.doc.peek_next_id().0;
        let mut cmds = Vec::new();
        let mut copies = vec![NodeId(0); ids.len()];
        for (at, id, parent, index) in slots {
            copies[at] = self.emit_copy(id, parent, index, style, &mut next, &mut cmds)?;
        }
        let label = if ids.len() == 1 {
            format!("Duplicate {}", self.doc.node(ids[0])?.name)
        } else {
            format!("Duplicate {} layers", ids.len())
        };
        self.apply_labeled(Command::Batch(cmds), Some(label))?;
        Ok(copies)
    }

    fn emit_copy(
        &self,
        src: NodeId,
        parent: NodeId,
        index: usize,
        style: CopyStyle,
        next: &mut u64,
        cmds: &mut Vec<Command>,
    ) -> Result<NodeId, EngineError> {
        let mut node = self.doc.node(src)?.clone();
        if style != CopyStyle::Exact {
            node.name = format!("{} copy", node.name);
        }
        if style == CopyStyle::Duplicate {
            // Nudge the copy so it is visible rather than hiding exactly
            // behind the original. A copy that is about to be dragged
            // wants none: it starts where the pointer took hold of it.
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
            self.emit_copy(*child, new_id, i, CopyStyle::Exact, next, cmds)?;
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

    /// Set the opacity of several layers as one undo step.
    pub fn set_opacity_of(&mut self, ids: &[NodeId], opacity: f32) -> Result<(), EngineError> {
        self.set_each(ids, "Opacity", |id| Command::SetOpacity { id, opacity })
    }

    /// A layer's look, without its shape: what it is painted with, what
    /// hangs off it, and how it sits on what is under it. Carried as
    /// JSON so it outlives the document it was taken from, the way the
    /// layer clipboard does.
    pub fn copy_style(&self, id: NodeId) -> Result<String, EngineError> {
        let node = self.doc.node(id)?;
        let (fill, stroke, gradient) = match &node.kind {
            chitrakar_doc::NodeKind::Vector {
                fill,
                stroke,
                gradient,
                ..
            } => (*fill, stroke.clone(), gradient.clone()),
            chitrakar_doc::NodeKind::Text(spec) => (Some(spec.fill), None, None),
            _ => (None, None, None),
        };
        let style = Style {
            fill,
            stroke,
            gradient,
            effects: node.effects.clone(),
            opacity: node.opacity,
            blend: node.blend,
        };
        serde_json::to_string(&style).map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Give that look to every layer named, in one entry.
    ///
    /// A layer takes what it can carry: a shape takes the fill, the
    /// stroke and the gradient, a block of text takes the fill alone,
    /// and everything takes the effects, the opacity and the blend.
    /// Nothing takes another layer's shape — that is not what a style
    /// is.
    pub fn paste_style(&mut self, json: &str, ids: &[NodeId]) -> Result<(), EngineError> {
        let style: Style =
            serde_json::from_str(json).map_err(|e| EngineError::BadCommand(e.to_string()))?;
        // A look taken from a layer that has nothing to paint with — an
        // adjustment, a group, a placed photo — says nothing about how
        // to paint, so it leaves the target's own paint alone rather
        // than stripping it. A shape that really is painted with
        // nothing but a stroke still clears a fill, which is the answer
        // that was asked for.
        let paints = style.fill.is_some() || style.stroke.is_some() || style.gradient.is_some();
        let mut cmds = Vec::new();
        for &id in ids {
            let node = self.doc.node(id)?;
            match &node.kind {
                chitrakar_doc::NodeKind::Vector { shape, .. } if paints => {
                    cmds.push(Command::SetKind {
                        id,
                        kind: Box::new(chitrakar_doc::NodeKind::Vector {
                            shape: shape.clone(),
                            fill: style.fill,
                            stroke: style.stroke.clone(),
                            gradient: style.gradient.clone(),
                        }),
                    });
                }
                chitrakar_doc::NodeKind::Text(spec) if paints => {
                    if let Some(fill) = style.fill {
                        let mut spec = spec.clone();
                        spec.fill = fill;
                        cmds.push(Command::SetKind {
                            id,
                            kind: Box::new(chitrakar_doc::NodeKind::Text(spec)),
                        });
                    }
                }
                _ => {}
            }
            cmds.push(Command::SetEffects {
                id,
                effects: style.effects.clone(),
            });
            cmds.push(Command::SetOpacity {
                id,
                opacity: style.opacity,
            });
            cmds.push(Command::SetBlendMode {
                id,
                blend: style.blend,
            });
        }
        if cmds.is_empty() {
            return Ok(());
        }
        let label = if ids.len() == 1 {
            "Paste style".to_string()
        } else {
            format!("Paste style on {} layers", ids.len())
        };
        self.apply_labeled(Command::Batch(cmds), Some(label))
    }

    /// Set the blend mode of several layers as one undo step.
    pub fn set_blend_of(
        &mut self,
        ids: &[NodeId],
        blend: chitrakar_doc::BlendMode,
    ) -> Result<(), EngineError> {
        self.set_each(ids, "Blend", |id| Command::SetBlendMode { id, blend })
    }

    /// The same edit on every layer named, in one entry called after
    /// what was changed and what it was changed on.
    fn set_each(
        &mut self,
        ids: &[NodeId],
        what: &str,
        make: impl Fn(NodeId) -> Command,
    ) -> Result<(), EngineError> {
        if ids.is_empty() {
            return Err(EngineError::BadCommand("nothing to change".into()));
        }
        let label = if ids.len() == 1 {
            format!("{what} of {}", self.doc.node(ids[0])?.name)
        } else {
            format!("{what} of {} layers", ids.len())
        };
        let cmds = ids.iter().map(|id| make(*id)).collect();
        self.apply_labeled(Command::Batch(cmds), Some(label))
    }

    /// Mirror layers about their shared box — left for right, or top for
    /// bottom — as one undo step. The box is the union of the picked
    /// layers' document-space bounds, so a pair flips as a pair and a
    /// single layer flips in place. A layer's transform is written in its
    /// parent's space, so the document-space mirror is carried through
    /// the ancestors and back.
    pub fn flip_nodes(&mut self, ids: &[NodeId], horizontal: bool) -> Result<(), EngineError> {
        let boxes: Vec<(NodeId, [f32; 4])> = ids
            .iter()
            .filter_map(|id| self.bounds_of(*id).map(|b| (*id, b)))
            .collect();
        if boxes.is_empty() {
            return Err(EngineError::BadCommand("nothing to flip".into()));
        }
        let union = boxes
            .iter()
            .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |acc, (_, b)| {
                [
                    acc[0].min(b[0]),
                    acc[1].min(b[1]),
                    acc[2].max(b[0] + b[2]),
                    acc[3].max(b[1] + b[3]),
                ]
            });
        let mirror = if horizontal {
            Transform {
                a: -1.0,
                e: union[0] + union[2],
                ..Default::default()
            }
        } else {
            Transform {
                d: -1.0,
                f: union[1] + union[3],
                ..Default::default()
            }
        };
        let mut cmds = Vec::new();
        for (id, _) in boxes {
            // The node draws through A·t; it should draw through M·A·t,
            // so its own transform becomes A⁻¹·M·A·t.
            let a = chitrakar_render::ancestor_space(&self.doc, id);
            let det = a.a * a.d - a.b * a.c;
            if det.abs() < 1e-9 {
                continue;
            }
            let inv = Transform {
                a: a.d / det,
                b: -a.b / det,
                c: -a.c / det,
                d: a.a / det,
                e: (a.c * a.f - a.d * a.e) / det,
                f: (a.b * a.e - a.a * a.f) / det,
            };
            let t = self.doc.node(id)?.transform;
            cmds.push(Command::SetTransform {
                id,
                transform: inv.compose(mirror).compose(a).compose(t),
            });
        }
        if cmds.is_empty() {
            return Ok(());
        }
        let label = if horizontal {
            "Flip horizontal"
        } else {
            "Flip vertical"
        };
        self.apply_labeled(Command::Batch(cmds), Some(label.to_string()))
    }

    /// Set a text block along a shape layer's outline: the shape's
    /// geometry is copied into the block's own space, so the block stands
    /// alone afterwards and the shape layer can go or stay. One undo step.
    pub fn text_along(&mut self, text: NodeId, shape: NodeId) -> Result<(), EngineError> {
        let NodeKind::Text(spec) = &self.doc.node(text)?.kind else {
            return Err(EngineError::BadCommand(
                "only text goes along a path".into(),
            ));
        };
        let NodeKind::Vector { shape: guide, .. } = &self.doc.node(shape)?.kind else {
            return Err(EngineError::BadCommand("the guide must be a shape".into()));
        };
        // Shape space → document → text space.
        let to_doc = chitrakar_render::ancestor_space(&self.doc, shape)
            .compose(self.doc.node(shape)?.transform);
        let text_to_doc = chitrakar_render::ancestor_space(&self.doc, text)
            .compose(self.doc.node(text)?.transform);
        let d = text_to_doc.a * text_to_doc.d - text_to_doc.b * text_to_doc.c;
        if d.abs() < 1e-9 {
            return Err(EngineError::BadCommand("the text block has no size".into()));
        }
        let t = text_to_doc;
        let from_doc = Transform {
            a: t.d / d,
            b: -t.b / d,
            c: -t.c / d,
            d: t.a / d,
            e: (t.c * t.f - t.d * t.e) / d,
            f: (t.b * t.e - t.a * t.f) / d,
        };
        let m = from_doc.compose(to_doc);
        let map = |p: [f32; 2]| [m.a * p[0] + m.c * p[1] + m.e, m.b * p[0] + m.d * p[1] + m.f];
        let map_offset = |p: [f32; 2]| [m.a * p[0] + m.c * p[1], m.b * p[0] + m.d * p[1]];
        let along = match guide {
            chitrakar_doc::VectorShape::Path {
                points,
                closed,
                smooth,
                handles,
                ..
            } => chitrakar_doc::VectorShape::Path {
                points: points.iter().map(|p| map(*p)).collect(),
                closed: *closed,
                smooth: *smooth,
                handles: handles
                    .iter()
                    .map(|h| {
                        let i = map_offset([h[0], h[1]]);
                        let o = map_offset([h[2], h[3]]);
                        [i[0], i[1], o[0], o[1]]
                    })
                    .collect(),
                subpaths: Vec::new(),
            },
            other => chitrakar_doc::VectorShape::Path {
                points: chitrakar_render::shape_rings(other)
                    .into_iter()
                    .next()
                    .unwrap_or_default()
                    .into_iter()
                    .map(map)
                    .collect(),
                closed: true,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
        };
        let mut spec = spec.clone();
        spec.along = Some(along);
        spec.along_offset = 0.0;
        self.apply_labeled(
            Command::SetKind {
                id: text,
                kind: Box::new(NodeKind::Text(spec)),
            },
            Some("Text on path".to_string()),
        )
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

    /// Export a one-page PDF: live vectors and images where PDF can carry
    /// them, this renderer's pixels where it cannot. With a press profile
    /// loaded the page is written in ink and carries that profile;
    /// otherwise it is sRGB.
    pub fn export_pdf(&self) -> Result<Vec<u8>, EngineError> {
        chitrakar_codecs::export_pdf_document(&self.doc)
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

    /// Render a region of the document — the whole page when `region` is
    /// None, otherwise `[x, y, w, h]` in document pixels — at `scale`
    /// pixels to the document pixel. This is what an export at 2x or of the
    /// selection is: the same root affine the viewport uses, aimed at a
    /// surface the size of what is wanted, so nothing is rendered and then
    /// cut down.
    pub fn render_scaled(
        &self,
        scale: f32,
        region: Option<[f32; 4]>,
    ) -> Result<Surface, EngineError> {
        let scale = scale.clamp(0.05, 16.0);
        let [x, y, w, h] = region.unwrap_or([
            0.0,
            0.0,
            self.doc.meta.width as f32,
            self.doc.meta.height as f32,
        ]);
        if !(w > 0.0 && h > 0.0) {
            return Err(EngineError::BadCommand("the export region is empty".into()));
        }
        let (pw, ph) = (
            (w * scale).round().max(1.0) as u32,
            (h * scale).round().max(1.0) as u32,
        );
        let mut surface = Surface::new(pw, ph);
        let full = ClipRect {
            x0: 0,
            y0: 0,
            x1: pw,
            y1: ph,
        };
        chitrakar_render::render_region_at(
            &self.doc,
            &mut surface,
            full,
            Transform {
                a: scale,
                d: scale,
                e: -x * scale,
                f: -y * scale,
                ..Default::default()
            },
        )?;
        Ok(surface)
    }

    /// PNG of a region at a scale; see [`render_scaled`](Self::render_scaled).
    pub fn render_png_at(
        &self,
        scale: f32,
        region: Option<[f32; 4]>,
    ) -> Result<Vec<u8>, EngineError> {
        let surface = self.render_scaled(scale, region)?;
        chitrakar_codecs::encode_png(surface.width, surface.height, &surface.to_srgb8())
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// JPEG of a region at a scale; transparency flattens onto white.
    pub fn export_jpeg_at(
        &self,
        scale: f32,
        region: Option<[f32; 4]>,
        quality: u8,
    ) -> Result<Vec<u8>, EngineError> {
        let surface = self.render_scaled(scale, region)?;
        chitrakar_codecs::encode_jpeg(surface.width, surface.height, &surface.pixels, quality)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Render and encode as JPEG. Transparency flattens onto white, since
    /// JPEG carries no alpha.
    pub fn export_jpeg(&self, quality: u8) -> Result<Vec<u8>, EngineError> {
        let surface = self.render()?;
        chitrakar_codecs::encode_jpeg(surface.width, surface.height, &surface.pixels, quality)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// What one axis of a resize does to a layer, as a scale and a shift in
    /// the frame's own coordinates: `from`..`to` is the layer's span on that
    /// axis and `change` is how much longer the frame just became.
    fn axis(pin: chitrakar_doc::Pin, from: f32, to: f32, change: f32) -> (f32, f32) {
        use chitrakar_doc::Pin;
        let span = to - from;
        match pin {
            // Its distance from the start edge is its own coordinate, which
            // nothing here touches.
            Pin::Start => (1.0, 0.0),
            Pin::End => (1.0, change),
            Pin::Middle => (1.0, change / 2.0),
            // Both distances kept, so the layer takes up the difference. A
            // layer with no width on this axis has nothing to stretch and is
            // left where a start-pinned one would be.
            Pin::Stretch if span > 1e-6 => ((span + change).max(1e-3) / span, 0.0),
            Pin::Stretch => (1.0, 0.0),
        }
    }

    /// Every layer in the order the page is painted in — a parent before
    /// what it holds, and each layer before the ones drawn over it. What
    /// "everything below this layer" means.
    fn painter_order(doc: &Document, group: NodeId, out: &mut Vec<NodeId>) {
        let Ok(children) = doc.children_of(group) else {
            return;
        };
        for &id in children {
            out.push(id);
            if doc
                .node(id)
                .map(|n| n.kind.holds_children())
                .unwrap_or(false)
            {
                Self::painter_order(doc, id, out);
            }
        }
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
                    NodeKind::Artboard { .. } => "artboard",
                    NodeKind::Vector { .. } => "vector",
                    NodeKind::Raster(_) => "raster",
                    NodeKind::Adjustment(_) => "adjustment",
                    NodeKind::Filter(_) => "filter",
                    NodeKind::Text(_) => "text",
                    NodeKind::Paint { .. } => "paint",
                    NodeKind::Clone { .. } => "clone",
                    NodeKind::Instance { .. } => "instance",
                },
                visible: node.visible,
                opacity: node.opacity,
                blend: node.blend,
                has_mask: node.mask.is_some(),
                painted_mask: matches!(
                    node.mask.as_ref().map(|m| &m.kind),
                    Some(chitrakar_doc::MaskKind::Painted { .. })
                ),
                has_effects: !node.effects.is_empty(),
                copies: match node.kind {
                    NodeKind::Instance { of } => of.0,
                    _ => 0,
                },
                locked: node.locked,
                clipped: node.clipped && index > 0,
                pinned: node.pinned,
                depth,
                parent: group.0,
                index,
                sibling_count: children.len(),
            });
            if node.kind.holds_children() {
                self.collect_layers(id, depth + 1, out);
            }
        }
    }

    /// Decode image bytes, pool them as a resource, and add a raster object
    /// referencing them at the top of the root group (one undo step).
    pub fn place_image(&mut self, bytes: &[u8], name: &str) -> Result<NodeId, EngineError> {
        let img =
            chitrakar_codecs::decode(bytes).map_err(|e| EngineError::BadCommand(e.to_string()))?;
        let (width, height) = (img.width, img.height);
        let resource_id = self.doc.add_resource(width, height, img.rgba8);
        let root = self.doc.root();
        let index = self.doc.children_of(root)?.len();
        let id = self.doc.peek_next_id();
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
        })?;
        Ok(id)
    }

    /// Bring an SVG in as a group of shape layers named after the file,
    /// on top of the stack, as one undo step. Returns the group's id.
    pub fn place_svg(&mut self, bytes: &[u8], name: &str) -> Result<NodeId, EngineError> {
        let imported = chitrakar_codecs::import_svg(bytes).map_err(EngineError::BadCommand)?;
        if imported.shapes.is_empty() {
            return Err(EngineError::BadCommand(
                "the SVG holds nothing to draw".into(),
            ));
        }
        let root = self.doc.root();
        let index = self.doc.children_of(root)?.len();
        let group = self.doc.peek_next_id();
        let mut cmds = vec![Command::AddNode {
            parent: root,
            index,
            node: Box::new(Node::group(name)),
        }];
        for (i, shape) in imported.shapes.into_iter().enumerate() {
            cmds.push(Command::AddNode {
                parent: group,
                index: i,
                node: Box::new(shape),
            });
        }
        self.apply_labeled(Command::Batch(cmds), Some(format!("Place {name}")))?;
        Ok(group)
    }

    /// Make a font available to every text block that names it, for the
    /// rest of the process. Names are the ones the Text panel offers.
    pub fn register_font(name: &str, bytes: Vec<u8>) -> Result<(), EngineError> {
        chitrakar_render::text::register_font(name, bytes).map_err(EngineError::BadCommand)
    }

    pub fn font_names() -> Vec<String> {
        chitrakar_render::text::font_names()
    }

    /// The node the last command — an undo or redo included — touched, if
    /// it still exists, so a selection can follow it.
    pub fn last_touched_node(&self) -> Option<NodeId> {
        self.last_touched.filter(|id| self.doc.node(*id).is_ok())
    }

    /// The colour the page shows at a document point, as straight sRGB
    /// with alpha — what an eyedropper picks up. Nothing off the page.
    /// Composited for that pixel alone rather than read from the frame,
    /// so it is the document's colour whatever the view is doing.
    pub fn color_at(&self, x: f32, y: f32) -> Option<[u8; 4]> {
        let (w, h) = (self.doc.meta.width as f32, self.doc.meta.height as f32);
        if !(x >= 0.0 && y >= 0.0 && x < w && y < h) {
            return None;
        }
        let mut surface = Surface::new(1, 1);
        chitrakar_render::render_region_at(
            &self.doc,
            &mut surface,
            ClipRect {
                x0: 0,
                y0: 0,
                x1: 1,
                y1: 1,
            },
            Transform::translation(-x.floor(), -y.floor()),
        )
        .ok()?;
        Some(surface.get(0, 0).to_srgb8())
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
        // The layer's own box, not what its effects can reach: this is the
        // number the panel shows, the box alignment lines up, and the
        // lines snapping catches on.
        match chitrakar_render::node_visual_bounds(&self.doc, id).ok()? {
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

    /// The document's resolution, which sizes its page in print.
    pub fn dpi(&self) -> f32 {
        self.doc.meta.dpi
    }

    /// Set the resolution. Not a command: like the press profile, it is
    /// document setup, not an edit — nothing on the page moves.
    pub fn set_dpi(&mut self, dpi: f32) -> Result<(), EngineError> {
        if !(dpi.is_finite() && (1.0..=2400.0).contains(&dpi)) {
            return Err(EngineError::BadCommand(format!(
                "resolution {dpi} is out of range (1..=2400 dpi)"
            )));
        }
        self.doc.meta.dpi = dpi;
        Ok(())
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

    /// Serialize to `.chitra` container bytes. The faces the document's
    /// text is set in travel inside it, so it reads the same wherever it
    /// is opened next — all but the bundled face, which every build has.
    pub fn save(&self) -> Result<Vec<u8>, EngineError> {
        let names = self.fonts_used();
        let fonts: Vec<(&str, &[u8])> = names
            .iter()
            .filter_map(|n| chitrakar_render::text::font_bytes(n).map(|b| (n.as_str(), b)))
            .collect();
        chitrakar_codecs::save_chitra_with_fonts(&self.doc, &fonts)
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// The faces this document's text blocks draw with — each one named,
    /// and the oblique twin an italic block is set in — each once, in the
    /// order they sort. Names nothing answers to are listed too: they are
    /// what the file asks for, whether or not this process can oblige.
    pub fn fonts_used(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .doc
            .nodes()
            .flat_map(|(_, n)| match &n.kind {
                NodeKind::Text(spec) => chitrakar_render::text::faces_used(spec),
                _ => Vec::new(),
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Open a `.chitra` container. The loaded document starts with a fresh
    /// history (undo does not cross save boundaries for now). Fonts the
    /// file carries are registered for names this process does not know
    /// yet; a face already loaded keeps precedence, and one that will not
    /// parse is passed over so the document still opens.
    pub fn load(bytes: &[u8]) -> Result<Self, EngineError> {
        let opened = chitrakar_codecs::load_chitra_with_fonts(bytes)
            .map_err(|e| EngineError::BadCommand(e.to_string()))?;
        for (name, bytes) in opened.fonts {
            if !chitrakar_render::text::has_font(&name) {
                let _ = chitrakar_render::text::register_font(&name, bytes);
            }
        }
        let mut session = Self::from_document(opened.doc);
        // An opened file can already hold copies; the flag is only kept
        // in step by the commands that could change it.
        session.note_copies();
        Ok(session)
    }
}

fn parse_command(json: &str) -> Result<Command, EngineError> {
    serde_json::from_str(json).map_err(|e| EngineError::BadCommand(e.to_string()))
}

/// How a copy differs from what it was made from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyStyle {
    /// A duplicate: named "… copy" and nudged clear of the original, so
    /// it is visible rather than hiding exactly behind it.
    Duplicate,
    /// The same, left where it stands — for a drag about to carry it off.
    InPlace,
    /// Neither renamed nor moved: a child inside a subtree being copied,
    /// which belongs exactly where it was.
    Exact,
}

/// A path's handles, one per anchor: an anchor with none gets a pair of
/// zeroes, which is a corner.
fn padded_handles(handles: &[[f32; 4]], n: usize) -> Vec<[f32; 4]> {
    let mut out = handles.to_vec();
    out.resize(n, [0.0; 4]);
    out
}

/// A point on the cubic between two anchors, given their handles.
fn cubic_at(a: [f32; 2], ha: [f32; 4], b: [f32; 2], hb: [f32; 4], t: f32) -> [f32; 2] {
    let (c1, c2) = ([a[0] + ha[2], a[1] + ha[3]], [b[0] + hb[0], b[1] + hb[1]]);
    let u = 1.0 - t;
    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [
        w0 * a[0] + w1 * c1[0] + w2 * c2[0] + w3 * b[0],
        w0 * a[1] + w1 * c1[1] + w2 * c2[1] + w3 * b[1],
    ]
}

/// A layer's look, without its shape — see [`Session::copy_style`].
#[derive(Serialize, serde::Deserialize)]
struct Style {
    fill: Option<chitrakar_color::AuthoredColor>,
    stroke: Option<chitrakar_doc::Stroke>,
    gradient: Option<chitrakar_doc::Gradient>,
    effects: Vec<chitrakar_doc::Effect>,
    opacity: f32,
    blend: chitrakar_doc::BlendMode,
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
    /// Whether that mask is one a brush can work on, which decides
    /// whether the brush paints the layer or the mask over it.
    pub painted_mask: bool,
    pub has_effects: bool,
    /// The layer this one is a live copy of, or 0 when it is not a copy.
    pub copies: u64,
    pub locked: bool,
    /// Confined to the layer below it. The bottom layer of a parent has
    /// nothing under it, so the flag reads false there however it is set.
    pub clipped: bool,
    /// What it does when the frame around it is given a new size.
    pub pinned: chitrakar_doc::Pinning,
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

    /// Two 40x40 squares on a 100x100 page, overlapping in a 20x20 corner.
    fn two_overlapping_squares() -> (Session, Vec<NodeId>) {
        let mut session = Session::new(100, 100, ColorMode::Rgb);
        let a = add_rect(&mut session, "a", 40.0, 40.0);
        session
            .apply(Command::SetTransform {
                id: a,
                transform: Transform::translation(10.0, 10.0),
            })
            .unwrap();
        let b = add_rect(&mut session, "b", 40.0, 40.0);
        session
            .apply(Command::SetTransform {
                id: b,
                transform: Transform::translation(30.0, 30.0),
            })
            .unwrap();
        (session, vec![a, b])
    }

    /// How many pixels the composite covers.
    fn ink(session: &mut Session) -> usize {
        session
            .render()
            .unwrap()
            .pixels
            .iter()
            .filter(|p| p.a > 0.5)
            .count()
    }

    #[test]
    fn booleans_combine_two_shapes_into_one_layer() {
        // Areas are the check: union is both minus the overlap counted
        // twice, intersect is the overlap, subtract is the lower shape
        // less the overlap.
        for (op, expected) in [
            ("union", 40 * 40 * 2 - 20 * 20),
            ("intersect", 20 * 20),
            ("subtract", 40 * 40 - 20 * 20),
        ] {
            let (mut session, ids) = two_overlapping_squares();
            let before = ink(&mut session);
            assert_eq!(before, 40 * 40 * 2 - 20 * 20, "the two squares as drawn");
            let id = session.boolean_nodes(&ids, op).unwrap();
            assert_eq!(
                session
                    .document()
                    .children_of(session.document().root())
                    .unwrap(),
                &[id],
                "{op} left one layer"
            );
            let got = ink(&mut session);
            assert!(
                (got as i32 - expected).abs() < 200,
                "{op}: covered {got}, expected about {expected}"
            );
            assert!(session.undo().unwrap());
            assert_eq!(ink(&mut session), before, "{op} undoes as one step");
            assert_eq!(
                session
                    .document()
                    .children_of(session.document().root())
                    .unwrap()
                    .len(),
                2,
                "{op} brings both shapes back"
            );
        }
    }

    #[test]
    fn subtracting_an_enclosed_shape_punches_a_hole() {
        // The result is one layer whose middle is empty — a compound path,
        // which is the whole reason a path can carry extra rings.
        let mut session = Session::new(100, 100, ColorMode::Rgb);
        let outer = add_rect(&mut session, "outer", 60.0, 60.0);
        session
            .apply(Command::SetTransform {
                id: outer,
                transform: Transform::translation(20.0, 20.0),
            })
            .unwrap();
        let inner = add_rect(&mut session, "inner", 20.0, 20.0);
        session
            .apply(Command::SetTransform {
                id: inner,
                transform: Transform::translation(40.0, 40.0),
            })
            .unwrap();
        session.boolean_nodes(&[outer, inner], "subtract").unwrap();
        let s = session.render().unwrap();
        assert_eq!(s.get(25, 25).a, 1.0, "the ring is filled");
        assert_eq!(s.get(50, 50).a, 0.0, "and the middle is a hole");
        assert_eq!(s.get(10, 10).a, 0.0, "outside is still outside");
    }

    #[test]
    fn combining_refuses_what_it_cannot_answer_for() {
        let (mut session, ids) = two_overlapping_squares();
        assert!(
            session.boolean_nodes(&ids[..1], "union").is_err(),
            "needs two"
        );
        assert!(session.boolean_nodes(&ids, "invert").is_err(), "unknown op");
        // A text layer has no outline to combine.
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::text(
                    "t",
                    chitrakar_doc::TextSpec::new(
                        "hi",
                        12.0,
                        AuthoredColor::Srgb {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        },
                    ),
                )),
            })
            .unwrap();
        let text = *session
            .document()
            .children_of(root)
            .unwrap()
            .last()
            .unwrap();
        assert!(session.boolean_nodes(&[ids[0], text], "union").is_err());
        // And nothing was half-applied.
        assert_eq!(session.document().children_of(root).unwrap().len(), 3);
    }

    #[test]
    fn a_drop_shadow_repaints_the_ground_it_covers() {
        // The shadow reaches outside the layer, so the dirty region has to
        // as well. If it does not, adding or removing one leaves a stain
        // where the cache was never revisited.
        let mut session = Session::new(48, 48, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 16.0, 16.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(12.0, 12.0),
            })
            .unwrap();
        session.render_cached().unwrap();
        let shadow = chitrakar_doc::Effect::DropShadow {
            dx: 7.0,
            dy: 7.0,
            blur: 3.0,
            color: AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            opacity: 1.0,
        };
        session
            .apply(Command::SetEffects {
                id,
                effects: vec![shadow],
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);
        // And taking it away has to clear the ground it covered.
        session
            .apply(Command::SetEffects {
                id,
                effects: Vec::new(),
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);
        assert!(session.undo().unwrap(), "the shadow comes back");
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn cropping_keeps_the_picture_where_it_was() {
        // A crop is a resize plus the shift that cancels it: the page gets
        // smaller, and what was inside the crop rectangle stays put.
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 10.0, 10.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(30.0, 30.0),
            })
            .unwrap();
        let (before, _) = session.render_cached().unwrap();
        assert_eq!(before.get(35, 35).a, 1.0);

        // Crop to (20,20)-(52,52): the rect should land at (10,10).
        session.resize_canvas(32, 32, -20.0, -20.0).unwrap();
        assert_eq!(session.document().meta.width, 32);
        let (after, _) = session.render_cached().unwrap();
        assert_eq!((after.width, after.height), (32, 32), "the cache resized");
        assert_eq!(after.get(15, 15).a, 1.0, "the rect moved with the page");
        assert_eq!(after.get(5, 5).a, 0.0, "and nothing else came with it");
        assert_cache_matches_fresh(&mut session);

        assert!(session.undo().unwrap());
        assert_eq!(session.document().meta.width, 64);
        let (back, _) = session.render_cached().unwrap();
        assert_eq!(back.get(35, 35).a, 1.0, "undo puts the page back");
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn cropping_carries_the_guides_with_the_artwork() {
        use chitrakar_doc::Guide;
        let mut session = Session::new(100, 100, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 10.0, 10.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(40.0, 40.0),
            })
            .unwrap();
        session
            .apply(Command::SetGuides {
                guides: vec![Guide::Vertical(40.0), Guide::Horizontal(50.0)],
            })
            .unwrap();
        session.resize_canvas(60, 60, -20.0, -20.0).unwrap();
        assert_eq!(
            session.document().guides(),
            &[Guide::Vertical(20.0), Guide::Horizontal(30.0)],
            "a guide placed against the rect's edge is still against it"
        );
        assert!(session.undo().unwrap());
        assert_eq!(
            session.document().guides(),
            &[Guide::Vertical(40.0), Guide::Horizontal(50.0)],
            "and undo puts them back"
        );
    }

    #[test]
    fn a_blur_inside_a_scaled_group_pads_by_what_it_actually_reaches() {
        // A filter's sigma is written in the space it sits in, so a group
        // scaled up multiplies its reach. Padding a region render by the
        // unscaled figure leaves a ring of stale pixels at the edge, which
        // comparing against a full render catches.
        let mut session = Session::new(80, 80, ColorMode::Rgb);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: filled_rect("under", 40.0, 40.0),
            })
            .unwrap();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::group("g")),
            })
            .unwrap();
        let group = *session
            .document()
            .children_of(root)
            .unwrap()
            .last()
            .unwrap();
        session
            .apply(Command::SetTransform {
                id: group,
                transform: Transform {
                    a: 3.0,
                    d: 3.0,
                    ..Default::default()
                },
            })
            .unwrap();
        session
            .apply(Command::AddNode {
                parent: group,
                index: 0,
                node: Box::new(Node::filter(
                    "blur",
                    chitrakar_doc::Filter::GaussianBlur { sigma: 4.0 },
                )),
            })
            .unwrap();
        session.render_cached().unwrap();
        let rect = *session
            .document()
            .children_of(root)
            .unwrap()
            .first()
            .unwrap();
        session
            .apply(Command::SetTransform {
                id: rect,
                transform: Transform::translation(6.0, 6.0),
            })
            .unwrap();
        assert_cache_matches_fresh(&mut session);
    }

    #[test]
    fn undo_says_which_layer_it_brought_back() {
        // Undoing a delete restores the layer; what it hands back is that
        // layer's id, so a selection can follow it instead of pointing at
        // nothing. Undoing an add removes the layer again, so nothing.
        let mut session = Session::new(32, 32, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 8.0, 8.0);
        assert_eq!(session.last_touched_node(), Some(id), "the add touched it");
        session.apply(Command::RemoveNode { id }).unwrap();
        assert_eq!(
            session.last_touched_node(),
            None,
            "gone, so nothing to point at"
        );
        assert!(session.undo().unwrap());
        assert_eq!(
            session.last_touched_node(),
            Some(id),
            "undo brought it back"
        );
        assert!(session.undo().unwrap(), "undo the add itself");
        assert_eq!(session.last_touched_node(), None);
        assert!(session.redo().unwrap());
        assert_eq!(
            session.last_touched_node(),
            Some(id),
            "and redo restores it"
        );
    }

    #[test]
    fn an_adjustment_scoped_to_a_layer_leaves_the_rest_alone() {
        // Two rects side by side; darken one. The group's isolation is
        // what confines the adjustment, and one undo takes the whole
        // arrangement away.
        let mut session = Session::new(64, 32, ColorMode::Rgb);
        let left = add_rect(&mut session, "left", 20.0, 20.0);
        let right = add_rect(&mut session, "right", 20.0, 20.0);
        session
            .apply(Command::SetTransform {
                id: right,
                transform: Transform::translation(40.0, 0.0),
            })
            .unwrap();
        let before = session.render().unwrap();
        let group = session
            .adjust_node(
                right,
                Node::adjustment(
                    "Exposure",
                    chitrakar_doc::Adjustment::Exposure { stops: -3.0 },
                ),
            )
            .unwrap();
        let root = session.document().root();
        assert_eq!(
            session.document().children_of(root).unwrap(),
            &[left, group],
            "the layer moved into a group where it stood"
        );
        assert_eq!(session.document().children_of(group).unwrap().len(), 2);
        let after = session.render().unwrap();
        assert!(
            after.get(50, 10).r < before.get(50, 10).r * 0.3,
            "the adjusted layer darkened: {} -> {}",
            before.get(50, 10).r,
            after.get(50, 10).r
        );
        assert_eq!(after.get(10, 10), before.get(10, 10), "the other did not");
        assert!(session.undo().unwrap());
        assert_eq!(
            session.document().children_of(root).unwrap(),
            &[left, right]
        );
        assert_eq!(session.render().unwrap().to_srgb8(), before.to_srgb8());
        // Only adjustments and filters can be scoped.
        assert!(session
            .adjust_node(right, *filled_rect("no", 1.0, 1.0))
            .is_err());
    }

    #[test]
    fn exporting_at_a_scale_and_of_a_region_renders_just_that() {
        let mut session = Session::new(100, 60, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 20.0, 20.0);
        session
            .apply(Command::SetTransform {
                id,
                transform: Transform::translation(30.0, 10.0),
            })
            .unwrap();
        // Twice the size: twice the pixels, and the rect's edge twice as
        // far in — re-solved, so it is a hard edge there rather than a
        // blurred one.
        let two = session.render_scaled(2.0, None).unwrap();
        assert_eq!((two.width, two.height), (200, 120));
        assert_eq!(two.get(59, 30).a, 0.0, "just outside the rect at 2x");
        assert_eq!(two.get(61, 30).a, 1.0, "just inside it");
        assert_eq!(two.get(99, 30).a, 1.0);
        assert_eq!(two.get(101, 30).a, 0.0);
        // A region: only what is inside it, at its own size, with the
        // document's origin no longer at the corner.
        let part = session
            .render_scaled(1.0, Some([25.0, 5.0, 30.0, 30.0]))
            .unwrap();
        assert_eq!((part.width, part.height), (30, 30));
        assert_eq!(part.get(2, 10).a, 0.0, "the five pixels before the rect");
        assert_eq!(part.get(10, 10).a, 1.0, "the rect, shifted by the region");
        assert_eq!(
            part.get(24, 10).a,
            1.0,
            "its far edge, five short of the region's"
        );
        assert_eq!(part.get(27, 10).a, 0.0);
        // Nothing outside the page is painted even when the region hangs
        // off it, and an empty region is refused.
        let off = session
            .render_scaled(1.0, Some([-10.0, -10.0, 20.0, 20.0]))
            .unwrap();
        assert_eq!(off.get(2, 2).a, 0.0);
        assert!(session
            .render_scaled(1.0, Some([0.0, 0.0, 0.0, 5.0]))
            .is_err());
        // The PNG carries the scaled size.
        let png = session.render_png_at(3.0, None).unwrap();
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        assert_eq!(w, 300);
    }

    #[test]
    fn a_zero_sized_canvas_is_refused() {
        let mut session = Session::new(16, 16, ColorMode::Rgb);
        assert!(session.resize_canvas(0, 10, 0.0, 0.0).is_err());
        assert_eq!(session.document().meta.width, 16, "and changes nothing");
    }

    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn many_layers_probe() {
        // A document with hundreds of small layers: what one drag frame
        // costs, and what the panel's per-refresh reads cost.
        let mut session = Session::new(1600, 1000, ColorMode::Rgb);
        let mut ids = Vec::new();
        for i in 0..300 {
            let id = add_rect(&mut session, &format!("r{i}"), 40.0, 30.0);
            session
                .apply(Command::SetTransform {
                    id,
                    transform: Transform::translation(
                        (i % 30) as f32 * 52.0,
                        (i / 30) as f32 * 95.0,
                    ),
                })
                .unwrap();
            ids.push(id);
        }
        session.set_viewport(0.8, 0.0, 0.0, 1400, 900);
        session.render_cached().unwrap();
        let t0 = std::time::Instant::now();
        for k in 0..20 {
            session
                .preview(Command::SetTransform {
                    id: ids[150],
                    transform: Transform::translation(300.0 + k as f32, 400.0),
                })
                .unwrap();
            session.render_cached().unwrap();
        }
        println!("300 layers, drag frame: {:?}", t0.elapsed() / 20);
        let t0 = std::time::Instant::now();
        for _ in 0..20 {
            let _ = session.layers();
            for id in &ids {
                let _ = session.bounds_of(*id);
            }
        }
        println!(
            "300 layers, panel refresh (layers + every bounds_of): {:?}",
            t0.elapsed() / 20
        );
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

    #[test]
    fn a_flip_mirrors_the_selection_about_its_own_box() {
        let mut session = Session::new(100, 100, ColorMode::Rgb);
        let rect = add_rect(&mut session, "r", 20.0, 10.0);
        session
            .apply(Command::SetTransform {
                id: rect,
                transform: Transform::translation(10.0, 10.0),
            })
            .unwrap();
        let other = add_rect(&mut session, "o", 10.0, 30.0);
        session
            .apply(Command::SetTransform {
                id: other,
                transform: Transform::translation(60.0, 40.0),
            })
            .unwrap();
        let before = session.render().unwrap();

        // Alone, a layer flips in place: same box, one history entry.
        session.flip_nodes(&[rect], true).unwrap();
        let b = session.bounds_of(rect).unwrap();
        assert!(
            (b[0] - 10.0).abs() < 1e-3 && (b[2] - 20.0).abs() < 1e-3,
            "{b:?}"
        );
        assert_eq!(session.document().node(rect).unwrap().transform.a, -1.0);
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Flip horizontal")
        );

        // Together, they mirror about the union: the rect (10..30) lands
        // at the far side of 10..70, the tall one at the near side.
        session.flip_nodes(&[rect, other], true).unwrap();
        let (r, o) = (
            session.bounds_of(rect).unwrap(),
            session.bounds_of(other).unwrap(),
        );
        assert!(
            (r[0] - 50.0).abs() < 1e-3 && (r[2] - 20.0).abs() < 1e-3,
            "rect {r:?}"
        );
        assert!(
            (o[0] - 10.0).abs() < 1e-3 && (o[2] - 10.0).abs() < 1e-3,
            "other {o:?}"
        );
        assert!(
            (r[1] - 10.0).abs() < 1e-3 && (o[1] - 40.0).abs() < 1e-3,
            "rows untouched"
        );
        session.flip_nodes(&[rect, other], false).unwrap();
        let (r, o) = (
            session.bounds_of(rect).unwrap(),
            session.bounds_of(other).unwrap(),
        );
        assert!(
            (r[1] - 60.0).abs() < 1e-3 && (o[1] - 10.0).abs() < 1e-3,
            "vertical: {r:?} {o:?}"
        );

        // Three undos put every pixel back.
        for _ in 0..3 {
            session.undo().unwrap();
        }
        assert_eq!(session.render().unwrap().pixels, before.pixels);

        // Inside a moved, scaled group the mirror still lands where the
        // document sees it: the box stays the box.
        let group = session.group_nodes(&[rect], "g").unwrap();
        session
            .apply(Command::SetTransform {
                id: group,
                transform: Transform {
                    a: 2.0,
                    d: 2.0,
                    e: 5.0,
                    f: 5.0,
                    ..Default::default()
                },
            })
            .unwrap();
        let boxed = session.bounds_of(rect).unwrap();
        session.flip_nodes(&[rect], false).unwrap();
        let after = session.bounds_of(rect).unwrap();
        assert!(
            boxed
                .iter()
                .zip(after.iter())
                .all(|(a, b)| (a - b).abs() < 1e-3),
            "{boxed:?} -> {after:?}"
        );
    }

    #[test]
    fn text_goes_along_a_shape_in_the_shapes_own_place() {
        let mut session = Session::new(200, 200, ColorMode::Rgb);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(text_in("")),
            })
            .unwrap();
        let text = session.document().children_of(root).unwrap()[0];
        session
            .apply(Command::SetTransform {
                id: text,
                transform: Transform::translation(20.0, 20.0),
            })
            .unwrap();
        // A circle over on the right; the text should end up around it,
        // wherever the block itself sits.
        let circle = add_rect(&mut session, "c", 1.0, 1.0);
        session
            .apply(Command::SetKind {
                id: circle,
                kind: Box::new(NodeKind::Vector {
                    shape: VectorShape::Ellipse { rx: 30.0, ry: 30.0 },
                    fill: None,
                    stroke: None,
                    gradient: None,
                }),
            })
            .unwrap();
        session
            .apply(Command::SetTransform {
                id: circle,
                transform: Transform::translation(120.0, 100.0),
            })
            .unwrap();
        let before = session.bounds_of(text).unwrap();
        session.text_along(text, circle).unwrap();
        let after = session.bounds_of(text).unwrap();
        assert!(
            after[0] > 100.0 && after[0] + after[2] < 200.0 && after[1] > 50.0,
            "the text now sits around the circle at (120..180, 100..160): {before:?} -> {after:?}"
        );
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Text on path")
        );
        let NodeKind::Text(spec) = &session.document().node(text).unwrap().kind else {
            unreachable!()
        };
        let Some(VectorShape::Path { points, closed, .. }) = &spec.along else {
            panic!("a path was copied in")
        };
        assert!(*closed && points.len() >= 16);
        // The ellipse's own origin is its top-left, so its centre is at
        // (150, 130) on the page — (130, 110) in the block's own space.
        let (cx, cy) = points.iter().fold((0.0, 0.0), |(x, y), p| {
            (
                x + p[0] / points.len() as f32,
                y + p[1] / points.len() as f32,
            )
        });
        assert!(
            (cx - 130.0).abs() < 1.0 && (cy - 110.0).abs() < 1.0,
            "{cx} {cy}"
        );
        assert!(session.undo().unwrap());
        assert_eq!(session.bounds_of(text).unwrap(), before);
        assert!(
            session.text_along(circle, text).is_err(),
            "a shape is not text"
        );
    }

    #[test]
    fn an_svg_is_placed_as_a_group_of_shapes_in_one_step() {
        let mut session = Session::new(120, 100, ColorMode::Rgb);
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="100">
            <rect x="10" y="10" width="40" height="30" fill="#ff0000"/>
            <circle cx="80" cy="25" r="15" fill="#0000ff"/></svg>"##;
        let group = session.place_svg(svg, "mark.svg").unwrap();
        let layers = session.layers();
        assert_eq!(layers.len(), 3);
        assert!(layers
            .iter()
            .any(|l| l.id == group.0 && l.kind == "group" && l.name == "mark.svg"));
        assert_eq!(layers.iter().filter(|l| l.parent == group.0).count(), 2);
        let b = session.bounds_of(group).unwrap();
        assert!(
            (b[0] - 10.0).abs() < 0.5 && (b[0] + b[2] - 95.0).abs() < 0.5,
            "{b:?}"
        );
        assert_eq!(
            session.render().unwrap().get(30, 25).to_srgb8(),
            [255, 0, 0, 255]
        );
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Place mark.svg")
        );
        assert!(session.undo().unwrap());
        assert!(
            session.layers().is_empty(),
            "one undo takes the whole group"
        );
        assert!(session
            .place_svg(b"<svg xmlns='http://www.w3.org/2000/svg'/>", "empty.svg")
            .is_err());
    }

    #[test]
    fn opacity_and_blend_reach_every_picked_layer_at_once() {
        let mut session = Session::new(60, 60, ColorMode::Rgb);
        let a = add_rect(&mut session, "a", 10.0, 10.0);
        let b = add_rect(&mut session, "b", 10.0, 10.0);
        session.set_opacity_of(&[a, b], 0.25).unwrap();
        assert!(session
            .layers()
            .iter()
            .all(|l| (l.opacity - 0.25).abs() < 1e-6));
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Opacity of 2 layers")
        );
        session
            .set_blend_of(&[a, b], chitrakar_doc::BlendMode::Multiply)
            .unwrap();
        assert!(session
            .layers()
            .iter()
            .all(|l| l.blend == chitrakar_doc::BlendMode::Multiply));
        assert!(session.undo().unwrap());
        assert!(session
            .layers()
            .iter()
            .all(|l| l.blend == chitrakar_doc::BlendMode::Normal));
        assert!(session.undo().unwrap());
        assert!(
            session.layers().iter().all(|l| l.opacity == 1.0),
            "both back at once"
        );
        // One layer says so by name.
        session.set_opacity_of(&[a], 0.5).unwrap();
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Opacity of a")
        );
        assert!(session.set_opacity_of(&[], 0.5).is_err());
    }

    #[test]
    fn several_layers_duplicate_in_one_step() {
        let mut session = Session::new(60, 60, ColorMode::Rgb);
        let a = add_rect(&mut session, "a", 10.0, 10.0);
        let b = add_rect(&mut session, "b", 20.0, 20.0);
        let copies = session.duplicate_nodes(&[a, b], false).unwrap();
        assert_eq!(copies.len(), 2);
        assert_eq!(session.layers().len(), 4);
        // Each copy sits directly above what it was copied from, and
        // carries its size.
        for (from, copy) in [a, b].iter().zip(&copies) {
            let (f, c) = (
                session
                    .layers()
                    .iter()
                    .find(|l| l.id == from.0)
                    .unwrap()
                    .index,
                session
                    .layers()
                    .iter()
                    .find(|l| l.id == copy.0)
                    .unwrap()
                    .index,
            );
            assert_eq!(c, f + 1, "the copy is the layer above");
            assert_eq!(session.bounds_of(*copy), session.bounds_of(*from));
        }
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Duplicate 2 layers")
        );
        assert!(session.undo().unwrap());
        assert_eq!(session.layers().len(), 2, "one undo takes both copies");
        assert!(session.duplicate_nodes(&[], false).is_err());
        // Asked to, it nudges each copy clear of its original instead.
        let nudged = session.duplicate_nodes(&[a], true).unwrap()[0];
        let (from, copy) = (
            session.bounds_of(a).unwrap(),
            session.bounds_of(nudged).unwrap(),
        );
        assert!(
            copy[0] > from[0] && copy[1] > from[1],
            "{from:?} -> {copy:?}"
        );
    }

    #[test]
    fn the_colour_at_a_point_is_what_the_page_shows_there() {
        let mut session = Session::new(40, 40, ColorMode::Rgb);
        let id = add_rect(&mut session, "r", 20.0, 20.0);
        session
            .apply(Command::SetKind {
                id,
                kind: Box::new(NodeKind::Vector {
                    shape: VectorShape::Rect {
                        width: 20.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    fill: Some(AuthoredColor::Srgb {
                        r: 1.0,
                        g: 0.5,
                        b: 0.0,
                        a: 1.0,
                    }),
                    stroke: None,
                    gradient: None,
                }),
            })
            .unwrap();
        let picked = session.color_at(10.0, 10.0).unwrap();
        assert_eq!(picked, [255, 128, 0, 255], "the fill, as it is shown");
        assert_eq!(
            session.color_at(30.0, 30.0).unwrap()[3],
            0,
            "bare page is clear"
        );
        assert_eq!(session.color_at(-1.0, 5.0), None, "nothing off the page");
        assert_eq!(session.color_at(40.0, 5.0), None);
        // Half opacity over nothing reads as half-covered, not as the fill.
        session
            .apply(Command::SetOpacity { id, opacity: 0.5 })
            .unwrap();
        let faded = session.color_at(10.0, 10.0).unwrap();
        assert!(
            (faded[3] as i32 - 128).abs() <= 1 && faded[0] == 255,
            "{faded:?}"
        );
        // And it is the document's colour, not the view's: zoomed out, the
        // same point reads the same.
        session.set_viewport(0.25, 0.0, 0.0, 10, 10);
        assert_eq!(session.color_at(10.0, 10.0).unwrap(), faded);
    }

    /// Changing the original repaints the copies too: they draw what it
    /// draws, and they are somewhere else on the page.
    #[test]
    fn changing_the_original_repaints_its_copies() {
        let mut session = Session::new(120, 60, ColorMode::Rgb);
        let master = add_rect(&mut session, "master", 20.0, 20.0);
        let copy = session.make_instance(master).unwrap();
        session
            .apply(Command::SetTransform {
                id: copy,
                transform: Transform::translation(80.0, 0.0),
            })
            .unwrap();
        // Draw once so the cache holds the page as it is.
        session.render_cached().unwrap();
        let before = session.render().unwrap().get(85, 5).to_srgb8();

        session
            .apply(Command::SetKind {
                id: master,
                kind: Box::new(filled_rect("master", 40.0, 40.0).kind),
            })
            .unwrap();
        let (cached, dirty) = session.render_cached().unwrap();
        assert!(dirty.is_some(), "something was repainted");
        assert_eq!(
            cached.get(85, 25).to_srgb8()[3],
            255,
            "the copy grew with the original in the cached frame"
        );
        let _ = before;

        // Take the copy away and the original is on its own again; put
        // it back and it follows once more. The dirty region asks a kept
        // flag whether there are copies at all, and this is what keeps
        // that flag honest.
        session.apply(Command::RemoveNode { id: copy }).unwrap();
        session.render_cached().unwrap();
        session
            .apply(Command::SetKind {
                id: master,
                kind: Box::new(filled_rect("master", 10.0, 10.0).kind),
            })
            .unwrap();
        let (alone, _) = session.render_cached().unwrap();
        assert_eq!(alone.get(85, 5).a, 0.0, "nothing is copying it now");
        // Undo brings the copy back, and with it the flag.
        session.undo().unwrap();
        session.undo().unwrap();
        session
            .apply(Command::SetKind {
                id: master,
                kind: Box::new(filled_rect("master", 30.0, 30.0).kind),
            })
            .unwrap();
        let (again, _) = session.render_cached().unwrap();
        assert_eq!(
            again.get(85, 25).to_srgb8()[3],
            255,
            "and with the copy back it follows again"
        );
    }

    /// A frame given a new size moves what is in it by how each layer is
    /// pinned: the start edge is followed, the end edge is kept away
    /// from, the middle is held, and a stretched layer takes up the
    /// difference.
    #[test]
    fn what_a_frame_holds_moves_by_how_it_is_pinned() {
        use chitrakar_doc::{Pin, Pinning};
        let mut session = Session::new(400, 400, ColorMode::Rgb);
        let board = session
            .add_artboard("Artboard 1", 50.0, 50.0, 200.0, 100.0, None)
            .unwrap();
        // Four layers across the frame, one per answer.
        let put = |session: &mut Session, name: &str, x: f32, w: f32, pin: Pin| {
            let index = session.child_count(board);
            session
                .apply(Command::AddNode {
                    parent: board,
                    index,
                    node: filled_rect(name, w, 20.0),
                })
                .unwrap();
            let id = session.document().children_of(board).unwrap()[index];
            session
                .apply(Command::SetTransform {
                    id,
                    transform: Transform::translation(x, 10.0),
                })
                .unwrap();
            session
                .apply(Command::SetPinning {
                    id,
                    pinned: Pinning {
                        x: pin,
                        y: Pin::Start,
                    },
                })
                .unwrap();
            id
        };
        let start = put(&mut session, "start", 10.0, 20.0, Pin::Start);
        let end = put(&mut session, "end", 170.0, 20.0, Pin::End);
        let middle = put(&mut session, "middle", 90.0, 20.0, Pin::Middle);
        let wide = put(&mut session, "wide", 10.0, 180.0, Pin::Stretch);

        // The frame goes from 200 wide to 300, its origin staying put.
        let cmd = session
            .artboard_resize(board, 300.0, 100.0, 0.0, 0.0)
            .unwrap();
        session.apply_json(&cmd).unwrap();

        let box_ = |session: &Session, id| session.bounds_of(id).unwrap();
        assert_eq!(
            box_(&session, start)[0],
            60.0,
            "the start-pinned layer did not move"
        );
        assert_eq!(
            box_(&session, end)[0],
            // The frame sits at 50 on the page, so a local 270 reads 320.
            320.0,
            "the end-pinned one kept 10 from the right"
        );
        assert_eq!(
            box_(&session, middle)[0],
            190.0,
            "the middle-pinned one kept the middle"
        );
        assert_eq!(
            box_(&session, wide)[0],
            60.0,
            "the stretched one still starts where it did"
        );
        assert_eq!(
            box_(&session, wide)[2],
            280.0,
            "and took up the whole difference"
        );
        assert_eq!(
            box_(&session, start)[2],
            20.0,
            "an unstretched layer is the size it was"
        );

        // Undo puts the whole resize back in one step.
        session.undo().unwrap();
        assert_eq!(
            (box_(&session, end)[0], box_(&session, wide)[2]),
            (170.0 + 50.0, 180.0)
        );
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Pin wide"),
            "one undo took the whole resize, so it was one entry"
        );
    }

    /// A histogram describes the picture on the page, not the hole
    /// around it, and an adjustment layer's asks about what is under it
    /// rather than about the finished page.
    #[test]
    fn a_histogram_counts_the_picture_and_an_adjustment_sees_what_is_under_it() {
        let mut session = Session::new(60, 60, ColorMode::Rgb);
        let rect = add_rect(&mut session, "rect", 30.0, 30.0);
        // The rect covers a quarter of the page; the rest is bare.
        let h = session.histogram(None).unwrap();
        assert_eq!(h.len(), 1024);
        let counted: u32 = h[768..1024].iter().sum();
        assert!(counted > 0, "something was counted");
        assert_eq!(
            counted,
            h[0..256].iter().sum::<u32>(),
            "every channel counts the same pixels"
        );
        // Only the covered quarter: a transparent page is not black.
        assert_eq!(h[768], 0, "and the bare page is not counted as black");

        // Under a curve that drives everything to white, the page reads
        // white — but the curve's own histogram is of what is beneath it,
        // so it does not.
        let before = session.histogram(None).unwrap();
        let root = session.document().root();
        let index = session.document().children_of(root).unwrap().len();
        session
            .apply(Command::AddNode {
                parent: root,
                index,
                node: Box::new(chitrakar_doc::Node::adjustment(
                    "curve",
                    chitrakar_doc::Adjustment::Curves {
                        points: vec![[0.0, 1.0], [1.0, 1.0]],
                        red: Vec::new(),
                        green: Vec::new(),
                        blue: Vec::new(),
                    },
                )),
            })
            .unwrap();
        let white = session.document().children_of(root).unwrap()[index];
        let after = session.histogram(None).unwrap();
        assert_ne!(after, before, "the page changed under the curve");
        assert_eq!(
            session.histogram(Some(white)).unwrap(),
            before,
            "and the curve is shown what it is given, not what it makes"
        );
        let _ = rect;
    }

    /// Dropping a layer into a frame that sits away from the origin
    /// leaves the layer exactly where it was on the page: the new
    /// parent's space is taken back out of the layer's own transform.
    #[test]
    fn a_layer_dropped_into_a_frame_stays_where_it_was() {
        let mut session = Session::new(200, 200, ColorMode::Rgb);
        let rect = add_rect(&mut session, "rect", 20.0, 20.0);
        session
            .apply(Command::SetTransform {
                id: rect,
                transform: Transform::translation(120.0, 90.0),
            })
            .unwrap();
        let board = session
            .add_artboard("Artboard 1", 100.0, 80.0, 60.0, 60.0, None)
            .unwrap();
        let before = session.bounds_of(rect).unwrap();
        session.reparent(rect, board, 0).unwrap();
        let after = session.bounds_of(rect).unwrap();
        for (a, b) in before.iter().zip(&after) {
            assert!((a - b).abs() < 1e-3, "{before:?} -> {after:?}");
        }
        assert_eq!(session.child_count(board), 1);
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Move rect"),
            "and it is one entry in history"
        );
        session.undo().unwrap();
        assert_eq!(session.child_count(board), 0);
        let back = session.bounds_of(rect).unwrap();
        for (a, b) in before.iter().zip(&back) {
            assert!((a - b).abs() < 1e-3, "one undo puts it back: {back:?}");
        }
    }

    /// A frame takes what is drawn inside it and exports on its own.
    #[test]
    fn a_frame_takes_what_is_drawn_in_it_and_exports_at_its_own_size() {
        let mut session = Session::new(200, 200, ColorMode::Rgb);
        let board = session
            .add_artboard("Artboard 1", 40.0, 30.0, 50.0, 60.0, None)
            .unwrap();
        assert_eq!(session.frame_at(60.0, 50.0), Some(board));
        assert_eq!(session.frame_at(10.0, 10.0), None, "off the frame");
        let inside = session.point_inside(board, 60.0, 50.0).unwrap();
        assert_eq!(inside, [20.0, 20.0], "the frame's own coordinates");
        assert!(
            session.layers().iter().any(|l| l.kind == "artboard"),
            "the panel calls it what it is"
        );
        let png = session.artboard_png(board, 1.0).unwrap();
        // PNG's IHDR carries the size in the two big-endian words at 16.
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!((w, h), (50, 60));
        assert!(
            session.artboard_png(NodeId(0), 1.0).is_err(),
            "and nothing else is a frame"
        );
    }

    #[test]
    fn a_locked_layer_is_not_picked_on_the_canvas() {
        let mut session = Session::new(60, 60, ColorMode::Rgb);
        let under = add_rect(&mut session, "under", 60.0, 60.0);
        let over = add_rect(&mut session, "over", 30.0, 30.0);
        assert_eq!(session.hit_test(10.0, 10.0), Some(over));
        session
            .apply(Command::SetLocked {
                id: over,
                locked: true,
            })
            .unwrap();
        assert_eq!(
            session.hit_test(10.0, 10.0),
            Some(under),
            "the pick falls through a locked layer"
        );
        assert!(session.layers().iter().any(|l| l.id == over.0 && l.locked));
        assert_eq!(
            session.history_labels().0.last().map(String::as_str),
            Some("Lock over")
        );
        assert_eq!(session.render().unwrap().get(10, 10).a, 1.0, "still drawn");
        // A locked group hides its contents from the pick too.
        let group = session.group_nodes(&[under], "g").unwrap();
        session
            .apply(Command::SetLocked {
                id: group,
                locked: true,
            })
            .unwrap();
        assert_eq!(session.hit_test(50.0, 50.0), None);
        assert!(session.undo().unwrap() && session.undo().unwrap() && session.undo().unwrap());
        assert_eq!(
            session.hit_test(10.0, 10.0),
            Some(over),
            "undone, it is picked again"
        );
        let bytes = session.save().unwrap();
        assert!(Session::load(&bytes).is_ok());
    }

    #[test]
    fn the_resolution_is_document_setup_that_saves_with_it() {
        let mut session = Session::new(300, 300, ColorMode::Rgb);
        assert_eq!(session.dpi(), 72.0);
        session.set_dpi(300.0).unwrap();
        assert!(session.set_dpi(0.0).is_err() && session.set_dpi(f32::NAN).is_err());
        assert_eq!(session.dpi(), 300.0, "a bad value changes nothing");
        assert!(session.history_labels().0.is_empty(), "not an edit");
        let bytes = session.save().unwrap();
        assert_eq!(Session::load(&bytes).unwrap().dpi(), 300.0);
        // A 300-dpi page is an inch across in the PDF: 72 points.
        let pdf = String::from_utf8_lossy(&session.export_pdf().unwrap()).to_string();
        assert!(
            pdf.contains("/MediaBox [0 0 72.000 72.000]"),
            "{}",
            &pdf[..300]
        );
    }

    fn text_in(font: &str) -> Node {
        let mut spec = chitrakar_doc::TextSpec::new(
            "Carried",
            24.0,
            AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        );
        spec.font = font.to_string();
        Node::text("t", spec)
    }

    #[test]
    fn the_fonts_text_is_set_in_travel_inside_the_chitra() {
        const BOLD: &[u8] = include_bytes!("../../../app/public/fonts/DejaVuSans-Bold.ttf");
        const OBLIQUE: &[u8] =
            include_bytes!("../../../app/public/fonts/DejaVuSansMono-Oblique.ttf");
        Session::register_font("Carried Face", BOLD.to_vec()).unwrap();
        Session::register_font("Carried Face Oblique", OBLIQUE.to_vec()).unwrap();
        let mut session = Session::new(64, 64, ColorMode::Rgb);
        let root = session.document().root();
        for (i, face) in [
            "Carried Face",
            "DejaVu Sans",
            "Nobody Has This",
            "Carried Face",
        ]
        .iter()
        .enumerate()
        {
            session
                .apply(Command::AddNode {
                    parent: root,
                    index: i,
                    node: Box::new(text_in(face)),
                })
                .unwrap();
        }
        assert_eq!(
            session.fonts_used(),
            ["Carried Face", "DejaVu Sans", "Nobody Has This"]
        );
        // An italic block in the face draws with its oblique twin, so the
        // twin is used — and carried — too.
        let mut italic = text_in("Carried Face");
        if let NodeKind::Text(spec) = &mut italic.kind {
            spec.italic = true;
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 4,
                node: Box::new(italic),
            })
            .unwrap();
        assert_eq!(
            session.fonts_used(),
            [
                "Carried Face",
                "Carried Face Oblique",
                "DejaVu Sans",
                "Nobody Has This"
            ]
        );

        let bytes = session.save().unwrap();
        let opened = chitrakar_codecs::load_chitra_with_fonts(&bytes).unwrap();
        let names: Vec<&str> = opened.fonts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["Carried Face", "Carried Face Oblique"],
            "only faces this process holds are carried: not the bundled one, not one it never saw"
        );
        assert_eq!(opened.fonts[0].1, BOLD, "and they are carried whole");
        assert_eq!(opened.fonts[1].1, OBLIQUE);
        let without = chitrakar_codecs::save_chitra(session.document()).unwrap();
        assert!(
            bytes.len() > without.len() + 100_000,
            "the file grew by the (deflated) face: {} over {} bytes",
            bytes.len(),
            without.len()
        );
    }

    #[test]
    fn opening_a_chitra_registers_the_fonts_it_carries() {
        const SERIF: &[u8] = include_bytes!("../../../app/public/fonts/DejaVuSerif.ttf");
        let name = "Only In The File";
        assert!(!Session::font_names().iter().any(|n| n == name));
        let mut doc = chitrakar_doc::Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(text_in(name)),
        })
        .unwrap();
        let bytes = chitrakar_codecs::save_chitra_with_fonts(
            &doc,
            &[(name, SERIF), ("Broken Face", b"not a font")],
        )
        .unwrap();

        let session = Session::load(&bytes).unwrap();
        assert!(
            Session::font_names().iter().any(|n| n == name),
            "the carried face is offered by name after the open"
        );
        assert!(
            !Session::font_names().iter().any(|n| n == "Broken Face"),
            "a face that will not parse is passed over, and the document still opened"
        );
        // The text now renders in the carried serif rather than the
        // bundled sans: set the same text in the bundled face and the ink
        // lands differently.
        let carried = session.render().unwrap().pixels;
        let mut plain = Session::new(64, 64, ColorMode::Rgb);
        let root = plain.document().root();
        plain
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(text_in("")),
            })
            .unwrap();
        assert_ne!(
            carried,
            plain.render().unwrap().pixels,
            "the carried face is the one drawn"
        );
    }
}

#[cfg(test)]
mod save_probe {
    use super::*;
    use chitrakar_color::ColorMode;

    /// However many points a brush stroke gathers as it is drawn, the
    /// whole of it is one entry in history — and one undo takes it off.
    #[test]
    fn a_brush_stroke_is_one_entry_in_history() {
        let mut session = Session::new(60, 60, ColorMode::Rgb);
        let layer = session.add_paint_layer("brush").unwrap();
        let before = session.history_labels().0.len();
        let blue = r#"{"Srgb":{"r":0.0,"g":0.0,"b":1.0,"a":1.0}}"#;
        let color: chitrakar_color::AuthoredColor = serde_json::from_str(blue).unwrap();
        session
            .paint_begin(layer, 10.0, 30.0, 4.0, color, 0.3, false, false)
            .unwrap();
        for x in 1..=20 {
            session.paint_extend(10.0 + x as f32, 30.0, 4.0).unwrap();
        }
        assert!(session.is_painting());
        assert!(session.commit_preview());
        assert!(!session.is_painting());
        assert_eq!(session.stroke_count(layer, false).unwrap(), 1);
        assert_eq!(
            session.history_labels().0.len(),
            before + 1,
            "one entry, not twenty"
        );
        assert!(
            session.render().unwrap().get(20, 30).a > 0.9,
            "and it is painted"
        );

        session.undo().unwrap();
        assert_eq!(session.stroke_count(layer, false).unwrap(), 0);
        assert_eq!(session.render().unwrap().get(20, 30).a, 0.0);
        session.redo().unwrap();
        assert_eq!(session.stroke_count(layer, false).unwrap(), 1);

        // A stroke abandoned mid-gesture leaves nothing behind.
        session
            .paint_begin(layer, 40.0, 40.0, 4.0, color, 0.3, false, false)
            .unwrap();
        session.paint_extend(45.0, 40.0, 4.0).unwrap();
        assert!(session.cancel_preview().unwrap());
        assert_eq!(session.stroke_count(layer, false).unwrap(), 1);
        assert!(!session.is_painting());
    }

    /// A layer's look travels to other layers without its shape: they
    /// keep what they are and take what they are painted with.
    #[test]
    fn a_style_travels_without_the_shape_it_came_from() {
        let mut session = Session::new(80, 80, ColorMode::Rgb);
        let root = session.document().root();
        let red = chitrakar_color::AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let mut source = chitrakar_doc::Node::vector(
            "source",
            chitrakar_doc::VectorShape::Rect {
                width: 20.0,
                height: 20.0,
                radius: 0.0,
            },
        );
        if let chitrakar_doc::NodeKind::Vector { fill, .. } = &mut source.kind {
            *fill = Some(red);
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(source),
            })
            .unwrap();
        let from = session.document().children_of(root).unwrap()[0];
        session
            .apply(Command::SetEffects {
                id: from,
                effects: vec![chitrakar_doc::Effect::Outline {
                    width: 2.0,
                    color: red,
                    opacity: 1.0,
                }],
            })
            .unwrap();
        session
            .apply(Command::SetOpacity {
                id: from,
                opacity: 0.5,
            })
            .unwrap();

        // An ellipse and a block of text to give it to.
        session
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(chitrakar_doc::Node::vector(
                    "target",
                    chitrakar_doc::VectorShape::Ellipse { rx: 10.0, ry: 6.0 },
                )),
            })
            .unwrap();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(chitrakar_doc::Node::text(
                    "words",
                    chitrakar_doc::TextSpec::new("hi", 12.0, red),
                )),
            })
            .unwrap();
        let kids = session.document().children_of(root).unwrap().to_vec();
        let (ellipse, text) = (kids[1], kids[2]);

        let style = session.copy_style(from).unwrap();
        let before = session.history_labels().0.len();
        session.paste_style(&style, &[ellipse, text]).unwrap();
        assert_eq!(
            session.history_labels().0.len(),
            before + 1,
            "both layers in one entry"
        );

        let node = session.document().node(ellipse).unwrap();
        let chitrakar_doc::NodeKind::Vector { shape, fill, .. } = &node.kind else {
            panic!("the ellipse stopped being a shape");
        };
        assert!(
            matches!(shape, chitrakar_doc::VectorShape::Ellipse { .. }),
            "it kept its own shape"
        );
        assert_eq!(*fill, Some(red), "and took the fill");
        assert_eq!(node.effects.len(), 1, "and the effects");
        assert_eq!(node.opacity, 0.5, "and the opacity");

        let words = session.document().node(text).unwrap();
        let chitrakar_doc::NodeKind::Text(spec) = &words.kind else {
            panic!("the text stopped being text");
        };
        assert_eq!(spec.text, "hi", "the text kept its words");
        assert_eq!(spec.fill, red, "and took the fill");
        assert_eq!(words.effects.len(), 1);

        // One undo takes the whole paste back.
        session.undo().unwrap();
        assert_eq!(session.document().node(ellipse).unwrap().opacity, 1.0);
        assert!(session.document().node(text).unwrap().effects.is_empty());
    }

    /// An anchor put onto a curve keeps the curve: the two halves take
    /// the control points the split gives them, so the path through the
    /// new anchor is the path that was already there.
    #[test]
    fn an_anchor_added_to_a_curve_leaves_the_curve_where_it_was() {
        let mut session = Session::new(200, 200, ColorMode::Rgb);
        let root = session.document().root();
        // One curved segment, bowing well away from its chord.
        let mut curve = chitrakar_doc::Node::vector(
            "curve",
            chitrakar_doc::VectorShape::Path {
                points: vec![[0.0, 0.0], [100.0, 0.0]],
                closed: false,
                smooth: false,
                handles: vec![[0.0, 0.0, 0.0, 60.0], [0.0, 60.0, 0.0, 0.0]],
                subpaths: Vec::new(),
            },
        );
        if let chitrakar_doc::NodeKind::Vector { stroke, fill, .. } = &mut curve.kind {
            *fill = None;
            *stroke = Some(chitrakar_doc::Stroke {
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                width: 4.0,
                widths: Vec::new(),
            });
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(curve),
            })
            .unwrap();
        let id = session.document().children_of(root).unwrap()[0];
        session
            .apply(Command::SetTransform {
                id,
                transform: chitrakar_doc::Transform::translation(40.0, 40.0),
            })
            .unwrap();
        let before = session.render().unwrap();

        // Halfway along, which is where the curve's own middle is.
        let at = session.insert_anchor(id, 90.0, 85.0, 8.0).unwrap();
        assert_eq!(at, 1, "the new anchor sits between the two it split");
        let node = session.document().node(id).unwrap();
        let chitrakar_doc::NodeKind::Vector { shape, .. } = &node.kind else {
            panic!("not a shape");
        };
        let chitrakar_doc::VectorShape::Path { points, .. } = shape else {
            panic!("not a path");
        };
        assert_eq!(points.len(), 3, "there are three anchors now");

        let after = session.render().unwrap();
        // Not pixel for pixel: one segment became two, so the curve is
        // flattened at twice the resolution and its edge lands a
        // fraction of a pixel differently. What matters is that the
        // curve is where it was, which the average says and a look at
        // the middle of it confirms.
        let mut total = 0.0f64;
        for (p, q) in before.pixels.iter().zip(&after.pixels) {
            total += (p.a - q.a).abs() as f64;
        }
        let mean = total / before.pixels.len() as f64;
        assert!(mean < 0.002, "the curve did not move ({mean:.5})");
        assert!(
            after.get(90, 85).a > 0.5,
            "it still runs through its own middle"
        );
        assert_eq!(
            after.get(90, 45).a,
            0.0,
            "and not through where it never did"
        );

        // And one comes off again.
        session.remove_anchor(id, 1).unwrap();
        let node = session.document().node(id).unwrap();
        let chitrakar_doc::NodeKind::Vector { shape, .. } = &node.kind else {
            panic!("not a shape");
        };
        let chitrakar_doc::VectorShape::Path { points, .. } = shape else {
            panic!("not a path");
        };
        assert_eq!(points.len(), 2, "and the anchor comes off again");
        // A path needs what it has left.
        assert!(session.remove_anchor(id, 0).is_err());
        // And an anchor goes on the outline, not merely inside the shape:
        // a point nowhere near it is not asking for one.
        assert!(session.insert_anchor(id, 90.0, 20.0, 8.0).is_err());
    }

    /// A clone gesture: the source is set once and the whole stroke
    /// carries it, however many points it gathers.
    #[test]
    fn a_clone_stroke_carries_the_source_it_was_given() {
        let mut session = Session::new(120, 120, ColorMode::Rgb);
        let root = session.document().root();
        let red = chitrakar_color::AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let mut patch = chitrakar_doc::Node::vector(
            "patch",
            chitrakar_doc::VectorShape::Rect {
                width: 24.0,
                height: 24.0,
                radius: 0.0,
            },
        );
        if let chitrakar_doc::NodeKind::Vector { fill, .. } = &mut patch.kind {
            *fill = Some(red);
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(patch),
            })
            .unwrap();
        let id = session.document().children_of(root).unwrap()[0];
        session
            .apply(Command::SetTransform {
                id,
                transform: chitrakar_doc::Transform::translation(10.0, 10.0),
            })
            .unwrap();

        let layer = session.add_clone_layer("clone").unwrap();
        let before = session.history_labels().0.len();
        session
            .paint_begin(layer, 80.0, 80.0, 7.0, red, 0.0, false, false)
            .unwrap();
        // Read from the patch, which is 60 up and to the left.
        session.paint_source(-60.0, -60.0, false).unwrap();
        session.paint_extend(84.0, 84.0, 7.0).unwrap();
        assert!(session.commit_preview());
        assert_eq!(
            session.history_labels().0.len(),
            before + 1,
            "one entry for the whole stroke, source and all"
        );
        let drawn = session.render().unwrap();
        assert_eq!(
            drawn.get(80, 80).to_srgb8(),
            [255, 0, 0, 255],
            "it laid down what its source shows"
        );
        assert_eq!(drawn.get(80, 40).a, 0.0, "and nothing where it did not go");
    }

    /// A look taken from a layer that has nothing to paint with says
    /// nothing about how to paint, so it leaves the target's own paint
    /// where it is rather than stripping it.
    #[test]
    fn a_style_with_no_paint_in_it_leaves_the_paint_alone() {
        let mut session = Session::new(60, 60, ColorMode::Rgb);
        let root = session.document().root();
        let red = chitrakar_color::AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let mut shape = chitrakar_doc::Node::vector(
            "shape",
            chitrakar_doc::VectorShape::Rect {
                width: 20.0,
                height: 20.0,
                radius: 0.0,
            },
        );
        if let chitrakar_doc::NodeKind::Vector { fill, .. } = &mut shape.kind {
            *fill = Some(red);
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(shape),
            })
            .unwrap();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(chitrakar_doc::Node::adjustment(
                    "exposure",
                    chitrakar_doc::Adjustment::Exposure { stops: 1.0 },
                )),
            })
            .unwrap();
        let kids = session.document().children_of(root).unwrap().to_vec();
        let (shape_id, adj) = (kids[0], kids[1]);
        session
            .apply(Command::SetOpacity {
                id: adj,
                opacity: 0.25,
            })
            .unwrap();

        let style = session.copy_style(adj).unwrap();
        session.paste_style(&style, &[shape_id]).unwrap();
        let node = session.document().node(shape_id).unwrap();
        let chitrakar_doc::NodeKind::Vector { fill, .. } = &node.kind else {
            panic!("not a shape any more");
        };
        assert_eq!(*fill, Some(red), "the shape kept its fill");
        assert_eq!(node.opacity, 0.25, "and took what the style did carry");
    }

    /// Rubbing at a layer that is not a paint layer takes a piece out of
    /// it through a mask, so the layer itself is untouched — and the
    /// brush puts the piece back.
    #[test]
    fn the_brush_takes_a_piece_out_of_a_layer_it_cannot_paint_on() {
        let mut session = Session::new(120, 120, ColorMode::Rgb);
        let root = session.document().root();
        let mut rect = chitrakar_doc::Node::vector(
            "photo",
            chitrakar_doc::VectorShape::Rect {
                width: 60.0,
                height: 60.0,
                radius: 0.0,
            },
        );
        let red = chitrakar_color::AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        if let chitrakar_doc::NodeKind::Vector { fill, .. } = &mut rect.kind {
            *fill = Some(red);
        }
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(rect),
            })
            .unwrap();
        let photo = session.document().children_of(root).unwrap()[0];
        // Moved, because a mask is written in the space the layer sits
        // in rather than the layer's own: a brush that confused the two
        // would rub at the wrong place, and only a moved layer says so.
        session
            .apply(Command::SetTransform {
                id: photo,
                transform: chitrakar_doc::Transform::translation(40.0, 40.0),
            })
            .unwrap();

        assert!(session.ensure_painted_mask(photo).unwrap());
        assert!(
            session.ensure_painted_mask(photo).unwrap(),
            "asking twice leaves the one that is already there"
        );
        session
            .paint_begin(photo, 70.0, 70.0, 8.0, red, 0.0, true, true)
            .unwrap();
        session.commit_preview();
        assert_eq!(
            session.render().unwrap().get(70, 70).a,
            0.0,
            "the layer shows a hole where the pointer was"
        );
        assert!(
            session.render().unwrap().get(45, 45).a > 0.9,
            "and is whole elsewhere"
        );

        // The brush, not erasing, paints the mask back.
        session
            .paint_begin(photo, 70.0, 70.0, 4.0, red, 0.0, false, true)
            .unwrap();
        session.commit_preview();
        assert!(
            session.render().unwrap().get(70, 70).a > 0.9,
            "and the brush puts the piece back"
        );

        // A mask that was drawn or placed deliberately is not replaced.
        let mut other = Session::new(120, 120, ColorMode::Rgb);
        let root = other.document().root();
        other
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(chitrakar_doc::Node::group("g")),
            })
            .unwrap();
        let g = other.document().children_of(root).unwrap()[0];
        other
            .apply(Command::SetMask {
                id: g,
                mask: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Vector {
                        shape: chitrakar_doc::VectorShape::Ellipse { rx: 10.0, ry: 10.0 },
                        transform: chitrakar_doc::Transform::default(),
                    },
                    invert: false,
                })),
            })
            .unwrap();
        assert!(!other.ensure_painted_mask(g).unwrap());
    }

    /// A paint layer inside a moved group takes the brush where the
    /// pointer is, not where the layer would be without the group.
    #[test]
    fn a_brush_follows_the_group_its_layer_is_in() {
        let mut session = Session::new(120, 120, ColorMode::Rgb);
        let root = session.document().root();
        session
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(chitrakar_doc::Node::group("g")),
            })
            .unwrap();
        let group = session.document().children_of(root).unwrap()[0];
        session
            .apply(Command::AddNode {
                parent: group,
                index: 0,
                node: Box::new(chitrakar_doc::Node::paint("brush")),
            })
            .unwrap();
        let layer = session.document().children_of(group).unwrap()[0];
        session
            .apply(Command::SetTransform {
                id: group,
                transform: chitrakar_doc::Transform::translation(40.0, 40.0),
            })
            .unwrap();
        let color: chitrakar_color::AuthoredColor =
            serde_json::from_str(r#"{"Srgb":{"r":0.0,"g":0.0,"b":1.0,"a":1.0}}"#).unwrap();
        session
            .paint_begin(layer, 60.0, 60.0, 6.0, color, 0.0, false, false)
            .unwrap();
        session.commit_preview();
        let s = session.render().unwrap();
        assert!(s.get(60, 60).a > 0.9, "the dab landed under the pointer");
        assert_eq!(s.get(20, 20).a, 0.0, "not where the group moved it from");
    }

    /// A stroke still being drawn repaints only what it just added, so a
    /// long stroke does not get slower the longer it gets.
    #[test]
    fn extending_a_stroke_repaints_only_its_tail() {
        let mut session = Session::new(400, 400, ColorMode::Rgb);
        let layer = session.add_paint_layer("brush").unwrap();
        let color: chitrakar_color::AuthoredColor =
            serde_json::from_str(r#"{"Srgb":{"r":0.0,"g":0.0,"b":1.0,"a":1.0}}"#).unwrap();
        session
            .paint_begin(layer, 20.0, 200.0, 6.0, color, 0.0, false, false)
            .unwrap();
        // Draw most of the way across the page, then measure one more
        // step of the same stroke.
        for x in (30..340).step_by(10) {
            session.paint_extend(x as f32, 200.0, 6.0).unwrap();
        }
        let _ = session.render();
        let before = session.pixels_recomputed();
        session.paint_extend(350.0, 200.0, 6.0).unwrap();
        let _ = session.render();
        let touched = session.pixels_recomputed() - before;
        assert!(
            touched < 2000,
            "one more step of a page-wide stroke repainted {touched} pixels"
        );
        session.commit_preview();
        assert!(
            session.render().unwrap().get(200, 200).a > 0.9,
            "and the whole stroke is still painted"
        );
    }

    /// A stroke repaints where the stroke is, not where the whole
    /// painting is: a dab in one corner leaves the other alone.
    #[test]
    fn a_dab_dirties_only_what_it_covers() {
        let mut session = Session::new(400, 400, ColorMode::Rgb);
        let layer = session.add_paint_layer("brush").unwrap();
        let color: chitrakar_color::AuthoredColor =
            serde_json::from_str(r#"{"Srgb":{"r":0.0,"g":0.0,"b":1.0,"a":1.0}}"#).unwrap();
        // A long stroke across the page, then a dab in one corner.
        session
            .paint_begin(layer, 20.0, 20.0, 6.0, color, 0.0, false, false)
            .unwrap();
        session.paint_extend(380.0, 380.0, 6.0).unwrap();
        session.commit_preview();
        let _ = session.render();
        let before = session.pixels_recomputed();
        session
            .paint_begin(layer, 30.0, 30.0, 5.0, color, 0.0, false, false)
            .unwrap();
        session.commit_preview();
        let _ = session.render();
        let touched = session.pixels_recomputed() - before;
        assert!(
            touched < 400 * 400 / 4,
            "a dab repainted {touched} pixels of a 160000-pixel page"
        );
    }

    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn how_long_a_save_takes_with_images() {
        let mut session = Session::new(2000, 1500, ColorMode::Rgb);
        let pixels: Vec<u8> = (0..2000 * 1500 * 4).map(|i| (i % 251) as u8).collect();
        let png = chitrakar_codecs::encode_png(2000, 1500, &pixels).unwrap();
        session.place_image(&png, "big").unwrap();
        let t = std::time::Instant::now();
        let bytes = session.save().unwrap();
        eprintln!("save: {:?} for {} bytes", t.elapsed(), bytes.len());
        let t = std::time::Instant::now();
        let _ = session.save().unwrap();
        eprintln!("again: {:?}", t.elapsed());
    }
}
