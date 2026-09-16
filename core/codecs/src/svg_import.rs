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
    walk(tree.root(), 1.0, &mut shapes, &mut images);
    Ok(ImportedSvg {
        width: tree.size().width(),
        height: tree.size().height(),
        shapes,
        images,
    })
}

fn walk(group: &usvg::Group, opacity: f32, out: &mut Vec<Node>, pics: &mut Vec<ImportedImage>) {
    let opacity = opacity * group.opacity().get();
    for child in group.children() {
        match child {
            usvg::Node::Group(g) => walk(g, opacity, out, pics),
            usvg::Node::Path(p) => {
                if p.is_visible() {
                    if let Some(node) = shape_of(p, opacity) {
                        out.push(node);
                    }
                }
            }
            // Text arrives as the outlines usvg set it in.
            usvg::Node::Text(t) => walk(t.flattened(), opacity, out, pics),
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
                    usvg::ImageKind::SVG(tree) => walk(tree.root(), opacity, out, pics),
                    kind => {
                        if let Some(pic) = picture_of(img, kind, opacity, out.len()) {
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

fn shape_of(path: &usvg::Path, opacity: f32) -> Option<Node> {
    let mut rings = rings_of(path);
    if rings.is_empty() {
        return None;
    }
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
            Some(Stroke {
                color: solid_of(s.paint(), s.opacity().get() * opacity)?,
                width: s.width().get() * ((sx.abs() + sy.abs()) / 2.0),
                widths: Vec::new(),
                dash: Vec::new(),
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
}
