//! SVG import: usvg parses the file, resolves styles, references and
//! transforms, and turns text into outlines with the bundled face; every
//! path that comes out becomes a shape layer in document space, with its
//! fill (solid or gradient), its stroke and the opacity of the groups
//! above it. Groups are flattened: the layers come in painter's order.

use chitrakar_color::AuthoredColor;
use chitrakar_doc::{Gradient, GradientStop, Node, NodeKind, Stroke, VectorShape};
use usvg::tiny_skia_path::PathSegment;

const FACE: &[u8] = include_bytes!("../../render/assets/DejaVuSans.ttf");

/// What an SVG file holds for the document: its page size, and its
/// shapes as nodes, bottom first.
pub struct ImportedSvg {
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Node>,
}

/// Bring an SVG in as shape layers.
pub fn import_svg(data: &[u8]) -> Result<ImportedSvg, String> {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_font_data(FACE.to_vec());
    // Text in a face the file cannot supply is set in the bundled one.
    opt.font_family = "DejaVu Sans".to_string();
    let tree = usvg::Tree::from_data(data, &opt).map_err(|e| e.to_string())?;
    let mut shapes = Vec::new();
    walk(tree.root(), 1.0, &mut shapes);
    Ok(ImportedSvg {
        width: tree.size().width(),
        height: tree.size().height(),
        shapes,
    })
}

fn walk(group: &usvg::Group, opacity: f32, out: &mut Vec<Node>) {
    let opacity = opacity * group.opacity().get();
    for child in group.children() {
        match child {
            usvg::Node::Group(g) => walk(g, opacity, out),
            usvg::Node::Path(p) => {
                if p.is_visible() {
                    if let Some(node) = shape_of(p, opacity) {
                        out.push(node);
                    }
                }
            }
            // Text arrives as the outlines usvg set it in.
            usvg::Node::Text(t) => walk(t.flattened(), opacity, out),
            // Raster images inside an SVG are left out for now.
            usvg::Node::Image(_) => {}
        }
    }
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
}
