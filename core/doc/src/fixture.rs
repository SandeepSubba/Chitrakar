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
    PaintStroke, Pin, Pinning, Swatch, Transform, VectorShape,
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
    };
    doc.apply(Command::AddStroke {
        id: painted,
        index: 0,
        stroke: Box::new(stroke.clone()),
        on_mask: false,
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
        stroke,
        ..
    } = f;
    let (root, group, under, over, painted) = (*root, *group, *under, *over, *painted);
    let mask = Box::new(Mask {
        kind: MaskKind::Vector {
            shape: VectorShape::Ellipse { rx: 5.0, ry: 4.0 },
            transform: Transform::translation(2.0, 3.0),
        },
        invert: true,
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
