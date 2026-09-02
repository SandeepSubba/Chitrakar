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

/// How far a duplicate is nudged from its original, in document units.
const DUPLICATE_OFFSET: f32 = 12.0;
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

/// An open document plus its edit history and cached composite.
pub struct Session {
    doc: Document,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    cache: Option<Surface>,
    /// Reused scratch surface for padded region renders under filters.
    scratch: Option<Surface>,
    /// Region of `cache` that must be recomputed before the next present.
    stale: Option<ClipRect>,
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
            Command::AddNode { .. } | Command::RestoreSubtree { .. } | Command::Batch(_) => None,
            Command::RemoveNode { id }
            | Command::SetOpacity { id, .. }
            | Command::SetVisible { id, .. }
            | Command::SetBlendMode { id, .. }
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
        let batch = matches!(cmd, Command::Batch(_));
        let pre = self.bounds_of_target(Self::command_target(&cmd));
        let inverse = self.doc.apply(cmd)?;
        let post = self.bounds_of_target(Self::command_target(&inverse));
        if batch {
            // Batches touch several nodes; whole-canvas is the safe region.
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
            Command::MoveNode { id, .. } => format!("Move {}", name(id)),
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
        let (w, h) = (self.doc.meta.width, self.doc.meta.height);
        let full = ClipRect {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        };
        if self.cache.is_none() {
            self.cache = Some(Surface::new(w, h));
            self.stale = Some(full);
        }
        match self.stale.take() {
            Some(clip) => {
                // Filters sample neighbors: a region render is only correct
                // deeper than the filter stack's reach inside its own edge.
                // So compute a padded region in scratch and copy back just
                // the exact region — the padding ring, whose values clamp
                // against stale surroundings, is discarded.
                let pad = chitrakar_render::filter_reach(&self.doc);
                if pad == 0 {
                    let cache = self.cache.as_mut().unwrap();
                    chitrakar_render::render_region(&self.doc, cache, clip)?;
                    self.pixels_recomputed += clip.area();
                } else {
                    let compute = ClipRect {
                        x0: clip.x0.saturating_sub(pad),
                        y0: clip.y0.saturating_sub(pad),
                        x1: (clip.x1 + pad).min(w),
                        y1: (clip.y1 + pad).min(h),
                    };
                    let scratch = self.scratch.get_or_insert_with(|| Surface::new(w, h));
                    chitrakar_render::render_region(&self.doc, scratch, compute)?;
                    self.cache.as_mut().unwrap().copy_region_from(scratch, clip);
                    self.pixels_recomputed += compute.area();
                }
                Ok((self.cache.as_ref().unwrap(), Some(clip)))
            }
            None => Ok((self.cache.as_ref().unwrap(), None)),
        }
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
