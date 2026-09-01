//! The one crate the app shells embed.
//!
//! The UI never touches pixel buffers or the document directly: it sends
//! [`Command`]s (as values natively, as JSON over the WASM boundary) and
//! receives rendered frames. wasm-bindgen bindings are added here in the
//! Phase 0 WebGPU spike.

pub use chitrakar_color::ColorMode;
pub use chitrakar_doc::{Command, Document, History, Node, NodeId};
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

    /// Render and encode as PNG — used by early UI plumbing and tests.
    pub fn render_png(&self) -> Result<Vec<u8>, EngineError> {
        let surface = self.render()?;
        chitrakar_codecs::encode_png(surface.width, surface.height, &surface.to_srgb8())
            .map_err(|e| EngineError::BadCommand(e.to_string()))
    }
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
}
