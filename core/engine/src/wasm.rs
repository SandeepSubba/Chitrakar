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

    /// Width of the presented frame in device pixels: the document width
    /// times the view scale.
    pub fn frame_width(&self) -> u32 {
        self.inner.present_size().0
    }

    pub fn frame_height(&self) -> u32 {
        self.inner.present_size().1
    }

    /// Ask for the composite at `scale` times document resolution so a
    /// zoomed-in canvas stays sharp. Returns the scale actually adopted,
    /// which is capped by a pixel budget.
    pub fn set_view_scale(&mut self, scale: f32) -> f32 {
        self.inner.set_view_scale(scale)
    }

    /// Re-render what changed since the last call into the internal RGBA8
    /// frame. Returns the dirty rect `[x, y, w, h]` in frame pixels, or an
    /// empty array when nothing changed. Read pixels via
    /// `frame_ptr`/`frame_len`.
    pub fn render_frame(&mut self) -> Result<Vec<u32>, JsError> {
        let (fw, fh) = self.inner.present_size();
        let expected = (fw * fh * 4) as usize;
        let first_frame = self.frame.len() != expected;
        if first_frame {
            self.frame = vec![0; expected];
        }
        let full = crate::ClipRect {
            x0: 0,
            y0: 0,
            x1: fw,
            y1: fh,
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

    /// History labels as JSON `{past: [...oldest first], future: [...next
    /// redo first]}`.
    pub fn history_json(&self) -> String {
        let (past, future) = self.inner.history_labels();
        serde_json::to_string(&serde_json::json!({ "past": past, "future": future }))
            .unwrap_or_else(|_| "{}".into())
    }

    /// Move through history: negative undoes, positive redoes.
    pub fn jump(&mut self, delta: i32) -> Result<(), JsError> {
        self.inner.jump(delta).map_err(to_js)
    }

    /// Group same-parent nodes (one undo step); returns the group id.
    pub fn group_nodes(&mut self, ids: Vec<f64>, name: &str) -> Result<f64, JsError> {
        let ids: Vec<NodeId> = ids.into_iter().map(|i| NodeId(i as u64)).collect();
        self.inner
            .group_nodes(&ids, name)
            .map(|id| id.0 as f64)
            .map_err(to_js)
    }

    /// Line up or space out several layers (see `Session::align_nodes`).
    pub fn align_nodes(&mut self, ids: Vec<f64>, mode: &str) -> Result<(), JsError> {
        let ids: Vec<NodeId> = ids.into_iter().map(|i| NodeId(i as u64)).collect();
        self.inner.align_nodes(&ids, mode).map_err(to_js)
    }

    /// Combine shape layers into one compound path. `op` is one of
    /// "union", "intersect", "subtract" or "exclude". Returns the new
    /// layer's id.
    pub fn boolean_nodes(&mut self, ids: Vec<f64>, op: &str) -> Result<f64, JsError> {
        let ids: Vec<NodeId> = ids.into_iter().map(|i| NodeId(i as u64)).collect();
        self.inner
            .boolean_nodes(&ids, op)
            .map(|id| id.0 as f64)
            .map_err(to_js)
    }

    /// Put a node and its subtree on the clipboard.
    pub fn copy_node(&self, id: f64) -> Result<(), JsError> {
        self.inner.copy_node(NodeId(id as u64)).map_err(to_js)
    }

    /// Paste the clipboard into the root; returns the new node's id, or
    /// undefined when the clipboard is empty.
    pub fn paste(&mut self) -> Result<Option<f64>, JsError> {
        self.inner
            .paste(None)
            .map(|id| id.map(|i| i.0 as f64))
            .map_err(to_js)
    }

    #[wasm_bindgen(getter)]
    pub fn has_clipboard(&self) -> bool {
        crate::clipboard_has_content()
    }

    /// Copy a node and its subtree just above itself; returns the copy's id.
    pub fn duplicate_node(&mut self, id: f64) -> Result<f64, JsError> {
        self.inner
            .duplicate_node(NodeId(id as u64))
            .map(|id| id.0 as f64)
            .map_err(to_js)
    }

    /// Dissolve a group into its parent (one undo step).
    pub fn ungroup_node(&mut self, id: f64) -> Result<(), JsError> {
        self.inner.ungroup_node(NodeId(id as u64)).map_err(to_js)
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

    /// Resize the page, shifting top-level layers so the picture stays
    /// put. Cropping is this with the crop rect's size and negated origin.
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        dx: f32,
        dy: f32,
    ) -> Result<(), JsError> {
        self.inner
            .resize_canvas(width, height, dx, dy)
            .map_err(to_js)
    }

    /// Force the next `render_frame` to redraw everything, for when the
    /// canvas it is copied into has been replaced.
    pub fn invalidate(&mut self) {
        self.inner.invalidate();
    }

    /// A node's effect list as JSON.
    pub fn effects_json(&self, id: f64) -> Result<String, JsError> {
        self.inner.effects_json(NodeId(id as u64)).map_err(to_js)
    }

    /// Doc-space bounds of a node as `[x, y, w, h]`; empty if it has none.
    pub fn bounds_of(&self, id: f64) -> Vec<f32> {
        self.inner
            .bounds_of(NodeId(id as u64))
            .map(|b| b.to_vec())
            .unwrap_or_default()
    }

    /// Transform from a node's parent space into document space, as
    /// `[a, b, c, d, e, f]`.
    pub fn parent_space_of(&self, id: f64) -> Vec<f32> {
        self.inner.parent_space_of(NodeId(id as u64)).to_vec()
    }

    /// Local-space bounds of a node as `[x0, y0, x1, y1]`; empty if none.
    pub fn local_bounds_of(&self, id: f64) -> Vec<f32> {
        self.inner
            .local_bounds_of(NodeId(id as u64))
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

    /// Render and encode as JPEG (export); transparency flattens onto white.
    pub fn export_jpeg(&self, quality: u8) -> Result<Vec<u8>, JsError> {
        self.inner.export_jpeg(quality).map_err(to_js)
    }

    /// Export a one-page PDF (CMYK-separated when a press profile is set).
    pub fn export_pdf(&self) -> Result<Vec<u8>, JsError> {
        self.inner.export_pdf().map_err(to_js)
    }

    /// Export as SVG markup.
    pub fn export_svg(&self) -> Result<String, JsError> {
        self.inner.export_svg().map_err(to_js)
    }

    /// Export a print-ready CMYK TIFF (needs a loaded press profile).
    pub fn export_cmyk_tiff(&self) -> Result<Vec<u8>, JsError> {
        self.inner.export_cmyk_tiff().map_err(to_js)
    }
}

fn to_js(e: crate::EngineError) -> JsError {
    JsError::new(&e.to_string())
}
