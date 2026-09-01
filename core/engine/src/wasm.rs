//! wasm-bindgen surface for the webview UI. Thin translation layer only:
//! all behavior lives in [`Session`](crate::Session).
//!
//! Conventions across the boundary:
//! - commands travel as serde-JSON strings of [`Command`](crate::Command);
//! - node ids travel as `f64` (they are sequence numbers, far below 2^53);
//! - frames stay in wasm memory: `render_frame` re-encodes only the dirty
//!   region into an internal RGBA8 buffer, exposed via `frame_ptr`/
//!   `frame_len` for zero-copy reads from JS.

use crate::{ColorMode, NodeId, Session};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmSession {
    inner: Session,
    frame: Vec<u8>,
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
            frame: Vec::new(),
        }
    }

    /// Open a `.chitra` file.
    pub fn open(bytes: &[u8]) -> Result<WasmSession, JsError> {
        Ok(WasmSession {
            inner: Session::load(bytes).map_err(to_js)?,
            frame: Vec::new(),
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

    /// Apply a command as a live drag preview (no history until commit).
    pub fn preview(&mut self, command_json: &str) -> Result<(), JsError> {
        self.inner.preview_json(command_json).map_err(to_js)
    }

    /// End the preview gesture as one undo step.
    pub fn commit_preview(&mut self) -> bool {
        self.inner.commit_preview()
    }

    /// Abort the preview gesture, restoring pre-gesture state.
    pub fn cancel_preview(&mut self) -> Result<bool, JsError> {
        self.inner.cancel_preview().map_err(to_js)
    }

    pub fn undo(&mut self) -> Result<bool, JsError> {
        self.inner.undo().map_err(to_js)
    }

    pub fn redo(&mut self) -> Result<bool, JsError> {
        self.inner.redo().map_err(to_js)
    }

    /// Re-render what changed since the last call into the internal RGBA8
    /// frame. Returns the dirty rect `[x, y, w, h]`, or an empty array when
    /// nothing changed. Read pixels via `frame_ptr`/`frame_len`.
    pub fn render_frame(&mut self) -> Result<Vec<u32>, JsError> {
        let expected = (self.width() * self.height() * 4) as usize;
        let first_frame = self.frame.len() != expected;
        if first_frame {
            self.frame = vec![0; expected];
        }
        let full = crate::ClipRect {
            x0: 0,
            y0: 0,
            x1: self.width(),
            y1: self.height(),
        };
        let (_, dirty) = self.inner.render_cached().map_err(to_js)?;
        let clip = match (dirty, first_frame) {
            (Some(clip), _) => clip,
            (None, false) => return Ok(Vec::new()),
            (None, true) => full,
        };
        self.inner.encode_present_region(clip, &mut self.frame);
        Ok(vec![clip.x0, clip.y0, clip.x1 - clip.x0, clip.y1 - clip.y0])
    }

    pub fn frame_ptr(&self) -> *const u8 {
        self.frame.as_ptr()
    }

    pub fn frame_len(&self) -> usize {
        self.frame.len()
    }

    /// Layers panel data: JSON array of `LayerInfo`, topmost first.
    pub fn layers_json(&self) -> String {
        serde_json::to_string(&self.inner.layers()).unwrap_or_else(|_| "[]".into())
    }

    /// Topmost clickable node at a document-space point, or undefined.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<f64> {
        self.inner.hit_test(x, y).map(|id| id.0 as f64)
    }

    /// Full affine transform of a node as `[a, b, c, d, e, f]`.
    pub fn transform_of(&self, id: f64) -> Result<Vec<f32>, JsError> {
        let t = self.inner.transform_of(NodeId(id as u64)).map_err(to_js)?;
        Ok(vec![t.a, t.b, t.c, t.d, t.e, t.f])
    }

    /// A node's kind parameters as JSON (see `Session::kind_json`).
    pub fn kind_json(&self, id: f64) -> Result<String, JsError> {
        self.inner.kind_json(NodeId(id as u64)).map_err(to_js)
    }

    /// A node's mask as JSON, `null` when unmasked.
    pub fn mask_json(&self, id: f64) -> Result<String, JsError> {
        self.inner.mask_json(NodeId(id as u64)).map_err(to_js)
    }

    /// Doc-space bounds of a node as `[x, y, w, h]`; empty if it has none.
    pub fn bounds_of(&self, id: f64) -> Vec<f32> {
        self.inner
            .bounds_of(NodeId(id as u64))
            .map(|b| b.to_vec())
            .unwrap_or_default()
    }

    /// Decode PNG/JPEG bytes and place them as a raster object (undoable).
    pub fn place_image(&mut self, bytes: &[u8], name: &str) -> Result<(), JsError> {
        self.inner.place_image(bytes, name).map_err(to_js)
    }

    /// Set the CMYK press profile from ICC bytes.
    pub fn set_cmyk_profile(&mut self, icc: &[u8]) -> Result<(), JsError> {
        self.inner.set_cmyk_profile(icc.to_vec()).map_err(to_js)
    }

    /// Toggle display soft-proofing (and gamut warning) through the press
    /// profile.
    pub fn set_proofing(&mut self, proof: bool, gamut_warn: bool) -> Result<(), JsError> {
        self.inner.set_proofing(proof, gamut_warn).map_err(to_js)
    }

    #[wasm_bindgen(getter)]
    pub fn has_cmyk_profile(&self) -> bool {
        self.inner.has_cmyk_profile()
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
