//! The one crate the app shells embed.
//!
//! The UI never touches pixel buffers or the document directly: it sends
//! [`Command`]s (as values natively, as JSON over the WASM boundary) and
//! receives rendered frames. The [`wasm`] module is the wasm-bindgen surface
//! the webview UI drives.

#[cfg(target_arch = "wasm32")]
pub mod wasm;

use serde::Serialize;

pub use chitrakar_color::ColorMode;
pub use chitrakar_doc::{Command, Document, History, Node, NodeId, NodeKind, Transform};
pub use chitrakar_render::Surface;

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

/// An open document plus its edit history.
pub struct Session {
    doc: Document,
    history: History,
}

impl Session {
    pub fn new(width: u32, height: u32, color_mode: ColorMode) -> Self {
        Self {
            doc: Document::new(width, height, color_mode),
            history: History::default(),
        }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn apply(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.history.apply(&mut self.doc, cmd)?;
        Ok(())
    }

    /// Apply a JSON-encoded command — the transport used across the WASM/IPC
    /// boundary.
    pub fn apply_json(&mut self, json: &str) -> Result<(), EngineError> {
        let cmd: Command =
            serde_json::from_str(json).map_err(|e| EngineError::BadCommand(e.to_string()))?;
        self.apply(cmd)
    }

    pub fn undo(&mut self) -> Result<bool, EngineError> {
        Ok(self.history.undo(&mut self.doc)?)
    }

    pub fn redo(&mut self) -> Result<bool, EngineError> {
        Ok(self.history.redo(&mut self.doc)?)
    }

    /// Render the current document state (full frame; tiled incremental
    /// rendering replaces this as the render graph matures).
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

    /// Serialize to `.chitra` container bytes.
    pub fn save(&self) -> Result<Vec<u8>, EngineError> {
        chitrakar_codecs::save_chitra(&self.doc).map_err(|e| EngineError::BadCommand(e.to_string()))
    }

    /// Open a `.chitra` container. The loaded document starts with a fresh
    /// history (undo does not cross save boundaries for now).
    pub fn load(bytes: &[u8]) -> Result<Self, EngineError> {
        let doc = chitrakar_codecs::load_chitra(bytes)
            .map_err(|e| EngineError::BadCommand(e.to_string()))?;
        Ok(Self {
            doc,
            history: History::default(),
        })
    }
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
        let restored = Session::load(&bytes).unwrap();
        assert_eq!(restored.document().node_count(), 2);
        assert_eq!(restored.layers()[0].name, "kept");
    }
}
