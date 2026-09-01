//! wasm-bindgen surface for the webview UI. Thin translation layer only:
//! all behavior lives in [`Session`](crate::Session).
//!
//! Conventions across the boundary:
//! - commands travel as serde-JSON strings of [`Command`](crate::Command);
//! - node ids travel as `f64` (they are sequence numbers, far below 2^53);
//! - pixels travel as sRGB RGBA8, ready for `putImageData`.

use crate::{ColorMode, NodeId, Session};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmSession {
    inner: Session,
}

#[wasm_bindgen]
impl WasmSession {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32, cmyk: bool) -> WasmSession {
        let mode = if cmyk {
            ColorMode::Cmyk
        } else {
            ColorMode::Rgb
        };
        WasmSession {
            inner: Session::new(width, height, mode),
        }
    }

    /// Open a `.chitra` file.
    pub fn open(bytes: &[u8]) -> Result<WasmSession, JsError> {
        Ok(WasmSession {
            inner: Session::load(bytes).map_err(to_js)?,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.inner.document().meta.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.inner.document().meta.height
    }

    #[wasm_bindgen(getter)]
    pub fn cmyk(&self) -> bool {
        self.inner.document().meta.color_mode == ColorMode::Cmyk
    }

    #[wasm_bindgen(getter)]
    pub fn root_id(&self) -> f64 {
        self.inner.document().root().0 as f64
    }

    pub fn apply(&mut self, command_json: &str) -> Result<(), JsError> {
        self.inner.apply_json(command_json).map_err(to_js)
    }

    pub fn undo(&mut self) -> Result<bool, JsError> {
        self.inner.undo().map_err(to_js)
    }

    pub fn redo(&mut self) -> Result<bool, JsError> {
        self.inner.redo().map_err(to_js)
    }

    /// Full-frame render as sRGB RGBA8 (`width * height * 4` bytes).
    pub fn render_rgba(&self) -> Result<Vec<u8>, JsError> {
        Ok(self.inner.render().map_err(to_js)?.to_srgb8())
    }

    /// Layers panel data: JSON array of `LayerInfo`, topmost first.
    pub fn layers_json(&self) -> String {
        serde_json::to_string(&self.inner.layers()).unwrap_or_else(|_| "[]".into())
    }

    /// Topmost clickable node at a document-space point, or undefined.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<f64> {
        self.inner.hit_test(x, y).map(|id| id.0 as f64)
    }

    /// Translation of a node as `[tx, ty]` (full affine over the boundary
    /// once rotation/scale tools exist).
    pub fn translation_of(&self, id: f64) -> Result<Vec<f32>, JsError> {
        let t = self.inner.transform_of(NodeId(id as u64)).map_err(to_js)?;
        Ok(vec![t.e, t.f])
    }

    /// Serialize to `.chitra` bytes.
    pub fn save(&self) -> Result<Vec<u8>, JsError> {
        self.inner.save().map_err(to_js)
    }

    /// Render and encode as PNG (export).
    pub fn export_png(&self) -> Result<Vec<u8>, JsError> {
        self.inner.render_png().map_err(to_js)
    }
}

fn to_js(e: crate::EngineError) -> JsError {
    JsError::new(&e.to_string())
}
