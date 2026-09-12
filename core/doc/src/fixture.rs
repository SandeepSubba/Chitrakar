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
    BlendMode, Command, Document, Effect, Gradient, GradientStop, Guide, Marker, Mask, MaskKind,
    Node, NodeId, NodeKind, PaintStroke, Pin, Pinning, RasterRef, Stroke, StyleRun, Swatch,
    TextSpec, Transform, VectorShape,
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
    /// The four that draw by reading rather than by covering.
    pub lifted: NodeId,
    pub softened: NodeId,
    pub borrowed: NodeId,
    pub copy: NodeId,
    /// A copy of the frame, which cuts and grounds what it draws where
    /// the copy is rather than where the frame stands.
    pub frame_copy: NodeId,
    /// A copy of the copy, which has to be followed one step further to
    /// reach what it is a picture of.
    pub echo: NodeId,
    /// A second frame, so that a page's frames are more than one.
    pub other_frame: NodeId,
    /// A layer drawn as itself and, a second time, as the coverage that
    /// holds the layer above it — and that layer, which shows only where
    /// the base's own alpha does.
    pub base: NodeId,
    pub held: NodeId,
    /// A plain group, and a copy of it with a layer of its own in place
    /// of one of the group's — the half of copies where a copy is not
    /// its original.
    pub badge: NodeId,
    pub differs: NodeId,
    pub own_ring: NodeId,
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

/// A smaller filled rectangle, for the corner of the page where a badge
/// and the copy that differs from it stand.
fn chip(name: &str, w: f32, h: f32, radius: f32, fill: [f32; 4]) -> Box<Node> {
    let mut node = Node::vector(
        name,
        VectorShape::Rect {
            width: w,
            height: h,
            radius,
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
    // And a shadow on the *group*, not on a layer in it. A group's
    // opacity belongs to its composite rather than to each child, so the
    // silhouette an effect grows from is the pair of them as one shape —
    // a different question from the one an effect on a layer asks, and
    // the only place in this document where it gets asked. It is also
    // what a copy of this group has to draw, and what a dissolve has to
    // take somewhere.
    doc.apply(Command::SetEffects {
        id: group,
        effects: vec![Effect::DropShadow {
            dx: 3.0,
            dy: 3.0,
            blur: 1.5,
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.1,
                g: 0.0,
                b: 0.2,
                a: 1.0,
            },
            opacity: 0.6,
        }],
    })
    .unwrap();
    // The document has a palette, and the lower shape reaches for it
    // rather than carrying a colour of its own: the same blue, said by
    // name. Which changes nothing about how the page looks — that is the
    // point of putting it here rather than in a test of its own — and
    // means every question the shared fixture asks is asked of a document
    // in which a colour is a reference. A command that changes the
    // palette recolours a layer; a file written from this has a name in
    // it to read back.
    doc.apply(Command::SetSwatches {
        swatches: vec![Swatch {
            name: "ink".into(),
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.2,
                g: 0.45,
                b: 0.8,
                a: 1.0,
            },
        }],
    })
    .unwrap();
    {
        let mut kind = doc.node(under).unwrap().kind.clone();
        if let NodeKind::Vector { fill, .. } = &mut kind {
            *fill = fill.as_ref().map(|c| c.standing_for("ink"));
        }
        doc.apply(Command::SetKind {
            id: under,
            kind: Box::new(kind),
        })
        .unwrap();
    }
    // A band round the one underneath, and a shadow cast inward on the
    // text further down: three of the three effect kinds are then in the
    // document rather than one. They are not variations on each other —
    // an outline is a ring outside the silhouette, an inner shadow is
    // kept inside it and painted over the layer rather than behind it —
    // so each is a different pass and a different reach.
    doc.apply(Command::SetEffects {
        id: under,
        effects: vec![Effect::Outline {
            width: 2.5,
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.95,
                g: 0.9,
                b: 0.3,
                a: 1.0,
            },
            opacity: 0.85,
        }],
    })
    .unwrap();
    // A shadow on the top one, and a generous one: an effect reaches
    // beyond the layer it belongs to, and how far is a figure
    // (`Effect::reach`) that bounds and dirty regions are grown by. Every
    // audit here is the better for a layer that draws outside its own box
    // — a page redrawn a region at a time leaves a trail behind one if
    // that figure is short, and the layer it belongs to is inside a group,
    // so the reach has a space to be read in as well.
    // And blending rather than covering. A layer with a blend mode is the
    // one thing that makes a plain group stop being transparent — a group
    // holding something that reads what is under it is drawn on a surface
    // of its own — so this one flag puts every audit's question to the
    // isolated path as well as to the straight one.
    doc.apply(Command::SetBlendMode {
        id: over,
        blend: BlendMode::Multiply,
    })
    .unwrap();
    doc.apply(Command::SetEffects {
        id: over,
        effects: vec![Effect::DropShadow {
            dx: 5.0,
            dy: 4.0,
            blur: 2.5,
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.0,
                g: 0.05,
                b: 0.15,
                a: 1.0,
            },
            opacity: 0.75,
        }],
    })
    .unwrap();
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
        // Laid down in ink rather than in light. A CMYK colour on an RGB
        // page resolves through the document's press profile — or, without
        // one, through the preview formula — which is a path nothing else
        // in this document takes, and one every audit that draws the page
        // walks. It sits on the paint layer on purpose: that is a layer
        // the GPU audit takes out anyway, since a second renderer
        // declines ink rather than guessing at a profile, so putting the
        // ink here costs that audit nothing it was not already giving up.
        color: chitrakar_color::AuthoredColor::Cmyk {
            c: 0.15,
            m: 0.85,
            y: 0.35,
            k: 0.05,
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
    // A second stroke beside it, in light rather than in ink, so both
    // ways of authoring a colour are on the page at once: a layer with
    // one of each is also a layer whose strokes cannot be handled by one
    // branch that happens to be right.
    doc.apply(Command::AddStroke {
        id: painted,
        index: 1,
        stroke: Box::new(PaintStroke {
            points: vec![[14.0, 50.0], [46.0, 44.0]],
            radii: vec![3.0],
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.2,
                g: 0.85,
                b: 0.45,
                a: 0.9,
            },
            softness: 0.0,
            ..stroke.clone()
        }),
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
        node: Box::new(Node::text("words", {
            let mut spec = TextSpec::new(
                "Agile",
                18.0,
                chitrakar_color::AuthoredColor::Srgb {
                    r: 0.05,
                    g: 0.05,
                    b: 0.1,
                    a: 1.0,
                },
            );
            // Part of it styled differently from the rest. A run is a
            // range of *bytes*, and the block's own settings are what
            // a run does not override — so a block with one is the
            // only thing that asks whether the two are read together,
            // and the only text here that is drawn in more than one
            // pass. Its own colour and weight, so the difference
            // shows on the page rather than only in the file.
            spec.runs = vec![StyleRun {
                start: 1,
                end: 4,
                fill: Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.85,
                    g: 0.2,
                    b: 0.15,
                    a: 1.0,
                }),
                bold: Some(true),
                italic: None,
                underline: Some(true),
                strike: None,
                font: None,
            }];
            spec
        })),
    })
    .unwrap();
    let words = doc.children_of(root).unwrap()[3];
    doc.apply(Command::SetTransform {
        id: words,
        transform: Transform::translation(8.0, 54.0),
    })
    .unwrap();
    // A mask read off an image, which is the one mask kind nothing here
    // held: a shape's coverage is its own geometry and a brushed one is
    // its strokes, but this one has *pixels* — so it is the only mask that
    // makes a resource travel for a reason other than a picture being on
    // the page, and the only one whose coverage a renderer has to sample
    // rather than solve. The picture's own bytes serve: a checkerboard
    // read as luminance is a coverage with holes in it.
    // Its own image rather than the picture's, so the file has to carry a
    // resource nothing on the page draws: a resource travels because
    // something refers to it, and a mask referring to one is the case that
    // is easy to write a saver for and forget.
    let coverage_id = doc.add_resource(
        4,
        4,
        (0..16)
            .flat_map(|i: u32| {
                let v = if (i / 4 + i % 4).is_multiple_of(2) {
                    255u8
                } else {
                    30
                };
                [v, v, v, 255]
            })
            .collect(),
    );
    doc.apply(Command::SetMask {
        id: words,
        mask: Some(Box::new(Mask {
            kind: MaskKind::Raster {
                resource_id: coverage_id,
                width: 4,
                height: 4,
                transform: Transform {
                    a: 8.0,
                    b: 0.0,
                    c: 0.0,
                    d: 2.0,
                    e: 6.0,
                    f: 50.0,
                },
            },
            invert: false,
            feather: 0.0,
        })),
    })
    .unwrap();
    doc.apply(Command::SetEffects {
        id: words,
        effects: vec![Effect::InnerShadow {
            dx: -2.0,
            dy: 3.0,
            blur: 1.5,
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.1,
                g: 0.0,
                b: 0.2,
                a: 1.0,
            },
            opacity: 0.8,
        }],
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
    // A group inside the frame, with a shape inside that: an empty frame
    // is a coloured rectangle and says nothing about being a frame, and
    // nothing else here is nested two deep. So this one layer answers
    // three questions nothing else did — what a frame does to what it
    // holds, what a group inside another parent's space does, and whether
    // anything walking the tree stops one level short.
    doc.apply(Command::AddNode {
        parent: frame,
        index: 0,
        node: Box::new(Node::group("inside")),
    })
    .unwrap();
    let inside = doc.children_of(frame).unwrap()[0];
    doc.apply(Command::AddNode {
        parent: inside,
        index: 0,
        node: rect("held", [0.15, 0.7, 0.55, 1.0]),
    })
    .unwrap();
    let held = doc.children_of(inside).unwrap()[0];
    doc.apply(Command::SetTransform {
        id: held,
        transform: Transform {
            a: 0.5,
            b: 0.0,
            c: 0.0,
            d: 0.5,
            e: 2.0,
            f: 2.0,
        },
    })
    .unwrap();
    // An adjustment *inside* a plain group. Every adjustment and filter
    // in this document has stood at the top of the page, where what it
    // changes is everything under it. Inside a group it changes its
    // neighbours in that group and nothing beneath, which means the
    // group has to be drawn on a surface of its own — and this is the
    // only group here that would be isolated for *what it holds* rather
    // than for an opacity, a mask or an effect it wears. Without one,
    // every renderer could ignore the question and still draw this page.
    doc.apply(Command::AddNode {
        parent: inside,
        index: 1,
        node: Box::new(Node::adjustment(
            "a stop down inside",
            crate::Adjustment::Exposure { stops: -1.0 },
        )),
    })
    .unwrap();

    // Painted with a gradient rather than a flat colour: a gradient is a
    // ramp baked from its stops and
    // read across the shape's own box, which is a different path from a
    // fill, and a CMYK colour on an RGB page goes through the press
    // profile — or, without one, the preview formula. Neither was
    // anywhere in this document, and both are read at every pixel they
    // cover.
    doc.apply(Command::SetKind {
        id: held,
        kind: Box::new(NodeKind::Vector {
            shape: VectorShape::Rect {
                width: 22.0,
                height: 18.0,
                radius: 3.0,
            },
            fill: Some(chitrakar_color::AuthoredColor::Srgb {
                r: 0.15,
                g: 0.7,
                b: 0.55,
                a: 1.0,
            }),
            gradient: Some(Gradient::Linear {
                from: [0.1, 0.0],
                to: [0.9, 1.0],
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.1,
                            g: 0.35,
                            b: 0.8,
                            a: 1.0,
                        },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.95,
                            g: 0.85,
                            b: 0.2,
                            a: 0.8,
                        },
                    },
                ],
            }),
            stroke: Some(Stroke {
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.8,
                    g: 0.15,
                    b: 0.4,
                    a: 1.0,
                },
                width: 1.5,
                widths: Vec::new(),
                // Broken into dashes, and with something on each end: a
                // dash pattern walks the outline by length and a marker is
                // a shape placed on a tangent, neither of which anything
                // else here asks for.
                dash: vec![3.0, 2.0],
                cap: Default::default(),
                join: Default::default(),
                align: None,
                start_marker: Marker::Arrow,
                end_marker: Marker::Dot,
            }),
        }),
    })
    .unwrap();
    // Pinned to the far corner, which is the only thing that makes a
    // frame's size mean anything to what is in it.
    doc.apply(Command::SetPinning {
        id: inside,
        pinned: Pinning {
            x: Pin::End,
            y: Pin::Stretch,
        },
    })
    .unwrap();
    // The four kinds that draw by reading rather than by covering: an
    // adjustment and a filter rewrite what is composited below them, a
    // clone lays down what the page already holds somewhere else, and a
    // copy draws another layer's content in its own place. Every one of
    // them makes a question about pixels — which region a command
    // dirties, what a backdrop is, what a second renderer has to agree
    // about — answer differently from a shape's, which is the whole
    // reason for having them here.
    doc.apply(Command::AddNode {
        parent: root,
        index: 5,
        node: Box::new(Node::adjustment(
            "lift",
            crate::Adjustment::Exposure { stops: 0.4 },
        )),
    })
    .unwrap();
    let lifted = doc.children_of(root).unwrap()[5];
    doc.apply(Command::SetMask {
        id: lifted,
        mask: Box::new(Mask {
            kind: MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 26.0,
                    height: 20.0,
                    radius: 0.0,
                },
                transform: Transform::translation(10.0, 30.0),
            },
            invert: false,
            feather: 0.75,
        })
        .into(),
    })
    .unwrap();
    let softened_at = 6;
    doc.apply(Command::AddNode {
        parent: root,
        index: softened_at,
        node: Box::new(Node::filter(
            "soften",
            crate::Filter::GaussianBlur { sigma: 1.5 },
        )),
    })
    .unwrap();
    let softened = doc.children_of(root).unwrap()[softened_at];
    doc.apply(Command::AddNode {
        parent: root,
        index: 7,
        node: Box::new(Node::clone_layer("borrowed")),
    })
    .unwrap();
    let borrowed = doc.children_of(root).unwrap()[7];
    doc.apply(Command::AddStroke {
        id: borrowed,
        index: 0,
        stroke: Box::new(PaintStroke {
            points: vec![[30.0, 12.0], [44.0, 18.0]],
            radii: vec![3.5],
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            softness: 0.0,
            erase: false,
            source: [-14.0, 20.0],
            heal: false,
            clip: None,
        }),
        on_mask: false,
    })
    .unwrap();
    doc.apply(Command::AddNode {
        parent: root,
        index: 8,
        node: Box::new(Node::instance("a copy", group)),
    })
    .unwrap();
    let copy = doc.children_of(root).unwrap()[8];
    doc.apply(Command::SetTransform {
        id: copy,
        transform: Transform::translation(44.0, 2.0),
    })
    .unwrap();
    // And a copy of the *frame*, which is not the same question as a copy
    // of a group. A frame has a size of its own, cuts what it holds to
    // that box, paints a ground behind them, and exports at its own
    // number of pixels. So a copy of one has to cut and to paint the
    // ground where the copy is put rather than where the original stands,
    // and what is pinned inside it has to be laid out against the
    // original's size, since what travels is the picture and not the
    // placement.
    doc.apply(Command::AddNode {
        parent: root,
        index: 9,
        node: Box::new(Node::instance("a copy of the frame", frame)),
    })
    .unwrap();
    let frame_copy = doc.children_of(root).unwrap()[9];
    doc.apply(Command::SetTransform {
        id: frame_copy,
        transform: Transform::translation(2.0, 44.0),
    })
    .unwrap();

    // A copy of the copy, and a second frame: two shapes a *document*
    // can have rather than two kinds of layer. Neither is a new node
    // kind, and both are things the code has to walk one step further
    // than it ever has here — a copy that follows a copy to reach what
    // it is a picture of, and a page whose frames are more than one.
    doc.apply(Command::AddNode {
        parent: root,
        index: 10,
        node: Box::new(Node::instance("a copy of a copy", copy)),
    })
    .unwrap();
    let echo = doc.children_of(root).unwrap()[10];
    doc.apply(Command::SetTransform {
        id: echo,
        transform: Transform::translation(2.0, 24.0),
    })
    .unwrap();
    doc.apply(Command::AddNode {
        parent: root,
        index: 11,
        node: Box::new(Node::artboard(
            "second frame",
            16.0,
            10.0,
            Some(chitrakar_color::AuthoredColor::Srgb {
                r: 0.9,
                g: 0.85,
                b: 0.95,
                a: 1.0,
            }),
        )),
    })
    .unwrap();
    let other_frame = doc.children_of(root).unwrap()[11];
    doc.apply(Command::SetTransform {
        id: other_frame,
        transform: Transform::translation(30.0, 46.0),
    })
    .unwrap();

    // A layer *held to* the one under it, which is the one way of one
    // layer deciding what another shows that this document has never
    // stood with. It is not a mask and not a frame: the base is drawn as
    // it always was, and what is above it shows only where the base's own
    // alpha does — which means the base has to be drawn twice over (once
    // as itself, once as a coverage for what it holds) and that the
    // layers it holds read pixels of it further out than they cover,
    // since a shadow's blur does. Hung off the base's corner on purpose,
    // so the clip has something to cut rather than being a formality.
    doc.apply(Command::AddNode {
        parent: root,
        index: 12,
        node: rect("a base", [0.35, 0.7, 0.45, 1.0]),
    })
    .unwrap();
    let base = doc.children_of(root).unwrap()[12];
    doc.apply(Command::SetTransform {
        id: base,
        transform: Transform::translation(52.0, 34.0),
    })
    .unwrap();
    doc.apply(Command::AddNode {
        parent: root,
        index: 13,
        node: rect("held to it", [0.95, 0.45, 0.2, 0.8]),
    })
    .unwrap();
    let held = doc.children_of(root).unwrap()[13];
    doc.apply(Command::SetTransform {
        id: held,
        transform: Transform::translation(62.0, 28.0),
    })
    .unwrap();
    doc.apply(Command::SetClipped {
        id: held,
        clipped: true,
    })
    .unwrap();

    // A copy that *differs* from what it follows. Three copies have
    // stood here since copies were written and every one of them draws
    // its original entire; a copy with a layer of its own in place of
    // one of the original's is the other half of the feature, and the
    // half nothing here has ever asked about. It is what a symbol is
    // for — a badge drawn once and used twice, with a different mark on
    // the second — so the thing to stand a copy against is a plain
    // group: `pair` carries a shadow, which makes it a layer drawn as a
    // whole rather than one whose parts can be swapped.
    //
    // The stand-in is a different colour, a different width and a
    // rounded corner, so it is not the original's layer in disguise:
    // what the copy draws, where the copy's box ends, and what goes
    // into a file or an export all have to follow the copy's own layer
    // rather than the one it replaced.
    doc.apply(Command::AddNode {
        parent: root,
        index: 14,
        node: Box::new(Node::group("a badge")),
    })
    .unwrap();
    let badge = doc.children_of(root).unwrap()[14];
    doc.apply(Command::SetTransform {
        id: badge,
        transform: Transform::translation(28.0, 2.0),
    })
    .unwrap();
    for (i, (name, at)) in [("a dot", 0.0), ("a ring", 11.0)].iter().enumerate() {
        doc.apply(Command::AddNode {
            parent: badge,
            index: i,
            node: chip(name, 10.0, 8.0, 0.0, [0.25, 0.55, 0.35, 1.0]),
        })
        .unwrap();
        let id = doc.children_of(badge).unwrap()[i];
        doc.apply(Command::SetTransform {
            id,
            transform: Transform::translation(*at, 0.0),
        })
        .unwrap();
    }
    doc.apply(Command::AddNode {
        parent: root,
        index: 15,
        node: Box::new(Node::instance("a copy that differs", badge)),
    })
    .unwrap();
    let differs = doc.children_of(root).unwrap()[15];
    doc.apply(Command::SetTransform {
        id: differs,
        transform: Transform::translation(26.0, 16.0),
    })
    .unwrap();
    doc.apply(Command::AddNode {
        parent: differs,
        index: 0,
        node: chip("a ring of its own", 7.0, 8.0, 2.0, [0.85, 0.3, 0.55, 1.0]),
    })
    .unwrap();
    let own_ring = doc.children_of(differs).unwrap()[0];
    doc.apply(Command::SetTransform {
        id: own_ring,
        transform: Transform::translation(11.0, 0.0),
    })
    .unwrap();
    doc.apply(Command::SetKind {
        id: differs,
        kind: Box::new(NodeKind::Instance {
            of: badge,
            replaces: vec![doc.children_of(badge).unwrap()[1]],
        }),
    })
    .unwrap();

    doc.apply(Command::SetGuides {
        guides: vec![Guide::Vertical(12.0)],
    })
    .unwrap();

    // A region picked out, and one put away by name. Both have been in
    // the list of commands since they were written, which asks what each
    // *does*; neither has ever been in the document the rest of the
    // audits are asked about, which is a different question — what a
    // page with a region on it survives. They are written in the page's
    // own space, so they are the third thing `map_page` has to carry
    // when the canvas turns, and they go into a file and onto a
    // clipboard like anything else. Deliberately not the ones the
    // command list sets, so that setting those still changes something.
    doc.apply(Command::SetSelection {
        selection: Some(Box::new(Mask {
            kind: MaskKind::Vector {
                shape: VectorShape::Ellipse { rx: 11.0, ry: 8.0 },
                transform: Transform::translation(34.0, 30.0),
            },
            invert: false,
            feather: 1.5,
        })),
    })
    .unwrap();
    doc.apply(Command::SetRegions {
        regions: vec![crate::KeptRegion {
            name: "the wall".into(),
            mask: Mask {
                kind: MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 18.0,
                        height: 14.0,
                        radius: 3.0,
                    },
                    transform: Transform::translation(56.0, 8.0),
                },
                invert: true,
                feather: 0.0,
            },
        }],
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
        lifted,
        softened,
        borrowed,
        copy,
        frame_copy,
        echo,
        other_frame,
        base,
        held,
        badge,
        differs,
        own_ring,
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
    // The one command nothing asks for by name: `RestoreSubtree` is what
    // a delete undoes to, so it is never written at the top of a list of
    // things to do and was the one variant this list did not hold — which
    // means every audit built on it, and there are five, had never seen
    // the command that puts a deleted layer back. It only means anything
    // on a document the subtree has been taken out of, so it arrives with
    // the taking: removed and put straight back. The two halves are each
    // other's inverse, so the batch changes nothing — the point is that
    // both are applied, and that the subtree makes every journey the list
    // is asked about.
    let restore = {
        let mut scratch = f.doc.clone();
        let back = scratch
            .apply(Command::RemoveNode { id: words })
            .expect("removing a layer gives back the command that restores it");
        let Command::RestoreSubtree { subtree, .. } = back else {
            unreachable!("what a removal undoes to is a restored subtree")
        };
        // Put back somewhere else. Removed and restored where it was is a
        // pair that changes nothing, and a command that changes nothing
        // is one whose inverse proves nothing — so the layer comes back
        // inside the group rather than at the top of the page, which is a
        // move a document can be held to.
        Command::RestoreSubtree {
            parent: group,
            index: 0,
            subtree,
        }
    };
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
        // A different one from the document's, so the command changes
        // something — and one that reads its backdrop the other way round,
        // since multiplying and screening are each other's opposite.
        Command::SetBlendMode {
            id: over,
            blend: BlendMode::Screen,
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
        Command::SetRegions {
            regions: vec![crate::KeptRegion {
                name: "the sky".into(),
                mask: Mask {
                    kind: MaskKind::Vector {
                        shape: VectorShape::Ellipse { rx: 9.0, ry: 7.0 },
                        transform: Transform::translation(4.0, 5.0),
                    },
                    invert: false,
                    feather: 1.25,
                },
            }],
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
        Command::Batch(vec![Command::RemoveNode { id: words }, restore]),
    ]
}

/// A command's variant, by name.
///
/// The match has no arm for "anything else", which is the point: a
/// variant added to [`Command`] stops this file compiling until it is
/// named here, and the line it stops on is next to the list that says
/// what to do about it. Add the name to [`EVERY_VARIANT`] and an
/// instance of the command to [`every_command`], and
/// `the_list_holds_every_command_there_is` goes quiet again.
pub fn variant_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::AddNode { .. } => "AddNode",
        Command::RemoveNode { .. } => "RemoveNode",
        Command::RestoreSubtree { .. } => "RestoreSubtree",
        Command::SetOpacity { .. } => "SetOpacity",
        Command::SetVisible { .. } => "SetVisible",
        Command::SetLocked { .. } => "SetLocked",
        Command::SetClipped { .. } => "SetClipped",
        Command::SetPinning { .. } => "SetPinning",
        Command::SetBlendMode { .. } => "SetBlendMode",
        Command::SetName { .. } => "SetName",
        Command::SetTransform { .. } => "SetTransform",
        Command::SetKind { .. } => "SetKind",
        Command::MoveNode { .. } => "MoveNode",
        Command::AddStroke { .. } => "AddStroke",
        Command::SetStroke { .. } => "SetStroke",
        Command::RemoveStroke { .. } => "RemoveStroke",
        Command::SetMask { .. } => "SetMask",
        Command::SetEffects { .. } => "SetEffects",
        Command::SetGuides { .. } => "SetGuides",
        Command::SetSwatches { .. } => "SetSwatches",
        Command::SetSelection { .. } => "SetSelection",
        Command::SetRegions { .. } => "SetRegions",
        Command::ResizeCanvas { .. } => "ResizeCanvas",
        Command::TurnCanvas { .. } => "TurnCanvas",
        Command::MirrorCanvas { .. } => "MirrorCanvas",
        Command::StraightenCanvas { .. } => "StraightenCanvas",
        Command::Batch(_) => "Batch",
    }
}

/// Every variant [`every_command`] is meant to hold one of.
///
/// The file's promise at the top — that adding a `Command` without
/// adding it here is what makes the audits fail — was a promise nothing
/// kept: `RestoreSubtree` was missing for as long as it had existed, so
/// the five audits built on the list had never once seen the command
/// that puts a deleted layer back. This is the list that keeps it.
pub const EVERY_VARIANT: &[&str] = &[
    "AddNode",
    "RemoveNode",
    "RestoreSubtree",
    "SetOpacity",
    "SetVisible",
    "SetLocked",
    "SetClipped",
    "SetPinning",
    "SetBlendMode",
    "SetName",
    "SetTransform",
    "SetKind",
    "MoveNode",
    "AddStroke",
    "SetStroke",
    "RemoveStroke",
    "SetMask",
    "SetEffects",
    "SetGuides",
    "SetSwatches",
    "SetSelection",
    "SetRegions",
    "ResizeCanvas",
    "TurnCanvas",
    "MirrorCanvas",
    "StraightenCanvas",
    "Batch",
];

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
