//! A document with something of everything in it, and one instance of
//! every [`Command`] that can be asked of it.
//!
//! It lives here rather than inside a test because more than one crate
//! has a question to ask of every command at once — that each undoes to
//! exactly where it started, that each dirties every pixel it changes —
//! and a list written out twice is a list a new command gets added to
//! once. Adding a `Command` variant without adding it here is meant to
//! be the thing that makes those tests fail.

use crate::{
    BlendMode, Command, Document, Effect, Guide, Mask, MaskKind, Node, NodeId, NodeKind,
    PaintStroke, Pin, Pinning, RasterRef, Swatch, TextSpec, Transform, VectorShape,
};
use chitrakar_color::ColorMode;

/// The document and the handful of nodes in it the commands refer to.
pub struct Fixture {
    pub doc: Document,
    pub root: NodeId,
    pub group: NodeId,
    pub under: NodeId,
    pub over: NodeId,
    pub painted: NodeId,
    pub picture: NodeId,
    pub words: NodeId,
    pub frame: NodeId,
    /// The stroke the paint layer was given, so a command can hand it
    /// back changed.
    pub stroke: PaintStroke,
}

/// A filled rectangle. The fill matters: a shape that draws nothing
/// makes a question about pixels — which region a command dirties, say —
/// answer itself.
fn rect(name: &str, fill: [f32; 4]) -> Box<Node> {
    let mut node = Node::vector(
        name,
        VectorShape::Rect {
            width: 22.0,
            height: 18.0,
            radius: 0.0,
        },
    );
    if let NodeKind::Vector { fill: f, .. } = &mut node.kind {
        *f = Some(chitrakar_color::AuthoredColor::Srgb {
            r: fill[0],
            g: fill[1],
            b: fill[2],
            a: fill[3],
        });
    }
    Box::new(node)
}

/// A group with two shapes in it, a paint layer carrying a stroke, a
/// guide: something for every command to have an effect on.
pub fn everything() -> Fixture {
    let mut doc = Document::new(80, 60, ColorMode::Rgb);
    let root = doc.root();
    // Something of everything: a group with two children, a loose
    // shape, a guide, a swatch.
    doc.apply(Command::AddNode {
        parent: root,
        index: 0,
        node: Box::new(Node::group("pair")),
    })
    .unwrap();
    let group = doc.children_of(root).unwrap()[0];
    // Two of them, overlapping and neither at the origin: a command that
    // moves one has to dirty both where it was and where it went, and a
    // half-transparent one on top means what is under it is read as well
    // as covered.
    for (i, (name, fill, at)) in [
        ("under", [0.2, 0.45, 0.8, 1.0], (6.0, 8.0)),
        ("over", [0.9, 0.7, 0.25, 0.6], (18.0, 16.0)),
    ]
    .iter()
    .enumerate()
    {
        doc.apply(Command::AddNode {
            parent: group,
            index: i,
            node: rect(name, *fill),
        })
        .unwrap();
        let id = doc.children_of(group).unwrap()[i];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(at.0, at.1),
        })
        .unwrap();
    }
    let (under, over) = {
        let kids = doc.children_of(group).unwrap();
        (kids[0], kids[1])
    };
    doc.apply(Command::AddNode {
        parent: root,
        index: 1,
        node: Box::new(Node::paint("painted")),
    })
    .unwrap();
    let painted = doc.children_of(root).unwrap()[1];
    let stroke = PaintStroke {
        points: vec![[10.0, 40.0], [52.0, 30.0]],
        radii: vec![5.0],
        color: chitrakar_color::AuthoredColor::Srgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        softness: 0.25,
        erase: false,
        source: [0.0; 2],
        heal: false,
        clip: None,
    };
    doc.apply(Command::AddStroke {
        id: painted,
        index: 0,
        stroke: Box::new(stroke.clone()),
        on_mask: false,
    })
    .unwrap();
    // A picture, a block of text and a frame: the three kinds whose
    // pixels come from somewhere other than a shape's own geometry, and
    // so the three a question about pixels is most likely to be wrong
    // about. The picture's bytes are a checkerboard, which has an edge
    // in it wherever a scale or a mirror would smear one.
    let pixels: Vec<u8> = (0..8 * 8)
        .flat_map(|i: u32| {
            let on = (i / 8 + i % 8).is_multiple_of(2);
            let v = if on { 235 } else { 40 };
            [v, (255 - v) / 2, 255 - v, 255]
        })
        .collect();
    let resource_id = doc.add_resource(8, 8, pixels);
    doc.apply(Command::AddNode {
        parent: root,
        index: 2,
        node: Box::new(Node::raster(
            "picture",
            RasterRef {
                resource_id,
                width: 8,
                height: 8,
            },
        )),
    })
    .unwrap();
    let picture = doc.children_of(root).unwrap()[2];
    doc.apply(Command::SetTransform {
        id: picture,
        transform: Transform {
            a: 2.5,
            b: 0.0,
            c: 0.0,
            d: 2.5,
            e: 50.0,
            f: 6.0,
        },
    })
    .unwrap();
    doc.apply(Command::AddNode {
        parent: root,
        index: 3,
        node: Box::new(Node::text(
            "words",
            TextSpec::new(
                "Ag",
                18.0,
                chitrakar_color::AuthoredColor::Srgb {
                    r: 0.05,
                    g: 0.05,
                    b: 0.1,
                    a: 1.0,
                },
            ),
        )),
    })
    .unwrap();
    let words = doc.children_of(root).unwrap()[3];
    doc.apply(Command::SetTransform {
        id: words,
        transform: Transform::translation(8.0, 54.0),
    })
    .unwrap();
    doc.apply(Command::AddNode {
        parent: root,
        index: 4,
        node: Box::new(Node::artboard(
            "frame",
            20.0,
            14.0,
            Some(chitrakar_color::AuthoredColor::Srgb {
                r: 0.95,
                g: 0.95,
                b: 0.9,
                a: 1.0,
            }),
        )),
    })
    .unwrap();
    let frame = doc.children_of(root).unwrap()[4];
    doc.apply(Command::SetTransform {
        id: frame,
        transform: Transform::translation(56.0, 40.0),
    })
    .unwrap();
    doc.apply(Command::SetGuides {
        guides: vec![Guide::Vertical(12.0)],
    })
    .unwrap();

    Fixture {
        doc,
        root,
        group,
        under,
        over,
        painted,
        picture,
        words,
        frame,
        stroke,
    }
}

/// One instance of every command, against the nodes of a [`Fixture`].
/// Each of them changes something: a command that changed nothing would
/// prove nothing about itself.
pub fn every_command(f: &Fixture) -> Vec<Command> {
    let Fixture {
        root,
        group,
        under,
        over,
        painted,
        picture,
        words,
        frame,
        stroke,
        ..
    } = f;
    let (root, group, under, over, painted) = (*root, *group, *under, *over, *painted);
    let (picture, words, frame) = (*picture, *words, *frame);
    let mask = Box::new(Mask {
        kind: MaskKind::Vector {
            shape: VectorShape::Ellipse { rx: 5.0, ry: 4.0 },
            transform: Transform::translation(2.0, 3.0),
        },
        invert: true,
        // Softened, and every audit is the better for it: a softness is
        // a number rather than a shape, which makes it the easiest
        // thing in a mask to drop on the way through a file, an inverse
        // or a space.
        feather: 1.5,
    });
    vec![
        Command::AddNode {
            parent: group,
            index: 1,
            node: rect("added", [0.1, 0.7, 0.4, 1.0]),
        },
        Command::RemoveNode { id: over },
        Command::SetOpacity {
            id: over,
            opacity: 0.25,
        },
        Command::SetVisible {
            id: over,
            visible: false,
        },
        Command::SetLocked {
            id: over,
            locked: true,
        },
        Command::SetClipped {
            id: over,
            clipped: true,
        },
        Command::SetPinning {
            id: over,
            pinned: Pinning {
                x: Pin::End,
                y: Pin::Stretch,
            },
        },
        Command::SetBlendMode {
            id: over,
            blend: BlendMode::Multiply,
        },
        Command::SetTransform {
            id: over,
            transform: Transform {
                a: 1.5,
                b: 0.25,
                c: -0.25,
                d: 1.5,
                e: 7.0,
                f: -3.0,
            },
        },
        Command::SetKind {
            id: over,
            kind: Box::new(NodeKind::Vector {
                shape: VectorShape::Ellipse { rx: 3.0, ry: 6.0 },
                fill: Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.0,
                    g: 0.5,
                    b: 1.0,
                    a: 0.75,
                }),
                stroke: None,
                gradient: None,
            }),
        },
        Command::AddStroke {
            id: painted,
            index: 1,
            stroke: Box::new(stroke.clone()),
            on_mask: false,
        },
        Command::RemoveStroke {
            id: painted,
            index: 0,
            on_mask: false,
        },
        Command::SetStroke {
            id: painted,
            index: 0,
            stroke: Box::new(PaintStroke {
                softness: 0.75,
                erase: true,
                ..stroke.clone()
            }),
            on_mask: false,
        },
        Command::SetName {
            id: over,
            name: "renamed".into(),
        },
        Command::SetMask {
            id: under,
            mask: Some(mask.clone()),
        },
        Command::SetEffects {
            id: under,
            effects: vec![Effect::Outline {
                width: 2.0,
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                opacity: 0.5,
            }],
        },
        Command::SetGuides {
            guides: vec![Guide::Horizontal(4.0), Guide::Vertical(40.0)],
        },
        Command::SetSelection {
            selection: Some(Box::new(Mask {
                kind: MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 30.0,
                        height: 24.0,
                        radius: 0.0,
                    },
                    transform: Transform::translation(9.0, 11.0),
                },
                invert: false,
                feather: 2.5,
            })),
        },
        Command::SetSwatches {
            swatches: vec![Swatch {
                name: "ink".into(),
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.2,
                    g: 0.4,
                    b: 0.6,
                    a: 1.0,
                },
            }],
        },
        Command::ResizeCanvas {
            width: 120,
            height: 90,
            dx: 7.0,
            dy: -4.0,
        },
        Command::MirrorCanvas { across_x: true },
        Command::MirrorCanvas { across_x: false },
        Command::StraightenCanvas {
            degrees: 7.5,
            width: 61,
            height: 44,
        },
        Command::TurnCanvas { quarters: 1 },
        Command::TurnCanvas { quarters: 2 },
        Command::MoveNode {
            id: under,
            parent: root,
            index: 0,
        },
        // A second turn at the variants most likely to be wrong about
        // pixels: a picture turned rather than moved, a block of text
        // rewritten the way the inline editor rewrites it on every
        // keystroke, a shadow whose reach is off to one side, and a
        // frame held to a mask.
        Command::SetTransform {
            id: picture,
            transform: Transform {
                a: 1.8,
                b: 1.1,
                c: -1.1,
                d: 1.8,
                e: 46.0,
                f: 10.0,
            },
        },
        Command::SetKind {
            id: words,
            kind: Box::new(NodeKind::Text(TextSpec::new(
                "Agility",
                18.0,
                chitrakar_color::AuthoredColor::Srgb {
                    r: 0.05,
                    g: 0.05,
                    b: 0.1,
                    a: 1.0,
                },
            ))),
        },
        Command::SetEffects {
            id: picture,
            effects: vec![Effect::DropShadow {
                dx: 6.0,
                dy: -4.0,
                blur: 3.0,
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                opacity: 0.7,
            }],
        },
        Command::SetMask {
            id: frame,
            mask: Some(mask.clone()),
        },
        Command::SetOpacity {
            id: frame,
            opacity: 0.35,
        },
        Command::Batch(vec![
            Command::SetOpacity {
                id: under,
                opacity: 0.1,
            },
            Command::SetName {
                id: painted,
                name: "batched".into(),
            },
            Command::MoveNode {
                id: over,
                parent: root,
                index: 2,
            },
        ]),
    ]
}

/// Whether a command can be undone bit for bit.
///
/// A page turned by anything but a quarter cannot be turned back
/// exactly — a sine and its cosine do not multiply out to one — and an
/// axis-aligned guide cannot record a tilt at all, so it comes back on
/// its own axis where it crosses the middle of the page. That is the
/// one command allowed to be near rather than exact.
pub fn exact(cmd: &Command) -> bool {
    !matches!(cmd, Command::StraightenCanvas { .. })
}

/// A document as a value, with the id counter set aside: an id is never
/// handed out twice, so a document that has had something added and
/// taken away again is not byte for byte where it started even though
/// everything in it is.
///
/// Compared as a value rather than as text — the nodes live in a hash
/// map, whose written order says nothing about what is in it.
pub fn state(doc: &Document) -> serde_json::Value {
    let mut v = serde_json::to_value(doc).expect("a document serializes");
    v["next_id"] = serde_json::Value::Null;
    v
}

/// Two documents the same to within `tol` on every number in them, and
/// exactly the same in everything else.
pub fn close(a: &serde_json::Value, b: &serde_json::Value, tol: f64) -> bool {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= tol,
            _ => x == y,
        },
        (V::Array(x), V::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y, tol))
        }
        (V::Object(x), V::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| close(v, w, tol)))
        }
        _ => a == b,
    }
}

/// Which part of a document came back different, for a message that says
/// something without printing the whole of it.
pub fn differing(a: &serde_json::Value, b: &serde_json::Value) -> String {
    match (a.as_object(), b.as_object()) {
        (Some(x), Some(y)) => x
            .keys()
            .filter(|k| x.get(*k) != y.get(*k))
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        _ => "the whole document".into(),
    }
}

/// Whether a document came back to where it started.
///
/// Exact means exact: every command but one puts every field back bit
/// for bit. The one that cannot — [`exact`] names it — is allowed its
/// numbers to within a float, and its guides to within a page pixel,
/// since an axis-aligned guide cannot record a tilt and comes back on
/// its own axis where it crosses the middle of the page.
///
/// `Err` carries what differed, for a message that says something
/// without printing the whole document.
pub fn came_back(
    got: &serde_json::Value,
    want: &serde_json::Value,
    exact: bool,
) -> Result<(), String> {
    if exact {
        return if got == want {
            Ok(())
        } else {
            Err(differing(got, want))
        };
    }
    let (mut got, mut want) = (got.clone(), want.clone());
    let (has, had) = (got["guides"].take(), want["guides"].take());
    if !close(&got, &want, 1e-5) {
        return Err(differing(&got, &want));
    }
    if !close(&has, &had, 1.0) {
        return Err(format!("a guide left somewhere else: {has} against {had}"));
    }
    Ok(())
}
