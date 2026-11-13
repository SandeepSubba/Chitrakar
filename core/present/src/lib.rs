//! Frames drawn by the GPU backend, for a shell that owns a native process.
//!
//! The editor's document lives in the engine the webview runs, and the
//! webview is handed pixels to show. In the desktop shell there is a
//! native process beside it with a graphics card to draw with, and this
//! is what draws there: a mirror of the document, kept up to date by the
//! webview, drawn by `chitrakar_gpu` into the same bytes the engine's own
//! frame would be.
//!
//! The mirror is the document's own JSON — what a `.chitra` manifest
//! holds — which leaves out the bytes of its pictures and the faces its
//! text is set in. Those travel once each, by name: [`Presenter::sync`]
//! answers with what it still wants, and the webview hands each over.
//! Until all of it has arrived, and whenever the backend declines what
//! the page holds, [`Presenter::frame`] answers nothing and the webview
//! draws the frame itself, as it always has. A frame from here is never
//! a different picture, only a faster one: what the GPU agrees to draw
//! it draws the way the CPU does (`whatever_the_gpu_agrees_to_draw_it_
//! draws_the_way_the_cpu_does` in `chitrakar_gpu`), and this crate's own
//! tests hold a frame to the engine's.

use std::collections::HashMap;

use chitrakar_doc::{Document, NodeKind, Transform};
use chitrakar_gpu::GpuRenderer;

/// What the mirror is still missing to draw the page: pictures by their
/// content id, and faces by the name the text asks for.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Wants {
    pub resources: Vec<String>,
    pub fonts: Vec<String>,
}

impl Wants {
    pub fn nothing(&self) -> bool {
        self.resources.is_empty() && self.fonts.is_empty()
    }
}

/// A picture's bytes, kept by content id across syncs: the pool is
/// immutable, so a picture handed over once is the picture for good.
struct Kept {
    rgba8: Vec<u8>,
}

pub struct Presenter {
    gpu: GpuRenderer,
    doc: Option<Document>,
    pictures: HashMap<String, Kept>,
    wants: Wants,
}

impl Presenter {
    /// A presenter on whatever graphics adapter this machine offers, or
    /// nothing when it offers none — in which case the webview goes on
    /// drawing its own frames.
    pub fn new() -> Option<Self> {
        Some(Presenter {
            gpu: GpuRenderer::new()?,
            doc: None,
            pictures: HashMap::new(),
            wants: Wants::default(),
        })
    }

    /// What the adapter calls itself.
    pub fn adapter(&self) -> &str {
        &self.gpu.adapter
    }

    /// Take the document as it now stands, from its JSON, and say what
    /// is still wanted to draw it. A document that is not a tree, or a
    /// page no surface could hold, is refused as opening it is.
    pub fn sync(&mut self, manifest: &str) -> Result<Wants, String> {
        let mut doc: Document = serde_json::from_str(manifest).map_err(|e| e.to_string())?;
        doc.check_structure().map_err(|e| e.to_string())?;
        if !chitrakar_doc::canvas_fits(doc.meta.width, doc.meta.height) {
            return Err("the page is larger than can be drawn".into());
        }
        let mut wants = Wants::default();
        let ids: Vec<String> = doc.resources().map(|(id, _)| id.clone()).collect();
        for id in ids {
            match self.pictures.get(&id) {
                Some(k) => {
                    doc.restore_resource_bytes(&id, k.rgba8.clone());
                }
                None => wants.resources.push(id),
            }
        }
        let mut faces: Vec<String> = doc
            .nodes()
            .filter_map(|(_, n)| match &n.kind {
                NodeKind::Text(spec) => Some(chitrakar_render::text::faces_used(spec)),
                _ => None,
            })
            .flatten()
            .filter(|f| !chitrakar_render::text::has_font(f))
            .collect();
        faces.sort();
        faces.dedup();
        wants.fonts = faces;
        self.doc = Some(doc);
        self.wants = wants.clone();
        Ok(wants)
    }

    /// A picture's bytes, handed over once. Refused unless they are the
    /// picture the id names: the id is the bytes' own hash, so a picture
    /// that arrived wrong cannot be drawn as though it were right.
    pub fn give_picture(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    ) -> Result<(), String> {
        if rgba8.len() as u64 != width as u64 * height as u64 * 4 {
            return Err("the picture is not the size it says".into());
        }
        if chitrakar_doc::content_id(width, height, &rgba8) != id {
            return Err("the picture is not the one its name says".into());
        }
        if let Some(doc) = &mut self.doc {
            doc.restore_resource_bytes(id, rgba8.clone());
        }
        self.wants.resources.retain(|w| w != id);
        self.pictures.insert(id.to_string(), Kept { rgba8 });
        Ok(())
    }

    /// A face the text is set in, handed over once.
    pub fn give_font(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), String> {
        chitrakar_render::text::register_font(name, bytes)?;
        self.wants.fonts.retain(|w| w != name);
        Ok(())
    }

    /// What the mirror still wants.
    pub fn wants(&self) -> &Wants {
        &self.wants
    }

    /// The viewport `width` by `height`, with the document's origin at
    /// (`x`, `y`) and `scale` device pixels to its pixel — the engine's
    /// own view — as straight sRGB bytes, row by row: what the engine's
    /// frame would hold. Nothing while something is still wanted, for a
    /// CMYK page (its colours are the press profile's, which the mirror
    /// does not carry), and for a page the backend declines.
    pub fn frame(&self, scale: f32, x: f32, y: f32, width: u32, height: u32) -> Option<Vec<u8>> {
        let doc = self.doc.as_ref()?;
        if !self.wants.nothing() || doc.meta.color_mode != chitrakar_color::ColorMode::Rgb {
            return None;
        }
        if width == 0 || height == 0 || !chitrakar_doc::canvas_fits(width, height) {
            return None;
        }
        let view = Transform {
            a: scale,
            d: scale,
            e: x,
            f: y,
            ..Default::default()
        };
        chitrakar_render::pyramid::next_frame();
        let surface = self.gpu.render_view(doc, view, (width, height))?;
        Some(surface.to_srgb8())
    }

    /// Whether a page is one the backend would draw at that view.
    pub fn draws(&self, scale: f32, x: f32, y: f32, width: u32, height: u32) -> bool {
        let Some(doc) = &self.doc else {
            return false;
        };
        let view = Transform {
            a: scale,
            d: scale,
            e: x,
            f: y,
            ..Default::default()
        };
        self.gpu.draws_view(doc, view, (width, height))
    }
}

#[cfg(test)]
mod tests;
