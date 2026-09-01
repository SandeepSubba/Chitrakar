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

/// An open document plus its edit history and cached composite.
pub struct Session {
    doc: Document,
    undo: Vec<Command>,
    redo: Vec<Command>,
    cache: Option<Surface>,
    /// Region of `cache` that must be recomputed before the next present.
    stale: Option<ClipRect>,
    /// Inverse restoring the state before the current preview gesture.
    preview_inverse: Option<Command>,
    /// Total pixels re-rendered so far (observability for tests and tuning).
    pixels_recomputed: u64,
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
            stale: None,
            preview_inverse: None,
            pixels_recomputed: 0,
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
            Command::AddNode { .. } | Command::RestoreSubtree { .. } => None,
            Command::RemoveNode { id }
            | Command::SetOpacity { id, .. }
            | Command::SetVisible { id, .. }
            | Command::SetBlendMode { id, .. }
            | Command::SetTransform { id, .. }
            | Command::SetKind { id, .. }
            | Command::SetName { id, .. }
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
        // Filters read pixel neighborhoods, so a partial region re-render
        // would sample stale surroundings at its edges: while any filter
        // layer exists (before or after this command), fall back to
        // whole-canvas invalidation. Padded region rendering can refine this.
        let had_filter = self.doc.has_filter();
        let pre = self.bounds_of_target(Self::command_target(&cmd));
        let inverse = self.doc.apply(cmd)?;
        let post = self.bounds_of_target(Self::command_target(&inverse));
        if had_filter || self.doc.has_filter() {
            self.mark_dirty(Bounds::Everything);
        } else {
            self.mark_dirty(pre.union(post));
        }
        Ok(inverse)
    }

    pub fn apply(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.commit_preview(); // a stray preview must not leak into this edit
        let inverse = self.apply_internal(cmd)?;
        self.undo.push(inverse);
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
        let inverse = self.apply_internal(cmd)?;
        if self.preview_inverse.is_none() {
            self.preview_inverse = Some(inverse);
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
            Some(inverse) => {
                self.undo.push(inverse);
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
            Some(inverse) => {
                self.apply_internal(inverse)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn undo(&mut self) -> Result<bool, EngineError> {
        self.commit_preview();
        match self.undo.pop() {
            None => Ok(false),
            Some(inverse) => {
                let redo = self.apply_internal(inverse)?;
                self.redo.push(redo);
                Ok(true)
            }
        }
    }

    pub fn redo(&mut self) -> Result<bool, EngineError> {
        match self.redo.pop() {
            None => Ok(false),
            Some(cmd) => {
                let inverse = self.apply_internal(cmd)?;
                self.undo.push(inverse);
                Ok(true)
            }
        }
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
        let cache = self.cache.as_mut().unwrap();
        match self.stale.take() {
            Some(clip) => {
                chitrakar_render::render_region(&self.doc, cache, clip)?;
                self.pixels_recomputed += clip.area();
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

    /// Render and encode as PNG — used by export and tests.
    pub fn render_png(&self) -> Result<Vec<u8>, EngineError> {
        let surface = self.render()?;
        chitrakar_codecs::encode_png(surface.width, surface.height, &surface.to_srgb8())
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
                },
                visible: node.visible,
                opacity: node.opacity,
                blend: node.blend,
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

        // An edit below the filter must invalidate the whole canvas —
        // the blur halo far from the rect has to update too.
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
