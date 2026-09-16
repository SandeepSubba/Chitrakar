//! SVG import: usvg parses the file, resolves styles, references and
//! transforms, and turns text into outlines with the bundled face; every
//! path that comes out becomes a shape layer in document space, with its
//! fill (solid or gradient), its stroke and the opacity of the groups
//! above it. Groups are flattened: the layers come in painter's order.

use chitrakar_color::AuthoredColor;
use chitrakar_doc::{Gradient, GradientStop, Node, NodeKind, Stroke, VectorShape};
use usvg::tiny_skia_path::PathSegment;

const FACE: &[u8] = include_bytes!("../../render/assets/DejaVuSans.ttf");

/// What an SVG file holds for the document: its page size, its shapes as
/// nodes, bottom first, and the pictures embedded in it.
pub struct ImportedSvg {
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Node>,
    /// Rasters the file carries, each saying how many shapes go below it
    /// so painter's order survives being split in two. They are kept
    /// apart from the shapes because a raster layer is a *reference* to
    /// pooled pixels and this crate has no document to pool them in —
    /// the caller adds the resource and gets the id back.
    pub images: Vec<ImportedImage>,
}

/// A picture the file had inside it, decoded, with where it goes.
pub struct ImportedImage {
    pub name: String,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Document space: the picture's own pixels carried to where the
    /// file puts them, its scale included.
    pub transform: chitrakar_doc::Transform,
    pub opacity: f32,
    /// How many of `shapes` are below it, which is its place in
    /// painter's order.
    pub below: usize,
    /// What it is seen through, where the file put it inside a clip.
    pub clip: Option<chitrakar_doc::Mask>,
}

/// Bring an SVG in as shape layers.
pub fn import_svg(data: &[u8]) -> Result<ImportedSvg, String> {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_font_data(FACE.to_vec());
    // Text in a face the file cannot supply is set in the bundled one.
    opt.font_family = "DejaVu Sans".to_string();
    let tree = usvg::Tree::from_data(data, &opt).map_err(|e| e.to_string())?;
    let mut shapes = Vec::new();
    let mut images = Vec::new();
    walk(tree.root(), 1.0, None, &mut shapes, &mut images);
    Ok(ImportedSvg {
        width: tree.size().width(),
        height: tree.size().height(),
        shapes,
        images,
    })
}

/// The region a group is seen through, in document space, or nothing.
///
/// A clip path is a set of outlines and the content shows where they
/// cover — usvg has already resolved which outlines and put the whole
/// placement into their absolute transforms, so what comes back here is
/// polygons in the same space the shapes are in. `clipPathUnits`,
/// nesting on the clip itself and a clip referring to another clip are
/// all resolved by then as well; a clip *on* the clip path narrows it,
/// which is an intersection like any other.
fn clip_rings(group: &usvg::Group) -> Option<Vec<Vec<[f32; 2]>>> {
    let cp = group.clip_path()?;
    let mut rings: Vec<Vec<[f32; 2]>> = Vec::new();
    collect_clip(cp.root(), &mut rings);
    if rings.is_empty() {
        return None;
    }
    // The outlines of a clip path show where *any* of them covers, which
    // is their union — not the even-odd of one compound shape.
    let mut acc = vec![rings.remove(0)];
    for next in rings {
        match chitrakar_render::boolean::combine_or_nudge(
            &acc,
            std::slice::from_ref(&next),
            chitrakar_render::boolean::BoolOp::Union,
        ) {
            Some(joined) => acc = joined,
            None => acc.push(next),
        }
    }
    // And a clip on the clip path narrows what it lets through.
    if let Some(inner) = clip_rings(cp.root()) {
        if let Some(both) = chitrakar_render::boolean::combine_or_nudge(
            &acc,
            &inner,
            chitrakar_render::boolean::BoolOp::Intersect,
        ) {
            acc = both;
        }
    }
    Some(acc)
}

fn collect_clip(group: &usvg::Group, out: &mut Vec<Vec<[f32; 2]>>) {
    for child in group.children() {
        match child {
            usvg::Node::Group(g) => collect_clip(g, out),
            usvg::Node::Path(p) => {
                for ring in rings_of(p) {
                    let flat = ring.flattened();
                    if flat.len() >= 3 {
                        out.push(flat);
                    }
                }
            }
            // A clip path holds outlines; usvg has already dropped
            // anything else, and text in one arrives as outlines.
            usvg::Node::Text(t) => collect_clip(t.flattened(), out),
            usvg::Node::Image(_) => {}
        }
    }
}

/// A region as the mask a layer wears. The rings are already in document
/// space, so the mask carries no transform of its own.
fn mask_of(rings: &[Vec<[f32; 2]>]) -> chitrakar_doc::Mask {
    let mut rings = rings.to_vec();
    let main = rings.remove(0);
    chitrakar_doc::Mask {
        kind: chitrakar_doc::MaskKind::Vector {
            shape: VectorShape::Path {
                handles: vec![[0.0; 4]; main.len()],
                points: main,
                closed: true,
                smooth: false,
                subpaths: rings,
            },
            transform: chitrakar_doc::Transform::default(),
        },
        invert: false,
        feather: 0.0,
    }
}

fn walk(
    group: &usvg::Group,
    opacity: f32,
    clip: Option<&Vec<Vec<[f32; 2]>>>,
    out: &mut Vec<Node>,
    pics: &mut Vec<ImportedImage>,
) {
    let opacity = opacity * group.opacity().get();
    // What this group is seen through, and everything under it with it. A
    // clip is the one thing about a group that survives the group being
    // flattened away: it multiplies coverage by nought or one, and that
    // distributes over the children exactly — each child shown only
    // inside the region is the same picture as the group shown only
    // inside it. Opacity and blending do not distribute that way, which
    // is why they are still folded into the colours instead.
    let here = clip_rings(group);
    let clip = match (clip, here.as_ref()) {
        (None, None) => None,
        (Some(outer), None) => Some(outer.clone()),
        (None, Some(inner)) => Some(inner.clone()),
        // A clip inside a clip shows only what both show.
        (Some(outer), Some(inner)) => chitrakar_render::boolean::combine_or_nudge(
            outer,
            inner,
            chitrakar_render::boolean::BoolOp::Intersect,
        )
        .or_else(|| Some(inner.clone())),
    };
    let clip = clip.as_ref();
    for child in group.children() {
        match child {
            usvg::Node::Group(g) => walk(g, opacity, clip, out, pics),
            usvg::Node::Path(p) => {
                if p.is_visible() {
                    if let Some(mut node) = shape_of(p, opacity) {
                        node.mask = clip.map(|c| mask_of(c));
                        out.push(node);
                    }
                }
            }
            // Text arrives as the outlines usvg set it in.
            usvg::Node::Text(t) => walk(t.flattened(), opacity, clip, out, pics),
            // A picture the file carries, which used to be dropped on the
            // floor: an SVG with a photograph in it came in as the shapes
            // around the photograph and nothing where it was, silently.
            usvg::Node::Image(img) => {
                if !img.is_visible() {
                    continue;
                }
                match img.kind() {
                    // A nested SVG is not a picture at all — it is more
                    // of the same file, and it comes in as shapes.
                    usvg::ImageKind::SVG(tree) => walk(tree.root(), opacity, clip, out, pics),
                    kind => {
                        if let Some(mut pic) = picture_of(img, kind, opacity, out.len()) {
                            pic.clip = clip.map(|c| mask_of(c));
                            pics.push(pic);
                        }
                    }
                }
            }
        }
    }
}

/// One embedded picture, decoded and placed.
///
/// usvg hands the bytes over as the file stored them and says what
/// rectangle they are drawn into; the picture's own pixel grid is
/// whatever the bytes turn out to be, so the scale between the two is
/// part of where it goes. GIF and WebP arrive here as bytes this crate
/// has no decoder for — they are passed over rather than guessed at,
/// which is the same answer as before for those two and a picture for
/// the two that matter.
fn picture_of(
    img: &usvg::Image,
    kind: &usvg::ImageKind,
    opacity: f32,
    below: usize,
) -> Option<ImportedImage> {
    let bytes: &[u8] = match kind {
        usvg::ImageKind::PNG(data) | usvg::ImageKind::JPEG(data) => data,
        _ => return None,
    };
    let decoded = crate::decode(bytes).ok()?;
    if decoded.width == 0 || decoded.height == 0 {
        return None;
    }
    // Where the file draws it. usvg has already done all of the work
    // that is about the *file* — the x, y, width and height, the
    // viewport, preserveAspectRatio and whatever transforms it sits
    // under — and left the answer as one absolute transform against the
    // picture's own pixel grid, which it reports as the image's size.
    // So there is nothing to scale here: the transform is the placement,
    // and a stretch asked for with preserveAspectRatio="none" arrives in
    // it as two different scales rather than as a size to divide by.
    let placed = img.abs_transform();
    let name = if img.id().is_empty() {
        "Image".to_string()
    } else {
        img.id().to_string()
    };
    Some(ImportedImage {
        name,
        rgba: decoded.rgba8,
        width: decoded.width,
        height: decoded.height,
        transform: chitrakar_doc::Transform {
            a: placed.sx,
            b: placed.ky,
            c: placed.kx,
            d: placed.sy,
            e: placed.tx,
            f: placed.ty,
        },
        opacity,
        below,
        clip: None,
    })
}

/// One subpath as anchors with bezier handles, and whether it closes.
struct Ring {
    points: Vec<[f32; 2]>,
    handles: Vec<[f32; 4]>,
    closed: bool,
}

impl Ring {
    fn curved(&self) -> bool {
        self.handles
            .iter()
            .any(|h| h.iter().any(|v| v.abs() > 1e-6))
    }

    /// The ring as straight segments, curves sampled: what the extra
    /// rings of a compound path are made of.
    fn flattened(&self) -> Vec<[f32; 2]> {
        if !self.curved() {
            return self.points.clone();
        }
        const STEPS: usize = 8;
        let n = self.points.len();
        let segments = if self.closed { n } else { n.saturating_sub(1) };
        let mut out = Vec::with_capacity(segments * STEPS + 1);
        for i in 0..segments {
            let j = (i + 1) % n;
            let (a, b) = (self.points[i], self.points[j]);
            let c1 = [a[0] + self.handles[i][2], a[1] + self.handles[i][3]];
            let c2 = [b[0] + self.handles[j][0], b[1] + self.handles[j][1]];
            for s in 0..STEPS {
                let t = s as f32 / STEPS as f32;
                let u = 1.0 - t;
                let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                out.push([
                    w0 * a[0] + w1 * c1[0] + w2 * c2[0] + w3 * b[0],
                    w0 * a[1] + w1 * c1[1] + w2 * c2[1] + w3 * b[1],
                ]);
            }
        }
        if !self.closed {
            out.push(self.points[n - 1]);
        }
        out
    }
}

/// The path's subpaths in document space, cubic beziers kept as handles
/// and quadratics raised to cubics.
fn rings_of(path: &usvg::Path) -> Vec<Ring> {
    let t = path.abs_transform();
    let map = |p: usvg::tiny_skia_path::Point| -> [f32; 2] {
        let mut q = p;
        t.map_point(&mut q);
        [q.x, q.y]
    };
    let mut rings: Vec<Ring> = Vec::new();
    let mut ring: Option<Ring> = None;
    let finish = |ring: &mut Option<Ring>, rings: &mut Vec<Ring>| {
        if let Some(r) = ring.take() {
            if r.points.len() >= 2 {
                rings.push(r);
            }
        }
    };
    for seg in path.data().segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                finish(&mut ring, &mut rings);
                ring = Some(Ring {
                    points: vec![map(p)],
                    handles: vec![[0.0; 4]],
                    closed: false,
                });
            }
            PathSegment::LineTo(p) => {
                if let Some(r) = &mut ring {
                    r.points.push(map(p));
                    r.handles.push([0.0; 4]);
                }
            }
            PathSegment::QuadTo(c, p) => {
                if let Some(r) = &mut ring {
                    let a = *r.points.last().unwrap();
                    let (c, p) = (map(c), map(p));
                    let c1 = [
                        a[0] + 2.0 / 3.0 * (c[0] - a[0]),
                        a[1] + 2.0 / 3.0 * (c[1] - a[1]),
                    ];
                    let c2 = [
                        p[0] + 2.0 / 3.0 * (c[0] - p[0]),
                        p[1] + 2.0 / 3.0 * (c[1] - p[1]),
                    ];
                    let last = r.handles.last_mut().unwrap();
                    last[2] = c1[0] - a[0];
                    last[3] = c1[1] - a[1];
                    r.points.push(p);
                    r.handles.push([c2[0] - p[0], c2[1] - p[1], 0.0, 0.0]);
                }
            }
            PathSegment::CubicTo(c1, c2, p) => {
                if let Some(r) = &mut ring {
                    let a = *r.points.last().unwrap();
                    let (c1, c2, p) = (map(c1), map(c2), map(p));
                    let last = r.handles.last_mut().unwrap();
                    last[2] = c1[0] - a[0];
                    last[3] = c1[1] - a[1];
                    r.points.push(p);
                    r.handles.push([c2[0] - p[0], c2[1] - p[1], 0.0, 0.0]);
                }
            }
            PathSegment::Close => {
                if let Some(r) = &mut ring {
                    // A closing segment back to the start: drop a last
                    // anchor that already sits there, keeping its handle.
                    if r.points.len() > 2 {
                        let (first, last) = (r.points[0], *r.points.last().unwrap());
                        if (first[0] - last[0]).abs() < 1e-4 && (first[1] - last[1]).abs() < 1e-4 {
                            let h = r.handles.pop().unwrap();
                            r.points.pop();
                            r.handles[0][0] = h[0];
                            r.handles[0][1] = h[1];
                        }
                    }
                    r.closed = true;
                }
                finish(&mut ring, &mut rings);
            }
        }
    }
    finish(&mut ring, &mut rings);
    rings
}

fn color_of(c: usvg::Color, alpha: f32) -> AuthoredColor {
    AuthoredColor::Srgb {
        r: c.red as f32 / 255.0,
        g: c.green as f32 / 255.0,
        b: c.blue as f32 / 255.0,
        a: alpha,
    }
}

fn stops_of(stops: &[usvg::Stop], alpha: f32) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|s| GradientStop {
            offset: s.offset().get(),
            color: color_of(s.color(), s.opacity().get() * alpha),
        })
        .collect()
}

/// A gradient in the shape's own box, 0..1 on each axis: usvg's is in
/// user space, so its ends go through the path's transform and the
/// gradient's own, then into the box the shape covers.
fn gradient_of(
    paint: &usvg::Paint,
    path: &usvg::Path,
    bbox: [f32; 4],
    alpha: f32,
) -> Option<Gradient> {
    let abs = path.abs_transform();
    let (bw, bh) = ((bbox[2] - bbox[0]).max(1e-6), (bbox[3] - bbox[1]).max(1e-6));
    let norm = |t: usvg::Transform, x: f32, y: f32| -> [f32; 2] {
        let mut p = usvg::tiny_skia_path::Point::from_xy(x, y);
        t.map_point(&mut p);
        abs.map_point(&mut p);
        [(p.x - bbox[0]) / bw, (p.y - bbox[1]) / bh]
    };
    match paint {
        usvg::Paint::LinearGradient(g) => Some(Gradient::Linear {
            from: norm(g.transform(), g.x1(), g.y1()),
            to: norm(g.transform(), g.x2(), g.y2()),
            stops: stops_of(g.stops(), alpha),
        }),
        usvg::Paint::RadialGradient(g) => {
            let center = norm(g.transform(), g.cx(), g.cy());
            let (sx, sy) = g.transform().get_scale();
            let (ax, ay) = abs.get_scale();
            let r = g.r().get() * ((sx * ax + sy * ay) / 2.0).abs();
            Some(Gradient::Radial {
                center,
                radius: r / (0.5 * (bw * bw + bh * bh).sqrt()),
                stops: stops_of(g.stops(), alpha),
            })
        }
        _ => None,
    }
}

/// A solid colour for a paint: the colour itself, or a gradient's first
/// stop where only a colour will do.
fn solid_of(paint: &usvg::Paint, alpha: f32) -> Option<AuthoredColor> {
    match paint {
        usvg::Paint::Color(c) => Some(color_of(*c, alpha)),
        usvg::Paint::LinearGradient(g) => g.stops().first().map(|s| color_of(s.color(), alpha)),
        usvg::Paint::RadialGradient(g) => g.stops().first().map(|s| color_of(s.color(), alpha)),
        usvg::Paint::Pattern(_) => None,
    }
}

/// A path SVG fills by winding, said in the only rule this engine has.
///
/// Subpaths here are filled even-odd: a point inside two of them is
/// outside the shape. SVG's default is *nonzero* — inside two is still
/// inside — and the two only part company where subpaths overlap. For a
/// letter with a counter, or any outline drawn with its holes wound the
/// other way, they agree and there is nothing to do; where they disagree
/// the file draws solid and this drew a hole, which is a shape coming in
/// wrong rather than a shade being off.
///
/// Where every ring is wound the same way, nonzero is *exactly* the
/// union of them: a point inside `k` of them has winding `±k`, which is
/// non-zero for every `k ≥ 1`, and that is what a union covers. So that
/// case is converted rather than approximated, through the same shape
/// booleans a selection is built with.
///
/// Rings wound both ways are left alone. They are the ordinary
/// outline-with-holes, where the two rules already agree, and the cases
/// where they do not — a hole inside two overlapping outlines still
/// being filled — cannot be said as a union and are not worth guessing
/// at. Nothing is done unless two same-wound rings actually overlap,
/// since the union flattens curves and a path that needs no correction
/// should not pay for one.
fn as_even_odd(path: &usvg::Path, rings: Vec<Ring>) -> Vec<Ring> {
    let nonzero = path
        .fill()
        .is_some_and(|f| f.rule() == usvg::FillRule::NonZero);
    if !nonzero || rings.len() < 2 {
        return rings;
    }
    let flat: Vec<Vec<[f32; 2]>> = rings.iter().map(Ring::flattened).collect();
    let area = |r: &[[f32; 2]]| -> f32 {
        let mut a = 0.0;
        for i in 0..r.len() {
            let (p, q) = (r[i], r[(i + 1) % r.len()]);
            a += p[0] * q[1] - q[0] * p[1];
        }
        a / 2.0
    };
    let signs: Vec<f32> = flat.iter().map(|r| area(r).signum()).collect();
    if signs.windows(2).any(|w| w[0] != w[1]) {
        return rings;
    }
    let box_of = |r: &[[f32; 2]]| {
        r.iter()
            .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            })
    };
    let boxes: Vec<[f32; 4]> = flat.iter().map(|r| box_of(r)).collect();
    let touching = (0..boxes.len()).any(|i| {
        (i + 1..boxes.len()).any(|j| {
            let (a, b) = (boxes[i], boxes[j]);
            a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
        })
    });
    if !touching {
        return rings;
    }
    let mut acc: Vec<Vec<[f32; 2]>> = vec![flat[0].clone()];
    for next in &flat[1..] {
        match chitrakar_render::boolean::combine_or_nudge(
            &acc,
            std::slice::from_ref(next),
            chitrakar_render::boolean::BoolOp::Union,
        ) {
            Some(joined) => acc = joined,
            // Outlines that only touch, or share an edge exactly, are
            // what `combine` declines. The shape as it stands is the
            // honest answer there rather than a guess.
            None => return rings,
        }
    }
    acc.into_iter()
        .map(|points| Ring {
            handles: vec![[0.0; 4]; points.len()],
            points,
            closed: true,
        })
        .collect()
}

fn shape_of(path: &usvg::Path, opacity: f32) -> Option<Node> {
    let mut rings = rings_of(path);
    if rings.is_empty() {
        return None;
    }
    rings = as_even_odd(path, rings);
    // The main ring keeps its curves; the rest, straight-sided, cut holes
    // or add islands. The first subpath is taken as the main one, which
    // is how outlines are usually drawn.
    let main = rings.remove(0);
    let subpaths: Vec<Vec<[f32; 2]>> = rings.iter().map(Ring::flattened).collect();
    let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in main.points.iter().chain(subpaths.iter().flatten()) {
        bbox = [
            bbox[0].min(p[0]),
            bbox[1].min(p[1]),
            bbox[2].max(p[0]),
            bbox[3].max(p[1]),
        ];
    }
    let name = if path.id().is_empty() {
        "Path"
    } else {
        path.id()
    };
    let mut node = Node::vector(
        name,
        VectorShape::Path {
            points: main.points,
            closed: main.closed,
            smooth: false,
            handles: main.handles,
            subpaths,
        },
    );
    if let NodeKind::Vector {
        fill,
        stroke,
        gradient,
        ..
    } = &mut node.kind
    {
        *fill = None;
        if let Some(f) = path.fill() {
            let alpha = f.opacity().get() * opacity;
            *gradient = gradient_of(f.paint(), path, bbox, alpha);
            *fill = solid_of(f.paint(), alpha);
        }
        *stroke = path.stroke().and_then(|s| {
            let (sx, sy) = path.abs_transform().get_scale();
            let scale = (sx.abs() + sy.abs()) / 2.0;
            Some(Stroke {
                color: solid_of(s.paint(), s.opacity().get() * opacity)?,
                width: s.width().get() * scale,
                widths: Vec::new(),
                // A broken line came in solid, which is a plain loss: the
                // engine has had dashes of its own all along and the
                // importer was writing an empty pattern over the file's.
                // They mean the same thing — lengths along the outline,
                // on and off in turn and repeating — and they are in the
                // same units as the width, so they take the same scale.
                //
                // An odd-length pattern needs no special handling: SVG
                // repeats it to make the runs alternate, and a pattern
                // walked round and round does that by itself. What is
                // *not* carried is `stroke-dashoffset`, which shifts
                // where the pattern starts along the line and has no
                // field here to land in. A line whose dashes begin a
                // little further along is much nearer the file than a
                // line with no dashes at all, so it comes in unshifted
                // rather than being refused.
                dash: s
                    .dasharray()
                    .map(|d| d.iter().map(|v| v * scale).collect())
                    .unwrap_or_default(),
                // What the file says, not what this engine happens to
                // default to: SVG's own default is a flat end and a
                // mitred corner, and a line imported round when it was
                // drawn flat is a line that came in wrong.
                cap: match s.linecap() {
                    usvg::LineCap::Butt => chitrakar_doc::StrokeCap::Butt,
                    usvg::LineCap::Round => chitrakar_doc::StrokeCap::Round,
                    usvg::LineCap::Square => chitrakar_doc::StrokeCap::Square,
                },
                join: match s.linejoin() {
                    usvg::LineJoin::Round => chitrakar_doc::StrokeJoin::Round,
                    usvg::LineJoin::Bevel => chitrakar_doc::StrokeJoin::Bevel,
                    // A miter that clips is a miter as far as this
                    // engine is concerned; it has one limit, not two.
                    usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => {
                        chitrakar_doc::StrokeJoin::Miter
                    }
                },
                // A file's own markers are shapes of its choosing, drawn
                // from a definition this engine has no room for; they
                // come in as the paths they are, alongside the line.
                start_marker: Default::default(),
                end_marker: Default::default(),
                // SVG has no notion of which side of the outline a
                // stroke lies on: it always straddles it. So a file
                // says "centred" by saying nothing.
                align: Some(chitrakar_doc::StrokeAlign::Centre),
            })
        });
    }
    Some(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::ColorMode;
    use chitrakar_doc::{Command, Document};

    const SAMPLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="100">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#ff0000"/>
      <stop offset="1" stop-color="#0000ff"/>
    </linearGradient>
  </defs>
  <rect id="box" x="10" y="10" width="40" height="30" fill="#ff0000"/>
  <circle cx="80" cy="25" r="15" fill="#0000ff" stroke="#000000" stroke-width="4"/>
  <path d="M0 0 h30 v30 h-30 z M10 10 h10 v10 h-10 z" fill="#00ff00" fill-rule="evenodd" transform="translate(10 50)"/>
  <g opacity="0.5"><rect x="60" y="80" width="20" height="10" fill="#000000"/></g>
  <rect x="60" y="50" width="40" height="20" fill="url(#g)"/>
  <text x="90" y="95" font-size="20">Hi</text>
</svg>"##;

    fn bbox(node: &Node) -> [f32; 4] {
        let NodeKind::Vector {
            shape: VectorShape::Path { points, .. },
            ..
        } = &node.kind
        else {
            panic!("a path")
        };
        points
            .iter()
            .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            })
    }

    #[test]
    fn shapes_come_in_as_paths_with_their_paint_in_document_space() {
        let svg = import_svg(SAMPLE.as_bytes()).unwrap();
        assert_eq!((svg.width, svg.height), (120.0, 100.0));
        let shapes = &svg.shapes;
        assert!(
            shapes.len() >= 6,
            "rect, circle, path, faded rect, gradient rect, the text's outline: {}",
            shapes.len()
        );
        // The rect: its four corners, named by its id, red.
        assert_eq!(shapes[0].name, "box");
        assert_eq!(bbox(&shapes[0]), [10.0, 10.0, 50.0, 40.0]);
        let NodeKind::Vector { fill, gradient, .. } = &shapes[0].kind else {
            panic!()
        };
        assert!(
            matches!(fill, Some(AuthoredColor::Srgb { r, g, b, a }) if *r == 1.0 && *g == 0.0 && *b == 0.0 && *a == 1.0)
        );
        assert!(gradient.is_none());
        // The circle keeps its curves and its stroke.
        let NodeKind::Vector {
            shape: VectorShape::Path {
                handles, closed, ..
            },
            stroke,
            ..
        } = &shapes[1].kind
        else {
            panic!()
        };
        assert!(
            *closed && handles.iter().any(|h| h[2].abs() > 1.0),
            "curved"
        );
        assert!((stroke.as_ref().unwrap().width - 4.0).abs() < 1e-3);
        let b = bbox(&shapes[1]);
        assert!(
            (b[0] - 65.0).abs() < 0.5 && (b[2] - 95.0).abs() < 0.5,
            "{b:?}"
        );
        // The compound path: moved by its transform, with its hole.
        let NodeKind::Vector {
            shape: VectorShape::Path { subpaths, .. },
            ..
        } = &shapes[2].kind
        else {
            panic!()
        };
        assert_eq!(subpaths.len(), 1);
        assert_eq!(bbox(&shapes[2]), [10.0, 50.0, 40.0, 80.0]);
        assert_eq!(subpaths[0][0], [20.0, 60.0]);
        // The group's opacity rides on its child's colour.
        let NodeKind::Vector { fill, .. } = &shapes[3].kind else {
            panic!()
        };
        assert!(matches!(fill, Some(AuthoredColor::Srgb { a, .. }) if (*a - 0.5).abs() < 1e-3));
        // The gradient: in the shape's own box, left to right, two stops.
        let NodeKind::Vector { gradient, .. } = &shapes[4].kind else {
            panic!()
        };
        let Some(Gradient::Linear { from, to, stops }) = gradient else {
            panic!("a linear gradient")
        };
        assert!(
            (from[0]).abs() < 1e-3 && (to[0] - 1.0).abs() < 1e-3 && stops.len() == 2,
            "{from:?} {to:?}"
        );
        // Text became outlines near where it was set.
        let glyphs = &shapes[5..];
        assert!(glyphs
            .iter()
            .all(|g| bbox(g)[0] > 85.0 && bbox(g)[3] < 100.0));

        // Rendered, the page reads as the file drew it.
        let mut doc = Document::new(120, 100, ColorMode::Rgb);
        let root = doc.root();
        for (i, node) in svg.shapes.iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(node.clone()),
            })
            .unwrap();
        }
        let page = chitrakar_render::render(&doc).unwrap();
        let px = |x, y| page.get(x, y).to_srgb8();
        assert_eq!(px(30, 25), [255, 0, 0, 255], "red rect");
        assert!(
            px(80, 25)[2] > 200 && px(80, 25)[0] < 50,
            "blue circle {:?}",
            px(80, 25)
        );
        assert!(
            px(66, 25)[3] == 255 && px(66, 25)[2] < 60,
            "black stroke on the circle's rim {:?}",
            px(66, 25)
        );
        assert!(px(15, 55)[1] > 200, "green path");
        assert_eq!(px(25, 65)[3], 0, "its hole shows through");
        assert!(
            (px(70, 85)[3] as i32 - 128).abs() < 3,
            "half-opaque rect {:?}",
            px(70, 85)
        );
        assert!(
            px(62, 60)[0] > 200 && px(98, 60)[2] > 200,
            "gradient runs red to blue"
        );
        assert!(
            (85..115).any(|x| (80..100).any(|y| px(x, y)[3] > 0)),
            "glyph ink"
        );
    }

    /// A line comes in ending and turning the way the file says, not the
    /// way this engine happens to default to. SVG's own defaults are a
    /// flat end and a mitred corner; ours are round.
    #[test]
    fn a_line_comes_in_ending_the_way_the_file_says() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="60">
  <path d="M5 5 h30 v30" fill="none" stroke="#000000" stroke-width="6"/>
  <path d="M5 45 h30" fill="none" stroke="#000000" stroke-width="6"
        stroke-linecap="square" stroke-linejoin="bevel"/>
</svg>"##;
        let shapes = import_svg(svg.as_bytes()).unwrap().shapes;
        let ends = |i: usize| {
            let NodeKind::Vector {
                stroke: Some(s), ..
            } = &shapes[i].kind
            else {
                panic!("a stroked path")
            };
            (s.cap, s.join)
        };
        assert_eq!(
            ends(0),
            (
                chitrakar_doc::StrokeCap::Butt,
                chitrakar_doc::StrokeJoin::Miter
            ),
            "what the file leaves unsaid is SVG's own default, not ours"
        );
        assert_eq!(
            ends(1),
            (
                chitrakar_doc::StrokeCap::Square,
                chitrakar_doc::StrokeJoin::Bevel
            ),
        );
    }

    #[test]
    fn what_this_writes_it_reads_back() {
        let mut doc = Document::new(120, 100, ColorMode::Rgb);
        let root = doc.root();
        let imported = import_svg(SAMPLE.as_bytes()).unwrap();
        for (i, node) in imported.shapes[..5].iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(node.clone()),
            })
            .unwrap();
        }
        let svg = crate::export_svg(&doc).unwrap();
        let again = import_svg(svg.as_bytes()).unwrap();
        assert_eq!(again.shapes.len(), 5);
        for (a, b) in imported.shapes[..5].iter().zip(&again.shapes) {
            let (ba, bb) = (bbox(a), bbox(b));
            assert!(
                ba.iter().zip(bb.iter()).all(|(p, q)| (p - q).abs() < 0.05),
                "{ba:?} vs {bb:?}"
            );
        }
        assert!(import_svg(b"<not svg").is_err());
    }

    /// A picture inside the file comes in as a picture.
    ///
    /// It used to be dropped where it stood: an SVG with a photograph in
    /// it imported as the shapes around the photograph and nothing where
    /// it was, with nothing said. What makes that easy to miss is that
    /// the import still succeeds and still draws — it is just missing a
    /// layer, and only somebody who knew what the file held would know.
    ///
    /// Two halves are worth asking separately. That the pixels arrive at
    /// all, unmuddled — the four corners of a two-by-two are four known
    /// colours, so a picture flipped, rotated or read in the wrong order
    /// says so plainly where a photograph would not. And that it lands
    /// where the file put it, which is the part the importer computes:
    /// usvg gives the rectangle and the picture's own grid is whatever
    /// the bytes were, so the scale between them is the importer's to
    /// get right.
    #[test]
    fn a_picture_inside_the_file_comes_in_as_a_picture() {
        // Two by two: red, green over blue, white. Drawn into a box
        // forty across and twenty down at (20,10), so a texel is twenty
        // by ten. The stretch is asked for, and both halves of that
        // matter: a square box makes the two scale factors equal, and so
        // does an oblong one under SVG's default, which letterboxes and
        // keeps the aspect — usvg resolves that into the size it reports,
        // which is why the importer can treat the two as a plain ratio.
        // Either way a test written on them could not tell the axes
        // apart, and would pass with the scale swapped.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEklEQVR4nGP4z8DwHwyBNBgAAEnICff5q7YNAAAAAElFTkSuQmCC";
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="80">
                 <rect x="0" y="0" width="100" height="80" fill="#202020"/>
                 <image x="20" y="10" width="40" height="20" preserveAspectRatio="none" xlink:href="data:image/png;base64,{png}"/>
                 <rect x="0" y="70" width="10" height="10" fill="#00ffff"/>
               </svg>"##
        );
        let imported = import_svg(svg.as_bytes()).unwrap();
        assert_eq!(imported.images.len(), 1, "the picture came in");
        let pic = &imported.images[0];
        assert_eq!((pic.width, pic.height), (2, 2), "its own grid, not the box");
        // Its own pixels, in reading order, undisturbed.
        assert_eq!(&pic.rgba[..4], &[255, 0, 0, 255], "top left is red");
        assert_eq!(&pic.rgba[4..8], &[0, 255, 0, 255], "top right is green");
        assert_eq!(&pic.rgba[8..12], &[0, 0, 255, 255], "bottom left is blue");
        // Carried to where the file drew it: a texel is twenty across
        // and ten down, each axis its own.
        let t = pic.transform;
        assert!(
            (t.a - 20.0).abs() < 1e-3 && (t.d - 10.0).abs() < 1e-3,
            "scaled into its box, each axis its own: {t:?}"
        );
        assert!(
            (t.e - 20.0).abs() < 1e-3 && (t.f - 10.0).abs() < 1e-3,
            "at the corner the file names: {t:?}"
        );
        // And it sits above the ground and below the mark, which is the
        // order the file wrote and the reason `below` is counted at all.
        assert_eq!(pic.below, 1, "one shape under it, one over");
        assert_eq!(imported.shapes.len(), 2);

        // Placed, it draws where it says. Putting the two halves back
        // together is the caller's job — the pixels into the pool, the
        // layer that names them into the tree — so this does it the way
        // the engine does and then asks the page.
        let mut doc = Document::new(100, 80, ColorMode::Rgb);
        let root = doc.root();
        for (i, shape) in imported.shapes.iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(shape.clone()),
            })
            .unwrap();
        }
        let resource_id = doc.add_resource(pic.width, pic.height, pic.rgba.clone());
        let mut raster = Node::raster(
            &pic.name,
            chitrakar_doc::RasterRef {
                resource_id,
                width: pic.width,
                height: pic.height,
            },
        );
        raster.transform = pic.transform;
        doc.apply(Command::AddNode {
            parent: root,
            index: pic.below,
            node: Box::new(raster),
        })
        .unwrap();
        let page = chitrakar_render::render(&doc).unwrap();
        let at = |x: u32, y: u32| {
            let p = page.get(x, y);
            [
                (p.r.max(0.0).powf(1.0 / 2.2) * 255.0).round() as u8,
                (p.g.max(0.0).powf(1.0 / 2.2) * 255.0).round() as u8,
                (p.b.max(0.0).powf(1.0 / 2.2) * 255.0).round() as u8,
            ]
        };
        // The four quarters of the box, sampled well inside each.
        let (tl, tr, bl, br) = (at(28, 14), at(50, 14), at(28, 25), at(50, 25));
        assert!(tl[0] > 200 && tl[1] < 60, "top left draws red: {tl:?}");
        assert!(tr[1] > 200 && tr[0] < 60, "top right draws green: {tr:?}");
        assert!(bl[2] > 200 && bl[0] < 60, "bottom left draws blue: {bl:?}");
        assert!(
            br[0] > 200 && br[1] > 200 && br[2] > 200,
            "bottom right draws white: {br:?}"
        );
        // And nothing of it outside the box the file gave it.
        let outside = at(10, 25);
        assert!(
            outside[0] < 60 && outside[1] < 60 && outside[2] < 60,
            "the ground beside it is still the ground: {outside:?}"
        );
    }

    /// A broken line comes in broken.
    ///
    /// The importer wrote an empty dash pattern over whatever the file
    /// said, so every dashed rule, every cut line and every selection
    /// border in an imported drawing arrived solid. It reads as a
    /// slightly wrong line rather than as a missing one, which is why it
    /// sat there: nothing is absent from the layer list and nothing
    /// fails.
    ///
    /// Asked of the page rather than the field, because the field only
    /// says the numbers were copied. What says the line is broken is ink
    /// where the pattern is on and paper where it is off, and the two
    /// read at the same place on the same line — so a line that came in
    /// solid fails on the gap and a line that came in missing fails on
    /// the dash.
    #[test]
    fn a_dashed_line_comes_in_dashed() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="20">
             <path d="M 5 10 H 55" stroke="#000000" stroke-width="6"
                   stroke-dasharray="10 10" fill="none"/>
           </svg>"##;
        let imported = import_svg(svg.as_bytes()).unwrap();
        assert_eq!(imported.shapes.len(), 1);
        let NodeKind::Vector { stroke, .. } = &imported.shapes[0].kind else {
            panic!("a stroked path")
        };
        let dash = &stroke.as_ref().expect("it is stroked").dash;
        assert_eq!(dash.len(), 2, "the pattern came over: {dash:?}");
        assert!(
            (dash[0] - 10.0).abs() < 1e-3 && (dash[1] - 10.0).abs() < 1e-3,
            "in the file's own units: {dash:?}"
        );

        let mut doc = Document::new(60, 20, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(imported.shapes[0].clone()),
        })
        .unwrap();
        let page = chitrakar_render::render(&doc).unwrap();
        // The line runs from x=5 to x=55 at y=10, ten on and ten off: ink
        // at 5..15, bare at 15..25, ink again at 25..35.
        let ink = |x: u32| page.get(x, 10).a;
        assert!(ink(9) > 0.9, "the first dash is drawn ({})", ink(9));
        assert!(ink(20) < 0.1, "the gap after it is bare ({})", ink(20));
        assert!(ink(30) > 0.9, "and the next dash is drawn ({})", ink(30));

        // And under a transform, which is the half the first case cannot
        // ask: a dash pattern is in the same units as the width and has
        // to take the same scale, but at scale one that is a no-op and a
        // test written on it passes with the scaling taken out. The same
        // line inside a doubling, where the pattern has to double too.
        let scaled = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40">
             <g transform="scale(2)">
               <path d="M 5 10 H 55" stroke="#000000" stroke-width="6"
                     stroke-dasharray="10 10" fill="none"/>
             </g>
           </svg>"##;
        let imported = import_svg(scaled.as_bytes()).unwrap();
        let NodeKind::Vector { stroke, .. } = &imported.shapes[0].kind else {
            panic!("a stroked path")
        };
        let st = stroke.as_ref().expect("it is stroked");
        assert!(
            (st.width - 12.0).abs() < 1e-3,
            "the width doubled: {}",
            st.width
        );
        assert!(
            (st.dash[0] - 20.0).abs() < 1e-3 && (st.dash[1] - 20.0).abs() < 1e-3,
            "and so did the pattern: {:?}",
            st.dash
        );
        let mut doc = Document::new(120, 40, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(imported.shapes[0].clone()),
        })
        .unwrap();
        let page = chitrakar_render::render(&doc).unwrap();
        let ink = |x: u32| page.get(x, 20).a;
        assert!(ink(18) > 0.9, "the first dash is drawn ({})", ink(18));
        assert!(ink(40) < 0.1, "the gap is twice as long ({})", ink(40));
        assert!(ink(60) > 0.9, "and the next dash is drawn ({})", ink(60));
    }

    /// A path SVG fills by winding comes in filled the same way.
    ///
    /// Subpaths here are even-odd and SVG's default is nonzero. They part
    /// company exactly where subpaths overlap, and there the file draws
    /// solid while this drew a hole — the shape itself wrong, not a shade
    /// off. Two rectangles in one path, wound the same way and overlapping
    /// in the middle, is the smallest thing that says so.
    ///
    /// Held against resvg rather than against a number picked by hand, so
    /// what it asserts is the file's own meaning rather than this
    /// importer's idea of it.
    #[test]
    fn a_path_filled_by_winding_comes_in_filled_by_winding() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <path d="M 5 5 H 35 V 35 H 5 Z M 20 10 H 50 V 30 H 20 Z"
                   fill="#cc0000"/>
           </svg>"##;
        let imported = import_svg(svg.as_bytes()).unwrap();
        let mut doc = Document::new(60, 40, ColorMode::Rgb);
        let root = doc.root();
        for (i, n) in imported.shapes.iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(n.clone()),
            })
            .unwrap();
        }
        let ours = chitrakar_render::render(&doc).unwrap();
        // What resvg makes of the same file, which is the answer.
        let tree = {
            let mut opt = usvg::Options::default();
            opt.fontdb_mut().load_font_data(FACE.to_vec());
            usvg::Tree::from_data(svg.as_bytes(), &opt).unwrap()
        };
        let mut pix = resvg::tiny_skia::Pixmap::new(60, 40).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pix.as_mut(),
        );
        let theirs = |x: u32, y: u32| pix.pixel(x, y).unwrap().alpha();

        for (x, y, what) in [
            (10u32, 20u32, "the first rectangle alone"),
            (
                27,
                20,
                "the overlap, which winding fills and even-odd would not",
            ),
            (45, 20, "the second rectangle alone"),
            (50, 5, "the paper outside both"),
        ] {
            let mine = ours.get(x, y).a > 0.5;
            let want = theirs(x, y) > 128;
            assert_eq!(mine, want, "{what} at {x},{y}");
        }

        // The other half, and the reason the winding sign is looked at
        // rather than every nonzero path being unioned: an outline with a
        // counter wound the other way is a hole under *both* rules, and
        // a union would fill it in. This is the ordinary case — every
        // letter with a hole in it — so getting it wrong would be worse
        // than the bug being fixed.
        let holed = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <path d="M 5 5 H 55 V 35 H 5 Z M 20 15 V 25 H 40 V 15 Z"
                   fill="#cc0000"/>
           </svg>"##;
        let imported = import_svg(holed.as_bytes()).unwrap();
        let mut doc = Document::new(60, 40, ColorMode::Rgb);
        let root = doc.root();
        for (i, n) in imported.shapes.iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(n.clone()),
            })
            .unwrap();
        }
        let ours = chitrakar_render::render(&doc).unwrap();
        let tree = {
            let mut opt = usvg::Options::default();
            opt.fontdb_mut().load_font_data(FACE.to_vec());
            usvg::Tree::from_data(holed.as_bytes(), &opt).unwrap()
        };
        let mut pix = resvg::tiny_skia::Pixmap::new(60, 40).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pix.as_mut(),
        );
        let theirs = |x: u32, y: u32| pix.pixel(x, y).unwrap().alpha();
        for (x, y, what) in [
            (10u32, 20u32, "the outline itself"),
            (30, 20, "the counter, which stays a hole"),
            (30, 8, "the outline above it"),
        ] {
            let mine = ours.get(x, y).a > 0.5;
            let want = theirs(x, y) > 128;
            assert_eq!(mine, want, "{what} at {x},{y}");
        }

        // And a third thing, which is what the overlap test is for rather
        // than the picture: the union goes through flattened outlines, so
        // a path that needs no correction must not be put through it and
        // come back a polygon. Two circles in one nonzero path, wound the
        // same way and nowhere near each other — the two rules already
        // agree, and the curves have to survive.
        let apart = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40">
             <path fill="#cc0000"
                   d="M 5 20 A 10 10 0 1 0 25 20 A 10 10 0 1 0 5 20 Z
                      M 55 20 A 8 8 0 1 0 71 20 A 8 8 0 1 0 55 20 Z"/>
           </svg>"##;
        let imported = import_svg(apart.as_bytes()).unwrap();
        let NodeKind::Vector {
            shape: VectorShape::Path { handles, .. },
            ..
        } = &imported.shapes[0].kind
        else {
            panic!("a path")
        };
        assert!(
            handles.iter().any(|h| h.iter().any(|v| v.abs() > 1.0)),
            "circles nowhere near each other keep their curves"
        );
    }

    /// What a clip path hides stays hidden.
    ///
    /// The importer never read `clip-path`, so a clipped group came in
    /// with everything showing — the loudest of the losses in this file,
    /// since the others made a line or a corner slightly wrong and this
    /// one puts artwork on the page that the file says is not there.
    ///
    /// A clip survives the group being flattened away, which is why it
    /// can be carried at all: it multiplies coverage by nought or one and
    /// that distributes over the children exactly, so each child seen
    /// only inside the region is the same picture as the group seen only
    /// inside it. Opacity and blending do not distribute that way and are
    /// still folded into the colours instead.
    #[test]
    fn what_a_clip_path_hides_stays_hidden() {
        let drawn = |svg: &str, w: u32, h: u32| {
            let imported = import_svg(svg.as_bytes()).unwrap();
            let mut doc = Document::new(w, h, ColorMode::Rgb);
            let root = doc.root();
            for (i, n) in imported.shapes.iter().enumerate() {
                doc.apply(Command::AddNode {
                    parent: root,
                    index: i,
                    node: Box::new(n.clone()),
                })
                .unwrap();
            }
            let ours = chitrakar_render::render(&doc).unwrap();
            let tree = {
                let mut opt = usvg::Options::default();
                opt.fontdb_mut().load_font_data(FACE.to_vec());
                usvg::Tree::from_data(svg.as_bytes(), &opt).unwrap()
            };
            let mut pix = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pix.as_mut(),
            );
            (ours, pix)
        };

        // A band twenty wide clipping a rectangle fifty wide: what is
        // outside the band is what used to come in anyway.
        let one = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs><clipPath id="c"><rect x="5" y="5" width="20" height="30"/></clipPath></defs>
             <g clip-path="url(#c)">
               <rect x="5" y="10" width="50" height="20" fill="#cc0000"/>
             </g>
           </svg>"##;
        // Two clips, one inside the other: only what both let through.
        let nested = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs>
               <clipPath id="a"><rect x="5" y="5" width="30" height="30"/></clipPath>
               <clipPath id="b"><rect x="20" y="5" width="30" height="30"/></clipPath>
             </defs>
             <g clip-path="url(#a)"><g clip-path="url(#b)">
               <rect x="0" y="10" width="60" height="20" fill="#0044cc"/>
             </g></g>
           </svg>"##;
        // A clip of two outlines shows where *either* covers, which is a
        // union rather than the even-odd of one compound shape.
        let two = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs><clipPath id="c">
               <rect x="5" y="5" width="15" height="30"/>
               <rect x="35" y="5" width="15" height="30"/>
             </clipPath></defs>
             <g clip-path="url(#c)">
               <rect x="0" y="10" width="60" height="20" fill="#118833"/>
             </g>
           </svg>"##;
        // And two that *overlap*, which is the case that says union
        // rather than even-odd. Where they cross, one compound shape
        // filled even-odd would punch a hole; a clip shows it.
        let crossing = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs><clipPath id="c">
               <rect x="5" y="5" width="25" height="30"/>
               <rect x="20" y="5" width="30" height="30"/>
             </clipPath></defs>
             <g clip-path="url(#c)">
               <rect x="0" y="10" width="60" height="20" fill="#884499"/>
             </g>
           </svg>"##;

        for (svg, what, spots) in [
            (
                one,
                "one clip",
                vec![(12u32, 20u32, "inside the band"), (40, 20, "outside it")],
            ),
            (
                nested,
                "a clip inside a clip",
                vec![
                    (27, 20, "where both let it through"),
                    (10, 20, "where only the outer does"),
                    (45, 20, "where only the inner does"),
                ],
            ),
            (
                two,
                "a clip of two outlines",
                vec![
                    (12, 20, "inside the first"),
                    (27, 20, "between them, which neither covers"),
                    (42, 20, "inside the second"),
                ],
            ),
            (
                crossing,
                "a clip of two outlines that cross",
                vec![
                    (12, 20, "inside the first alone"),
                    (25, 20, "where they cross, which a union shows"),
                    (45, 20, "inside the second alone"),
                    (55, 20, "beyond both"),
                ],
            ),
        ] {
            let (ours, pix) = drawn(svg, 60, 40);
            for (x, y, where_) in spots {
                let mine = ours.get(x, y).a > 0.5;
                let want = pix.pixel(x, y).unwrap().alpha() > 128;
                assert_eq!(mine, want, "{what}: {where_} at {x},{y}");
            }
        }
    }
}
