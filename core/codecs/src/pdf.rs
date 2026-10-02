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
        doc: doc.clone(),
        separate: icc
            .map(chitrakar_color::cms::RgbToCmyk::new)
            .transpose()
            .map_err(PdfError::Color)?,
        objects: Vec::new(),
        pages: Vec::new(),
        xobjects: Vec::new(),
        forms: Vec::new(),
        gstates: Vec::new(),
        content: String::new(),
        icc_objects: None,
        fonts: Vec::new(),
    };
    // 1 catalog and 2 pages: reserved, written last, once every page
    // they list has a number. The pages themselves are pushed at the
    // end, after everything they refer to.
    for _ in 0..2 {
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
    let meta = &page.doc.meta;
    let whole = [0.0, 0.0, meta.width as f32, meta.height as f32];
    let body = std::mem::take(&mut page.content);
    page.pages.push((body, whole));
    page.finish()
}

/// The document's frames as the pages of one file, in the order they sit
/// on the page — a brochure laid out as artboards comes out a brochure.
///
/// Each page shows one frame and is that frame's own size; what lies
/// outside it is off the page rather than drawn, which is what a frame
/// means. A document with no frames has one page, as before.
pub fn export_pdf_frames(doc: &Document) -> Result<Vec<u8>, PdfError> {
    let root = doc.root();
    let frames: Vec<NodeId> = doc
        .children_of(root)
        .map(|kids| {
            kids.iter()
                .copied()
                .filter(|id| {
                    doc.node(*id)
                        .is_ok_and(|n| n.visible && matches!(n.kind, NodeKind::Artboard { .. }))
                })
                .collect()
        })
        .unwrap_or_default();
    if frames.is_empty() {
        return export_pdf_document(doc);
    }
    let icc = doc.cmyk_profile_bytes();
    let mut page = Page {
        doc: doc.clone(),
        separate: icc
            .map(chitrakar_color::cms::RgbToCmyk::new)
            .transpose()
            .map_err(PdfError::Color)?,
        objects: Vec::new(),
        pages: Vec::new(),
        xobjects: Vec::new(),
        forms: Vec::new(),
        gstates: Vec::new(),
        content: String::new(),
        icc_objects: None,
        fonts: Vec::new(),
    };
    for _ in 0..2 {
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
    for frame in frames {
        // One frame at a time, with the rest of the page hidden: the
        // drawing is the drawing it always was, and the page's own box
        // is what says which part of it is this page.
        let mut alone = doc.clone();
        for id in alone.children_of(root)?.to_vec() {
            if id != frame {
                alone.apply(Command::SetVisible { id, visible: false })?;
            }
        }
        // The frame's own box, not what it can touch: a frame cuts what
        // goes into it to its box, so that box is the page.
        let chitrakar_render::Bounds::Rect(x0, y0, x1, y1) =
            chitrakar_render::node_visual_bounds(doc, frame)?
        else {
            continue;
        };
        let box_ = [x0, y0, x1, y1];
        page.doc = alone;
        page.content.clear();
        page.draw_page()?;
        let body = std::mem::take(&mut page.content);
        page.pages.push((body, box_));
    }
    page.finish()
}

/// One page under construction: the objects so far (index + 1 is the
/// object number), the resources the content stream names, and the
/// content itself, in document pixels — the page's own transform maps
/// those to points and turns y downwards.
struct Page {
    /// The document this page draws. Owned, because exporting the
    /// frames as pages draws a differently-hidden copy of it per page
    /// while keeping one pool of objects for the file.
    doc: Document,
    /// The press separation, when the document has a profile: the file
    /// is then written in ink.
    separate: Option<chitrakar_color::cms::RgbToCmyk>,
    objects: Vec<Vec<u8>>,
    /// The pages written so far: each a finished content stream and the
    /// document-space box it shows. One page for the whole document,
    /// or one per frame.
    pages: Vec<(String, [f32; 4])>,
    xobjects: Vec<(String, usize)>,
    /// Transparency groups: the object number reserved for each and what
    /// it draws. Written at the end, once the resources every content
    /// stream shares are known.
    /// A soft mask's group is the third: grey, read for its luminosity.
    forms: Vec<(usize, String, bool)>,
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

/// Whether what a layer draws depends on what is already under it in a
/// way PDF cannot say: an adjustment or a filter changes it, a clone
/// layer lifts from it, and a copy of any of the three does the same
/// where the copy stands. A *group* holding one is not among them — the
/// engine isolates such a group, so what is inside it works only on
/// what is inside it, and the group drawn on its own is already right.
fn works_on_what_is_under(doc: &Document, id: NodeId) -> bool {
    matches!(
        doc.node(id).map(|n| &n.kind),
        Ok(NodeKind::Adjustment(_) | NodeKind::Filter(_) | NodeKind::Clone { .. })
    ) || chitrakar_render::rewrites_what_is_under_it(doc, id)
        || chitrakar_render::copies_a_clone(doc, id)
}

fn stream_object(dict_head: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("{dict_head} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

impl Page {
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
        let mut next = 0;
        while next < children.len() {
            let i = next;
            let child = children[i];
            // A layer and the ones held to it go down together, whatever
            // they are: what is held is cut by the layer under it, which a
            // picture of one without the other cannot do. A plain run
            // joins the pictures waiting; one with a blend in it, or with
            // something in it that works on what is under it, is the page
            // so far as one picture. Split, a layer held to a filter went
            // into a picture of its own with the filter hidden, where the
            // hold let nothing through, and was in no PDF.
            let mut end = i + 1;
            while end < children.len() && self.doc.node(children[end])?.clipped {
                end += 1;
            }
            next = end;
            if end > i + 1 {
                let run = &children[i..end];
                // Held live, where every layer of the run can be.
                if self.siblings_live(run)? {
                    self.flush(&mut pending)?;
                    self.draw_siblings(run)?;
                    continue;
                }
                let mut whole = false;
                for &c in run {
                    let n = self.doc.node(c)?;
                    whole |= n.blend != BlendMode::Normal || works_on_what_is_under(&self.doc, c);
                }
                if whole {
                    pending.clear();
                    self.content.clear();
                    let shown = children[..end].to_vec();
                    self.place_rendered(&shown, BlendMode::Normal)?;
                } else {
                    pending.extend_from_slice(run);
                }
                continue;
            }
            let node = self.doc.node(child)?.clone();
            if !node.visible || node.opacity <= 0.0 {
                continue;
            }
            match &node.kind {
                // And a clone layer, which lifts from it, and a copy of any
                // of the three. Placed as pixels on its own, a clone had
                // nothing under it to lift and laid nothing, and every PDF
                // of a retouched page came out without the retouching.
                _ if works_on_what_is_under(&self.doc, child) => {
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
        // A mask is a soft mask on a transparency group (`soft_mask`); an
        // effect has no live form here.
        if !node.effects.is_empty() {
            return Ok(false);
        }
        // A layer held to the one below it is drawn under a soft mask of
        // that one (`draw_siblings`); whether a run can be is asked of the
        // run, by whoever holds it (`siblings_live`).
        Ok(match &node.kind {
            // A frame is a group with a rectangle clipped round it, and
            // PDF clips to a rectangle natively, so it stays live on the
            // same terms a group does.
            // Less than full opacity, or a blend, applies to the group's
            // composite, which is a transparency group (`draw_node`).
            NodeKind::Group | NodeKind::Artboard { .. } => {
                self.siblings_live(self.doc.children_of(id)?)?
            }
            NodeKind::Vector {
                gradient, stroke, ..
            } => gradient.is_none() && stroke.as_ref().is_none_or(|s| s.widths.is_empty()),
            NodeKind::Raster(_) | NodeKind::Text(_) => true,
            // A copy is drawn by drawing the original again, so it is as
            // live as the original is — or, where it stands in for some
            // of the original's layers, as live as what it actually
            // draws, which is not the same list.
            NodeKind::Instance { of, .. } => {
                let stand_ins = if chitrakar_render::takes_stand_ins(&self.doc, *of) {
                    chitrakar_render::copy_children(&self.doc, id)?
                } else {
                    Vec::new()
                };
                // Faded or blended, a copy is composited as one picture,
                // as a group is — a transparency group (`draw_node`). Drawn
                // live without one, nothing applied its opacity at all: a
                // copy faded to a third was solid in every PDF.
                if self.doc.node(*of).is_err() {
                    // A copy of a layer that has since gone draws
                    // nothing, which PDF says as well as anything.
                    true
                } else if stand_ins.is_empty() {
                    self.is_live(*of)?
                } else {
                    self.siblings_live(&stand_ins)?
                }
            }
            // A brush layer of hard strokes is what each stroke covers
            // (`strokes`), filled in its colour. The renderer lays the
            // strokes first and fades and blends the finished layer, so
            // several strokes faded or blended are a transparency group
            // (`draw_node`); two strokes each faded where they overlap
            // would be faded twice there.
            NodeKind::Paint { strokes } => crate::strokes::sayable(strokes),
            // A soft stroke, an eraser, a clone's lifting: no live form in
            // PDF, so the layer goes over as the pixels it paints.
            NodeKind::Clone { .. } | NodeKind::Adjustment(_) | NodeKind::Filter(_) => false,
        })
    }

    /// Render the page with only `shown` of the top-level layers visible
    /// and place what comes out as an image, trimmed to its ink, landing
    /// with `blend`. Opacity is already in the pixels; a blend is not, as
    /// there was nothing under them to blend with.
    fn place_rendered(&mut self, shown: &[NodeId], blend: BlendMode) -> Result<(), PdfError> {
        // Everything else is put aside into a hidden group rather than
        // hidden itself: a copy draws what it copies only while that is
        // visible, and a copy drawn as a picture of its own — one wearing
        // a blend — found its original hidden here and drew nothing. Put
        // aside, the original is still visible and still not on the page.
        // The group stands where the page does, so nothing moved into it
        // moves.
        let mut alone = self.doc.clone();
        let root = alone.root();
        let aside: Vec<NodeId> = alone
            .children_of(root)?
            .iter()
            .copied()
            .filter(|id| !shown.contains(id))
            .collect();
        if !aside.is_empty() {
            let holder = alone.peek_next_id();
            alone.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(chitrakar_doc::Node::group("aside")),
            })?;
            alone.apply(Command::SetVisible {
                id: holder,
                visible: false,
            })?;
            for (k, id) in aside.into_iter().enumerate() {
                alone.apply(Command::MoveNode {
                    id,
                    parent: holder,
                    index: k,
                })?;
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
        // Out to whole pixels of the page. Cropped at the ink, a picture
        // oversampled four times started at a quarter of a pixel, and a
        // reader bringing it down to the page averaged samples from two
        // page pixels into one — every edge in it softened by a reader,
        // an upright frame's crisp one included.
        let step = over.round().max(1.0) as usize;
        let snapped = (over - step as f32).abs() < 1e-6;
        let (x0, y0, x1, y1) = if snapped {
            (
                x0 / step * step,
                y0 / step * step,
                x1.div_ceil(step) * step,
                y1.div_ceil(step) * step,
            )
        } else {
            (x0, y0, x1, y1)
        };
        let (x1, y1) = (x1.min(w), y1.min(h));
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
        self.gstate_masked(fill_alpha, stroke_alpha, blend, None)
    }

    /// The same, and a soft mask: a group and how it is read — its grey
    /// (`Luminosity`, a mask's picture) or its alpha (`Alpha`, the layer a
    /// held one is held to) — saying how much of what is painted under
    /// this state shows.
    fn gstate_masked(
        &mut self,
        fill_alpha: f32,
        stroke_alpha: f32,
        blend: BlendMode,
        mask: Option<(usize, &str)>,
    ) -> Option<String> {
        if fill_alpha >= 1.0 && stroke_alpha >= 1.0 && blend == BlendMode::Normal && mask.is_none()
        {
            return None;
        }
        let soft = match mask {
            Some((g, read)) => format!(" /SMask << /Type /Mask /S /{read} /G {g} 0 R >>"),
            None => String::new(),
        };
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
                "<< /Type /ExtGState /ca {} /CA {} /BM /{mode}{soft} >>",
                num(fill_alpha),
                num(stroke_alpha)
            )
            .as_bytes(),
        );
        let name = format!("GS{}", self.gstates.len() + 1);
        self.gstates.push((name.clone(), obj));
        Some(name)
    }

    /// A stretch set in a bold no face could supply, thickened the way
    /// the page thickens it: the outline swept `thicken` along its own
    /// baseline, which widens the stems and leaves the height alone.
    ///
    /// PDF's own way to thicken text is to stroke it as well as fill it
    /// (`2 Tr`), and that is not this: a pen is round, so the outline grows
    /// as much up and down as sideways, and ghostscript put a fifth more
    /// ink on than the page did at any resolution. Squashing the pen flat
    /// does not survive a reader either, which strokes text under the
    /// text matrix and so undoes whatever squashed it. So the ink is drawn
    /// as what the page draws — the outline swept along the baseline,
    /// which is exactly the outline where the sweep starts and the band
    /// each of its edges sweeps (whatever the sweep reaches that the
    /// outline does not, it reached by crossing an edge) — as one path,
    /// all wound the same way and filled once. One paint puts
    /// a faded or blended bold down once, where fill and stroke overlapped
    /// and put it down twice. The text itself is set over it invisibly,
    /// so it can still be found and copied.
    fn heavy_run(
        &mut self,
        run: &chitrakar_render::text::PlacedRun,
        font: &str,
    ) -> Result<(), PdfError> {
        use chitrakar_render::text::GlyphCurve;
        let _ = writeln!(self.content, "BT\n/{font} {} Tf\n3 Tr", num(run.em));
        for g in &run.glyphs {
            let (sin, cos) = g.angle.sin_cos();
            let lean = run.lean;
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

        let k = run.em / run.face.units_per_em;
        let mut path = String::new();
        for g in &run.glyphs {
            let curves = run.face.outline(g.id);
            if curves.is_empty() {
                continue;
            }
            let (sin, cos) = g.angle.sin_cos();
            // Font units, y up, to the page: scaled, leaned, turned back
            // up, turned the way the baseline runs.
            let place = |p: [f32; 2]| {
                let (x, y) = (k * (p[0] + run.lean * p[1]), -k * p[1]);
                [g.x + x * cos - y * sin, g.y + x * sin + y * cos]
            };
            let at = |p: [f32; 2]| format!("{} {}", num(p[0]), num(p[1]));
            // The outline where the sweep starts.
            let mut last: Option<[f32; 2]> = None;
            for c in &curves {
                let (from, to) = match *c {
                    GlyphCurve::Line(a, b) => (a, b),
                    GlyphCurve::Quad(a, _, b) => (a, b),
                    GlyphCurve::Cubic(a, _, _, b) => (a, b),
                };
                if last != Some(from) {
                    if last.is_some() {
                        path.push_str("h\n");
                    }
                    let _ = writeln!(path, "{} m", at(place(from)));
                }
                let _ = match *c {
                    GlyphCurve::Line(_, b) => writeln!(path, "{} l", at(place(b))),
                    // A quadratic is the cubic with its handles two
                    // thirds of the way to the control point.
                    GlyphCurve::Quad(a, q, b) => {
                        let third = |e: [f32; 2]| {
                            [
                                e[0] + (q[0] - e[0]) * 2.0 / 3.0,
                                e[1] + (q[1] - e[1]) * 2.0 / 3.0,
                            ]
                        };
                        writeln!(
                            path,
                            "{} {} {} c",
                            at(place(third(a))),
                            at(place(third(b))),
                            at(place(b))
                        )
                    }
                    GlyphCurve::Cubic(_, p, q, b) => {
                        writeln!(path, "{} {} {} c", at(place(p)), at(place(q)), at(place(b)))
                    }
                };
                last = Some(to);
            }
            path.push_str("h\n");
            // The band each edge sweeps: between the edge and the edge
            // moved along, closed across its ends — exactly the swept
            // region wherever the edge runs one way across the sweep, so
            // a curve is cut where it turns back. Each band is wound the
            // way the outline is, so that under the nonzero rule nothing
            // cancels. Where the outline winds is read off its control
            // polygon, which outer contours dominate.
            let pieces: Vec<[[f32; 2]; 4]> = curves
                .iter()
                .flat_map(|c| {
                    let cubic = match *c {
                        GlyphCurve::Line(a, b) => {
                            let (a, b) = (place(a), place(b));
                            return vec![[a, a, b, b]];
                        }
                        GlyphCurve::Quad(a, q, b) => {
                            let third = |e: [f32; 2]| {
                                [
                                    e[0] + (q[0] - e[0]) * 2.0 / 3.0,
                                    e[1] + (q[1] - e[1]) * 2.0 / 3.0,
                                ]
                            };
                            [place(a), place(third(a)), place(third(b)), place(b)]
                        }
                        GlyphCurve::Cubic(a, p, q, b) => [place(a), place(p), place(q), place(b)],
                    };
                    one_way_across(cubic, [-sin, cos])
                })
                .collect();
            let cross = |a: [f32; 2], b: [f32; 2]| a[0] * b[1] - a[1] * b[0];
            let winding: f32 = pieces
                .iter()
                .map(|c| cross(c[0], c[1]) + cross(c[1], c[2]) + cross(c[2], c[3]))
                .sum();
            let d = [run.thicken * cos, run.thicken * sin];
            let plus = |p: [f32; 2]| [p[0] + d[0], p[1] + d[1]];
            for c in &pieces {
                let turn = cross([c[3][0] - c[0][0], c[3][1] - c[0][1]], d);
                if turn.abs() < 1e-6 {
                    continue;
                }
                let c = if (turn > 0.0) == (winding > 0.0) {
                    *c
                } else {
                    [c[3], c[2], c[1], c[0]]
                };
                let _ = writeln!(
                    path,
                    "{} m {} {} {} c {} l {} {} {} c h",
                    at(c[0]),
                    at(c[1]),
                    at(c[2]),
                    at(c[3]),
                    at(plus(c[3])),
                    at(plus(c[2])),
                    at(plus(c[1])),
                    at(plus(c[0]))
                );
            }
        }
        if !path.is_empty() {
            let _ = write!(
                self.content,
                "{}\n{path}f\n",
                self.color_op(&run.fill, false)?
            );
        }
        Ok(())
    }

    /// The operator setting a colour, for filling (`stroke` false) or
    /// stroking: ink when the document is in ink, sRGB otherwise. Alpha
    /// is not here; it goes into the graphics state.
    fn color_op(&self, color: &AuthoredColor, stroke: bool) -> Result<String, PdfError> {
        // A PDF carries no palette of ours, so a colour standing for a
        // swatch prints as what that swatch means.
        let color = color.flat();
        let (space_op, set_op, rgb_op) = if stroke {
            ("CS", "SC", "RG")
        } else {
            ("cs", "sc", "rg")
        };
        match (&self.separate, color) {
            // `flat` returns one of the two below.
            (_, AuthoredColor::Named { .. }) => unreachable!(),
            (Some(_), AuthoredColor::Cmyk { c, m, y, k, .. }) => Ok(format!(
                "/CS0 {space_op} {} {} {} {} {set_op}",
                num(c.clamp(0.0, 1.0)),
                num(m.clamp(0.0, 1.0)),
                num(y.clamp(0.0, 1.0)),
                num(k.clamp(0.0, 1.0))
            )),
            (Some(sep), AuthoredColor::Srgb { r, g, b, .. }) => {
                let ink = sep.separate(&[*r, *g, *b]).map_err(PdfError::Color)?;
                Ok(format!(
                    "/CS0 {space_op} {} {} {} {} {set_op}",
                    num(ink[0] as f32 / 255.0),
                    num(ink[1] as f32 / 255.0),
                    num(ink[2] as f32 / 255.0),
                    num(ink[3] as f32 / 255.0)
                ))
            }
            (None, color) => {
                let opaque = match *color {
                    AuthoredColor::Srgb { r, g, b, .. } => AuthoredColor::Srgb { r, g, b, a: 1.0 },
                    AuthoredColor::Cmyk { c, m, y, k, .. } => {
                        AuthoredColor::Cmyk { c, m, y, k, a: 1.0 }
                    }
                    // `flat` returns one of the two above.
                    AuthoredColor::Named { .. } => unreachable!(),
                };
                // Through the naive formula, as the renderer shows it
                // without a profile.
                let [r, g, b, _] = chitrakar_color::to_working(&opaque).to_srgb8();
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
        self.draw_node_held(id, None)
    }

    /// Draw a layer, and — when `hold` is the layer under it drawn as a
    /// group of its own — only where that layer is: a soft mask read by
    /// its alpha, which is the engine's hold (`Cover`). The mask is set in
    /// the space the two layers share, before this one's own placement;
    /// the layer goes down as one isolated group under it, so a fill and
    /// a stroke over it are held once.
    fn draw_node_held(&mut self, id: NodeId, hold: Option<usize>) -> Result<(), PdfError> {
        let node = self.doc.node(id)?.clone();
        self.content.push_str("q\n");
        if let Some(g) = hold {
            if let Some(gs) = self.gstate_masked(1.0, 1.0, BlendMode::Normal, Some((g, "Alpha"))) {
                let _ = writeln!(self.content, "/{gs} gs");
            }
        }
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
        // A layer the engine composites as one picture — a group, a frame,
        // a copy, a brush layer's several strokes — and then fades or
        // blends, is a transparency group here: drawn into a form of its
        // own, isolated as the engine isolates it, and that form laid down
        // once with the fade and the blend. Its parts drawn one by one
        // would each take the fade, twice where two overlap, and each
        // blend against the others; so such a layer went as pixels.
        //
        // And any layer with a mask, which the engine takes once over the
        // layer's own surface: a shape's fill and its stroke masked each
        // as it painted would be masked twice where they overlap. The mask
        // goes on the group as it lands, as a soft mask (`soft_mask`).
        let composite = matches!(
            &node.kind,
            NodeKind::Group
                | NodeKind::Artboard { .. }
                | NodeKind::Instance { .. }
                | NodeKind::Paint { .. }
        );
        let many = match &node.kind {
            NodeKind::Paint { strokes } => strokes.len() > 1,
            _ => composite,
        };
        let isolate = node.mask.is_some()
            || hold.is_some()
            || (many && (node.opacity < 1.0 || node.blend != BlendMode::Normal));
        let outer = isolate.then(|| std::mem::take(&mut self.content));
        // Inside the group, what it draws goes down plainly and the blend
        // is the group's, taken as it lands. So is the fade of a layer the
        // engine fades once, over the whole of it; a shape, a picture or
        // type is faded as it paints, there as here, and keeps its fade.
        let (opacity, blend) = match (isolate, composite) {
            (false, _) => (node.opacity, node.blend),
            (true, true) => (1.0, BlendMode::Normal),
            (true, false) => (node.opacity, BlendMode::Normal),
        };
        match &node.kind {
            // Hard strokes, each a filled path of what it covers, in the
            // order they were laid (see `is_live`).
            NodeKind::Paint { strokes } => {
                for stroke in strokes {
                    let outlines = crate::strokes::outlines(stroke);
                    if outlines.is_empty() {
                        continue;
                    }
                    // A state of its own: a translucent stroke's alpha is
                    // graphics state, and would otherwise outlive it onto
                    // an opaque one after it.
                    self.content.push_str("q\n");
                    if let Some(gs) = self.gstate(opacity * stroke.color.alpha(), 1.0, blend) {
                        let _ = writeln!(self.content, "/{gs} gs");
                    }
                    let mut path = String::new();
                    for o in outlines {
                        let _ = writeln!(path, "{} {} m", num(o.start[0]), num(o.start[1]));
                        for p in &o.pieces {
                            let _ = match p {
                                crate::strokes::Piece::Line(e) => {
                                    writeln!(path, "{} {} l", num(e[0]), num(e[1]))
                                }
                                crate::strokes::Piece::Cubic(c1, c2, e) => writeln!(
                                    path,
                                    "{} {} {} {} {} {} c",
                                    num(c1[0]),
                                    num(c1[1]),
                                    num(c2[0]),
                                    num(c2[1]),
                                    num(e[0]),
                                    num(e[1])
                                ),
                            };
                        }
                        path.push_str("h\n");
                    }
                    // Nonzero: the outlines of one stroke overlap where its
                    // segments meet, and a stroke lays its paint once.
                    let _ = write!(
                        self.content,
                        "{}\n{path}f\nQ\n",
                        self.color_op(&stroke.color, false)?
                    );
                }
            }
            // `is_live` already sent these down the pixel path.
            NodeKind::Clone { .. } => {}
            NodeKind::Group => {
                // The group's own fade and blend, if it has either, are
                // the transparency group it is drawn into: nothing to set
                // here, just the children in order.
                let kids = self.doc.children_of(id)?.to_vec();
                self.draw_siblings(&kids)?;
            }
            // A block to leave rather than a function to return from:
            // every way out has to reach the `Q` below, which closes the
            // copy's own `q` and takes its placement off again. Returned
            // from, a copy with layers of its own standing in left its
            // placement on everything drawn after it in the PDF.
            NodeKind::Instance { of, .. } => 'copy: {
                // Where the copy stands in for some of the original's
                // layers with layers of its own, what goes over is what
                // the copy draws. Those layers are written in the
                // original group's own child space — which is the space
                // undoing the original's placement arrives at — so they
                // are drawn here with nothing between.
                let stand_ins = if chitrakar_render::takes_stand_ins(&self.doc, *of) {
                    chitrakar_render::copy_children(&self.doc, id)?
                } else {
                    Vec::new()
                };
                if !stand_ins.is_empty() {
                    self.draw_siblings(&stand_ins)?;
                    break 'copy;
                }
                // The original's own placement is undone first: a copy
                // puts the picture where the copy is. A copy of a layer
                // that has since gone draws nothing, as on the page —
                // asked with `?`, the whole export failed.
                let Ok(master) = self.doc.node(*of) else {
                    break 'copy;
                };
                // Nor does a copy of one that is hidden or faded to
                // nothing: a copy draws what the original draws, and that
                // is nothing. Drawn anyway, a copy of a hidden layer was
                // in every PDF and on no page.
                if !master.visible || master.opacity <= 0.0 {
                    break 'copy;
                }
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
                ..
            } => {
                // The ground first, then the frame's rectangle as the
                // clip everything inside is drawn against. Both are
                // already inside this node's own q/Q, so the clip lifts
                // with it.
                let rect = format!("0 0 {} {} re\n", num(*width), num(*height));
                if let Some(color) = background {
                    let _ = writeln!(self.content, "{}", self.color_op(color, false)?);
                    let _ = writeln!(self.content, "{rect}f");
                }
                let _ = writeln!(self.content, "{rect}W n");
                let kids = self.doc.children_of(id)?.to_vec();
                self.draw_siblings(&kids)?;
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                ..
            } => {
                let alpha = |c: Option<&AuthoredColor>| c.map_or(1.0, |c| c.alpha());
                let gs = self.gstate(
                    opacity * alpha(fill.as_ref()),
                    opacity * alpha(stroke.as_ref().map(|s| &s.color)),
                    blend,
                );
                if let Some(gs) = gs {
                    let _ = writeln!(self.content, "/{gs} gs");
                }
                let path = path_ops(shape);
                if let Some(fill) = fill {
                    let _ = writeln!(self.content, "{}", self.color_op(fill, false)?);
                    let rule = if matches!(shape, VectorShape::Path { .. }) {
                        "f*"
                    } else {
                        "f"
                    };
                    let _ = writeln!(self.content, "{path}{rule}");
                }
                if let Some(stroke) = stroke {
                    if stroke.width > 0.0 {
                        let _ = writeln!(self.content, "{}", self.color_op(&stroke.color, true)?);
                        // PDF's own dash: the same lengths on and off,
                        // starting at the beginning of the line.
                        let dash: Vec<String> = stroke.dash.iter().map(|d| num(*d)).collect();
                        let _ = writeln!(
                            self.content,
                            "[{}] 0 d",
                            if dash.is_empty() {
                                String::new()
                            } else {
                                dash.join(" ")
                            }
                        );
                        match shape {
                            // A band to one side of the edge is a
                            // straddling band on a shape moved half a
                            // width that way — which is how SVG is given
                            // it too. Clipping to one side would do it as
                            // well, but a clip's own edge is antialiased
                            // against an edge the fill already
                            // antialiased, and the two do not add up: a
                            // seam all the way round.
                            VectorShape::Rect { .. } | VectorShape::Ellipse { .. } => {
                                let off = match chitrakar_doc::stroke_align(shape, stroke) {
                                    chitrakar_doc::StrokeAlign::Inside => stroke.width / 2.0,
                                    chitrakar_doc::StrokeAlign::Centre => 0.0,
                                    chitrakar_doc::StrokeAlign::Outside => -stroke.width / 2.0,
                                };
                                let w = num(stroke.width);
                                if off == 0.0 {
                                    let _ = writeln!(self.content, "{w} w\n{path}S");
                                } else if let Some(moved) = chitrakar_render::inset(shape, off) {
                                    let _ = writeln!(
                                        self.content,
                                        "q\n1 0 0 1 {} {} cm\n{}{w} w S\nQ",
                                        num(off),
                                        num(off),
                                        path_ops(&moved)
                                    );
                                }
                                // A band as thick as the shape leaves no
                                // shape to move, and nothing to draw.
                            }
                            // A centred line, ending and turning the way
                            // the renderer draws it. PDF numbers the same
                            // three of each: butt, round, square for a
                            // cap; miter, round, bevel for a join.
                            VectorShape::Path { .. } => {
                                let cap = match stroke.cap {
                                    chitrakar_doc::StrokeCap::Butt => 0,
                                    chitrakar_doc::StrokeCap::Round => 1,
                                    chitrakar_doc::StrokeCap::Square => 2,
                                };
                                let join = match stroke.join {
                                    chitrakar_doc::StrokeJoin::Miter => 0,
                                    chitrakar_doc::StrokeJoin::Round => 1,
                                    chitrakar_doc::StrokeJoin::Bevel => 2,
                                };
                                let _ = writeln!(
                                    self.content,
                                    "{cap} J {join} j {} M {} w\n{path}S",
                                    num(chitrakar_doc::MITER_LIMIT),
                                    num(stroke.width)
                                );
                                // PDF has no markers, so what a line
                                // carries at its ends is drawn: filled
                                // outlines in the line's own colour,
                                // which keeps them vector rather than a
                                // picture of themselves.
                                for piece in marker_outlines(shape, stroke) {
                                    let _ = writeln!(
                                        self.content,
                                        "{}\n{piece}f",
                                        self.color_op(&stroke.color, false)?
                                    );
                                }
                            }
                        }
                    }
                }
            }
            NodeKind::Raster(raster) => {
                if let Some(res) = self.doc.resource(&raster.resource_id).cloned() {
                    if !res.rgba8.is_empty() {
                        if let Some(gs) = self.gstate(opacity, opacity, blend) {
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
                let alpha = spec.fill.alpha();
                if let Some(gs) = self.gstate(opacity * alpha, 1.0, blend) {
                    let _ = writeln!(self.content, "/{gs} gs");
                }
                let typeset = chitrakar_render::text::placed(spec);
                // One text object per stretch set in one way: a face, a
                // colour and a weight are chosen once and the glyphs
                // shown, rather than switched per letter.
                for run in &typeset.runs {
                    let font = self.font_resource(run.face.clone(), &run.glyphs);
                    if run.thicken > 0.0 {
                        self.heavy_run(run, &font)?;
                        continue;
                    }
                    // Said every time: the mode is graphics state, not the
                    // text object's, and outlives `ET` — so a stretch set
                    // after a bold one was stroked too, and the regular
                    // letters of a word came out as heavy as the rest.
                    let _ = writeln!(
                        self.content,
                        "BT\n/{font} {} Tf\n{}\n0 Tr",
                        num(run.em),
                        self.color_op(&run.fill, false)?
                    );
                    // Each glyph on its own matrix: the shaper's position,
                    // turned the way its baseline runs, y turned back up for
                    // the glyph, and the lean a synthesized italic would
                    // draw with.
                    for g in &run.glyphs {
                        let (sin, cos) = g.angle.sin_cos();
                        let lean = run.lean;
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
                }
                // Underline and strike-through: bands in the colour of
                // whichever stretch each belongs to.
                for ([x0, y0, x1, y1], fill) in &typeset.decorations {
                    let _ = writeln!(
                        self.content,
                        "{}\n{} {} {} {} re f",
                        self.color_op(fill, false)?,
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
        if let Some(outer) = outer {
            let inner = std::mem::replace(&mut self.content, outer);
            // The number now, the stream at the end (`finish`).
            let obj = self.push(&[]);
            let name = format!("Fm{}", self.forms.len());
            self.xobjects.push((name.clone(), obj));
            self.forms.push((obj, inner, false));
            let mask = match node.mask {
                Some(_) => Some(self.soft_mask(id, node.transform)?),
                None => None,
            };
            let alpha = if composite { node.opacity } else { 1.0 };
            let mask = mask.map(|g| (g, "Luminosity"));
            // Held and masked both, the layer wants two soft masks and a
            // state has one. So its own mask goes on a group of its own,
            // and that group is what the hold — already set, in the space
            // the two layers share — and the fade and blend come down on:
            // the layer masked first and then held, as the engine cuts it.
            let (name, mask) = match (hold, mask) {
                (Some(_), Some(own)) => {
                    let gs = self
                        .gstate_masked(1.0, 1.0, BlendMode::Normal, Some(own))
                        .expect("a soft mask is a state");
                    let obj = self.push(&[]);
                    let outer = format!("Fm{}", self.forms.len());
                    self.xobjects.push((outer.clone(), obj));
                    self.forms
                        .push((obj, format!("/{gs} gs\n/{name} Do\n"), false));
                    (outer, None)
                }
                (_, mask) => (name, mask),
            };
            if let Some(gs) = self.gstate_masked(alpha, alpha, node.blend, mask) {
                let _ = writeln!(self.content, "/{gs} gs");
            }
            let _ = writeln!(self.content, "/{name} Do");
        }
        self.content.push_str("Q\n");
        Ok(())
    }

    /// Layers side by side, bottom first, each held to the one under it
    /// that it is held to: a run's first layer is drawn as it is, and each
    /// held above it only where that one is (`draw_node_held`). A layer
    /// held to one that draws nothing — hidden, or faded to nothing —
    /// draws nothing, as on the page.
    fn draw_siblings(&mut self, kids: &[NodeId]) -> Result<(), PdfError> {
        let mut base: Option<NodeId> = None;
        let mut held_to: Option<usize> = None;
        for (k, &c) in kids.iter().enumerate() {
            let node = self.doc.node(c)?.clone();
            let held = k > 0 && node.clipped;
            if !held {
                base = Some(c);
                held_to = None;
            }
            if !node.visible || node.opacity <= 0.0 {
                continue;
            }
            if !held {
                self.draw_node(c)?;
                continue;
            }
            let Some(under) = base else { continue };
            let u = self.doc.node(under)?;
            if !u.visible || u.opacity <= 0.0 {
                continue;
            }
            let g = match held_to {
                Some(g) => g,
                None => {
                    let g = self.as_group(under)?;
                    held_to = Some(g);
                    g
                }
            };
            self.draw_node_held(c, Some(g))?;
        }
        Ok(())
    }

    /// A layer drawn into a transparency group of its own, not placed on
    /// the page: what a layer held to it is held by.
    fn as_group(&mut self, id: NodeId) -> Result<usize, PdfError> {
        let outer = std::mem::take(&mut self.content);
        let drawn = self.draw_node(id);
        let inner = std::mem::replace(&mut self.content, outer);
        drawn?;
        let obj = self.push(&[]);
        self.forms.push((obj, inner, false));
        Ok(obj)
    }

    /// Whether a run of layers side by side can all be drawn live, holds
    /// and all: every one that shows is live, and none works on what is
    /// under it.
    fn siblings_live(&self, kids: &[NodeId]) -> Result<bool, PdfError> {
        for &c in kids {
            let n = self.doc.node(c)?;
            if !n.visible || n.opacity <= 0.0 {
                continue;
            }
            if works_on_what_is_under(&self.doc, c) || !self.is_live(c)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A layer's mask as a soft mask: a luminosity group drawing what the
    /// mask lets through as grey — the engine's own reading of it
    /// (`mask_pixels`, the picture the SVG carries too), one pixel to a
    /// unit of the space the layer sits in. `t` is the layer's own
    /// transform, which is in force where the mask is set, and which the
    /// group undoes, since a mask is authored in the space around the
    /// layer rather than in the layer's. Outside the picture it is black,
    /// and lets nothing through; a mask the engine can make no picture of
    /// is black everywhere, which is what it shows.
    fn soft_mask(&mut self, id: NodeId, t: Transform) -> Result<usize, PdfError> {
        let mut content = String::new();
        // As fine as the pictures placed for print (`place_rendered`).
        let over = (300.0 / self.doc.meta.dpi.max(1.0)).clamp(1.0, 4.0).round();
        if let (Some(m), Some(back)) = (
            chitrakar_render::mask_pixels_at(&self.doc, id, over)?,
            chitrakar_render::invert(t),
        ) {
            let grey: Vec<u8> = m.rgba8.chunks(4).map(|p| p[3]).collect();
            let img = self.push(&stream_object(
                &format!(
                    "<< /Type /XObject /Subtype /Image /Width {} /Height {} \
                     /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
                    m.width, m.height
                ),
                &deflate(&grey)?,
            ));
            let name = format!("Mk{}", self.xobjects.len() + 1);
            self.xobjects.push((name.clone(), img));
            let [x, y] = m.origin;
            let _ = write!(
                content,
                "{} {} {} {} {} {} cm\n{} 0 0 {} {} {} cm\n/{name} Do\n",
                num(back.a),
                num(back.b),
                num(back.c),
                num(back.d),
                num(back.e),
                num(back.f),
                num(m.width as f32 / over),
                num(-(m.height as f32) / over),
                num(x),
                num(y + m.height as f32 / over)
            );
        }
        let obj = self.push(&[]);
        self.forms.push((obj, content, true));
        Ok(obj)
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
        let pt = 72.0 / self.doc.meta.dpi.max(1.0);

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
        // Each transparency group: what it draws, in the space it is laid
        // down in, sharing the page's resources. Isolated — what is inside
        // meets only what is inside, then the whole meets the page — and
        // blended in the page's own colour: ink in ink. Its box is the
        // largest a reader should need, since what a group draws is cut
        // by the page in any case; it only has to hold everything.
        let space = if self.separate.is_some() {
            "/DeviceCMYK"
        } else {
            "/DeviceRGB"
        };
        for (obj, inner, luminosity) in std::mem::take(&mut self.forms) {
            let group = if luminosity {
                "/S /Transparency /CS /DeviceGray".to_string()
            } else {
                format!("/S /Transparency /I true /CS {space}")
            };
            self.objects[obj - 1] = stream_object(
                &format!(
                    "<< /Type /XObject /Subtype /Form /BBox [-100000 -100000 100000 100000] \
                     /Group << {group} >> /Resources <<{resources} >> /Filter /FlateDecode"
                ),
                &deflate(inner.as_bytes())?,
            );
        }
        // Each page: its content in points with y downwards from its own
        // top-left corner, and a box the size of what it shows. A page
        // showing the whole document sits at the document's origin; one
        // showing a frame is the same content moved so that frame's own
        // corner is the corner of the page.
        let mut kids = String::new();
        let pages = std::mem::take(&mut self.pages);
        for (body, box_) in &pages {
            let (page_w, page_h) = ((box_[2] - box_[0]) * pt, (box_[3] - box_[1]) * pt);
            let content = format!(
                "q\n{} 0 0 {} {} {} cm\n{body}Q\n",
                num(pt),
                num(-pt),
                num(-box_[0] * pt),
                num(box_[3] * pt)
            );
            // Compressed: the content is text, and a bold no face supplies
            // is drawn as its outlines, which fill a page's worth of it.
            let stream = self.push(&stream_object(
                "<< /Filter /FlateDecode",
                &deflate(content.as_bytes())?,
            ));
            let page = self.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.3} {page_h:.3}] \
                     /Resources <<{resources} >> /Contents {stream} 0 R >>"
                )
                .as_bytes(),
            );
            let _ = write!(kids, "{}{page} 0 R", if kids.is_empty() { "" } else { " " });
        }
        self.objects[1] =
            format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).into_bytes();
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
/// The outlines of what a line carries at its ends, as path operators in
/// the shape's own space, ready to be filled.
///
/// The engine states these as pieces of the stroke's own region, so this
/// asks it rather than working them out again: a head drawn on a page a
/// hair from where the engine draws it is exactly the kind of drift a
/// second copy of the arithmetic invites.
fn marker_outlines(shape: &VectorShape, stroke: &chitrakar_doc::Stroke) -> Vec<String> {
    if stroke.start_marker == chitrakar_doc::Marker::None
        && stroke.end_marker == chitrakar_doc::Marker::None
    {
        return Vec::new();
    }
    let bare = chitrakar_doc::Stroke {
        start_marker: chitrakar_doc::Marker::None,
        end_marker: chitrakar_doc::Marker::None,
        ..stroke.clone()
    };
    // Whatever the markers add to the line's own region is the markers.
    let plain = chitrakar_render::stroke_pieces(shape, &bare).len();
    chitrakar_render::stroke_pieces(shape, stroke)
        .into_iter()
        .skip(plain)
        .filter_map(|piece| match piece {
            chitrakar_render::StrokePiece::Corner(pts, n) => {
                let mut out = format!("{} {} m\n", num(pts[0][0]), num(pts[0][1]));
                for p in &pts[1..n] {
                    let _ = writeln!(out, "{} {} l", num(p[0]), num(p[1]));
                }
                out.push_str("h\n");
                Some(out)
            }
            // A disc as four bezier quarters, the same way an ellipse is
            // written out on a page.
            chitrakar_render::StrokePiece::Disc { at, r } => {
                let k = r * 0.5522847;
                let (x, y) = (at[0], at[1]);
                let mut out = format!("{} {} m\n", num(x + r), num(y));
                for [c1x, c1y, c2x, c2y, ex, ey] in [
                    [x + r, y + k, x + k, y + r, x, y + r],
                    [x - k, y + r, x - r, y + k, x - r, y],
                    [x - r, y - k, x - k, y - r, x, y - r],
                    [x + k, y - r, x + r, y - k, x + r, y],
                ] {
                    let _ = writeln!(
                        out,
                        "{} {} {} {} {} {} c",
                        num(c1x),
                        num(c1y),
                        num(c2x),
                        num(c2y),
                        num(ex),
                        num(ey)
                    );
                }
                out.push_str("h\n");
                Some(out)
            }
            chitrakar_render::StrokePiece::Band { .. } => None,
        })
        .collect()
}

/// A cubic cut where it turns back across `n`: the pieces each run one
/// way along `n` from start to end, with no turning point strictly
/// inside.
fn one_way_across(c: [[f32; 2]; 4], n: [f32; 2]) -> Vec<[[f32; 2]; 4]> {
    let along: Vec<f32> = c.iter().map(|p| p[0] * n[0] + p[1] * n[1]).collect();
    // Where the derivative along `n` is zero: a quadratic in t.
    let (e0, e1, e2) = (
        along[1] - along[0],
        along[2] - along[1],
        along[3] - along[2],
    );
    let (qa, qb, qc) = (e0 - 2.0 * e1 + e2, 2.0 * (e1 - e0), e0);
    let mut ts: Vec<f32> = if qa.abs() < 1e-9 {
        if qb.abs() < 1e-9 {
            vec![]
        } else {
            vec![-qc / qb]
        }
    } else {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc < 0.0 {
            vec![]
        } else {
            let r = disc.sqrt();
            vec![(-qb - r) / (2.0 * qa), (-qb + r) / (2.0 * qa)]
        }
    };
    ts.retain(|t| *t > 1e-4 && *t < 1.0 - 1e-4);
    ts.sort_by(f32::total_cmp);
    let mut out = Vec::new();
    let (mut rest, mut from) = (c, 0.0f32);
    for t in ts {
        // De Casteljau at the turning point, measured along what is left.
        let local = (t - from) / (1.0 - from);
        let lerp =
            |a: [f32; 2], b: [f32; 2]| [a[0] + (b[0] - a[0]) * local, a[1] + (b[1] - a[1]) * local];
        let [p0, p1, p2, p3] = rest;
        let (a, b, cc) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
        let (ab, bc) = (lerp(a, b), lerp(b, cc));
        let mid = lerp(ab, bc);
        out.push([p0, a, ab, mid]);
        rest = [mid, bc, cc, p3];
        from = t;
    }
    out.push(rest);
    out
}

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
        page_content(pdf, 0)
    }

    /// The content stream of page `nth`, found by following the page's
    /// own `/Contents` reference rather than by guessing an object
    /// number — the pages are written last, so their numbers depend on
    /// how much the file holds.
    fn page_content(pdf: &[u8], nth: usize) -> String {
        // Bytes throughout: a compressed stream is not text, and read as
        // text its offsets are not the file's.
        let find = |from: usize, what: &str| {
            pdf[from..]
                .windows(what.len())
                .position(|w| w == what.as_bytes())
                .map(|at| at + from)
        };
        let mut refs = Vec::new();
        let mut from = 0;
        while let Some(at) = find(from, "/Contents ") {
            let rest = &pdf[at + "/Contents ".len()..];
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            refs.push(
                std::str::from_utf8(&rest[..digits])
                    .unwrap()
                    .parse::<usize>()
                    .unwrap(),
            );
            from = at + 1;
        }
        let obj = find(0, &format!("\n{} 0 obj", refs[nth])).expect("content object");
        let start = find(obj, "stream\n").unwrap() + 7;
        let flate = String::from_utf8_lossy(&pdf[obj..start]).contains("/FlateDecode");
        let end = find(start, "\nendstream").unwrap();
        let data = &pdf[start..end];
        if flate {
            let mut out = String::new();
            ZlibDecoder::new(data).read_to_string(&mut out).unwrap();
            out
        } else {
            String::from_utf8_lossy(data).into_owned()
        }
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

    /// A copy with a layer of its own in place of one of the original's
    /// goes over as what it *draws*, not as the original again.
    ///
    /// Every copy this exporter had ever written drew its original
    /// entire, so the branch that writes one could put the original down
    /// again and be right every time. A badge used twice with a
    /// different mark on the second came out as two identical badges —
    /// a wrong picture in a file that reads perfectly well.
    #[test]
    fn a_copy_that_differs_goes_over_as_what_it_draws() {
        let leaf = AuthoredColor::Srgb {
            r: 0.25,
            g: 0.55,
            b: 0.35,
            a: 1.0,
        };
        let pink = AuthoredColor::Srgb {
            r: 0.85,
            g: 0.3,
            b: 0.55,
            a: 1.0,
        };
        let chip = |name: &str, w: f32, radius: f32, fill| {
            shape(
                name,
                VectorShape::Rect {
                    width: w,
                    height: 8.0,
                    radius,
                },
                Some(fill),
            )
        };
        let mut doc = Document::new(60, 40, chitrakar_color::ColorMode::Rgb);
        let badge = add(&mut doc, chitrakar_doc::Node::group("a badge"), [6.0, 4.0]);
        for (i, (name, at)) in [("a dot", 0.0), ("a ring", 12.0)].iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: badge,
                index: i,
                node: Box::new(chip(name, 10.0, 0.0, leaf.clone())),
            })
            .unwrap();
            let id = doc.children_of(badge).unwrap()[i];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(*at, 0.0),
            })
            .unwrap();
        }
        let copy = add(
            &mut doc,
            chitrakar_doc::Node::instance("a copy that differs", badge),
            [6.0, 20.0],
        );
        doc.apply(Command::AddNode {
            parent: copy,
            index: 0,
            node: Box::new(chip("a ring of its own", 7.0, 2.0, pink)),
        })
        .unwrap();
        let own = doc.children_of(copy).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: own,
            transform: Transform::translation(12.0, 0.0),
        })
        .unwrap();
        doc.apply(Command::SetKind {
            id: copy,
            kind: Box::new(NodeKind::Instance {
                of: badge,
                replaces: vec![doc.children_of(badge).unwrap()[1]],
            }),
        })
        .unwrap();

        let pdf = export_pdf_document(&doc).unwrap();
        let Some(drawn) = rasterized(&pdf, (60, 40)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        let ours = chitrakar_render::render(&doc).unwrap();
        let at = |x: usize, y: usize| &drawn.rgba8[(y * 60 + x) * 4..(y * 60 + x) * 4 + 3];

        // Inside every shape the two draw the same colour; edges are
        // left out, where two rasterizers antialias their own way.
        let (mut points, mut off) = (0usize, Vec::new());
        for y in 1..39u32 {
            for x in 1..59u32 {
                let p = ours.get(x, y);
                let inside = p.a > 0.999
                    && (-1..=1i32).all(|dy| {
                        (-1..=1i32).all(|dx| {
                            let q = ours.get((x as i32 + dx) as u32, (y as i32 + dy) as u32);
                            q.a > 0.999
                                && (q.r - p.r).abs() < 1e-4
                                && (q.g - p.g).abs() < 1e-4
                                && (q.b - p.b).abs() < 1e-4
                        })
                    });
                if !inside {
                    continue;
                }
                points += 1;
                let want = [p.r, p.g, p.b]
                    .map(|v| (chitrakar_color::linear_to_srgb(v) * 255.0).round() as i32);
                let got = at(x as usize, y as usize);
                let worst = (0..3)
                    .map(|c| (got[c] as i32 - want[c]).unsigned_abs())
                    .max()
                    .unwrap();
                if worst > 4 {
                    off.push((x, y, worst));
                }
            }
        }
        assert!(
            off.is_empty(),
            "{} points differ, first {:?}",
            off.len(),
            &off[..off.len().min(4)]
        );
        assert!(points > 120, "{points} interior points were compared");

        // Said plainly as well, since what went wrong was a whole layer
        // rather than a level of colour.
        let green = |p: &[u8]| p[1] > p[0] + 30 && p[1] > p[2] + 20;
        assert!(
            green(at(11, 8)),
            "the original's first mark {:?}",
            at(11, 8)
        );
        assert!(green(at(23, 8)), "and its second {:?}", at(23, 8));
        assert!(
            green(at(11, 24)),
            "the copy follows for the first {:?}",
            at(11, 24)
        );
        assert!(
            at(21, 24)[0] > 200 && at(21, 24)[1] < 120,
            "and draws its own for the second {:?}",
            at(21, 24)
        );
        assert!(
            at(26, 24).iter().all(|&c| c > 245),
            "which is narrower than the mark it replaced {:?}",
            at(26, 24)
        );
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
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                start_marker: Default::default(),
                end_marker: Default::default(),
                // A band lying outside the edge, which SVG and a page
                // can only say by moving the shape or by clipping — so
                // it is worth a witness watching them say it.
                align: Some(chitrakar_doc::StrokeAlign::Outside),
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
        // A line with ends and a corner of its own, so the page has to
        // say how it ends and turns rather than taking PDF's own flat
        // ends and mitred corners.
        let mut elbow = shape(
            "elbow",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [14.0, 0.0], [14.0, 20.0]],
                closed: false,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
            None,
        );
        if let NodeKind::Vector { stroke, .. } = &mut elbow.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: BLUE,
                width: 6.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: chitrakar_doc::StrokeCap::Square,
                join: chitrakar_doc::StrokeJoin::Bevel,
                start_marker: chitrakar_doc::Marker::None,
                end_marker: chitrakar_doc::Marker::Arrow,
                align: None,
            });
        }
        add(&mut doc, elbow, [98.0, 6.0]);
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
        assert!(content.starts_with("q\n1 0 0 -1 0 80 cm\n"), "{content}");
        // The rect: a translation, a red fill, a `re`.
        assert!(
            content.contains("1 0 0 1 10 10 cm\n1 0 0 rg\n0 0 40 30 re\nf\n"),
            "{content}"
        );
        // The ellipse: four beziers filled, then its band — which lies
        // outside the edge, so it is a stroke down a shape moved half a
        // width out, under a translation that puts it back.
        assert!(content.contains("15 20 c\n") && content.contains("0 0 1 rg\n"));
        assert!(
            content.contains("1 0 0 1 -2 -2 cm\n") && content.contains("4 w S\nQ"),
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

    /// Frames become the pages of one file: each page its frame's own
    /// size, showing that frame and nothing else, in the order the
    /// frames sit on the document.
    #[test]
    fn frames_become_the_pages_of_one_file() {
        let mut doc = Document::new(300, 200, chitrakar_color::ColorMode::Rgb);
        // Two frames of different sizes, each with a mark of its own, and
        // a loose shape outside both that belongs to no page.
        for (i, (x, w, h, color)) in [(10.0, 80.0, 60.0, RED), (140.0, 120.0, 40.0, BLUE)]
            .into_iter()
            .enumerate()
        {
            let frame = add(
                &mut doc,
                chitrakar_doc::Node::artboard(&format!("sheet {i}"), w, h, None),
                [x, 20.0],
            );
            doc.apply(Command::AddNode {
                parent: frame,
                index: 0,
                node: Box::new(shape(
                    "mark",
                    VectorShape::Rect {
                        width: 20.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    Some(color),
                )),
            })
            .unwrap();
        }
        add(
            &mut doc,
            shape(
                "loose",
                VectorShape::Rect {
                    width: 10.0,
                    height: 10.0,
                    radius: 0.0,
                },
                Some(RED),
            ),
            [280.0, 180.0],
        );

        let pdf = export_pdf_frames(&doc).unwrap();
        assert_xref_is_sound(&pdf);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 2"), "two frames, two pages");
        // Each page is its own frame's size, in points at 72 dpi.
        assert!(
            text.contains("/MediaBox [0 0 80.000 60.000]")
                && text.contains("/MediaBox [0 0 120.000 40.000]"),
            "each page is its frame's own size: {text}"
        );
        // And each is the frame moved to its own corner: the second
        // frame sits 140 across the document and 20 down, so its page
        // starts 140 back and 60 down from the document's top.
        assert!(
            page_content(&pdf, 1).starts_with("q\n1 0 0 -1 -140 60 cm\n"),
            "{}",
            page_content(&pdf, 1)
        );
        // A document with no frames is the one page it always was.
        let plain = Document::new(40, 30, chitrakar_color::ColorMode::Rgb);
        let one = export_pdf_frames(&plain).unwrap();
        assert!(String::from_utf8_lossy(&one).contains("/Count 1"));
    }

    /// The frames of a file, drawn: each page is its frame and what is on
    /// it, and nothing of the document around it. Self-skips without `gs`.
    #[test]
    fn ghostscript_draws_each_frame_as_its_own_page() {
        if std::process::Command::new("gs")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipped: no ghostscript");
            return;
        }
        let mut doc = Document::new(300, 200, chitrakar_color::ColorMode::Rgb);
        for (i, (x, color)) in [(10.0, RED), (140.0, BLUE)].into_iter().enumerate() {
            let frame = add(
                &mut doc,
                chitrakar_doc::Node::artboard(
                    &format!("sheet {i}"),
                    100.0,
                    80.0,
                    Some(chitrakar_color::AuthoredColor::Srgb {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                ),
                [x, 20.0],
            );
            doc.apply(Command::AddNode {
                parent: frame,
                index: 0,
                node: Box::new(shape(
                    "mark",
                    VectorShape::Rect {
                        width: 40.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    Some(color),
                )),
            })
            .unwrap();
        }
        let pdf = export_pdf_frames(&doc).unwrap();
        let dir = std::env::temp_dir().join(format!("chitrakar-frames-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf_path = dir.join("book.pdf");
        std::fs::write(&pdf_path, &pdf).unwrap();
        let status = std::process::Command::new("gs")
            .args([
                "-q",
                "-dNOPAUSE",
                "-dBATCH",
                "-dSAFER",
                "-sDEVICE=png16m",
                "-r72",
            ])
            .arg(format!("-sOutputFile={}", dir.join("page%d.png").display()))
            .arg(&pdf_path)
            .status()
            .unwrap();
        assert!(status.success(), "ghostscript accepted the file");
        for (n, want) in [(1, [255u8, 0, 0]), (2, [0, 0, 255])] {
            let drawn =
                crate::decode(&std::fs::read(dir.join(format!("page{n}.png"))).unwrap()).unwrap();
            assert_eq!(
                (drawn.width, drawn.height),
                (100, 80),
                "page {n} is the frame's own size"
            );
            let at = |x: usize, y: usize| &drawn.rgba8[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3];
            assert_eq!(at(20, 20), &want, "page {n} carries its own mark");
            assert_eq!(
                at(80, 60),
                &[255, 255, 255],
                "and the frame's ground where it has none"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clone layer goes into a PDF as what it lays.
    ///
    /// It paints with what is under it, so it has no picture of its own —
    /// and it was being placed as if it had: it joined the run of layers
    /// that go over as pixels and was rendered with only that run shown,
    /// which left it nothing under it to lift and so nothing to lay. The
    /// retouching went missing from every PDF of a retouched page while
    /// the file read perfectly well. It now goes the way an adjustment
    /// and a filter do, flattening what is under it into the picture it
    /// is part of, and a reader that is not us draws the lifted patch
    /// where it was laid.
    #[test]
    fn a_clone_layer_goes_over_as_what_it_lays() {
        let srgb = |r, g, b| AuthoredColor::Srgb { r, g, b, a: 1.0 };
        let mut doc = Document::new(60, 40, chitrakar_color::ColorMode::Rgb);
        add(
            &mut doc,
            shape(
                "ground",
                VectorShape::Rect {
                    width: 60.0,
                    height: 40.0,
                    radius: 0.0,
                },
                Some(srgb(0.9, 0.9, 0.85)),
            ),
            [0.0, 0.0],
        );
        add(
            &mut doc,
            shape(
                "patch",
                VectorShape::Rect {
                    width: 14.0,
                    height: 30.0,
                    radius: 0.0,
                },
                Some(srgb(0.85, 0.1, 0.1)),
            ),
            [4.0, 5.0],
        );
        let clone = add(
            &mut doc,
            chitrakar_doc::Node::clone_layer("retouch"),
            [0.0, 0.0],
        );
        doc.apply(Command::AddStroke {
            id: clone,
            index: 0,
            on_mask: false,
            stroke: Box::new(chitrakar_doc::PaintStroke {
                points: vec![[42.0, 20.0]],
                radii: vec![7.0],
                color: srgb(0.0, 0.0, 0.0),
                softness: 0.0,
                erase: false,
                source: [-31.0, 0.0],
                heal: false,
                clip: None,
            }),
        })
        .unwrap();
        let ours = chitrakar_render::render(&doc).unwrap();
        let red = ours.get(42, 20).to_srgb8();
        assert!(red[0] > 200 && red[1] < 60, "the page has it ({red:?})");

        let pdf = export_pdf_document(&doc).unwrap();
        let Some(drawn) = rasterized(&pdf, (60, 40)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        let at = |x: usize, y: usize| &drawn.rgba8[(y * 60 + x) * 4..(y * 60 + x) * 4 + 3];
        for (x, y) in [(42usize, 20usize), (40, 18), (44, 23)] {
            let want = ours.get(x as u32, y as u32).to_srgb8();
            let got = at(x, y);
            assert!(
                (0..3).all(|c| (got[c] as i32 - want[c] as i32).abs() <= 4),
                "the file has it too at {x},{y}: {got:?} against {want:?}"
            );
        }
        // And the patch it lifted from is still there, untouched.
        let got = at(10, 20);
        assert!(got[0] > 200 && got[1] < 60, "and what it lifted ({got:?})");

        // And a copy of it, somewhere else on the page: a copy of a
        // clone lifts where the copy stands, so it is the same case again
        // and went missing the same way — it is not a clone layer by kind,
        // so a rule reading the kind alone would miss it.
        let copy = add(
            &mut doc,
            chitrakar_doc::Node::instance("again", clone),
            [0.0, 0.0],
        );
        doc.apply(Command::SetTransform {
            id: copy,
            transform: Transform::translation(0.0, 12.0),
        })
        .unwrap();
        let ours = chitrakar_render::render(&doc).unwrap();
        let again = ours.get(42, 32).to_srgb8();
        assert!(
            again[0] > 200 && again[1] < 60,
            "the page has the copy ({again:?})"
        );
        let pdf = export_pdf_document(&doc).unwrap();
        let drawn = rasterized(&pdf, (60, 40)).unwrap();
        let at = |x: usize, y: usize| &drawn.rgba8[(y * 60 + x) * 4..(y * 60 + x) * 4 + 3];
        for (what, x, y) in [
            ("the clone", 42usize, 20usize),
            ("its copy", 42, 32),
            ("the patch under both", 10, 20),
        ] {
            let want = ours.get(x as u32, y as u32).to_srgb8();
            let got = at(x, y);
            assert!(
                (0..3).all(|c| (got[c] as i32 - want[c] as i32).abs() <= 4),
                "{what}: the file has {got:?} where the page has {want:?}"
            );
        }
    }

    /// Hand a PDF to Ghostscript and get back what it drew, or nothing
    /// when the machine has no Ghostscript to hand it to.
    fn rasterized(pdf: &[u8], size: (u32, u32)) -> Option<crate::SourceImage> {
        if std::process::Command::new("gs")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            return None;
        }
        let dir = std::env::temp_dir().join(format!(
            "chitrakar-ink-{}-{:p}",
            std::process::id(),
            pdf.as_ptr()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (src, out) = (dir.join("page.pdf"), dir.join("page.png"));
        std::fs::write(&src, pdf).unwrap();
        // A pixel to the point, which is what the MediaBox is written in.
        let ran = std::process::Command::new("gs")
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
            .arg(format!("-sOutputFile={}", out.display()))
            .arg(&src)
            .status()
            .unwrap();
        assert!(ran.success(), "ghostscript accepted the file");
        let drawn = crate::decode(&std::fs::read(&out).unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((drawn.width, drawn.height), size);
        Some(drawn)
    }

    /// The text in a PDF lands where the engine sets it.
    ///
    /// A PDF's text is live — an embedded face, a glyph at a time, each
    /// with a matrix of its own — and everything asserted about it so far
    /// is either the string reading back out or the right operators being
    /// present. Neither says where a glyph *landed*. The interiors
    /// reading cannot help: a glyph's stem is a pixel or two across, so
    /// it is all edge and nothing inside, and a page-wide mean would not
    /// notice a line of type moved two pixels along.
    ///
    /// So this asks the one question that survives two rasterizers
    /// hinting and antialiasing their own way: where the ink *is*. The
    /// page is drawn twice on each side, once with the text and once with
    /// it hidden; what the text put down is the difference; and the two
    /// are held to the same centre of mass — over the line, and over each
    /// half of it, so that a glyph moved inside the line is caught too —
    /// and to the same extent.
    #[test]
    fn the_pdf_sets_text_where_the_engine_sets_it() {
        let (w, h) = (200u32, 48u32);
        let page = |showing: bool| {
            let mut doc = Document::new(w, h, chitrakar_color::ColorMode::Rgb);
            // A band for the line to sit on, so what is measured is ink
            // landing on something rather than ink against bare paper.
            add(
                &mut doc,
                shape(
                    "band",
                    VectorShape::Rect {
                        width: 200.0,
                        height: 22.0,
                        radius: 0.0,
                    },
                    Some(AuthoredColor::Srgb {
                        r: 0.9,
                        g: 0.88,
                        b: 0.8,
                        a: 1.0,
                    }),
                ),
                [0.0, 14.0],
            );
            // Long enough to have kerning pairs, ascenders and
            // descenders in it, which is what puts a glyph somewhere a
            // sloppy matrix would not.
            let words = add(
                &mut doc,
                chitrakar_doc::Node::text(
                    "line",
                    chitrakar_doc::TextSpec::new("Hamburgefonstiv", 18.0, BLUE),
                ),
                [11.0, 15.0],
            );
            if !showing {
                doc.apply(Command::SetVisible {
                    id: words,
                    visible: false,
                })
                .unwrap();
            }
            doc
        };
        // What the line put down: how far each pixel moved when the text
        // was added, on each side.
        let ink = |a: &[u8], b: &[u8]| -> Vec<f64> {
            (0..(w * h) as usize)
                .map(|i| {
                    (0..3)
                        .map(|c| (a[i * 4 + c] as i32 - b[i * 4 + c] as i32).unsigned_abs())
                        .max()
                        .unwrap() as f64
                })
                .collect()
        };
        // Ink, its centre of mass and its extent over a stretch of
        // columns. Under eight levels is the tail of an antialiased edge,
        // and is left to it.
        let read = |v: &[f64], from: u32, to: u32| {
            let (mut mass, mut mx, mut my) = (0.0f64, 0.0f64, 0.0f64);
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
            for y in 0..h {
                for x in from..to {
                    let d = v[(y * w + x) as usize];
                    if d < 8.0 {
                        continue;
                    }
                    mass += d;
                    mx += d * x as f64;
                    my += d * y as f64;
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
            (mass, mx / mass, my / mass, x0, y0, x1, y1)
        };
        // Quarters of the line, found from where the ink actually is
        // rather than from the page: a glyph moved inside the line is
        // averaged away by the whole of it, and the more stretches this
        // is read over the less room there is for one to hide in.
        let bands = |x0: u32, x1: u32| -> Vec<(u32, u32, String)> {
            let span = x1 - x0;
            (0..4)
                .map(|i| {
                    (
                        x0 + i * span / 4,
                        x0 + (i + 1) * span / 4,
                        format!("quarter {}", i + 1),
                    )
                })
                .chain([(x0, x1, "the line".to_string())])
                .collect()
        };

        let Some(with) = rasterized(&export_pdf_document(&page(true)).unwrap(), (w, h)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        let without = rasterized(&export_pdf_document(&page(false)).unwrap(), (w, h)).unwrap();
        let theirs = ink(&with.rgba8, &without.rgba8);
        let mine = ink(
            &chitrakar_render::render(&page(true)).unwrap().to_srgb8(),
            &chitrakar_render::render(&page(false)).unwrap().to_srgb8(),
        );
        let all = read(&mine, 0, w);
        {
            for (from, to, what) in bands(all.3, all.5 + 1) {
                let (a, b) = (read(&theirs, from, to), read(&mine, from, to));
                let where_ = what;
                assert!(
                    a.0 > 4000.0 && b.0 > 4000.0,
                    "{where_}: there is ink to weigh ({} against {})",
                    a.0,
                    b.0
                );
                // The coarse half. A pixel of slack in the middle and one
                // in the extent: a rasterizer that darkens stems moves
                // the weight about inside a glyph without moving the
                // glyph, and reaches a row further out at the faint end
                // of an edge.
                assert!(
                    (a.1 - b.1).abs() < 1.0 && (a.2 - b.2).abs() < 1.0,
                    "{where_}: the ink has about the same centre ({:.2},{:.2}) \
                     against ({:.2},{:.2})",
                    a.1,
                    a.2,
                    b.1,
                    b.2
                );
                assert!(
                    a.3.abs_diff(b.3) <= 1
                        && a.4.abs_diff(b.4) <= 1
                        && a.5.abs_diff(b.5) <= 1
                        && a.6.abs_diff(b.6) <= 1,
                    "{where_}: and about the same extent {:?} against {:?}",
                    (a.3, a.4, a.5, a.6),
                    (b.3, b.4, b.5, b.6)
                );
                // A face set at the wrong size lays down a different
                // quantity of ink even where it starts in the right
                // place.
                assert!(
                    (a.0 - b.0).abs() < 0.3 * b.0,
                    "{where_}: and about as much of it ({} against {})",
                    a.0,
                    b.0
                );
            }
        }
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
        let at = |x: usize, y: usize| &drawn.rgba8[(y * 120 + x) * 4..(y * 120 + x) * 4 + 3];

        // Every layer's own interior, which is the sharp reading. The
        // mean above is coarse and gets coarser: every edge the two
        // rasterize their own way costs it a little, so it rises as the
        // page gains elements and the threshold has to be loosened to let
        // innocent additions through. What is *not* allowed to differ is
        // the inside of a shape, where neither has an edge to disagree
        // about and neither has a half-opaque layer to composite in a
        // space of its own — so each layer is drawn alone, its opaque
        // interior found, and only the points where the page shows that
        // layer's own colour unmixed are compared. The two readings are
        // complementary: the mean covers what is occluded and what is
        // half-opaque, which this leaves out, and this holds the rest to
        // four levels out of 255 however busy the page gets.
        let ids: Vec<chitrakar_doc::NodeId> = doc
            .nodes()
            .map(|(id, _)| *id)
            .filter(|id| *id != doc.root())
            .collect();
        // One exception, and it is about the reader rather than the
        // export: a raster enlarged is resampled, and no two readers use
        // the same kernel. It reaches past the picture, since a
        // transparent texel enlarged lets a little of an opaque
        // neighbour bleed into what is under it, so what is left out is
        // every point a raster *covers* rather than every point it
        // paints.
        let mut rastered = vec![false; 120 * 80];
        for &id in &ids {
            if !matches!(
                doc.node(id).unwrap().kind,
                chitrakar_doc::NodeKind::Raster(_)
            ) {
                continue;
            }
            if let Ok(chitrakar_render::Bounds::Rect(x0, y0, x1, y1)) =
                chitrakar_render::bounds_in_parent_space(&doc, id)
            {
                for y in (y0.floor().max(0.0) as usize)..=(y1.ceil().min(79.0) as usize) {
                    for x in (x0.floor().max(0.0) as usize)..=(x1.ceil().min(119.0) as usize) {
                        rastered[y * 120 + x] = true;
                    }
                }
            }
        }
        let (mut layers, mut points, mut off) = (0usize, 0usize, Vec::new());
        for id in ids {
            let mut alone = chitrakar_render::Surface::new(120, 80);
            chitrakar_render::render_showing_at(
                &doc,
                &mut alone,
                chitrakar_render::ClipRect {
                    x0: 0,
                    y0: 0,
                    x1: 120,
                    y1: 80,
                },
                chitrakar_doc::Transform::default(),
                chitrakar_render::Showing::Alone(id),
            )
            .unwrap();
            let mut here = 0usize;
            for y in 1..79u32 {
                for x in 1..119u32 {
                    let p = alone.get(x, y);
                    // Opaque, and the same as all eight of its
                    // neighbours: an inside rather than an edge.
                    let inside = p.a > 0.999
                        && (-1..=1i32).all(|dy| {
                            (-1..=1i32).all(|dx| {
                                let q = alone.get((x as i32 + dx) as u32, (y as i32 + dy) as u32);
                                q.a > 0.999
                                    && (q.r - p.r).abs() < 1e-4
                                    && (q.g - p.g).abs() < 1e-4
                                    && (q.b - p.b).abs() < 1e-4
                            })
                        });
                    if !inside || rastered[(y * 120 + x) as usize] {
                        continue;
                    }
                    // And the page shows that layer's own colour there,
                    // so nothing above it and nothing half-opaque under
                    // it has been mixed in.
                    let s = ours.get(x, y);
                    if s.a < 0.999
                        || (s.r - p.r).abs() > 1e-4
                        || (s.g - p.g).abs() > 1e-4
                        || (s.b - p.b).abs() > 1e-4
                    {
                        continue;
                    }
                    here += 1;
                    let want = [s.r, s.g, s.b]
                        .map(|v| (chitrakar_color::linear_to_srgb(v) * 255.0).round() as i32);
                    let got = at(x as usize, y as usize);
                    let bad = (0..3)
                        .map(|c| (got[c] as i32 - want[c]).unsigned_abs())
                        .max()
                        .unwrap();
                    if bad > 4 {
                        off.push((doc.node(id).unwrap().name.clone(), x, y, bad));
                    }
                }
            }
            if here > 0 {
                layers += 1;
                points += here;
            }
        }
        assert!(
            off.is_empty(),
            "inside a shape the reader draws the engine's colour: {} points off, first {:?}",
            off.len(),
            &off[..off.len().min(4)]
        );
        // And it looked at something: a filter that quietly stopped
        // finding interiors would pass this without reading a pixel.
        assert!(
            layers >= 5 && points >= 300,
            "the interiors of {layers} layers, {points} points, were compared"
        );

        // Spot checks, which reach where the interiors do not — a shape's
        // outermost pixel, and the paper beside it: inside the rect, the
        // ellipse's band and its middle, the hole in the compound path,
        // the image's two pixels.
        assert_eq!(at(30, 25), &[255, 0, 0], "rect");
        assert!(
            at(75, 8)[0] > 200 && at(75, 8)[2] < 60,
            "ellipse band lies outside its edge {:?}",
            at(75, 8)
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
        assert!(
            at(112, 28)[2] > 200 && at(112, 28)[0] < 60,
            "the line is squared off past its last point {:?}",
            at(112, 28)
        );
        assert_eq!(at(112, 31), &[255, 255, 255], "and stops there");
        assert_eq!(
            at(114, 3),
            &[255, 255, 255],
            "its corner is cut across, not carried out to a point"
        );
        assert!(
            at(108, 12)[2] > 200 && at(108, 12)[0] < 60,
            "the head is drawn where the line points {:?}",
            at(108, 12)
        );
        assert_eq!(
            at(104, 20),
            &[255, 255, 255],
            "and narrows to its tip rather than filling its box"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A reader can find the words: Ghostscript's text extractor reads
    /// A block with style runs reaches the page as one text object per
    /// stretch, each choosing its own colour and face — still text, not
    /// a picture of it, so a reader can still select and search it.
    #[test]
    fn style_runs_reach_the_page_as_stretches_of_their_own() {
        let mut doc = Document::new(200, 60, chitrakar_color::ColorMode::Rgb);
        let mut spec = chitrakar_doc::TextSpec::new("one two", 20.0, BLUE);
        let mut run = chitrakar_doc::StyleRun::over(4, 7);
        run.fill = Some(RED);
        spec.runs = vec![run];
        add(&mut doc, chitrakar_doc::Node::text("t", spec), [4.0, 4.0]);
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert_eq!(
            content.matches("BT\n").count(),
            2,
            "one text object a stretch: {content}"
        );
        assert!(
            content.contains("0 0 1 rg") && content.contains("1 0 0 rg"),
            "each in its own colour: {content}"
        );
        // Seven letters shown across the two, none twice.
        assert_eq!(content.matches("> Tj").count(), 7, "{content}");
    }

    /// A bold no face can supply is drawn on the page the way the raster
    /// draws it: the glyphs filled and then stroked in the same colour,
    /// which is a page's own way of putting weight on an upright. A face
    /// that carries its own bold is just set in it.
    /// Every state a page saves it restores, in order.
    ///
    /// A layer's placement, clip and colours are set inside a `q`/`Q`
    /// pair so they end with the layer. A copy with layers of its own
    /// standing in left its pair by an early return past the `Q`, and its
    /// placement stayed on everything drawn after it — a file every
    /// reader would draw wrongly and none would complain about. The pages
    /// nobody wrote put their copies last, where a leak has nothing to
    /// land on, so this reads the stream itself.
    #[test]
    fn every_state_a_page_saves_it_restores() {
        for seed in 0..120u64 {
            let mut doc = chitrakar_doc::fixture::page(seed);
            // Something after everything, for a leak to land on.
            let root = doc.root();
            let at = doc.children_of(root).unwrap().len();
            add_at(&mut doc, at);
            let content = content_of(&export_pdf_document(&doc).unwrap());
            let mut depth = 0i32;
            for line in content.lines() {
                match line.trim() {
                    "q" => depth += 1,
                    "Q" => {
                        depth -= 1;
                        assert!(depth >= 0, "seed {seed}: a Q with no q");
                    }
                    _ => {}
                }
            }
            assert_eq!(
                depth, 0,
                "seed {seed}: {depth} states saved and never restored"
            );
        }
    }

    fn add_at(doc: &mut Document, at: usize) {
        let root = doc.root();
        let mut last = chitrakar_doc::Node::vector(
            "last",
            VectorShape::Rect {
                width: 4.0,
                height: 4.0,
                radius: 0.0,
            },
        );
        if let NodeKind::Vector { fill, .. } = &mut last.kind {
            *fill = Some(BLUE);
        }
        doc.apply(Command::AddNode {
            parent: root,
            index: at,
            node: Box::new(last),
        })
        .unwrap();
    }

    /// A regular stretch after a bold one is drawn. The text render mode
    /// is graphics state, not the text object's, and outlives `ET`, so a
    /// stretch that did not say its own mode was drawn in the one before
    /// it: when a bold was stroked, a word whose last letters were set
    /// regular came out heavy to the end, and with a bold's text set
    /// invisibly over its ink they would not come out at all.
    #[test]
    fn a_regular_stretch_after_a_bold_one_is_drawn() {
        let mut doc = Document::new(120, 40, chitrakar_color::ColorMode::Rgb);
        let mut spec = chitrakar_doc::TextSpec::new("Heavy", 20.0, BLUE);
        spec.bold = true;
        let mut run = chitrakar_doc::StyleRun::over(3, 5);
        run.bold = Some(false);
        spec.runs = vec![run];
        add(&mut doc, chitrakar_doc::Node::text("t", spec), [4.0, 4.0]);
        let content = content_of(&export_pdf_document(&doc).unwrap());
        let heavy = content
            .find("3 Tr")
            .expect("the bold stretch's text is invisible");
        assert!(
            content[heavy..].contains("0 Tr"),
            "and the regular one after it says it is not: {content}"
        );
    }

    /// A bold no face supplies goes as its ink drawn — outlines, not a
    /// picture of them — with the text set invisibly over it, and nothing
    /// stroked (see `heavy_run`).
    #[test]
    fn a_synthesized_bold_is_drawn_rather_than_faked_with_pixels() {
        let mut doc = Document::new(120, 40, chitrakar_color::ColorMode::Rgb);
        let mut spec = chitrakar_doc::TextSpec::new("Heavy", 20.0, BLUE);
        spec.bold = true;
        add(&mut doc, chitrakar_doc::Node::text("t", spec), [4.0, 4.0]);
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert!(
            content.contains("3 Tr") && content.contains(" c\n") && content.contains("h\nf\n"),
            "the text invisible and the outlines filled: {content}"
        );
        assert!(!content.contains("2 Tr"), "and nothing stroked: {content}");
        assert!(!content.contains("/Im"), "and no picture: {content}");
        // Still text, not a picture of text: the glyphs are shown.
        assert!(
            content.contains(" Tm <") && content.contains("> Tj"),
            "{content}"
        );

        let mut doc = Document::new(120, 40, chitrakar_color::ColorMode::Rgb);
        chitrakar_render::text::register_font(
            "Press Test Bold",
            include_bytes!("../../../app/public/fonts/DejaVuSans-Bold.ttf").to_vec(),
        )
        .unwrap();
        chitrakar_render::text::register_font(
            "Press Test",
            include_bytes!("../../../app/public/fonts/DejaVuSansMono.ttf").to_vec(),
        )
        .unwrap();
        let mut spec = chitrakar_doc::TextSpec::new("Heavy", 20.0, BLUE);
        spec.font = "Press Test".into();
        spec.bold = true;
        add(&mut doc, chitrakar_doc::Node::text("t", spec), [4.0, 4.0]);
        let content = content_of(&export_pdf_document(&doc).unwrap());
        assert!(
            !content.contains("3 Tr"),
            "a real bold cut is set as itself: {content}"
        );
    }

    /// Ghostscript draws a bold no face supplies with the ink the page
    /// puts on it, faded once, and reads its words back.
    ///
    /// Stroked with PDF's round pen, the outline grew up and down as
    /// much as sideways, and ghostscript laid a fifth more ink than the
    /// page — at four times the page's resolution and at sixteen, so the
    /// pen and not the reader. And a faded one was darker at every edge,
    /// where the fill and the stroke over it were each faded and laid
    /// down twice.
    #[test]
    fn a_synthesized_bold_puts_on_the_ink_the_page_does() {
        let page = |opacity: f32| {
            let mut doc = Document::new(64, 36, chitrakar_color::ColorMode::Rgb);
            let mut spec = chitrakar_doc::TextSpec::new("Wave hi", 18.0, BLUE);
            spec.bold = true;
            let mut node = chitrakar_doc::Node::text("t", spec);
            node.opacity = opacity;
            add(&mut doc, node, [1.0, 9.0]);
            doc
        };
        let docs = [page(1.0), page(0.5)];
        let Some(theirs) = ghostscript_alpha(&docs) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        let (ours, gs): (f32, f32) = (engine_alpha(&docs[0]).iter().sum(), theirs[0].iter().sum());
        assert!(
            (gs / ours - 1.0).abs() < 0.08,
            "ghostscript lays {gs:.1} pixels of ink where the page lays {ours:.1}"
        );
        let most = theirs[1].iter().cloned().fold(0.0f32, f32::max);
        assert!(
            most < 0.55,
            "faded to half, nothing is covered more than half: {most:.3}"
        );

        let dir = std::env::temp_dir().join(format!("chitrakar-pdfbold-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bold.pdf");
        std::fs::write(&path, export_pdf_document(&docs[0]).unwrap()).unwrap();
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
            .arg(&path)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let read = String::from_utf8_lossy(&out.stdout);
        assert!(read.contains("Wave hi"), "the words come back: {read:?}");
    }

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
        let ligature = typeset.runs[0]
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
        // One profile, referred to by number from both the output intent
        // and the colour space — read rather than named, since which
        // number it takes is the writer's business and moves as objects
        // are added.
        let after = |key: &str| -> String {
            text.split(key)
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or_else(|| panic!("no {key} in the PDF"))
                .to_string()
        };
        assert!(text.contains("/OutputIntents"));
        let profile = after("/DestOutputProfile ");
        assert!(
            text.contains(&format!("[/ICCBased {profile} 0 R]")) && text.contains("/N 4"),
            "the colour space is the intent's own profile, four inks deep"
        );
        let space = after("/ColorSpace << /CS0 ");
        assert_ne!(space, profile, "the colour space is an object of its own");
        assert!(
            text.contains(&format!("\n{space} 0 obj")),
            "and the page's colour space is written out"
        );
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

    /// What ghostscript covers, pixel by pixel, drawing each document's
    /// PDF onto nothing: one run of ghostscript for all of them, since
    /// starting it is most of what it costs. `None` without ghostscript.
    ///
    /// Drawn at four times the page's resolution and averaged down, as
    /// the engine's side is. At the page's own resolution ghostscript sets
    /// type a quarter lighter than it does at four times or sixteen —
    /// its own rasterizer at small sizes, which is the reader and not
    /// what is being asked about — and that, with an underline thinned
    /// by the engine, was all that had kept text out of this audit.
    ///
    /// Images are interpolated. A picture the exporter places is rendered
    /// at up to four times the page's resolution for print, and without
    /// interpolation ghostscript brings it down by taking one sample in
    /// sixteen, which aliases every edge in it; that is the reader too.
    fn ghostscript_alpha(docs: &[Document]) -> Option<Vec<Vec<f32>>> {
        std::process::Command::new("gs")
            .arg("--version")
            .output()
            .ok()?;
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("chitrakar-pdf-cover-{}-{call}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for (i, d) in docs.iter().enumerate() {
            let p = dir.join(format!("p{i:04}.pdf"));
            std::fs::write(&p, export_pdf_document(d).unwrap()).unwrap();
            paths.push(p);
        }
        let status = std::process::Command::new("gs")
            .args([
                "-q",
                "-dNOPAUSE",
                "-dBATCH",
                "-dSAFER",
                "-sDEVICE=pngalpha",
                "-r288",
                "-dGraphicsAlphaBits=4",
                "-dTextAlphaBits=4",
                "-dDOINTERPOLATE",
            ])
            .arg(format!("-sOutputFile={}", dir.join("o%04d.png").display()))
            .args(&paths)
            .status()
            .unwrap();
        assert!(status.success(), "ghostscript accepted the files");
        let out = (1..=docs.len())
            .map(|k| {
                let png = std::fs::read(dir.join(format!("o{k:04}.png"))).unwrap();
                let img = crate::decode(&png).unwrap();
                let (w, h) = (img.width / 4, img.height / 4);
                let mut out = Vec::with_capacity((w * h) as usize);
                for y in 0..h {
                    for x in 0..w {
                        let mut a = 0u32;
                        for j in 0..4 {
                            for i in 0..4 {
                                a += img.rgba8
                                    [(((y * 4 + j) * img.width + x * 4 + i) * 4 + 3) as usize]
                                    as u32;
                            }
                        }
                        out.push(a as f32 / (255.0 * 16.0));
                    }
                }
                out
            })
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        Some(out)
    }

    /// What the engine covers, rendered at the four times the exporter
    /// renders the pictures it places for print, and brought down to the
    /// page by averaging. At that resolution a frame's edge is rounded to
    /// a quarter of a pixel rather than a whole one, and that is the
    /// picture the PDF carries.
    fn engine_alpha(doc: &Document) -> Vec<f32> {
        let k = 4u32;
        let (w, h) = (doc.meta.width, doc.meta.height);
        let mut big = chitrakar_render::Surface::new(w * k, h * k);
        let clip = big.full_clip();
        chitrakar_render::render_region_at(
            doc,
            &mut big,
            clip,
            Transform {
                a: k as f32,
                d: k as f32,
                ..Default::default()
            },
        )
        .unwrap();
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let mut a = 0.0;
                for j in 0..k {
                    for i in 0..k {
                        a += big.get(x * k + i, y * k + j).a;
                    }
                }
                out.push(a / (k * k) as f32);
            }
        }
        out
    }

    /// Ghostscript draws the pages nobody wrote covering what the engine
    /// covers — the PDF's half of the question the SVG exporter's
    /// `a_reader_covers_what_the_engine_covers_on_pages_nobody_wrote`
    /// asks, and for the same reason in coverage rather than colour.
    ///
    /// Text is in, bold, italic and on a guide: every one of the three
    /// hundred pages agrees once ghostscript is read at four times,
    /// the underline is as thick at every zoom, and a bold no face
    /// supplies is drawn as the page draws it (`heavy_run`) rather than
    /// stroked with a round pen. The ground is left out, so that what a
    /// page covers is not everywhere.
    ///
    /// Two thirds of the pages disagreed the first time. Among what that
    /// found: a copy of a hidden layer drawn anyway; a copy pointing at a
    /// removed layer stopping the export, and — once it did not — leaving
    /// its placement on everything after it, as a copy with stand-ins of
    /// its own always had (an early return past the `Q`); a layer held to
    /// a filter put in a picture of its own with the filter hidden, where
    /// the hold let nothing through; a copy with a blend drawn as a
    /// picture in which its original had been hidden; a copy faded to a
    /// third drawn solid; an upright frame cut at its exact box; and
    /// every placed picture starting at a quarter of a pixel.
    #[test]
    fn ghostscript_covers_what_the_engine_covers_on_pages_nobody_wrote() {
        const SEEDS: u64 = 300;
        let docs: Vec<Document> = (0..SEEDS)
            .map(|seed| {
                let mut doc = chitrakar_doc::fixture::page(seed);
                let page = doc.clone();
                for (id, n) in page.nodes() {
                    if n.name == "ground" {
                        doc.apply(Command::SetVisible {
                            id: *id,
                            visible: false,
                        })
                        .unwrap();
                    }
                }
                doc
            })
            .collect();
        let Some(theirs) = ghostscript_alpha(&docs) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        let mut touched = 0usize;
        for (seed, (doc, t)) in docs.iter().zip(&theirs).enumerate() {
            let ours = engine_alpha(doc);
            assert_eq!(ours.len(), t.len(), "seed {seed}: the page is its own size");
            let (mut bad, mut worst, mut at) = (0usize, 0.0f32, 0usize);
            for (i, (o, g)) in ours.iter().zip(t).enumerate() {
                let d = (o - g).abs();
                if d > 0.5 {
                    bad += 1;
                }
                if d > worst {
                    worst = d;
                    at = i;
                }
            }
            let w = doc.meta.width as usize;
            assert!(
                bad <= 6,
                "seed {seed}: {bad} pixels are covered differently by more than half, \
                 the worst by {worst:.3} at ({}, {})",
                at % w,
                at / w
            );
            touched += (bad > 0) as usize;
        }
        // None do when this was written.
        assert!(
            touched <= 3,
            "{touched} pages differ by more than half somewhere"
        );
    }

    /// A faded group goes live, as one transparency group: what is inside
    /// meets only what is inside, and the whole is faded once as it lands,
    /// so where two of its layers overlap it is no darker than where one
    /// is. A copy of it goes the same way. Both went as pictures, since
    /// drawn one by one each layer took the fade, twice where two met —
    /// which the pages nobody wrote cannot see, a quarter's difference
    /// being under the half they are held to.
    #[test]
    fn a_faded_group_goes_live_as_one_transparency_group() {
        let mut doc = Document::new(64, 24, chitrakar_color::ColorMode::Rgb);
        let group = add(&mut doc, chitrakar_doc::Node::group("pair"), [2.0, 4.0]);
        for (i, x) in [0.0f32, 6.0].into_iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: group,
                index: i,
                node: Box::new(shape(
                    "square",
                    VectorShape::Rect {
                        width: 12.0,
                        height: 12.0,
                        radius: 0.0,
                    },
                    Some(BLUE),
                )),
            })
            .unwrap();
            let id = doc.children_of(group).unwrap()[i];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(x, 0.0),
            })
            .unwrap();
        }
        doc.apply(Command::SetOpacity {
            id: group,
            opacity: 0.5,
        })
        .unwrap();
        let copy = add(
            &mut doc,
            chitrakar_doc::Node::instance("again", group),
            [34.0, 4.0],
        );
        doc.apply(Command::SetBlendMode {
            id: copy,
            blend: BlendMode::Multiply,
        })
        .unwrap();
        let pdf = export_pdf_document(&doc).unwrap();
        let content = content_of(&pdf);
        // Two groups on the page — the copy's holding the original's again,
        // which is what it draws — and no pictures.
        assert!(
            content.matches("/Fm").count() == 2 && !content.contains("/Im"),
            "{content}"
        );
        assert!(String::from_utf8_lossy(&pdf).contains("/Group << /S /Transparency /I true"));

        let ours = engine_alpha(&doc);
        let at = |a: &[f32], x: usize, y: usize| a[y * 64 + x];
        assert!(
            (at(&ours, 10, 10) - at(&ours, 4, 10)).abs() < 1e-3,
            "the engine's overlap is its single part"
        );
        let Some(theirs) = ghostscript_alpha(std::slice::from_ref(&doc)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        for (x, y) in [(4, 10), (10, 10), (16, 10), (36, 10), (42, 10), (48, 10)] {
            let (o, g) = (at(&ours, x, y), at(&theirs[0], x, y));
            assert!(
                (o - g).abs() < 0.02,
                "({x}, {y}): ghostscript covers {g:.3} where the page covers {o:.3}"
            );
        }
    }

    /// A curve that turns back across the sweep is cut where it turns, so
    /// that each band a bold's edges sweep runs one way: a band between an
    /// arch and the arch moved sideways folds over itself at the top, and
    /// half of it winds against the rest.
    #[test]
    fn a_curve_is_cut_where_it_turns_back() {
        let arch = [[0.0, 0.0], [1.0, 2.0], [2.0, 2.0], [3.0, 0.0]];
        let pieces = one_way_across(arch, [0.0, 1.0]);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        assert_eq!(pieces[0][0], arch[0]);
        assert_eq!(pieces[1][3], arch[3]);
        assert_eq!(pieces[0][3], pieces[1][0], "the pieces meet");
        let top = pieces[0][3];
        assert!(
            (top[0] - 1.5).abs() < 1e-5 && (top[1] - 1.5).abs() < 1e-5,
            "{top:?}"
        );
        for p in &pieces {
            let ys: Vec<f32> = p.iter().map(|q| q[1]).collect();
            let up = ys.windows(2).all(|w| w[1] >= w[0] - 1e-6);
            let down = ys.windows(2).all(|w| w[1] <= w[0] + 1e-6);
            assert!(up || down, "each piece runs one way: {ys:?}");
        }
        // Across the other way, the arch already runs one way.
        assert_eq!(one_way_across(arch, [1.0, 0.0]).len(), 1);
        // An S turns twice.
        let s = [[0.0, 0.0], [3.0, 3.0], [-3.0, 3.0], [0.0, 0.0]];
        assert_eq!(one_way_across(s, [1.0, 0.0]).len(), 3);
    }

    /// A brush layer of hard strokes goes as the curves they cover, live
    /// on the page rather than a picture of it — faded or blended with
    /// more than one stroke, as a transparency group, since a fade taken
    /// on each fill would be taken twice where two strokes meet. Each
    /// stroke keeps its own alpha to itself: an opaque stroke after a
    /// translucent one is opaque.
    #[test]
    fn a_hard_brush_goes_live_as_the_curves_it_covers() {
        let dab = |at: [f32; 2], alpha: f32| chitrakar_doc::PaintStroke {
            points: vec![at, [at[0] + 6.0, at[1] + 2.0]],
            radii: vec![3.0],
            color: AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: alpha,
            },
            softness: 0.0,
            erase: false,
            source: [0.0, 0.0],
            heal: false,
            clip: None,
        };
        let page = |strokes: Vec<chitrakar_doc::PaintStroke>, opacity: f32| {
            let mut doc = Document::new(40, 20, chitrakar_color::ColorMode::Rgb);
            let mut node = chitrakar_doc::Node::paint("ink");
            node.kind = NodeKind::Paint { strokes };
            node.opacity = opacity;
            add(&mut doc, node, [0.0, 0.0]);
            doc
        };
        let live = page(vec![dab([6.0, 8.0], 0.5), dab([24.0, 8.0], 1.0)], 1.0);
        let content = content_of(&export_pdf_document(&live).unwrap());
        assert!(
            content.contains(" c\n") && content.contains("f\nQ") && !content.contains("/Im"),
            "{content}"
        );
        let faded = page(vec![dab([6.0, 8.0], 1.0), dab([9.0, 8.0], 1.0)], 0.5);
        let content = content_of(&export_pdf_document(&faded).unwrap());
        assert!(
            content.contains("/Fm0 Do") && !content.contains("/Im"),
            "faded, two strokes go as the one group they make: {content}"
        );
        let one = page(vec![dab([6.0, 8.0], 1.0)], 0.5);
        assert!(!content_of(&export_pdf_document(&one).unwrap()).contains("/Im"));

        let Some(theirs) = ghostscript_alpha(std::slice::from_ref(&live)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        let at = |x: usize, y: usize| theirs[0][y * 40 + x];
        assert!(
            (at(9, 9) - 0.5).abs() < 0.05,
            "the translucent stroke: {}",
            at(9, 9)
        );
        assert!(at(27, 9) > 0.98, "the opaque one after it: {}", at(27, 9));
    }

    /// A masked layer goes live, the mask a soft mask on it as one
    /// transparency group: a faded shape whose stroke overlaps its fill is
    /// faded as it paints and then masked once, as the engine does it, and
    /// masked type is still type. Both went as pictures. The pages nobody
    /// wrote could not tell a mask taken twice from one taken once — at a
    /// feathered edge that is a tenth of a pixel's coverage — so this
    /// reads ghostscript against the engine to three hundredths, across
    /// the feather.
    #[test]
    fn a_masked_layer_goes_live_under_a_soft_mask() {
        let mut doc = Document::new(64, 40, chitrakar_color::ColorMode::Rgb);
        let mut boxed = shape(
            "boxed",
            VectorShape::Rect {
                width: 30.0,
                height: 20.0,
                radius: 0.0,
            },
            Some(BLUE),
        );
        if let NodeKind::Vector { stroke, .. } = &mut boxed.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: RED,
                width: 6.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                start_marker: Default::default(),
                end_marker: Default::default(),
                align: None,
            });
        }
        boxed.opacity = 0.6;
        // An oval over the left of it, feathered, written in the space the
        // layer sits in — the page's.
        boxed.mask = Some(chitrakar_doc::Mask {
            kind: chitrakar_doc::MaskKind::Vector {
                shape: VectorShape::Ellipse { rx: 14.0, ry: 11.0 },
                transform: Transform::translation(10.0, 9.0),
            },
            invert: false,
            feather: 3.0,
        });
        add(&mut doc, boxed, [10.0, 10.0]);
        let mut word =
            chitrakar_doc::Node::text("word", chitrakar_doc::TextSpec::new("Ab", 12.0, BLUE));
        word.mask = Some(chitrakar_doc::Mask {
            kind: chitrakar_doc::MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 8.0,
                    height: 20.0,
                    radius: 0.0,
                },
                transform: Transform::translation(46.0, 0.0),
            },
            invert: false,
            feather: 0.0,
        });
        add(&mut doc, word, [44.0, 2.0]);

        let pdf = export_pdf_document(&doc).unwrap();
        let content = content_of(&pdf);
        assert!(
            content.matches("/Fm").count() == 2 && !content.contains("/Im"),
            "{content}"
        );
        assert!(String::from_utf8_lossy(&pdf).contains("/S /Luminosity"));
        assert!(
            pdfish_text(&pdf).contains("Tj"),
            "the masked word is still type"
        );

        let ours = engine_alpha(&doc);
        let Some(theirs) = ghostscript_alpha(std::slice::from_ref(&doc)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        // Along the middle of the box, from the stroke over the fill into
        // the feather and out of it; and down its left side.
        let points: Vec<(usize, usize)> = (8..36)
            .map(|x| (x, 20))
            .chain((8..32).map(|y| (12, y)))
            .collect();
        for (x, y) in points {
            let (o, g) = (ours[y * 64 + x], theirs[0][y * 64 + x]);
            assert!(
                (o - g).abs() < 0.03,
                "({x}, {y}): ghostscript covers {g:.3} where the page covers {o:.3}"
            );
        }
    }

    /// A layer held to the one under it goes live, held by a soft mask
    /// read off the alpha of that layer drawn again as a group: exactly
    /// the engine's hold, so what is held shows as much as the layer under
    /// it does — faded, masked, a group — and a fill and a stroke over it
    /// are held once. A run of held layers went into every PDF as one
    /// picture.
    #[test]
    fn a_held_layer_goes_live_under_the_alpha_of_the_one_under_it() {
        let mut doc = Document::new(64, 40, chitrakar_color::ColorMode::Rgb);
        let mut oval = shape(
            "oval",
            VectorShape::Ellipse { rx: 16.0, ry: 12.0 },
            Some(RED),
        );
        oval.opacity = 0.6;
        oval.mask = Some(chitrakar_doc::Mask {
            kind: chitrakar_doc::MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 20.0,
                    height: 40.0,
                    radius: 0.0,
                },
                transform: Transform::translation(4.0, 0.0),
            },
            invert: false,
            feather: 2.0,
        });
        add(&mut doc, oval, [8.0, 6.0]);
        let mut band = shape(
            "band",
            VectorShape::Rect {
                width: 30.0,
                height: 10.0,
                radius: 0.0,
            },
            Some(BLUE),
        );
        if let NodeKind::Vector { stroke, .. } = &mut band.kind {
            *stroke = Some(chitrakar_doc::Stroke {
                color: RED,
                width: 4.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                start_marker: Default::default(),
                end_marker: Default::default(),
                align: None,
            });
        }
        band.clipped = true;
        add(&mut doc, band, [6.0, 13.0]);
        let mut word =
            chitrakar_doc::Node::text("word", chitrakar_doc::TextSpec::new("Ab", 14.0, BLUE));
        word.clipped = true;
        add(&mut doc, word, [14.0, 2.0]);
        // A faded group, and a square held to it.
        let pair = add(&mut doc, chitrakar_doc::Node::group("pair"), [42.0, 6.0]);
        for (i, y) in [0.0f32, 12.0].into_iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: pair,
                index: i,
                node: Box::new(shape(
                    "square",
                    VectorShape::Rect {
                        width: 16.0,
                        height: 14.0,
                        radius: 0.0,
                    },
                    Some(RED),
                )),
            })
            .unwrap();
            let id = doc.children_of(pair).unwrap()[i];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(0.0, y),
            })
            .unwrap();
        }
        doc.apply(Command::SetOpacity {
            id: pair,
            opacity: 0.5,
        })
        .unwrap();
        let mut over = shape(
            "over",
            VectorShape::Rect {
                width: 12.0,
                height: 30.0,
                radius: 0.0,
            },
            Some(BLUE),
        );
        over.clipped = true;
        add(&mut doc, over, [46.0, 2.0]);

        let pdf = export_pdf_document(&doc).unwrap();
        let content = content_of(&pdf);
        assert!(
            !content.contains("/Im"),
            "nothing went as pixels: {content}"
        );
        assert!(String::from_utf8_lossy(&pdf).contains("/S /Alpha"));
        assert!(
            pdfish_text(&pdf).contains("Tj"),
            "the held word is still type"
        );

        let ours = engine_alpha(&doc);
        let Some(theirs) = ghostscript_alpha(std::slice::from_ref(&doc)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        // Across the band and the oval, through the stroke over the fill
        // and the mask's feather; and across the group and what it holds.
        for (x, y) in (2..62).map(|x| (x, 18)).chain((2..62).map(|x| (x, 26))) {
            let (o, g) = (ours[y * 64 + x], theirs[0][y * 64 + x]);
            assert!(
                (o - g).abs() < 0.03,
                "({x}, {y}): ghostscript covers {g:.3} where the page covers {o:.3}"
            );
        }
    }

    /// A held layer wearing a mask of its own goes live too: its mask on a
    /// group of its own, and the hold on the group around that — two soft
    /// masks, which one graphics state cannot carry, nested so that the
    /// layer is masked and then held, as the engine cuts it. It went as
    /// pixels.
    #[test]
    fn a_held_layer_with_a_mask_of_its_own_goes_live() {
        let mut doc = Document::new(48, 32, chitrakar_color::ColorMode::Rgb);
        add(
            &mut doc,
            shape(
                "disc",
                VectorShape::Ellipse { rx: 14.0, ry: 12.0 },
                Some(RED),
            ),
            [6.0, 4.0],
        );
        let mut bar = shape(
            "bar",
            VectorShape::Rect {
                width: 40.0,
                height: 12.0,
                radius: 0.0,
            },
            Some(BLUE),
        );
        bar.clipped = true;
        bar.opacity = 0.7;
        bar.blend = BlendMode::Multiply;
        // Down its right half, feathered.
        bar.mask = Some(chitrakar_doc::Mask {
            kind: chitrakar_doc::MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 30.0,
                    height: 32.0,
                    radius: 0.0,
                },
                transform: Transform::translation(18.0, 0.0),
            },
            invert: false,
            feather: 2.0,
        });
        add(&mut doc, bar, [2.0, 10.0]);
        let pdf = export_pdf_document(&doc).unwrap();
        let content = content_of(&pdf);
        assert!(
            !content.contains("/Im"),
            "nothing went as pixels: {content}"
        );
        let file = String::from_utf8_lossy(&pdf);
        assert!(file.contains("/S /Alpha") && file.contains("/S /Luminosity"));

        let ours = engine_alpha(&doc);
        let Some(theirs) = ghostscript_alpha(std::slice::from_ref(&doc)) else {
            eprintln!("skipped: no ghostscript");
            return;
        };
        // Along the bar, across the disc's edge it is held by and the feather
        // of its own mask; and down through it, inside the disc.
        for (x, y) in (2..46).map(|x| (x, 15)).chain((8..26).map(|y| (22, y))) {
            let (o, g) = (ours[y * 48 + x], theirs[0][y * 48 + x]);
            assert!(
                (o - g).abs() < 0.03,
                "({x}, {y}): ghostscript covers {g:.3} where the page covers {o:.3}"
            );
        }
    }

    /// Every content stream in a PDF, inflated and joined: a form's
    /// drawing is in its own stream rather than the page's.
    fn pdfish_text(pdf: &[u8]) -> String {
        // Bytes throughout: read as text, a compressed stream's offsets
        // are not the file's.
        let find = |from: usize, what: &[u8]| {
            pdf[from..]
                .windows(what.len())
                .position(|w| w == what)
                .map(|at| at + from)
        };
        let mut out = String::new();
        let mut from = 0;
        while let Some(at) = find(from, b"stream\n") {
            let start = at + 7;
            if at >= 3 && &pdf[at - 3..at] == b"end" {
                from = start;
                continue;
            }
            let Some(end) = find(start, b"\nendstream") else {
                break;
            };
            let mut inflated = String::new();
            if ZlibDecoder::new(&pdf[start..end])
                .read_to_string(&mut inflated)
                .is_ok()
            {
                out.push_str(&inflated);
            }
            from = end;
        }
        out
    }
}
