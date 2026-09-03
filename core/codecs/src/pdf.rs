//! PDF export for print handoff.
//!
//! One page sized from the document's pixel dimensions and dpi. Two
//! flavors, chosen by whether the document has a press profile:
//!
//! - **CMYK**: ink is written as ink — authored CMYK fills go into the
//!   file as the values they were typed, sRGB colours and pixels are
//!   separated through the profile — in an ICCBased (N=4) color space
//!   carrying that same profile, which is also the page's output intent,
//!   so a RIP reproduces what soft proofing showed.
//! - **RGB**: sRGB in DeviceRGB.
//!
//! [`export_pdf_document`] draws the document live where PDF has the
//! vocabulary: rectangles (rounded too), ellipses and paths as paths with
//! solid fills and strokes, groups as nested transforms, placed images as
//! image XObjects with their alpha as a soft mask, opacity and blend as
//! graphics states, text as text — each face embedded once as a CID font
//! addressed by glyph id, every glyph placed where the shaper put it (so
//! kerning and ligatures survive) with a ToUnicode map so the words can
//! be found and copied. What PDF cannot say — gradients, effects, masks,
//! varying strokes, a group that needs isolating — is rendered by the
//! engine alone on the page and placed as an image, trimmed to its ink;
//! an adjustment or filter layer, which changes everything under it,
//! flattens everything under it into one. [`export_pdf`] is the whole
//! composite as one image, which is what the vector writer falls back to.
//!
//! Image data is Flate-compressed (lossless — this is print output, so DCT
//! is not an option). The writer is deliberately small and explicit rather
//! than a PDF library: hand-rolling it keeps the dependency surface honest.

use chitrakar_color::{AuthoredColor, LinearRgba};
use chitrakar_doc::{
    BlendMode, Command, DocError, Document, NodeId, NodeKind, Transform, VectorShape,
};
use flate2::{write::ZlibEncoder, Compression};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("color conversion failed: {0}")]
    Color(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("document error: {0}")]
    Doc(#[from] DocError),
    #[error("render failed: {0}")]
    Render(String),
}

/// Flatten a premultiplied linear composite to non-linear sRGB over white
/// paper. PDF images here have no alpha; unprinted areas are the page.
fn flatten_to_srgb(pixels: &[LinearRgba]) -> Vec<f32> {
    let mut out = Vec::with_capacity(pixels.len() * 3);
    for px in pixels {
        let over_white = |v: f32| (v + (1.0 - px.a)).clamp(0.0, 1.0);
        out.push(chitrakar_color::linear_to_srgb(over_white(px.r)));
        out.push(chitrakar_color::linear_to_srgb(over_white(px.g)));
        out.push(chitrakar_color::linear_to_srgb(over_white(px.b)));
    }
    out
}

fn deflate(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

/// Write a one-page PDF containing the composite.
///
/// `dpi` sets the physical page size (pixels / dpi * 72pt). When `cmyk_icc`
/// is given, the page is separated into that profile's ink and the profile
/// travels with the file.
pub fn export_pdf(
    pixels: &[LinearRgba],
    width: u32,
    height: u32,
    dpi: f32,
    cmyk_icc: Option<&[u8]>,
) -> Result<Vec<u8>, PdfError> {
    let srgb = flatten_to_srgb(pixels);

    // Sample data and the color space that describes it.
    let (samples, components) = match cmyk_icc {
        Some(icc) => {
            let sep = chitrakar_color::cms::RgbToCmyk::new(icc).map_err(PdfError::Color)?;
            (sep.separate(&srgb).map_err(PdfError::Color)?, 4)
        }
        None => (
            srgb.iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect(),
            3,
        ),
    };
    let image_stream = deflate(&samples)?;
    let icc_stream = cmyk_icc.map(deflate).transpose()?;

    let pt = 72.0 / dpi.max(1.0);
    let (page_w, page_h) = (width as f32 * pt, height as f32 * pt);

    // Objects are appended in order, each recording its byte offset for the
    // cross-reference table.
    let mut out: Vec<u8> = Vec::with_capacity(image_stream.len() + 4096);
    let mut offsets: Vec<usize> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    // 1 catalog, 2 pages, 3 page, 4 contents, 5 image, [6 icc]
    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    );
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.3} {page_h:.3}] \
             /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        )
        .as_bytes(),
    );

    // Content stream: scale the unit image to fill the page.
    let content = format!("q\n{page_w:.3} 0 0 {page_h:.3} 0 0 cm\n/Im0 Do\nQ\n");
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        )
        .as_bytes(),
    );

    // ICCBased must be an array holding an *indirect* reference to the
    // profile stream — a stream cannot live inline inside an array.
    let color_space = if icc_stream.is_some() {
        "6 0 R".to_string()
    } else {
        "/DeviceRGB".to_string()
    };
    let mut image_obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace {color_space} /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        image_stream.len()
    )
    .into_bytes();
    image_obj.extend_from_slice(&image_stream);
    image_obj.extend_from_slice(b"\nendstream");
    obj(&mut out, &mut offsets, &image_obj);

    if let Some(icc) = icc_stream {
        // 6: the color space array, referring to 7: the profile stream.
        obj(&mut out, &mut offsets, b"[/ICCBased 7 0 R]");
        let mut icc_obj = format!(
            "<< /N {components} /Filter /FlateDecode /Length {} >>\nstream\n",
            icc.len()
        )
        .into_bytes();
        icc_obj.extend_from_slice(&icc);
        icc_obj.extend_from_slice(b"\nendstream");
        obj(&mut out, &mut offsets, &icc_obj);
    }

    // Cross-reference table and trailer.
    let xref_at = out.len();
    let count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Ok(out)
}

/// The bezier handle length that draws a quarter circle closest.
const KAPPA: f32 = 0.552_284_8;

/// Draw a document as one PDF page: live vectors where PDF can carry
/// them, the engine's pixels where it cannot (see the module docs).
pub fn export_pdf_document(doc: &Document) -> Result<Vec<u8>, PdfError> {
    let icc = doc.cmyk_profile_bytes();
    let mut page = Page {
        doc,
        separate: icc
            .map(chitrakar_color::cms::RgbToCmyk::new)
            .transpose()
            .map_err(PdfError::Color)?,
        objects: Vec::new(),
        xobjects: Vec::new(),
        gstates: Vec::new(),
        content: String::new(),
        icc_objects: None,
        fonts: Vec::new(),
    };
    // 1 catalog, 2 pages, 3 page, 4 contents: reserved, written last,
    // once everything they refer to has a number.
    for _ in 0..4 {
        page.objects.push(Vec::new());
    }
    if let Some(icc) = icc {
        let profile = page.push(&stream_object(
            "<< /N 4 /Filter /FlateDecode",
            &deflate(icc)?,
        ));
        let space = page.push(format!("[/ICCBased {profile} 0 R]").as_bytes());
        page.icc_objects = Some((profile, space));
    }
    page.draw_page()?;
    page.finish()
}

/// One page under construction: the objects so far (index + 1 is the
/// object number), the resources the content stream names, and the
/// content itself, in document pixels — the page's own transform maps
/// those to points and turns y downwards.
struct Page<'a> {
    doc: &'a Document,
    /// The press separation, when the document has a profile: the file
    /// is then written in ink.
    separate: Option<chitrakar_color::cms::RgbToCmyk>,
    objects: Vec<Vec<u8>>,
    xobjects: Vec<(String, usize)>,
    gstates: Vec<(String, usize)>,
    content: String,
    /// (profile stream, colour space array) object numbers, in ink.
    icc_objects: Option<(usize, usize)>,
    /// Faces the page sets type in, each written once at the end: the
    /// file to embed, the resource name the content uses, and the text
    /// each glyph used so far stands for, for the ToUnicode map.
    fonts: Vec<FontUse>,
}

struct FontUse {
    face: chitrakar_render::text::FaceFile,
    resource: String,
    unicode: BTreeMap<u16, String>,
}

fn stream_object(dict_head: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("{dict_head} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

impl<'a> Page<'a> {
    fn push(&mut self, body: &[u8]) -> usize {
        self.objects.push(body.to_vec());
        self.objects.len()
    }

    fn draw_page(&mut self) -> Result<(), PdfError> {
        let root = self.doc.root();
        let children = self.doc.children_of(root)?.to_vec();
        // Layers that go as pixels, waiting to be rendered together: a run
        // of them composites the same whether drawn one by one or as one
        // picture, so long as each lands with the default blend.
        let mut pending: Vec<NodeId> = Vec::new();
        for (i, &child) in children.iter().enumerate() {
            let node = self.doc.node(child)?;
            if !node.visible || node.opacity <= 0.0 {
                continue;
            }
            match &node.kind {
                NodeKind::Adjustment(_) | NodeKind::Filter(_) => {
                    // Everything under it changes, so everything under it
                    // — what was drawn so far — becomes one picture.
                    pending.clear();
                    self.content.clear();
                    let shown = children[..=i].to_vec();
                    self.place_rendered(&shown, BlendMode::Normal)?;
                }
                _ if self.is_live(child)? => {
                    self.flush(&mut pending)?;
                    self.draw_node(child)?;
                }
                _ if node.blend == BlendMode::Normal => pending.push(child),
                // A blend reads what is under it, so the layer is rendered
                // by itself and lands with that blend.
                _ => {
                    self.flush(&mut pending)?;
                    self.place_rendered(&[child], node.blend)?;
                }
            }
        }
        self.flush(&mut pending)?;
        Ok(())
    }

    fn flush(&mut self, pending: &mut Vec<NodeId>) -> Result<(), PdfError> {
        if !pending.is_empty() {
            self.place_rendered(pending, BlendMode::Normal)?;
            pending.clear();
        }
        Ok(())
    }

    /// Whether PDF can draw the node as it is: solid paint on plain
    /// geometry, no mask or effect, a group that needs no isolating (full
    /// opacity, and every visible child live in turn), a placed image.
    fn is_live(&self, id: NodeId) -> Result<bool, PdfError> {
        let node = self.doc.node(id)?;
        if node.mask.is_some() || !node.effects.is_empty() {
            return Ok(false);
        }
        // A layer confined to the one below it, and the layer it is
        // confined to: neither can be drawn on its own, so both go to
        // pixels — and, being neighbours, into the same picture, where
        // the engine works the confinement out as it always does.
        if self.in_clip_run(id)? {
            return Ok(false);
        }
        Ok(match &node.kind {
            // A frame is a group with a rectangle clipped round it, and
            // PDF clips to a rectangle natively, so it stays live on the
            // same terms a group does.
            NodeKind::Group | NodeKind::Artboard { .. } => {
                // Less than full opacity, or a blend, applies to the
                // group's composite; its children drawn one by one would
                // each apply it against each other.
                if node.opacity < 1.0 || node.blend != BlendMode::Normal {
                    return Ok(false);
                }
                for &child in self.doc.children_of(id)? {
                    let c = self.doc.node(child)?;
                    if c.visible && c.opacity > 0.0 && !self.is_live(child)? {
                        return Ok(false);
                    }
                }
                true
            }
            NodeKind::Vector {
                gradient, stroke, ..
            } => gradient.is_none() && stroke.as_ref().is_none_or(|s| s.widths.is_empty()),
            NodeKind::Raster(_) | NodeKind::Text(_) => true,
            // A copy is drawn by drawing the original again, so it is as
            // live as the original is.
            NodeKind::Instance { of, .. } => self.is_live(*of)?,
            // A brush layer has no live form in PDF, so it goes over as
            // the pixels it paints.
            NodeKind::Paint { .. }
            | NodeKind::Clone { .. }
            | NodeKind::Adjustment(_)
            | NodeKind::Filter(_) => false,
        })
    }

    /// Render the page with only `shown` of the top-level layers visible
    /// and place what comes out as an image, trimmed to its ink, landing
    /// with `blend`. Opacity is already in the pixels; a blend is not, as
    /// there was nothing under them to blend with.
    /// Whether the node is part of a run of clipped layers — either one
    /// of the clipped layers, or the one they are all confined to.
    fn in_clip_run(&self, id: NodeId) -> Result<bool, PdfError> {
        let Some(parent) = self.doc.parent_of(id) else {
            return Ok(false);
        };
        let siblings = self.doc.children_of(parent)?;
        let Some(at) = siblings.iter().position(|&s| s == id) else {
            return Ok(false);
        };
        // The bottom-most layer has nothing under it, so a flag on it
        // confines nothing — exactly as the renderer reads it.
        if at > 0 && self.doc.node(id)?.clipped {
            return Ok(true);
        }
        Ok(siblings
            .get(at + 1)
            .map(|&above| self.doc.node(above).map(|n| n.clipped).unwrap_or(false))
            .unwrap_or(false))
    }

    fn place_rendered(&mut self, shown: &[NodeId], blend: BlendMode) -> Result<(), PdfError> {
        let mut alone = self.doc.clone();
        let root = alone.root();
        for id in alone.children_of(root)?.to_vec() {
            if !shown.contains(&id) {
                alone.apply(Command::SetVisible { id, visible: false })?;
            }
        }
        // Pixels for print: a screen-resolution document would put
        // screen-resolution text on the page, so the render is oversampled
        // towards 300 dpi (up to four times).
        let over = (300.0 / self.doc.meta.dpi.max(1.0)).clamp(1.0, 4.0);
        let meta = &self.doc.meta;
        let (w, h) = (
            (meta.width as f32 * over).ceil().max(1.0) as usize,
            (meta.height as f32 * over).ceil().max(1.0) as usize,
        );
        let mut surface = chitrakar_render::Surface::new(w as u32, h as u32);
        chitrakar_render::render_region_at(
            &alone,
            &mut surface,
            chitrakar_render::ClipRect {
                x0: 0,
                y0: 0,
                x1: w as u32,
                y1: h as u32,
            },
            Transform {
                a: over,
                d: over,
                ..Default::default()
            },
        )?;
        // The ink's bounding box; nothing to place when there is none.
        let inked = |x: usize, y: usize| surface.pixels[y * w + x].a > 0.0;
        let Some(y0) = (0..h).find(|&y| (0..w).any(|x| inked(x, y))) else {
            return Ok(());
        };
        let y1 = (0..h).rev().find(|&y| (0..w).any(|x| inked(x, y))).unwrap() + 1;
        let x0 = (0..w).find(|&x| (y0..y1).any(|y| inked(x, y))).unwrap();
        let x1 = (0..w)
            .rev()
            .find(|&x| (y0..y1).any(|y| inked(x, y)))
            .unwrap()
            + 1;
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut rgba = Vec::with_capacity(cw * ch * 4);
        for y in y0..y1 {
            for x in x0..x1 {
                rgba.extend_from_slice(&surface.pixels[y * w + x].to_srgb8());
            }
        }
        let name = self.image(cw as u32, ch as u32, &rgba)?;
        let gs = self
            .gstate(1.0, 1.0, blend)
            .map(|gs| format!("/{gs} gs\n"))
            .unwrap_or_default();
        // Placed in document pixels, however many samples it holds.
        let _ = writeln!(
            self.content,
            "q\n{gs}{} 0 0 {} {} {} cm\n/{name} Do\nQ",
            num(cw as f32 / over),
            num(-(ch as f32) / over),
            num(x0 as f32 / over),
            num(y1 as f32 / over)
        );
        Ok(())
    }

    /// An image XObject from straight sRGB pixels with alpha; the alpha
    /// becomes a soft mask when any of it is short of opaque. Returns the
    /// resource name the content stream draws it by.
    fn image(&mut self, width: u32, height: u32, rgba8: &[u8]) -> Result<String, PdfError> {
        let n = (width * height) as usize;
        let (samples, space) = match &self.separate {
            Some(sep) => {
                let srgb: Vec<f32> = rgba8
                    .chunks(4)
                    .flat_map(|p| [p[0], p[1], p[2]].map(|v| v as f32 / 255.0))
                    .collect();
                let space = self.icc_objects.expect("profile objects").1;
                (
                    sep.separate(&srgb).map_err(PdfError::Color)?,
                    format!("{space} 0 R"),
                )
            }
            None => (
                rgba8.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect(),
                "/DeviceRGB".to_string(),
            ),
        };
        let alpha: Vec<u8> = rgba8.chunks(4).map(|p| p[3]).collect();
        let smask = if alpha.iter().any(|&a| a < 255) {
            let mask = self.push(&stream_object(
                &format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
                     /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"
                ),
                &deflate(&alpha)?,
            ));
            format!(" /SMask {mask} 0 R")
        } else {
            String::new()
        };
        debug_assert_eq!(alpha.len(), n);
        let obj = self.push(&stream_object(
            &format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
                 /ColorSpace {space} /BitsPerComponent 8 /Filter /FlateDecode{smask}"
            ),
            &deflate(&samples)?,
        ));
        let name = format!("Im{}", self.xobjects.len() + 1);
        self.xobjects.push((name.clone(), obj));
        Ok(name)
    }

    /// A graphics state carrying the opacity and blend a node paints with,
    /// or nothing when both are the defaults.
    fn gstate(&mut self, fill_alpha: f32, stroke_alpha: f32, blend: BlendMode) -> Option<String> {
        if fill_alpha >= 1.0 && stroke_alpha >= 1.0 && blend == BlendMode::Normal {
            return None;
        }
        let mode = match blend {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            // PDF names them exactly as the spec does, in CamelCase.
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "ColorDodge",
            BlendMode::ColorBurn => "ColorBurn",
            BlendMode::HardLight => "HardLight",
            BlendMode::SoftLight => "SoftLight",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        };
        let obj = self.push(
            format!(
                "<< /Type /ExtGState /ca {} /CA {} /BM /{mode} >>",
                num(fill_alpha),
                num(stroke_alpha)
            )
            .as_bytes(),
        );
        let name = format!("GS{}", self.gstates.len() + 1);
        self.gstates.push((name.clone(), obj));
        Some(name)
    }

    /// The operator setting a colour, for filling (`stroke` false) or
    /// stroking: ink when the document is in ink, sRGB otherwise. Alpha
    /// is not here; it goes into the graphics state.
    fn color_op(&self, color: AuthoredColor, stroke: bool) -> Result<String, PdfError> {
        let (space_op, set_op, rgb_op) = if stroke {
            ("CS", "SC", "RG")
        } else {
            ("cs", "sc", "rg")
        };
        match (&self.separate, color) {
            (Some(_), AuthoredColor::Cmyk { c, m, y, k, .. }) => Ok(format!(
                "/CS0 {space_op} {} {} {} {} {set_op}",
                num(c.clamp(0.0, 1.0)),
                num(m.clamp(0.0, 1.0)),
                num(y.clamp(0.0, 1.0)),
                num(k.clamp(0.0, 1.0))
            )),
            (Some(sep), AuthoredColor::Srgb { r, g, b, .. }) => {
                let ink = sep.separate(&[r, g, b]).map_err(PdfError::Color)?;
                Ok(format!(
                    "/CS0 {space_op} {} {} {} {} {set_op}",
                    num(ink[0] as f32 / 255.0),
                    num(ink[1] as f32 / 255.0),
                    num(ink[2] as f32 / 255.0),
                    num(ink[3] as f32 / 255.0)
                ))
            }
            (None, color) => {
                let opaque = match color {
                    AuthoredColor::Srgb { r, g, b, .. } => AuthoredColor::Srgb { r, g, b, a: 1.0 },
                    AuthoredColor::Cmyk { c, m, y, k, .. } => {
                        AuthoredColor::Cmyk { c, m, y, k, a: 1.0 }
                    }
                };
                // Through the naive formula, as the renderer shows it
                // without a profile.
                let [r, g, b, _] = chitrakar_color::to_working(opaque).to_srgb8();
                Ok(format!(
                    "{} {} {} {rgb_op}",
                    num(r as f32 / 255.0),
                    num(g as f32 / 255.0),
                    num(b as f32 / 255.0)
                ))
            }
        }
    }

    /// Draw a live node inside the current transform.
    fn draw_node(&mut self, id: NodeId) -> Result<(), PdfError> {
        let node = self.doc.node(id)?.clone();
        self.content.push_str("q\n");
        let t = node.transform;
        if t != Transform::default() {
            let _ = writeln!(
                self.content,
                "{} {} {} {} {} {} cm",
                num(t.a),
                num(t.b),
                num(t.c),
                num(t.d),
                num(t.e),
                num(t.f)
            );
        }
        match &node.kind {
            // `is_live` already sent these down the pixel path.
            NodeKind::Paint { .. } | NodeKind::Clone { .. } => {}
            NodeKind::Group => {
                // Full opacity and the default blend by construction (see
                // is_live): nothing to set, just the children in order.
                for &child in self.doc.children_of(id)? {
                    let c = self.doc.node(child)?;
                    if c.visible && c.opacity > 0.0 {
                        self.draw_node(child)?;
                    }
                }
            }
            NodeKind::Instance { of, .. } => {
                // The original's own placement is undone first: a copy
                // puts the picture where the copy is.
                let master = self.doc.node(*of)?;
                if let Some(back) = chitrakar_render::invert(master.transform) {
                    let _ = writeln!(
                        self.content,
                        "{} {} {} {} {} {} cm",
                        num(back.a),
                        num(back.b),
                        num(back.c),
                        num(back.d),
                        num(back.e),
                        num(back.f)
                    );
                    self.draw_node(*of)?;
                }
            }
            NodeKind::Artboard {
                width,
                height,
                background,
            } => {
                // The ground first, then the frame's rectangle as the
                // clip everything inside is drawn against. Both are
                // already inside this node's own q/Q, so the clip lifts
                // with it.
                let rect = format!("0 0 {} {} re\n", num(*width), num(*height));
                if let Some(color) = background {
                    let _ = writeln!(self.content, "{}", self.color_op(*color, false)?);
                    let _ = writeln!(self.content, "{rect}f");
                }
                let _ = writeln!(self.content, "{rect}W n");
                for &child in self.doc.children_of(id)? {
                    let c = self.doc.node(child)?;
                    if c.visible && c.opacity > 0.0 {
                        self.draw_node(child)?;
                    }
                }
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                ..
            } => {
                let alpha = |c: Option<AuthoredColor>| {
                    c.map_or(1.0, |c| match c {
                        AuthoredColor::Srgb { a, .. } | AuthoredColor::Cmyk { a, .. } => a,
                    })
                };
                let gs = self.gstate(
                    node.opacity * alpha(*fill),
                    node.opacity * alpha(stroke.as_ref().map(|s| s.color)),
                    node.blend,
                );
                if let Some(gs) = gs {
                    let _ = writeln!(self.content, "/{gs} gs");
                }
                let path = path_ops(shape);
                if let Some(fill) = fill {
                    let _ = writeln!(self.content, "{}", self.color_op(*fill, false)?);
                    let rule = if matches!(shape, VectorShape::Path { .. }) {
                        "f*"
                    } else {
                        "f"
                    };
                    let _ = writeln!(self.content, "{path}{rule}");
                }
                if let Some(stroke) = stroke {
                    if stroke.width > 0.0 {
                        let _ = writeln!(self.content, "{}", self.color_op(stroke.color, true)?);
                        match shape {
                            // An inner band: clip to the shape and stroke
                            // twice the width down its edge, so the half
                            // that shows is the inside half.
                            VectorShape::Rect { .. } | VectorShape::Ellipse { .. } => {
                                let _ = writeln!(
                                    self.content,
                                    "q\n{path}W n\n{path}{} w S\nQ",
                                    num(stroke.width * 2.0)
                                );
                            }
                            // A centred line with round caps and joins, as
                            // the renderer draws it.
                            VectorShape::Path { .. } => {
                                let _ = writeln!(
                                    self.content,
                                    "1 J 1 j {} w\n{path}S",
                                    num(stroke.width)
                                );
                            }
                        }
                    }
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = self.doc.resource(&raster.resource_id) {
                    if !res.rgba8.is_empty() {
                        if let Some(gs) = self.gstate(node.opacity, node.opacity, node.blend) {
                            let _ = writeln!(self.content, "/{gs} gs");
                        }
                        let name = self.image(res.width, res.height, &res.rgba8)?;
                        // The unit square's top row is the image's first
                        // row: with y downwards here, that is a flip.
                        let _ = writeln!(
                            self.content,
                            "{} 0 0 {} 0 {} cm\n/{name} Do",
                            res.width,
                            -(res.height as i64),
                            res.height
                        );
                    }
                }
            }
            NodeKind::Text(spec) => {
                let alpha = match spec.fill {
                    AuthoredColor::Srgb { a, .. } | AuthoredColor::Cmyk { a, .. } => a,
                };
                if let Some(gs) = self.gstate(node.opacity * alpha, 1.0, node.blend) {
                    let _ = writeln!(self.content, "/{gs} gs");
                }
                let typeset = chitrakar_render::text::placed(spec);
                let font = self.font_resource(typeset.face, &typeset.glyphs);
                let _ = writeln!(
                    self.content,
                    "BT\n/{font} {} Tf\n{}",
                    num(typeset.em),
                    self.color_op(spec.fill, false)?
                );
                // Each glyph on its own matrix: the shaper's position,
                // turned the way its baseline runs, y turned back up for
                // the glyph, and the lean a synthesized italic would
                // draw with.
                for g in &typeset.glyphs {
                    let (sin, cos) = g.angle.sin_cos();
                    let lean = typeset.lean;
                    let _ = writeln!(
                        self.content,
                        "{} {} {} {} {} {} Tm <{:04X}> Tj",
                        num(cos),
                        num(sin),
                        num(lean * cos + sin),
                        num(lean * sin - cos),
                        num(g.x),
                        num(g.y),
                        g.id
                    );
                }
                self.content.push_str("ET\n");
                // Underline and strike-through: bands in the same colour.
                for [x0, y0, x1, y1] in &typeset.decorations {
                    let _ = writeln!(
                        self.content,
                        "{} {} {} {} re f",
                        num(*x0),
                        num(*y0),
                        num(x1 - x0),
                        num(y1 - y0)
                    );
                }
            }
            NodeKind::Adjustment(_) | NodeKind::Filter(_) => {
                unreachable!("not live: see is_live")
            }
        }
        self.content.push_str("Q\n");
        Ok(())
    }

    /// The resource name of the font a face is embedded as, noting the
    /// text its glyphs stand for. A face is written once however many
    /// blocks set type in it.
    fn font_resource(
        &mut self,
        face: chitrakar_render::text::FaceFile,
        glyphs: &[chitrakar_render::text::PlacedGlyph],
    ) -> String {
        let at = match self.fonts.iter().position(|f| f.face.name == face.name) {
            Some(at) => at,
            None => {
                self.fonts.push(FontUse {
                    resource: format!("F{}", self.fonts.len() + 1),
                    face,
                    unicode: BTreeMap::new(),
                });
                self.fonts.len() - 1
            }
        };
        for g in glyphs {
            // Every glyph used is noted, so the subset keeps it; only
            // those standing for text get a ToUnicode entry.
            self.fonts[at]
                .unicode
                .entry(g.id)
                .and_modify(|t| {
                    if t.is_empty() {
                        *t = g.text.clone();
                    }
                })
                .or_insert_with(|| g.text.clone());
        }
        self.fonts[at].resource.clone()
    }

    /// Write a face out: the file itself, its descriptor, the CID font
    /// addressed by glyph id, the ToUnicode map, and the Type0 font the
    /// content names. Returns the Type0 object.
    fn write_font(&mut self, at: usize) -> Result<usize, PdfError> {
        let (name, bytes, ascent, descent, count, unicode) = {
            let f = &self.fonts[at];
            let per_mille = 1000.0 / f.face.units_per_em;
            (
                f.face
                    .name
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>(),
                f.face.bytes,
                f.face.ascent * per_mille,
                f.face.descent * per_mille,
                f.face.glyph_count,
                f.unicode.clone(),
            )
        };
        let name = if name.is_empty() {
            "Face".to_string()
        } else {
            name
        };
        // Only the glyphs used travel, when the file is one that can be
        // cut down; ids stay put, so the identity map still holds.
        let used: std::collections::BTreeSet<u16> = unicode.keys().copied().collect();
        let subset = crate::subset::subset_ttf(bytes, &used);
        let bytes = subset.as_deref().unwrap_or(bytes);
        let file = self.push(&stream_object(
            &format!("<< /Length1 {} /Filter /FlateDecode", bytes.len()),
            &deflate(bytes)?,
        ));
        let descriptor = self.push(
            format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags 4 \
                 /FontBBox [-1000 {} 2000 {}] /ItalicAngle 0 /Ascent {} /Descent {} \
                 /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",
                num(descent),
                num(ascent),
                num(ascent),
                num(descent),
                num(ascent * 0.7)
            )
            .as_bytes(),
        );
        // The used glyphs' advances, so a reader lays the text out as set
        // even where it reflows it.
        let mut widths = String::new();
        for &g in unicode.keys() {
            if (g as usize) < count {
                let _ = write!(widths, "{g} [{}] ", num(self.fonts[at].face.advance(g)));
            }
        }
        let cid = self.push(
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
                 /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor {descriptor} 0 R /DW 1000 /W [{}] /CIDToGIDMap /Identity >>",
                widths.trim_end()
            )
            .as_bytes(),
        );
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
             /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
             1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let mapped: Vec<(&u16, &String)> = unicode.iter().filter(|(_, t)| !t.is_empty()).collect();
        for chunk in mapped.chunks(100) {
            let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
            for (gid, text) in chunk {
                let utf16: String = text.encode_utf16().map(|u| format!("{u:04X}")).collect();
                let _ = writeln!(cmap, "<{gid:04X}> <{utf16}>");
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        let to_unicode = self.push(&stream_object("<<", cmap.as_bytes()));
        Ok(self.push(
            format!(
                "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
                 /DescendantFonts [{cid} 0 R] /ToUnicode {to_unicode} 0 R >>"
            )
            .as_bytes(),
        ))
    }

    /// Fill in the reserved objects and write the file out.
    fn finish(mut self) -> Result<Vec<u8>, PdfError> {
        let mut font_objects = Vec::new();
        for at in 0..self.fonts.len() {
            font_objects.push((self.fonts[at].resource.clone(), self.write_font(at)?));
        }
        let meta = &self.doc.meta;
        let pt = 72.0 / meta.dpi.max(1.0);
        let (page_w, page_h) = (meta.width as f32 * pt, meta.height as f32 * pt);
        // Document pixels to points, y downwards from the top-left.
        let content = format!(
            "q\n{} 0 0 {} 0 {page_h:.3} cm\n{}Q\n",
            num(pt),
            num(-pt),
            self.content
        );
        self.objects[3] = stream_object("<<", content.as_bytes());

        let mut resources = String::new();
        if !self.xobjects.is_empty() {
            resources.push_str(" /XObject <<");
            for (name, obj) in &self.xobjects {
                let _ = write!(resources, " /{name} {obj} 0 R");
            }
            resources.push_str(" >>");
        }
        if !self.gstates.is_empty() {
            resources.push_str(" /ExtGState <<");
            for (name, obj) in &self.gstates {
                let _ = write!(resources, " /{name} {obj} 0 R");
            }
            resources.push_str(" >>");
        }
        if let Some((_, space)) = self.icc_objects {
            let _ = write!(resources, " /ColorSpace << /CS0 {space} 0 R >>");
        }
        if !font_objects.is_empty() {
            resources.push_str(" /Font <<");
            for (name, obj) in &font_objects {
                let _ = write!(resources, " /{name} {obj} 0 R");
            }
            resources.push_str(" >>");
        }
        self.objects[2] = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.3} {page_h:.3}] \
             /Resources <<{resources} >> /Contents 4 0 R >>"
        )
        .into_bytes();
        self.objects[1] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec();
        // In ink, the profile is the page's output intent as well as its
        // colour space, which is what tells a RIP the ink is meant for it.
        let intent = match self.icc_objects {
            Some((profile, _)) => format!(
                " /OutputIntents [<< /Type /OutputIntent /S /GTS_PDFX \
                 /OutputConditionIdentifier (Custom) /Info (Document press profile) \
                 /DestOutputProfile {profile} 0 R >>]"
            ),
            None => String::new(),
        };
        self.objects[0] = format!("<< /Type /Catalog /Pages 2 0 R{intent} >>").into_bytes();

        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (i, body) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        let count = offsets.len() + 1;
        out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        Ok(out)
    }
}

/// A number as PDF wants it: no exponent, no more digits than the page
/// can show.
fn num(v: f32) -> String {
    if v == 0.0 {
        return "0".to_string(); // and not "-0"
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// The path-construction operators for a shape, ending in a newline, in
/// the shape's own coordinates.
fn path_ops(shape: &VectorShape) -> String {
    let mut d = String::new();
    let p = |d: &mut String, op: &str, pts: &[[f32; 2]]| {
        for q in pts {
            let _ = write!(d, "{} {} ", num(q[0]), num(q[1]));
        }
        let _ = writeln!(d, "{op}");
    };
    match shape {
        VectorShape::Rect {
            width,
            height,
            radius,
        } => {
            let r = radius
                .max(0.0)
                .min((width / 2.0).min(height / 2.0).max(0.0));
            if r <= 0.0 {
                let _ = writeln!(d, "0 0 {} {} re", num(*width), num(*height));
            } else {
                let (w, h, k) = (*width, *height, KAPPA * r);
                p(&mut d, "m", &[[r, 0.0]]);
                p(&mut d, "l", &[[w - r, 0.0]]);
                p(&mut d, "c", &[[w - r + k, 0.0], [w, r - k], [w, r]]);
                p(&mut d, "l", &[[w, h - r]]);
                p(&mut d, "c", &[[w, h - r + k], [w - r + k, h], [w - r, h]]);
                p(&mut d, "l", &[[r, h]]);
                p(&mut d, "c", &[[r - k, h], [0.0, h - r + k], [0.0, h - r]]);
                p(&mut d, "l", &[[0.0, r]]);
                p(&mut d, "c", &[[0.0, r - k], [r - k, 0.0], [r, 0.0]]);
                d.push_str("h\n");
            }
        }
        VectorShape::Ellipse { rx, ry } => {
            let (cx, cy, kx, ky) = (*rx, *ry, KAPPA * rx, KAPPA * ry);
            p(&mut d, "m", &[[cx + rx, cy]]);
            p(
                &mut d,
                "c",
                &[[cx + rx, cy + ky], [cx + kx, cy + ry], [cx, cy + ry]],
            );
            p(
                &mut d,
                "c",
                &[[cx - kx, cy + ry], [cx - rx, cy + ky], [cx - rx, cy]],
            );
            p(
                &mut d,
                "c",
                &[[cx - rx, cy - ky], [cx - kx, cy - ry], [cx, cy - ry]],
            );
            p(
                &mut d,
                "c",
                &[[cx + kx, cy - ry], [cx + rx, cy - ky], [cx + rx, cy]],
            );
            d.push_str("h\n");
        }
        VectorShape::Path {
            points,
            closed,
            smooth,
            handles,
            subpaths,
        } => {
            let n = points.len();
            if n == 0 {
                return d;
            }
            let curved =
                handles.len() == n && handles.iter().any(|h| h.iter().any(|v| v.abs() > 1e-6));
            let segments = if *closed { n } else { n.saturating_sub(1) };
            p(&mut d, "m", &[points[0]]);
            if curved && n >= 2 {
                for i in 0..segments {
                    let j = (i + 1) % n;
                    let (a, b) = (points[i], points[j]);
                    p(
                        &mut d,
                        "c",
                        &[
                            [a[0] + handles[i][2], a[1] + handles[i][3]],
                            [b[0] + handles[j][0], b[1] + handles[j][1]],
                            b,
                        ],
                    );
                }
            } else if *smooth && n >= 3 {
                // The renderer's Catmull-Rom spline, as the cubic beziers
                // it is exactly equal to: each handle a sixth of the chord
                // between the anchor's neighbours.
                let get = |i: isize| -> [f32; 2] {
                    if *closed {
                        points[i.rem_euclid(n as isize) as usize]
                    } else {
                        points[i.clamp(0, n as isize - 1) as usize]
                    }
                };
                for i in 0..segments as isize {
                    let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
                    p(
                        &mut d,
                        "c",
                        &[
                            [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0],
                            [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0],
                            p2,
                        ],
                    );
                }
            } else {
                for q in &points[1..] {
                    p(&mut d, "l", &[*q]);
                }
            }
            if *closed {
                d.push_str("h\n");
            }
            for ring in subpaths {
                if ring.is_empty() {
                    continue;
                }
                p(&mut d, "m", &[ring[0]]);
                for q in &ring[1..] {
                    p(&mut d, "l", &[*q]);
                }
                d.push_str("h\n");
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn red_and_clear() -> Vec<LinearRgba> {
        vec![
            LinearRgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            LinearRgba::TRANSPARENT,
        ]
    }

    /// Payload of the nth `stream ... endstream` in the file (0-based).
    /// Matches "\nstream\n" rather than "stream\n": the latter also occurs
    /// inside "endstream\n", which would land on the wrong boundary.
    fn nth_stream(pdf: &[u8], n: usize) -> Vec<u8> {
        let mut cursor = 0usize;
        for _ in 0..n {
            let rel = pdf[cursor..]
                .windows(8)
                .position(|w| w == b"\nstream\n")
                .expect("enough streams");
            cursor += rel + 8;
        }
        let rel = pdf[cursor..]
            .windows(8)
            .position(|w| w == b"\nstream\n")
            .expect("stream marker");
        let start = cursor + rel + 8;
        let end = pdf[start..]
            .windows(9)
            .position(|w| w == b"endstream")
            .expect("endstream marker");
        // Streams are written with a trailing newline before endstream.
        pdf[start..start + end - 1].to_vec()
    }

    #[test]
    fn rgb_pdf_has_valid_structure_and_lossless_pixels() {
        let pdf = export_pdf(&red_and_clear(), 2, 1, 72.0, None).unwrap();

        assert!(pdf.starts_with(b"%PDF-1.7"), "header");
        assert!(pdf.ends_with(b"%%EOF\n"), "trailer");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Catalog"));
        assert!(
            text.contains("/MediaBox [0 0 2.000 1.000]"),
            "72dpi -> 1pt/px"
        );
        assert!(text.contains("/ColorSpace /DeviceRGB"));
        assert!(text.contains("/Filter /FlateDecode"));

        // The xref offsets must actually point at their objects, or readers
        // reject the file.
        let xref_at: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(&pdf[xref_at..xref_at + 4], b"xref");
        for (i, line) in text[xref_at..]
            .lines()
            .skip(2) // "xref", "0 N"
            .take_while(|l| l.ends_with(" n "))
            .enumerate()
        {
            let off: usize = line.split_whitespace().next().unwrap().parse().unwrap();
            let expect = format!("{} 0 obj", i + 1);
            assert_eq!(
                &pdf[off..off + expect.len()],
                expect.as_bytes(),
                "xref entry {i} points at its object"
            );
        }

        // Image samples round-trip losslessly: red, then white paper.
        let mut raw = Vec::new();
        ZlibDecoder::new(&nth_stream(&pdf, 1)[..])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw.len(), 6, "two RGB pixels");
        assert_eq!(&raw[0..3], &[255, 0, 0], "opaque red survives");
        assert_eq!(
            &raw[3..6],
            &[255, 255, 255],
            "transparent flattens to paper"
        );
    }

    /// Every xref entry points at its object, or readers reject the file.
    fn assert_xref_is_sound(pdf: &[u8]) {
        let text = String::from_utf8_lossy(pdf);
        let xref_at: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(&pdf[xref_at..xref_at + 4], b"xref");
        // Sliced as bytes: the lossy text runs long where image streams
        // held bytes that are not UTF-8.
        let table = String::from_utf8_lossy(&pdf[xref_at..]);
        let mut seen = 0;
        for (i, line) in table
            .lines()
            .skip(3) // "xref", "0 N", the free head
            .take_while(|l| l.ends_with(" n "))
            .enumerate()
        {
            let off: usize = line.split_whitespace().next().unwrap().parse().unwrap();
            let expect = format!("{} 0 obj", i + 1);
            assert_eq!(
                &pdf[off..off + expect.len()],
                expect.as_bytes(),
                "xref entry {i} points at its object"
            );
            seen += 1;
        }
        assert!(seen > 0);
    }

    /// The page's content stream, inflated when it is compressed (it is
    /// not; the vector writer keeps it readable).
    fn content_of(pdf: &[u8]) -> String {
        let text = String::from_utf8_lossy(pdf);
        let at = text.find("/Contents 4 0 R").expect("page");
        let _ = at;
        let obj = text.find("4 0 obj").expect("content object");
        let start = text[obj..].find("stream\n").unwrap() + obj + 7;
        let end = text[start..].find("endstream").unwrap() + start;
        text[start..end].to_string()
    }

    const RED: AuthoredColor = AuthoredColor::Srgb {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    const BLUE: AuthoredColor = AuthoredColor::Srgb {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    fn shape(name: &str, shape: VectorShape, fill: Option<AuthoredColor>) -> chitrakar_doc::Node {
        let mut node = chitrakar_doc::Node::vector(name, shape);
        if let NodeKind::Vector { fill: f, .. } = &mut node.kind {
            *f = fill;
        }
        node
    }

    /// A 10px rect with a gradient fill: a layer PDF has to take as pixels.
    fn shaded(name: &str) -> chitrakar_doc::Node {
        let mut node = shape(
            name,
            VectorShape::Rect {
                width: 10.0,
                height: 10.0,
                radius: 0.0,
            },
            Some(RED),
        );
        if let NodeKind::Vector { gradient, .. } = &mut node.kind {
            *gradient = Some(chitrakar_doc::Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 0.0],
                stops: vec![
                    chitrakar_doc::GradientStop {
                        offset: 0.0,
                        color: RED,
                    },
                    chitrakar_doc::GradientStop {
                        offset: 1.0,
                        color: BLUE,
                    },
                ],
            });
        }
        node
    }

    fn add(doc: &mut Document, node: chitrakar_doc::Node, at: [f32; 2]) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node: Box::new(node),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[index];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(at[0], at[1]),
        })
        .unwrap();
        id
    }

    /// A page of everything: a red rect, an ellipse with an inner stroke,
    /// a curved compound path with a hole, a group moving a rounded rect,
    /// a placed image with clear pixels, a text block, a hidden layer.
    fn everything() -> Document {
        let mut doc = Document::new(120, 80, chitrakar_color::ColorMode::Rgb);
        add(
            &mut doc,
            shape(
                "rect",
                VectorShape::Rect {
                    width: 40.0,
                    height: 30.0,
                    radius: 0.0,
                },
                Some(RED),
            ),
            [10.0, 10.0],
        );
        let mut ring = shape(
            "ring",
            VectorShape::Ellipse { rx: 15.0, ry: 10.0 },
            Some(BLUE),
        );
        if let NodeKind::Vector { stroke, .. } = &mut ring.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: RED,
                width: 4.0,
                widths: Vec::new(),
            });
        }
        add(&mut doc, ring, [60.0, 10.0]);
        add(
            &mut doc,
            shape(
                "curve",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [30.0, 0.0], [30.0, 30.0], [0.0, 30.0]],
                    closed: true,
                    smooth: false,
                    handles: vec![
                        [0.0, 0.0, 10.0, -8.0],
                        [-10.0, -8.0, 0.0, 0.0],
                        [0.0; 4],
                        [0.0; 4],
                    ],
                    subpaths: vec![vec![[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]]],
                },
                Some(BLUE),
            ),
            [10.0, 45.0],
        );
        let group = add(&mut doc, chitrakar_doc::Node::group("g"), [50.0, 45.0]);
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: Box::new(shape(
                "round",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 6.0,
                },
                Some(RED),
            )),
        })
        .unwrap();
        let res = doc.add_resource(2, 1, vec![0, 255, 0, 255, 0, 0, 0, 0]);
        let img = add(
            &mut doc,
            chitrakar_doc::Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: res,
                    width: 2,
                    height: 1,
                },
            ),
            [0.0, 0.0],
        );
        doc.apply(Command::SetTransform {
            id: img,
            transform: Transform {
                a: 10.0,
                d: 10.0,
                e: 80.0,
                f: 50.0,
                ..Default::default()
            },
        })
        .unwrap();
        add(
            &mut doc,
            chitrakar_doc::Node::text("t", chitrakar_doc::TextSpec::new("Hi", 16.0, BLUE)),
            [80.0, 62.0],
        );
        let hidden = add(
            &mut doc,
            shape(
                "hidden",
                VectorShape::Rect {
                    width: 120.0,
                    height: 80.0,
                    radius: 0.0,
                },
                Some(BLUE),
            ),
            [0.0, 0.0],
        );
        doc.apply(Command::SetVisible {
            id: hidden,
            visible: false,
        })
        .unwrap();
        doc
    }

    #[test]
    fn the_vector_pdf_draws_what_pdf_can_and_places_pixels_for_the_rest() {
        let doc = everything();
        let pdf = export_pdf_document(&doc).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.7") && pdf.ends_with(b"%%EOF\n"));
        assert_xref_is_sound(&pdf);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox [0 0 120.000 80.000]"));
        let content = content_of(&pdf);

        // The page maps document pixels to points with y downwards.
        assert!(
            content.starts_with("q\n1 0 0 -1 0 80.000 cm\n"),
            "{content}"
        );
        // The rect: a translation, a red fill, a `re`.
        assert!(
            content.contains("1 0 0 1 10 10 cm\n1 0 0 rg\n0 0 40 30 re\nf\n"),
            "{content}"
        );
        // The ellipse: four beziers filled, then the inner stroke as a
        // clip and a double-width stroke.
        assert!(content.contains("15 20 c\n") && content.contains("0 0 1 rg\n"));
        assert!(
            content.contains("W n\n") && content.contains("8 w S\nQ"),
            "{content}"
        );
        // The compound path: cubic segments, a second ring, even-odd.
        assert!(content.contains("10 -8 20 -8 30 0 c\n"), "{content}");
        assert!(content.contains("10 10 m\n20 10 l\n") && content.contains("h\nf*\n"));
        // The group nests its transform; the rounded rect is beziers.
        assert!(
            content.contains("1 0 0 1 50 45 cm\nq\n1 0 0 rg\n6 0 m\n14 0 l\n"),
            "{content}"
        );
        // The image: flipped into its unit square, with a soft mask for
        // the clear pixel.
        assert!(content.contains("2 0 0 -1 0 1 cm\n/Im1 Do"), "{content}");
        assert!(text.contains("/SMask"));
        assert!(text.contains("/Width 2 /Height 1"));
        // The text: live, in the embedded face, glyph by glyph.
        assert!(content.contains("BT\n/F1 "), "{content}");
        assert_eq!(
            content.matches(" Tm <").count(),
            2,
            "two glyphs of 'Hi': {content}"
        );
        assert!(
            content.contains("1 0 0 -1 0 "),
            "upright, from the block's own origin: {content}"
        );
        assert!(text.contains("/FontFile2") && text.contains("/Identity-H"));
        assert!(text.contains("/CIDToGIDMap /Identity") && text.contains("/ToUnicode"));
        assert!(text.contains("/Font << /F1 "));
        // Nothing of the hidden layer.
        assert!(!content.contains("0 0 120 80 re"));
        assert!(!text.contains(
            "/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length 0"
        ));
    }

    #[test]
    fn an_adjustment_flattens_what_is_under_it_and_vectors_go_on_above() {
        let mut doc = everything();
        let root = doc.root();
        let count = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index: count,
            node: Box::new(chitrakar_doc::Node::adjustment(
                "exp",
                chitrakar_doc::Adjustment::Exposure { stops: 1.0 },
            )),
        })
        .unwrap();
        add(
            &mut doc,
            shape(
                "top",
                VectorShape::Rect {
                    width: 5.0,
                    height: 5.0,
                    radius: 0.0,
                },
                Some(RED),
            ),
            [100.0, 70.0],
        );
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert!(
            !content.contains("0 0 40 30 re"),
            "the rect under the adjustment is in the picture now"
        );
        assert!(content.contains("0 0 5 5 re"), "the rect above it is live");
        let picture = content.find(" Do").unwrap();
        assert!(
            picture < content.find("0 0 5 5 re").unwrap(),
            "and drawn after the picture"
        );
        // The picture is trimmed to the ink: the page's top-left corner is
        // bare, so it starts at the rect's corner.
        assert!(
            content.contains(" 10 75 cm\n"),
            "trimmed to the ink, from x=10 down to y=75 ({content})"
        );
    }

    #[test]
    fn a_layer_that_needs_pixels_goes_as_pixels_and_the_rest_stays_live() {
        let mut doc = everything();
        // Opacity on a plain shape is a graphics state, not a raster.
        let root = doc.root();
        let rect = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetOpacity {
            id: rect,
            opacity: 0.5,
        })
        .unwrap();
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert!(
            content.contains("/GS1 gs\n1 0 0 rg\n0 0 40 30 re"),
            "{content}"
        );
        assert!(String::from_utf8_lossy(&export_pdf_document(&doc).unwrap())
            .contains("/ca 0.5 /CA 0.5 /BM /Normal"));
        // A gradient, a mask, an effect, a varying stroke: pixels.
        let mut shaded = shape(
            "shaded",
            VectorShape::Rect {
                width: 10.0,
                height: 10.0,
                radius: 0.0,
            },
            Some(RED),
        );
        if let NodeKind::Vector { gradient, .. } = &mut shaded.kind {
            *gradient = Some(chitrakar_doc::Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 0.0],
                stops: vec![
                    chitrakar_doc::GradientStop {
                        offset: 0.0,
                        color: RED,
                    },
                    chitrakar_doc::GradientStop {
                        offset: 1.0,
                        color: BLUE,
                    },
                ],
            });
        }
        add(&mut doc, shaded, [100.0, 0.0]);
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert!(
            !content.contains("0 0 10 10 re"),
            "the gradient rect is not drawn as a path"
        );
        // At 72 dpi it is rendered four times over for print: a 10px rect
        // is a 40-sample image placed 10 wide.
        let pdf_text = String::from_utf8_lossy(&export_pdf_document(&doc).unwrap()).to_string();
        assert!(
            pdf_text.contains("/Width 40 /Height 40"),
            "oversampled for print"
        );
        assert!(
            content.contains("10 0 0 -10 100 10 cm"),
            "placed in document pixels: {content}"
        );
        assert_eq!(
            content.matches(" Do").count(),
            2,
            "the image, then the gradient rect as a picture: {content}"
        );
    }

    #[test]
    fn pixels_keep_their_blend_and_a_run_of_them_is_one_picture() {
        let mut doc = everything();
        // Three gradient rects in a row: one picture rather than three
        // renders of the page.
        for (i, at) in [[100.0, 0.0], [100.0, 20.0], [100.0, 40.0]]
            .iter()
            .enumerate()
        {
            add(&mut doc, shaded(&format!("g{i}")), *at);
        }
        let pdf = export_pdf_document(&doc).unwrap();
        let content = content_of(&pdf);
        assert_eq!(
            content.matches(" Do").count(),
            2,
            "the placed image, then one picture of three gradients: {content}"
        );
        // A multiplied gradient reads what is under it, so it is its own
        // picture and lands with the blend.
        let root = doc.root();
        let last = *doc.children_of(root).unwrap().last().unwrap();
        doc.apply(Command::SetBlendMode {
            id: last,
            blend: BlendMode::Multiply,
        })
        .unwrap();
        let pdf = export_pdf_document(&doc).unwrap();
        let content = content_of(&pdf);
        assert_eq!(content.matches(" Do").count(), 3, "{content}");
        assert!(
            content.contains("gs\n") && String::from_utf8_lossy(&pdf).contains("/BM /Multiply"),
            "{content}"
        );
        let multiplied = content.rfind(" Do").unwrap();
        let gs = content[..multiplied].rfind("/GS").unwrap();
        assert!(
            content[gs..multiplied].contains(" gs\n"),
            "the blend is set right before it"
        );
        // A multiplied group composites as one, so it goes as pixels too.
        let group = doc.children_of(root).unwrap()[3];
        doc.apply(Command::SetBlendMode {
            id: group,
            blend: BlendMode::Multiply,
        })
        .unwrap();
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert!(
            !content.contains("6 0 m\n14 0 l"),
            "the rounded rect is no longer a path"
        );
    }

    /// Ghostscript, when it is installed, rasterizes the file; the page it
    /// draws is the page the engine draws. Self-skips without `gs`.
    #[test]
    fn ghostscript_draws_the_same_page_the_engine_does() {
        if std::process::Command::new("gs")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipped: no ghostscript");
            return;
        }
        let doc = everything();
        let pdf = export_pdf_document(&doc).unwrap();
        let dir = std::env::temp_dir().join(format!("chitrakar-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (pdf_path, png_path) = (dir.join("page.pdf"), dir.join("page.png"));
        std::fs::write(&pdf_path, &pdf).unwrap();
        let status = std::process::Command::new("gs")
            .args([
                "-q",
                "-dNOPAUSE",
                "-dBATCH",
                "-dSAFER",
                "-sDEVICE=png16m",
                "-r72",
                "-dGraphicsAlphaBits=4",
                "-dTextAlphaBits=4",
            ])
            .arg(format!("-sOutputFile={}", png_path.display()))
            .arg(&pdf_path)
            .status()
            .unwrap();
        assert!(status.success(), "ghostscript accepted the file");
        let drawn = crate::decode(&std::fs::read(&png_path).unwrap()).unwrap();
        assert_eq!((drawn.width, drawn.height), (120, 80));

        let ours = chitrakar_render::render(&doc).unwrap();
        let mut total = 0u64;
        for (i, px) in ours.pixels.iter().enumerate() {
            // Over white paper, as the page is.
            let over = |v: f32| chitrakar_color::linear_to_srgb((v + 1.0 - px.a).clamp(0.0, 1.0));
            let expect = [over(px.r), over(px.g), over(px.b)].map(|v| (v * 255.0).round() as i32);
            for (c, want) in expect.iter().enumerate() {
                total += (drawn.rgba8[i * 4 + c] as i32 - want).unsigned_abs() as u64;
            }
        }
        let mean = total as f64 / (ours.pixels.len() * 3) as f64;
        assert!(
            mean < 3.0,
            "mean channel difference {mean:.2} against the engine"
        );
        // Spot checks: inside the rect, the ellipse's band and its middle,
        // the hole in the compound path, the image's two pixels.
        let at = |x: usize, y: usize| &drawn.rgba8[(y * 120 + x) * 4..(y * 120 + x) * 4 + 3];
        assert_eq!(at(30, 25), &[255, 0, 0], "rect");
        assert!(
            at(75, 12)[0] > 200 && at(75, 12)[2] < 60,
            "ellipse band is the stroke {:?}",
            at(75, 12)
        );
        assert!(
            at(75, 20)[2] > 200 && at(75, 20)[0] < 60,
            "ellipse middle is the fill {:?}",
            at(75, 20)
        );
        assert_eq!(at(25, 60), &[255, 255, 255], "the hole shows paper");
        assert!(
            at(85, 55)[1] > 200 && at(85, 55)[0] < 60,
            "image pixel {:?}",
            at(85, 55)
        );
        assert_eq!(at(95, 55), &[255, 255, 255], "its clear pixel shows paper");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A reader can find the words: Ghostscript's text extractor reads
    /// them back through the ToUnicode map. Self-skips without `gs`.
    #[test]
    fn the_text_in_the_pdf_can_be_read_back() {
        if std::process::Command::new("gs")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipped: no ghostscript");
            return;
        }
        let mut doc = Document::new(200, 60, chitrakar_color::ColorMode::Rgb);
        let mut spec = chitrakar_doc::TextSpec::new("Office fi AV", 20.0, BLUE);
        spec.italic = true; // a synthesized lean: still text
        add(&mut doc, chitrakar_doc::Node::text("t", spec), [5.0, 5.0]);
        let pdf = export_pdf_document(&doc).unwrap();
        let dir = std::env::temp_dir().join(format!("chitrakar-pdftext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf_path = dir.join("text.pdf");
        std::fs::write(&pdf_path, &pdf).unwrap();
        let out = std::process::Command::new("gs")
            .args([
                "-q",
                "-dNOPAUSE",
                "-dBATCH",
                "-dSAFER",
                "-sDEVICE=txtwrite",
                "-o",
                "-",
            ])
            .arg(&pdf_path)
            .output()
            .unwrap();
        assert!(out.status.success(), "ghostscript read the file");
        let read = String::from_utf8_lossy(&out.stdout);
        assert!(
            read.contains("fi AV") && read.contains("ce"),
            "the words come back through ToUnicode: {read:?}"
        );
        // The "ffi" ligature is one glyph standing for three letters, and
        // the map says so (Ghostscript's extractor prints a three-letter
        // entry oddly, so this is checked in the file rather than through
        // it).
        let typeset = chitrakar_render::text::placed(&{
            let mut spec = chitrakar_doc::TextSpec::new("Office fi AV", 20.0, BLUE);
            spec.italic = true;
            spec
        });
        let ligature = typeset
            .glyphs
            .iter()
            .find(|g| g.text == "ffi")
            .expect("the face ligates ffi");
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains(&format!("<{:04X}> <006600660069>", ligature.id)),
            "the ligature maps back to its three letters"
        );
        assert!(
            content_of(&pdf).contains("1 0 0.2 -1 "),
            "the lean is in the text matrix"
        );
        // Along a guide running down the page every glyph is a quarter
        // turn: the text matrix turns with it.
        let mut spec = chitrakar_doc::TextSpec::new("Down", 20.0, BLUE);
        spec.along = Some(chitrakar_doc::VectorShape::Path {
            points: vec![[20.0, 0.0], [20.0, 200.0]],
            closed: false,
            smooth: false,
            handles: Vec::new(),
            subpaths: Vec::new(),
        });
        let mut along = Document::new(60, 200, chitrakar_color::ColorMode::Rgb);
        add(&mut along, chitrakar_doc::Node::text("d", spec), [0.0, 0.0]);
        let content = content_of(&export_pdf_document(&along).unwrap());
        assert!(
            content.contains("0 1 1 0 20 "),
            "a quarter turn, up-axis to the guide's left: {content}"
        );
        // An underline is a band after the glyphs, in the text's colour.
        let mut spec = chitrakar_doc::TextSpec::new("Under", 20.0, BLUE);
        spec.underline = true;
        let mut lined = Document::new(200, 60, chitrakar_color::ColorMode::Rgb);
        add(&mut lined, chitrakar_doc::Node::text("u", spec), [5.0, 5.0]);
        let content = content_of(&export_pdf_document(&lined).unwrap());
        let (et, band) = (
            content.find("ET\n").unwrap(),
            content.rfind(" re f\n").unwrap(),
        );
        assert!(band > et, "the band follows the glyphs: {content}");
        assert!(
            pdf.len() < 60_000,
            "the face travels as a subset: {} bytes for the whole file",
            pdf.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ink in, ink out: needs a real CMYK press profile.
    #[test]
    fn the_vector_pdf_writes_authored_ink_as_ink() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let mut doc = Document::new(40, 40, chitrakar_color::ColorMode::Cmyk);
        doc.set_cmyk_profile(icc.clone()).unwrap();
        add(
            &mut doc,
            shape(
                "ink",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                Some(AuthoredColor::Cmyk {
                    c: 0.0,
                    m: 1.0,
                    y: 0.5,
                    k: 0.1,
                    a: 1.0,
                }),
            ),
            [0.0, 0.0],
        );
        add(
            &mut doc,
            shape(
                "rgb",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                Some(RED),
            ),
            [20.0, 20.0],
        );
        let pdf = export_pdf_document(&doc).unwrap();
        assert_xref_is_sound(&pdf);
        let text = String::from_utf8_lossy(&pdf);
        let content = content_of(&pdf);
        assert!(
            content.contains("/CS0 cs 0 1 0.5 0.1 sc"),
            "authored ink goes in as typed: {content}"
        );
        // The sRGB red separates to magenta and yellow through the profile.
        let sep = content
            .lines()
            .filter(|l| l.starts_with("/CS0 cs"))
            .nth(1)
            .unwrap();
        let ink: Vec<f32> = sep
            .split_whitespace()
            .skip(2)
            .take(4)
            .map(|v| v.parse().unwrap())
            .collect();
        assert!(
            ink[1] > 0.4 && ink[2] > 0.4 && ink[0] < 0.35,
            "red as ink: {ink:?}"
        );
        assert!(text.contains("[/ICCBased 5 0 R]") && text.contains("/N 4"));
        assert!(text.contains("/OutputIntents") && text.contains("/DestOutputProfile 5 0 R"));
        assert!(text.contains("/ColorSpace << /CS0 6 0 R >>"));
    }

    #[test]
    fn cmyk_pdf_embeds_the_profile_and_separates_ink() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let pdf = export_pdf(&red_and_clear(), 2, 1, 300.0, Some(&icc)).unwrap();
        let text = String::from_utf8_lossy(&pdf);

        assert!(text.contains("[/ICCBased 7 0 R]"), "colorspace is indirect");
        assert!(text.contains("/N 4"), "four ink components");
        assert!(
            text.contains("/MediaBox [0 0 0.480 0.240]"),
            "300dpi page size"
        );

        // Ink data: red separates to magenta+yellow, paper stays near bare.
        let mut ink = Vec::new();
        ZlibDecoder::new(&nth_stream(&pdf, 1)[..])
            .read_to_end(&mut ink)
            .unwrap();
        assert_eq!(ink.len(), 8, "two CMYK pixels");
        assert!(
            ink[1] > 100 && ink[2] > 100 && ink[0] < 90,
            "red ink {:?}",
            &ink[0..4]
        );
        assert!(ink[4..8].iter().all(|v| *v < 40), "paper {:?}", &ink[4..8]);

        // The profile itself travels with the file.
        let mut embedded = Vec::new();
        ZlibDecoder::new(&nth_stream(&pdf, 2)[..])
            .read_to_end(&mut embedded)
            .unwrap();
        assert_eq!(embedded, icc, "profile embedded verbatim");
    }
}
