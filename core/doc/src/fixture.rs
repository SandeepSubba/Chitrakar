//! A document with something of everything in it, and one instance of
//! every [`Command`] that can be asked of it.
//!
//! It lives here rather than inside a test because more than one crate
//! has a question to ask of every command at once — that each undoes to
//! exactly where it started, that each dirties every pixel it changes —
//! and a list written out twice is a list a new command gets added to
//! once. Adding a `Command` variant without adding it here is meant to
//! be the thing that makes those tests fail.

use crate::{Adjustment, Filter};
use crate::{
    BlendMode, Command, Document, Effect, Gradient, GradientStop, Guide, KeptStyle, Look, Marker,
    Mask, MaskKind, Node, NodeId, NodeKind, PaintStroke, Pin, Pinning, RasterRef, Stroke,
    StrokeAlign, StyleRun, Swatch, TextSpec, Transform, VectorShape,
};
use chitrakar_color::{AuthoredColor, ColorMode};

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
    /// A copy wearing a mask of its own — the one thing no copy here had.
    pub worn: NodeId,
    /// A layer whose blend is one of the four that work on the whole
    /// colour rather than a channel at a time.
    pub lent: NodeId,
    /// A filter that is a function of where a pixel is and of nothing
    /// else, so the same page drawn twice any two ways has to grain the
    /// same.
    pub grain: NodeId,
    /// A closed path with bezier handles of its own, filled, with a
    /// second ring inside it that is a hole — the two things a path can
    /// hold that no path in this document held: every path here was
    /// straight-sided or smoothed by rule, and none had a subpath. A
    /// hole is what a boolean leaves behind, and handles are what the
    /// pen draws and the node tool takes hold of.
    pub pierced: NodeId,
    /// A curves adjustment held to the pierced path: a master curve and
    /// a curve per channel, which is what a colour grade is made of and
    /// what no adjustment here had — an exposure is one number, a ramp
    /// is a list of colours, and this is four lists of points read
    /// through a table.
    pub graded: NodeId,
    /// Text set along a guide: an open arc, smoothed.
    pub arc: NodeId,
    /// A levels adjustment held to the picture: input black and white
    /// points, a gamma, and an output pair.
    pub leveled: NodeId,
    /// An ellipse in soft light over the group.
    pub soft: NodeId,
    /// A stroked rectangle standing at an angle.
    pub tilted: NodeId,
    /// A second picture on the first one's bytes, standing turned.
    pub again: NodeId,
    /// A block tracked and leaded so that the tracking decides its break.
    pub spaced: NodeId,
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
    // A stroke lying *outside* the outline, which is the one thing about
    // a stroke this document had never said. Every band in it was inside
    // one — not by choice but by default, since `align: None` means
    // whatever a shape has always been stroked as and for a rect that is
    // inside. So `StrokeAlign` had three variants and the shared document
    // held none of them, which is a different sort of gap from a field
    // nobody set: the code reads the enum everywhere and no audit had
    // ever handed it anything but the fallback.
    //
    // Outside is the variant that changes more than colour. The band
    // reaches a whole width past the outline where an inside one reaches
    // nothing, so it grows the layer's box — `stroke_pad` is what says by
    // how much, generously — and the dirty region, the hit test and any
    // effect's silhouette all move with it; and it is the band SVG and PDF have
    // no way to ask for, both of them drawing it as a centred stroke on
    // a shape pushed half a width outwards. A sharp corner is where that
    // substitution is worth testing, so this rect keeps its square ones.
    {
        let mut kind = doc.node(under).unwrap().kind.clone();
        if let NodeKind::Vector { stroke, .. } = &mut kind {
            *stroke = Some(Stroke {
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.1,
                    g: 0.15,
                    b: 0.3,
                    a: 1.0,
                },
                width: 2.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                align: Some(StrokeAlign::Outside),
                start_marker: Marker::None,
                end_marker: Marker::None,
            });
        }
        doc.apply(Command::SetKind {
            id: under,
            kind: Box::new(kind),
        })
        .unwrap();
    }
    // A *radial* gradient, which this document had only ever had a linear
    // one of — and so had the pages drawn from a seed. It is its own
    // geometry in the renderer, its own element in SVG, and its own
    // arithmetic on the GPU: a distance from a centre in units of a
    // radius rather than a projection onto a line. Everything that reads
    // a gradient had been reading the same one.
    {
        let mut kind = doc.node(over).unwrap().kind.clone();
        if let NodeKind::Vector { gradient, .. } = &mut kind {
            *gradient = Some(Gradient::Radial {
                center: [0.35, 0.4],
                radius: 0.7,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.95,
                            g: 0.75,
                            b: 0.2,
                            a: 1.0,
                        },
                    },
                    GradientStop {
                        offset: 0.6,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.6,
                            g: 0.2,
                            b: 0.5,
                            a: 0.85,
                        },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.05,
                            g: 0.1,
                            b: 0.35,
                            a: 1.0,
                        },
                    },
                ],
            });
        }
        doc.apply(Command::SetKind {
            id: over,
            kind: Box::new(kind),
        })
        .unwrap();
    }
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
        swatches: vec![
            Swatch {
                name: "ink".into(),
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.2,
                    g: 0.45,
                    b: 0.8,
                    a: 1.0,
                },
            },
            // What the second frame's ground stands for.
            Swatch {
                name: "lilac".into(),
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.9,
                    g: 0.85,
                    b: 0.95,
                    a: 1.0,
                },
            },
        ],
    })
    .unwrap();
    // And a look kept by name, so a file written from this has a style
    // in it to read back, and every audit is asked of a document that
    // holds one.
    doc.apply(Command::SetStyles {
        styles: vec![KeptStyle {
            name: "warm".into(),
            look: Look {
                fill: Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.9,
                    g: 0.5,
                    b: 0.2,
                    a: 1.0,
                }),
                stroke: None,
                gradient: None,
                effects: Vec::new(),
                opacity: 0.8,
                blend: BlendMode::Multiply,
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
    // And it does not sit at the origin. A paint layer's own strokes are
    // written in its own space and its painted mask's are written in its
    // parent's, which is the one thing about a brush that has two spaces
    // in it — and with the layer at the origin those two spaces are the
    // same transform, so nothing here could tell them apart. Compute a
    // mask stroke's dirty region in the layer's space instead of the
    // parent's and, until this moved, every audit passed.
    doc.apply(Command::SetTransform {
        id: painted,
        transform: Transform::translation(-3.0, 5.0),
    })
    .unwrap();
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
            // And it was painted inside a region, which it carries: a
            // stroke laid down while something was picked out stays
            // confined to it after the region is let go of, and under
            // the clip the stroke is whole. Nothing in this document
            // had ever carried one, so every audit that asks about a
            // stroke had been asking about the easy half of what a
            // stroke is. The region cuts this one partway along, so a
            // clip read in the wrong space draws a different picture
            // rather than the same one.
            clip: Some(Box::new(Mask {
                kind: MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 18.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    transform: Transform::translation(10.0, 38.0),
                },
                invert: false,
                feather: 0.0,
            })),
            ..stroke.clone()
        }),
        on_mask: false,
    })
    .unwrap();
    // And a mask brushed on by hand, which is a kind of mask the shared
    // document had never held: the two others — a shape and a picture —
    // were both here, and this one is the one with strokes of its own,
    // living in the same slot a layer's strokes live in and read by the
    // same code. It goes on the paint layer, so one layer has strokes
    // on itself and strokes on its mask at once: anything that handles
    // a stroke without saying which of the two it means is then wrong
    // about this document rather than right by luck.
    doc.apply(Command::SetMask {
        id: painted,
        mask: Some(Box::new(Mask {
            kind: MaskKind::Painted {
                strokes: vec![PaintStroke {
                    points: vec![[8.0, 34.0], [56.0, 52.0]],
                    radii: vec![9.0],
                    color: chitrakar_color::AuthoredColor::Srgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    softness: 0.4,
                    erase: true,
                    ..stroke.clone()
                }],
            },
            invert: false,
            feather: 0.0,
        })),
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
                resource_id: resource_id.clone(),
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
                // Two words and a width, so the block *wraps*. Every text
                // block this document has held was one short word on one
                // line, which left the whole of the multi-line path
                // unasked here: a line count, a line height, a second
                // baseline, and — in the SVG exporter — a tspan of its
                // own per line anchored at the alignment's x. Centred for
                // the same reason: alignment only means something once a
                // line is shorter than the block it sits in, so on one
                // full line it cannot be told from the default.
                "Agile mark",
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
            spec.width = 30.0;
            spec.align = crate::TextAlign::Center;
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
    // High enough that the line is *on* the page. An 18pt block written
    // at a baseline of 54 on a page 60 tall hangs most of itself off the
    // bottom: with no mask at all it put sixty-nine pixels down, and with
    // the mask it had, twenty-five. Every audit that has ever compared
    // text over this document was comparing those twenty-five, and none
    // of them has eight neighbours of its own colour, so the interior
    // reading could not see them either — the GPU backend can be made to
    // draw no text whatever and the cross-renderer audit still passes.
    // Two hundred and seventy-one now.
    doc.apply(Command::SetTransform {
        id: words,
        transform: Transform::translation(8.0, 34.0),
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
                // Over the glyphs rather than under them. Written at
                // (6, 50) it covered y 50 to 58, while an 18pt line on a
                // baseline at 54 has its glyphs at roughly y 40 to 54 —
                // so it cut all but the bottom few rows off, and the
                // only text in this document changed twenty-five pixels
                // of four thousand eight hundred. Every audit that has
                // ever compared text over this fixture was comparing
                // twenty-five antialiased pixels, none of them with
                // eight neighbours of their own colour: the GPU backend
                // can be made to draw no text at all and both the
                // whole-page mean and the interior reading still pass.
                // It still cuts — the mask's own greys are what make it
                // worth having — it simply cuts the text instead of the
                // air below it.
                transform: Transform {
                    a: 12.0,
                    b: 0.0,
                    c: 0.0,
                    d: 4.0,
                    e: 6.0,
                    f: 38.0,
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
    // An effect on the *frame*. Every effect in this document has hung on
    // a layer or on a group, and a frame is neither: it is the one kind
    // whose silhouette is not what it drew but the rectangle it cuts what
    // it drew to. Nothing here had ever asked what an effect makes of
    // that — and until an hour ago nothing could, since the GPU backend
    // handed such a page back and would have declined the whole fixture
    // with one on it.
    //
    // The shadow falls up and to the left, away from the page's edge:
    // the frame sits at (56, 40) on an 80 by 60 page, so a shadow cast
    // down and to the right would be mostly off it and would be asking
    // about clipping rather than about frames.
    doc.apply(Command::SetEffects {
        id: frame,
        effects: vec![Effect::DropShadow {
            dx: -3.0,
            dy: -2.0,
            blur: 1.5,
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.05,
                g: 0.05,
                b: 0.15,
                a: 0.9,
            },
            opacity: 0.7,
        }],
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
    // And a stroke that heals rather than clones. It lifts the texture
    // from under its source and takes its colour from where it lands,
    // and the colour it takes is an average over the whole stroke —
    // so every pixel of it depends on every pixel under the rest of
    // it, which is a reach no other stroke has. Long, so that reach is
    // the stroke's length rather than the offset it reads from.
    doc.apply(Command::AddStroke {
        id: borrowed,
        index: 1,
        stroke: Box::new(PaintStroke {
            points: vec![[6.0, 50.0], [74.0, 54.0]],
            radii: vec![3.0],
            color: chitrakar_color::AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            softness: 0.3,
            erase: false,
            source: [2.0, -24.0],
            heal: true,
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
            // A ground that stands for a palette entry: every named
            // colour in this document had been on a shape, in text or
            // in a ramp, and a frame's ground is the one other place a
            // colour lives — so the palette walk had never reached one.
            Some(
                chitrakar_color::AuthoredColor::Srgb {
                    r: 0.9,
                    g: 0.85,
                    b: 0.95,
                    a: 1.0,
                }
                .standing_for("lilac"),
            ),
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

    // An adjustment stated by a *ramp of colours*, which is two things
    // this document has never held. Every adjustment in it until now was
    // an exposure — one of thirteen kinds, and the one whose parameters
    // are a single number — so an adjustment carrying a *list* had never
    // gone through the file format, the clipboard or the undo runs, and
    // an adjustment carrying a *colour* had never been walked by the
    // palette at all.
    //
    // One of its stops stands for the palette entry, which is what makes
    // the second half bite: a named colour in a gradient map did not
    // settle when the palette moved, because the colour walk waved every
    // adjustment past with `Adjustment(_)`.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::adjustment(
            "a ramp",
            crate::Adjustment::GradientMap {
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.08,
                            g: 0.05,
                            b: 0.2,
                            a: 1.0,
                        },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: chitrakar_color::AuthoredColor::Srgb {
                            r: 0.2,
                            g: 0.45,
                            b: 0.8,
                            a: 1.0,
                        }
                        .standing_for("ink"),
                    },
                ],
            },
        )),
    })
    .unwrap();
    let ramp = *doc.children_of(root).unwrap().last().unwrap();
    // Held to the layer under it, so it recolours that layer rather than
    // the whole page: a gradient map over everything is a page nobody
    // would keep, and holding it also puts a *list-stated* adjustment
    // into a clip run, which nothing here had either.
    doc.apply(Command::SetClipped {
        id: ramp,
        clipped: true,
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
    // A *path*, which this document has never held: every shape in it
    // until now was a rectangle or an ellipse, and a path is what the pen
    // draws and what a brush stroke becomes. It is its own rasterizer —
    // spans from an edge table rather than a formula — its own stroke,
    // which is a skeleton walked with caps and joins rather than a band
    // lying inside a closed outline, and its own path data in both
    // exporters. None of that had ever been asked of by the audits over
    // this document.
    //
    // And its stroke *swells and tapers*, which is the other half of the
    // same gap: `Stroke::widths` is a width per anchor, what a pressure
    // pen leaves behind, and it only means anything on a path. Not
    // dashed, since a dashed stroke is drawn at one width by rule — the
    // two are different ideas about the same line and the dashes win —
    // so a dash here would have hidden the widths entirely.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: {
            let mut node = Node::vector(
                "a drawn line",
                VectorShape::Path {
                    points: vec![[4.0, 6.0], [16.0, 2.0], [26.0, 12.0], [38.0, 4.0]],
                    closed: false,
                    smooth: true,
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                },
            );
            if let NodeKind::Vector { stroke, .. } = &mut node.kind {
                *stroke = Some(Stroke {
                    color: chitrakar_color::AuthoredColor::Srgb {
                        r: 0.1,
                        g: 0.35,
                        b: 0.75,
                        a: 1.0,
                    },
                    width: 4.0,
                    widths: vec![0.25, 1.0, 0.4, 0.1],
                    dash: Vec::new(),
                    cap: crate::StrokeCap::Round,
                    join: crate::StrokeJoin::Round,
                    align: None,
                    start_marker: Marker::None,
                    end_marker: Marker::None,
                });
            }
            Box::new(node)
        },
    })
    .unwrap();
    let drawn = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: drawn,
        transform: Transform::translation(20.0, 40.0),
    })
    .unwrap();

    // A path with curves of its own and a hole through it. Every path
    // in this document was straight-sided, or smoothed by rule with no
    // handles written down, and none had a second ring — so the bezier
    // flattening, the even-odd fill, both exporters' path data for
    // curves and holes, and the anchor arithmetic on handles had never
    // been asked of by anything here. A lens: two anchors on a line,
    // with handles long enough that the curves between them bulge well
    // past the anchors — so anything that takes a path's extent from
    // its anchors, rather than from where its curves go, is wrong about
    // this one by half its height — and a square hole through it.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: {
            let mut node = Node::vector(
                "pierced",
                VectorShape::Path {
                    points: vec![[0.0, 8.0], [20.0, 8.0]],
                    closed: true,
                    smooth: false,
                    handles: vec![[0.0, 10.0, 0.0, -10.0], [0.0, -10.0, 0.0, 10.0]],
                    subpaths: vec![vec![[7.0, 5.0], [13.0, 5.0], [13.0, 11.0], [7.0, 11.0]]],
                },
            );
            if let NodeKind::Vector { fill, .. } = &mut node.kind {
                *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.85,
                    g: 0.55,
                    b: 0.15,
                    a: 1.0,
                });
            }
            Box::new(node)
        },
    })
    .unwrap();
    let pierced = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: pierced,
        transform: Transform::translation(54.0, 40.0),
    })
    .unwrap();

    // A curves adjustment: a master curve and a curve per channel, read
    // through tables the renderer builds once a pass. Every adjustment
    // here was a single number or a list of colours; four lists of
    // points, one of them empty, had never gone through the file, the
    // clipboard, the undo runs or the two renderers' agreement. Held to
    // the pierced path, as the ramp is held to its layer, so a grade
    // over the whole page does not move every pixel the other audits
    // read. The master lifts the middle, red lifts its shadows, blue
    // lifts its shadows further still — the path is orange, with next to
    // no blue, so a curve that pulled blue's *highlights* moved it by
    // less than the audits' tolerance and a GPU that forgot the blue
    // curve went unnoticed; lifted from the bottom, blue has room to
    // move — and green is left empty on purpose, the identity said by an
    // empty list being a case of its own.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::adjustment(
            "a grade",
            crate::Adjustment::Curves {
                points: vec![[0.0, 0.0], [0.5, 0.62], [1.0, 1.0]],
                red: vec![[0.0, 0.12], [1.0, 1.0]],
                green: Vec::new(),
                blue: vec![[0.0, 0.5], [1.0, 1.0]],
            },
        )),
    })
    .unwrap();
    let graded = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetClipped {
        id: graded,
        clipped: true,
    })
    .unwrap();

    // Text set along a guide rather than in lines. Every text block this
    // document had held sat in lines from its origin, so the whole of
    // the other way of setting type — a guide flattened to a polyline
    // and its arc length tabulated, each glyph turned to the direction
    // there, an offset along it, and the box the glyphs land in taken
    // from where they land rather than from the origin — had never gone
    // through the file, the clipboard, the undo runs, the dirty region
    // or the exporters here, where the SVG is a textPath over a path in
    // the defs and the PDF sets each glyph with a matrix of its own. An
    // open arc, smoothed, with the text short enough to fit along it:
    // what runs off an open guide's end is not drawn, and a block that
    // shows nothing would be asking nothing.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::text("arc", {
            let mut spec = TextSpec::new(
                "curve",
                7.0,
                chitrakar_color::AuthoredColor::Srgb {
                    r: 0.05,
                    g: 0.35,
                    b: 0.3,
                    a: 1.0,
                },
            );
            spec.along = Some(VectorShape::Path {
                points: vec![[0.0, 8.0], [12.0, 0.0], [24.0, 8.0]],
                closed: false,
                smooth: true,
                handles: Vec::new(),
                subpaths: Vec::new(),
            });
            spec.along_offset = 2.0;
            spec
        })),
    })
    .unwrap();
    let arc = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: arc,
        transform: Transform::translation(3.0, 44.0),
    })
    .unwrap();

    // A levels adjustment held to the picture: an input black and white
    // point, a gamma between them, and an output pair — five numbers
    // where every adjustment here had been one, a ramp, or the curves'
    // four lists. Above the raster, which is the one layer with tones
    // enough for a black point and a gamma to be told apart on; held
    // to it so a grade over the whole page does not move every pixel
    // the other audits read. Right above it in the stack, since a run
    // of held layers is held to the first unheld one beneath the run.
    doc.apply(Command::AddNode {
        parent: root,
        index: 3,
        node: Box::new(Node::adjustment(
            "levelled",
            crate::Adjustment::Levels {
                in_black: 0.1,
                in_white: 0.85,
                gamma: 1.4,
                out_black: 0.2,
                out_white: 0.9,
            },
        )),
    })
    .unwrap();
    let leveled = doc.children_of(root).unwrap()[3];
    doc.apply(Command::SetClipped {
        id: leveled,
        clipped: true,
    })
    .unwrap();

    // A layer in soft light. Ten of the sixteen blend modes stood in
    // this document and on the pages nobody wrote, and the six that did
    // not were all separable — one function of a channel and its
    // opposite number, run three times — so a soft light's curve, a
    // dodge's division and a burn's had been drawn by nothing here. A
    // warm ellipse over the group, where there is something under it
    // for the blend to read; its edit swaps it to a colour dodge, the
    // second of the missing six.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: {
            let mut node = Node::vector("soft", VectorShape::Ellipse { rx: 7.0, ry: 5.0 });
            if let NodeKind::Vector { fill, .. } = &mut node.kind {
                *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.85,
                    g: 0.6,
                    b: 0.3,
                    a: 1.0,
                });
            }
            node.blend = BlendMode::SoftLight;
            Box::new(node)
        },
    })
    .unwrap();
    let soft = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: soft,
        transform: Transform::translation(16.0, 20.0),
    })
    .unwrap();

    // A layer that *stands* at an angle. Every transform in this
    // document was a translation or an axis-aligned scale — two
    // commands turn things, so the undo and repaint runs had seen a
    // rotation, but no layer had ever been *saved*, copied, exported or
    // drawn by the second renderer while turned. Stroked as well as
    // filled, because a stroke's reach is a length in the layer's own
    // space and the box it needs on the page is that length carried
    // through the turn: the one piece of arithmetic that a rotation
    // makes different rather than merely harder.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: {
            let mut node = Node::vector(
                "tilted",
                VectorShape::Rect {
                    width: 14.0,
                    height: 8.0,
                    radius: 1.0,
                },
            );
            if let NodeKind::Vector { fill, stroke, .. } = &mut node.kind {
                *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.35,
                    g: 0.7,
                    b: 0.55,
                    a: 1.0,
                });
                *stroke = Some(Stroke {
                    color: chitrakar_color::AuthoredColor::Srgb {
                        r: 0.1,
                        g: 0.25,
                        b: 0.2,
                        a: 1.0,
                    },
                    width: 2.0,
                    widths: Vec::new(),
                    dash: Vec::new(),
                    cap: Default::default(),
                    join: Default::default(),
                    align: None,
                    start_marker: Marker::None,
                    end_marker: Marker::None,
                });
            }
            Box::new(node)
        },
    })
    .unwrap();
    let tilted = *doc.children_of(root).unwrap().last().unwrap();
    // Thirty degrees and a fifth again as large: an angle that is not a
    // quarter turn, so the turned box is wider *and* taller than the
    // shape and no axis is left alone.
    doc.apply(Command::SetTransform {
        id: tilted,
        transform: Transform {
            a: 1.039_230_5,
            b: 0.6,
            c: -0.6,
            d: 1.039_230_5,
            e: 30.0,
            f: 28.0,
        },
    })
    .unwrap();

    // A second picture, and two things at once that this document has
    // never held. It refers to **the same resource** as the first: two
    // layers on one set of bytes is how a file with a repeated
    // photograph is written, and until now every resource here was
    // referred to exactly once — so a saver that wrote a resource per
    // reference, or a loader that gave the second layer its own copy,
    // would have passed everything. And it *stands turned*, which no
    // raster had: a turned picture is sampled through the inverse of a
    // rotation rather than along its own rows, and both exporters have
    // to write the matrix rather than a place and a size.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::raster(
            "again",
            RasterRef {
                resource_id: resource_id.clone(),
                width: 8,
                height: 8,
            },
        )),
    })
    .unwrap();
    let again = *doc.children_of(root).unwrap().last().unwrap();
    // Forty degrees and half again as large, about a point inside the
    // page so the turned square clears every edge.
    doc.apply(Command::SetTransform {
        id: again,
        transform: Transform {
            a: 1.147_2,
            b: 0.966_3,
            c: -0.966_3,
            d: 1.147_2,
            e: 30.0,
            f: 6.0,
        },
    })
    .unwrap();

    // Type set loosely — tracking, and a line height of its own — which
    // no block here had: every one was spaced the way its face spaces
    // itself. Placed so the tracking *decides the break*. Set tight,
    // these two words at twelve points come to fifty-five and a half
    // pixels and share a sixty-pixel line; tracked by eight hundredths
    // of an em they come to sixty-four and do not. So a measure that
    // broke lines without counting the tracking would put both words on
    // one line and then draw a line four pixels wider than the block —
    // and the line height moves the second baseline only a block that
    // really does break can show. The wrapped block above could not ask
    // this: at eighteen points its two words are wider than the page
    // before tracking comes into it.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::text("spaced", {
            let mut spec = TextSpec::new(
                "Agile mark",
                12.0,
                chitrakar_color::AuthoredColor::Srgb {
                    r: 0.15,
                    g: 0.1,
                    b: 0.3,
                    a: 1.0,
                },
            );
            spec.width = 60.0;
            spec.letter_spacing = 0.08;
            spec.line_height = 1.35;
            spec
        })),
    })
    .unwrap();
    let spaced = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: spaced,
        transform: Transform::translation(4.0, 20.0),
    })
    .unwrap();

    // A copy that *wears something*. Four copies have stood in this
    // document since copies were written and every one of them is bare:
    // no mask, no fade, no blend, nothing of its own at all. So the whole
    // question of what a copy's own mask means had never been asked here
    // — and it is the question that has been producing defects. A mask is
    // what sends a copy to a surface of its own, and a copy is not always
    // the same picture there: it lost the blend of what it copies, and
    // before that a copy of an adjustment vanished outright. Both were
    // found by pages nobody wrote, which is the argument for putting the
    // shape somewhere a person can see it.
    //
    // Of the plain group rather than of `pair`: `pair` wears a shadow, so
    // a mask on a copy of it would be answering two questions at once —
    // what a mask does to a copy, and what it does to the effects the
    // copy draws. One at a time.
    //
    // The mask cuts it rather than covering it, and deliberately across
    // the middle: a mask that hides nothing proves nothing here, and one
    // that hides everything proves less.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::instance("a copy wearing a mask", badge)),
    })
    .unwrap();
    let worn = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: worn,
        transform: Transform::translation(52.0, 40.0),
    })
    .unwrap();
    doc.apply(Command::SetMask {
        id: worn,
        mask: Some(Box::new(Mask {
            kind: MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 13.0,
                    height: 12.0,
                    radius: 0.0,
                },
                transform: Transform::translation(50.0, 36.0),
            },
            invert: false,
            feather: 0.0,
        })),
    })
    .unwrap();

    // A layer that blends *non-separably*. Sixteen blend modes, and this
    // document held two of them: Normal, and the Multiply on the layer
    // inside the pair. That looks like a gap of degree — twelve more of
    // the same arithmetic — and four of the twelve are not the same
    // arithmetic at all. The separable ones are a function of one
    // channel and its opposite number, applied three times; Hue,
    // Saturation, Color and Luminosity are a function of the whole
    // colour, taking one of brightness, hue and saturation from the
    // layer and the rest from what is under it. Different code in both
    // renderers, different names in both exporters, and neither the
    // shared document nor the pages nobody writes had ever carried one
    // (`BLENDS`, which those pages draw from, was six separable ones).
    //
    // Hue of the four, because it is the one that uses all of the
    // machinery: `set_lum(set_sat(source, sat(backdrop)), lum(backdrop))`
    // reaches both the brightness transfer and the saturation one, where
    // Color and Luminosity reach only the first.
    //
    // Over the pair, which is the most coloured part of this page: a
    // non-separable blend against a grey backdrop is a blend that does
    // nothing, since there is no hue to take and none to lend.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: rect("a hue lent to what is under it", [0.85, 0.1, 0.6, 1.0]),
    })
    .unwrap();
    let lent = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetTransform {
        id: lent,
        transform: Transform::translation(10.0, 12.0),
    })
    .unwrap();
    doc.apply(Command::SetBlendMode {
        id: lent,
        blend: BlendMode::Hue,
    })
    .unwrap();

    // Grain, which is the one thing on a page that is a function of
    // *where a pixel is* and of nothing else. Six kinds of filter and
    // this document held one of them, a blur — and a blur is a
    // neighbourhood, which every audit here already asks about through
    // the shadows. What none of them had ever asked is whether an answer
    // that depends only on position is the *same answer every time*: a
    // page redrawn a region at a time has to grain exactly as the page
    // drawn whole, since a cell is a `floor` and a cell boundary read
    // from the region's corner instead of the page's would put a speck
    // in the wrong square. The undo runs and the file round trip ask the
    // same question a different way.
    //
    // Masked rather than page-wide, and over the pair, which is the most
    // drawn-on part of this page: grain moves a pixel by a share of its
    // own alpha, so grain over nothing is nothing.
    doc.apply(Command::AddNode {
        parent: root,
        index: doc.children_of(root).unwrap().len(),
        node: Box::new(Node::filter(
            "grain",
            crate::Filter::Noise {
                amount: 0.28,
                grain: 1.75,
                mono: false,
                seed: 60_913,
            },
        )),
    })
    .unwrap();
    let grain = *doc.children_of(root).unwrap().last().unwrap();
    doc.apply(Command::SetMask {
        id: grain,
        mask: Some(Box::new(Mask {
            kind: MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 26.0,
                    height: 20.0,
                    radius: 0.0,
                },
                transform: Transform::translation(8.0, 10.0),
            },
            invert: false,
            feather: 0.0,
        })),
    })
    .unwrap();

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
        worn,
        lent,
        grain,
        pierced,
        graded,
        arc,
        leveled,
        soft,
        tilted,
        again,
        spaced,
        stroke,
    }
}

/// A page nobody wrote: layers, placements and settings drawn from a
/// seed rather than chosen.
///
/// The shared fixture answers "does this hold for a document with one of
/// everything in it", which is the question worth asking first and the
/// one a person can keep in their head. It cannot answer "does this hold
/// for the *combinations* nobody thought of" — a blend under a mask
/// inside a faded group, a copy of a layer that is held to the one under
/// it — because every one of those had to be thought of to be put in.
/// These pages are for that: cheap, unreadable, and there are as many of
/// them as an audit cares to ask for.
///
/// Deliberately small: a page a few layers deep, mostly opaque, so that
/// what a comparison finds is a disagreement rather than a haze of
/// antialiasing. And deliberately *drawable*: nothing here is refused by
/// the document, so a page that does not come back is the audit's answer
/// rather than the generator's mistake.
pub fn page(seed: u64) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let mut doc = Document::new(48, 36, ColorMode::Rgb);
    let root = doc.root();
    // Something to see through the layers above, so a blend and an
    // adjustment have work to do.
    let ground = rng.color(1.0);
    doc.apply(Command::AddNode {
        parent: root,
        index: 0,
        node: shape_node(
            "ground",
            VectorShape::Rect {
                width: 48.0,
                height: 36.0,
                radius: 0.0,
            },
            ground,
        ),
    })
    .unwrap();
    let mut made: Vec<NodeId> = vec![doc.children_of(root).unwrap()[0]];
    let how_many = 2 + rng.upto(5) as usize;
    for i in 0..how_many {
        let at = doc.children_of(root).unwrap().len();
        let node = rng.node(&mut doc, &made, i);
        doc.apply(Command::AddNode {
            parent: root,
            index: at,
            node,
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[at];
        made.push(id);
        rng.dress(&mut doc, id, &made);
    }
    // A quarter of the pages end on a symbol: a plain group, placed but
    // otherwise left alone, and a copy of it dressed like any other layer
    // — which half the time gives it a layer of its own in place of one
    // of the group's. Left to chance, a copy met a group it could stand
    // in for parts of on fewer than one page in a hundred.
    if rng.chance(4) {
        let at = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index: at,
            node: Box::new(Node::group("symbol")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[at];
        let placed = Transform::translation(rng.between(-4.0, 24.0), rng.between(-4.0, 18.0));
        doc.apply(Command::SetTransform {
            id: group,
            transform: placed,
        })
        .unwrap();
        rng.hold(&mut doc, group);
        made.push(group);
        doc.apply(Command::AddNode {
            parent: root,
            index: at + 1,
            node: Box::new(Node::instance("used", group)),
        })
        .unwrap();
        let copy = doc.children_of(root).unwrap()[at + 1];
        made.push(copy);
        rng.dress(&mut doc, copy, &made);
    }
    doc
}

/// A small xorshift, so a seed names a page and the same seed names it
/// again on any machine. `rand` is not a dependency here and this is not
/// cryptography: what is wanted is a spread, repeatably.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn upto(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, one_in: u64) -> bool {
        self.upto(one_in) == 0
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }
    fn color(&mut self, alpha: f32) -> AuthoredColor {
        AuthoredColor::Srgb {
            r: self.unit(),
            g: self.unit(),
            b: self.unit(),
            a: alpha,
        }
    }
    fn shape(&mut self) -> VectorShape {
        match self.upto(3) {
            0 => VectorShape::Rect {
                width: self.between(6.0, 24.0),
                height: self.between(6.0, 20.0),
                radius: if self.chance(2) {
                    self.between(0.0, 4.0)
                } else {
                    0.0
                },
            },
            1 => VectorShape::Ellipse {
                rx: self.between(3.0, 12.0),
                ry: self.between(3.0, 10.0),
            },
            _ => {
                let n = 3 + self.upto(3) as usize;
                let points = (0..n)
                    .map(|_| [self.between(0.0, 20.0), self.between(0.0, 18.0)])
                    .collect();
                VectorShape::Path {
                    points,
                    closed: true,
                    smooth: self.chance(3),
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                }
            }
        }
    }

    /// One layer, of a kind picked from everything a page can hold.
    /// `made` is what is already on the page, so a copy has something to
    /// be a copy of and cannot reach itself.
    fn node(&mut self, doc: &mut Document, made: &[NodeId], i: usize) -> Box<Node> {
        let name = format!("l{i}");
        match self.upto(11) {
            0 => Box::new(Node::group(&name)),
            1 => {
                let mut node = Node::vector(&name, self.shape());
                let fill = self.color(1.0);
                if let NodeKind::Vector {
                    fill: f,
                    stroke,
                    gradient,
                    ..
                } = &mut node.kind
                {
                    *f = Some(fill);
                    if self.chance(3) {
                        // Dressed now and then the way a stroke can be —
                        // broken into dashes, capped and joined each way,
                        // kept inside or outside the outline, ended with
                        // a marker, swelling along a path — where every
                        // stroke on these pages had been the plain one.
                        let marker = |r: &mut Self| match r.upto(6) {
                            0 => Marker::Arrow,
                            1 => Marker::Bar,
                            2 => Marker::Dot,
                            _ => Marker::None,
                        };
                        *stroke = Some(Stroke {
                            color: self.color(1.0),
                            width: self.between(0.5, 3.0),
                            widths: if self.chance(4) {
                                (0..5).map(|_| self.between(0.2, 1.0)).collect()
                            } else {
                                Vec::new()
                            },
                            dash: if self.chance(4) {
                                vec![self.between(1.0, 5.0), self.between(1.0, 4.0)]
                            } else {
                                Vec::new()
                            },
                            cap: match self.upto(3) {
                                0 => crate::StrokeCap::Butt,
                                1 => crate::StrokeCap::Round,
                                _ => crate::StrokeCap::Square,
                            },
                            join: match self.upto(3) {
                                0 => crate::StrokeJoin::Miter,
                                1 => crate::StrokeJoin::Round,
                                _ => crate::StrokeJoin::Bevel,
                            },
                            align: match self.upto(4) {
                                0 => Some(StrokeAlign::Inside),
                                1 => Some(StrokeAlign::Outside),
                                _ => None,
                            },
                            start_marker: marker(self),
                            end_marker: marker(self),
                        });
                    }
                    if self.chance(4) && self.chance(2) {
                        *gradient = Some(Gradient::Radial {
                            center: [self.between(0.2, 0.8), self.between(0.2, 0.8)],
                            radius: self.between(0.4, 1.2),
                            stops: vec![
                                GradientStop {
                                    offset: 0.0,
                                    color: self.color(1.0),
                                },
                                GradientStop {
                                    offset: 1.0,
                                    color: self.color(1.0),
                                },
                            ],
                        });
                    } else if self.chance(4) {
                        *gradient = Some(Gradient::Linear {
                            from: [0.0, 0.0],
                            to: [1.0, 1.0],
                            stops: vec![
                                GradientStop {
                                    offset: 0.0,
                                    color: self.color(1.0),
                                },
                                GradientStop {
                                    offset: 1.0,
                                    color: self.color(1.0),
                                },
                            ],
                        });
                    }
                }
                Box::new(node)
            }
            2 => {
                // Text as it is set, rather than two letters on one line:
                // wrapped to a width, aligned, tracked and spaced, ruled
                // under and through, a run of it styled differently, and
                // now and then set along a curve instead of a line.
                let words = ["Ab", "Wave hi", "Mixed type set", "Café au lait"];
                let text = words[self.upto(words.len() as u64) as usize];
                let mut spec = TextSpec::new(text, self.between(8.0, 20.0), self.color(1.0));
                spec.bold = self.chance(3);
                spec.italic = self.chance(3);
                spec.underline = self.chance(5);
                spec.strike = self.chance(6);
                spec.align = match self.upto(3) {
                    0 => crate::TextAlign::Left,
                    1 => crate::TextAlign::Center,
                    _ => crate::TextAlign::Right,
                };
                if self.chance(3) {
                    spec.width = self.between(14.0, 40.0);
                }
                if self.chance(3) {
                    spec.letter_spacing = self.between(-0.05, 0.2);
                    spec.line_height = self.between(0.9, 1.6);
                }
                // A styled run over the first word, which is plain ASCII in
                // every one of the strings, so its bytes are its letters.
                if self.chance(3) {
                    let end = text.find(' ').unwrap_or(text.len()).min(2);
                    spec.runs = vec![StyleRun {
                        start: 0,
                        end,
                        fill: Some(self.color(1.0)),
                        bold: Some(self.chance(2)),
                        italic: None,
                        underline: Some(self.chance(2)),
                        strike: None,
                        font: None,
                    }];
                }
                if self.chance(6) {
                    spec.along = Some(if self.chance(2) {
                        VectorShape::Ellipse {
                            rx: self.between(10.0, 20.0),
                            ry: self.between(6.0, 14.0),
                        }
                    } else {
                        VectorShape::Path {
                            points: vec![[0.0, 10.0], [15.0, 0.0], [30.0, 10.0]],
                            closed: false,
                            smooth: true,
                            handles: Vec::new(),
                            subpaths: Vec::new(),
                        }
                    });
                    spec.along_offset = self.between(0.0, 10.0);
                }
                Box::new(Node::text(&name, spec))
            }
            3 => {
                // A tiny picture, two by two, in its own colours — and a
                // third of them see-through in places. Every picture here
                // was opaque to the last pixel, so what a picture's own
                // alpha does under a mask, a blend, a turn or an effect
                // had been asked by nothing but the pages somebody wrote.
                let clear = self.chance(3);
                let mut bytes = Vec::with_capacity(16);
                for _ in 0..4 {
                    bytes.extend_from_slice(&[
                        (self.unit() * 255.0) as u8,
                        (self.unit() * 255.0) as u8,
                        (self.unit() * 255.0) as u8,
                        if clear {
                            (self.unit() * 255.0) as u8
                        } else {
                            255
                        },
                    ]);
                }
                let id = doc.add_resource(2, 2, bytes);
                Box::new(Node::raster(
                    &name,
                    RasterRef {
                        resource_id: id,
                        width: 6 + self.upto(14) as u32,
                        height: 6 + self.upto(12) as u32,
                    },
                ))
            }
            // Every adjustment rather than three. The other nine had never
            // stood on one of these pages, so none of them had met a
            // random mask, blend, turn, copy or layer held to it, nor the
            // second renderer on a page it did not choose.
            4 => Box::new(Node::adjustment(
                &name,
                match self.upto(12) {
                    0 => Adjustment::Exposure {
                        stops: self.between(-1.5, 1.5),
                    },
                    1 => Adjustment::BrightnessContrast {
                        brightness: self.between(-0.3, 0.3),
                        contrast: self.between(-0.3, 0.3),
                    },
                    2 => Adjustment::HueSaturation {
                        hue_degrees: self.between(-60.0, 60.0),
                        saturation: self.between(-0.5, 0.5),
                        lightness: self.between(-0.2, 0.2),
                    },
                    3 => {
                        let in_black = self.between(0.0, 0.3);
                        let out_black = self.between(0.0, 0.2);
                        Adjustment::Levels {
                            in_black,
                            in_white: self.between(in_black + 0.3, 1.0),
                            gamma: self.between(0.5, 2.0),
                            out_black,
                            out_white: self.between(out_black + 0.5, 1.0),
                        }
                    }
                    4 => {
                        let curve = |r: &mut Self| {
                            vec![
                                [0.0, r.between(0.0, 0.2)],
                                [0.5, r.between(0.3, 0.7)],
                                [1.0, r.between(0.8, 1.0)],
                            ]
                        };
                        Adjustment::Curves {
                            points: curve(self),
                            red: if self.chance(2) {
                                curve(self)
                            } else {
                                Vec::new()
                            },
                            green: Vec::new(),
                            blue: if self.chance(2) {
                                curve(self)
                            } else {
                                Vec::new()
                            },
                        }
                    }
                    5 => Adjustment::WhiteBalance {
                        temperature: self.between(-0.8, 0.8),
                        tint: self.between(-0.5, 0.5),
                    },
                    6 => Adjustment::Vibrance {
                        amount: self.between(-0.8, 0.8),
                    },
                    7 => Adjustment::BlackAndWhite {
                        red: self.between(0.0, 1.0),
                        green: self.between(0.0, 1.0),
                        blue: self.between(0.0, 1.0),
                    },
                    8 => Adjustment::GradientMap {
                        stops: vec![
                            GradientStop {
                                offset: 0.0,
                                color: self.color(1.0),
                            },
                            GradientStop {
                                offset: 1.0,
                                color: self.color(1.0),
                            },
                        ],
                    },
                    9 => Adjustment::Invert {
                        amount: self.between(0.2, 1.0),
                    },
                    10 => Adjustment::SelectiveHsl {
                        bands: (0..6)
                            .map(|_| {
                                [
                                    self.between(-0.5, 0.5),
                                    self.between(-0.6, 0.6),
                                    self.between(-0.3, 0.3),
                                ]
                            })
                            .collect(),
                    },
                    _ => {
                        if self.chance(2) {
                            Adjustment::ShadowsHighlights {
                                shadows: self.between(-0.8, 0.8),
                                highlights: self.between(-0.8, 0.8),
                            }
                        } else {
                            let mut three = || {
                                [
                                    self.between(-0.6, 0.6),
                                    self.between(-0.6, 0.6),
                                    self.between(-0.6, 0.6),
                                ]
                            };
                            Adjustment::ColorBalance {
                                shadows: three(),
                                midtones: three(),
                                highlights: three(),
                                preserve_luminosity: self.chance(2),
                            }
                        }
                    }
                },
            )),
            // All six filters rather than two. The four that were
            // missing are each a different shape of question and none of
            // them was being asked here: a sharpen reads a
            // neighbourhood and gives back more than it was given, so it
            // is the one that can put a value above white on the page; a
            // smear reads along a line rather than across the axes; a
            // vignette is a function of where a pixel is on the *page*
            // rather than of what is under it, so a group's transform
            // does not carry it; and noise is a function of position and
            // of nothing else, which makes it the one whose answer has
            // to be the same answer every time it is asked — by a second
            // renderer, by a page redrawn a region at a time, and by an
            // undo.
            5 => Box::new(Node::filter(
                &name,
                match self.upto(6) {
                    0 => Filter::GaussianBlur {
                        sigma: self.between(0.5, 2.5),
                    },
                    1 => Filter::Pixelate {
                        size: self.between(2.0, 6.0),
                    },
                    2 => Filter::Sharpen {
                        sigma: self.between(0.5, 2.0),
                        amount: self.between(0.2, 1.5),
                    },
                    3 => Filter::MotionBlur {
                        distance: self.between(1.0, 6.0),
                        degrees: self.between(0.0, 180.0),
                    },
                    4 => Filter::Vignette {
                        amount: self.between(-0.8, 0.8),
                        radius: self.between(0.1, 0.8),
                        softness: self.between(0.1, 1.0),
                    },
                    _ => Filter::Noise {
                        amount: self.between(0.05, 0.4),
                        grain: self.between(0.8, 3.0),
                        mono: self.chance(2),
                        seed: self.upto(1 << 16) as u32,
                    },
                },
            )),
            6 => {
                let mut node = Node::paint(&name);
                let stroke = PaintStroke {
                    points: (0..3)
                        .map(|_| [self.between(0.0, 40.0), self.between(0.0, 30.0)])
                        .collect(),
                    radii: vec![self.between(1.0, 4.0)],
                    color: self.color(1.0),
                    softness: if self.chance(2) {
                        self.between(0.0, 1.0)
                    } else {
                        0.0
                    },
                    erase: false,
                    source: [0.0, 0.0],
                    heal: false,
                    clip: None,
                };
                // Now and then a second stroke that erases across the
                // first, or one laid inside a region it carries — the two
                // things a brush does that are not laying paint down.
                let second = if self.chance(3) {
                    let clip = self.chance(2).then(|| {
                        Box::new(Mask {
                            kind: MaskKind::Vector {
                                shape: VectorShape::Rect {
                                    width: self.between(8.0, 30.0),
                                    height: self.between(8.0, 24.0),
                                    radius: 0.0,
                                },
                                transform: Transform::translation(
                                    self.between(0.0, 20.0),
                                    self.between(0.0, 16.0),
                                ),
                            },
                            invert: false,
                            feather: 0.0,
                        })
                    });
                    Some(PaintStroke {
                        points: (0..2)
                            .map(|_| [self.between(0.0, 40.0), self.between(0.0, 30.0)])
                            .collect(),
                        radii: vec![self.between(1.5, 5.0)],
                        erase: clip.is_none(),
                        clip,
                        ..stroke.clone()
                    })
                } else {
                    None
                };
                if let NodeKind::Paint { strokes } = &mut node.kind {
                    strokes.push(stroke);
                    strokes.extend(second);
                }
                Box::new(node)
            }
            7 if !made.is_empty() => {
                // Half the time a plain group where there is one, since
                // that is the only original a copy can stand in for parts
                // of (`dress`), and picked from everything alone it was
                // one page in two hundred.
                let plain: Vec<NodeId> = made
                    .iter()
                    .copied()
                    .filter(|m| {
                        doc.node(*m)
                            .map(|o| {
                                matches!(o.kind, NodeKind::Group)
                                    && o.opacity >= 1.0
                                    && o.blend == BlendMode::Normal
                                    && o.mask.is_none()
                                    && o.effects.is_empty()
                            })
                            .unwrap_or(false)
                    })
                    .collect();
                let of = if !plain.is_empty() && self.chance(2) {
                    plain[self.upto(plain.len() as u64) as usize]
                } else {
                    made[self.upto(made.len() as u64) as usize]
                };
                Box::new(Node::instance(&name, of))
            }
            // A clone layer, which no page here held: what it lays is
            // whatever the page under it shows somewhere else, so a mask,
            // a blend, a turn or an effect on it meets a layer that
            // paints with its neighbours. Half its strokes heal, which
            // reads the whole stroke before laying any of it.
            8 => {
                let mut node = Node::clone_layer(&name);
                let strokes: Vec<PaintStroke> = (0..1 + self.upto(2))
                    .map(|_| PaintStroke {
                        points: (0..1 + self.upto(3))
                            .map(|_| [self.between(0.0, 40.0), self.between(0.0, 30.0)])
                            .collect(),
                        radii: vec![self.between(2.0, 6.0)],
                        color: self.color(1.0),
                        softness: self.between(0.0, 0.8),
                        erase: false,
                        source: [self.between(-20.0, 20.0), self.between(-16.0, 16.0)],
                        heal: self.chance(2),
                        clip: None,
                    })
                    .collect();
                if let NodeKind::Clone { strokes: s } = &mut node.kind {
                    *s = strokes;
                }
                Box::new(node)
            }
            // A frame, which no page here held: it cuts what it holds to
            // its own box and paints a ground behind it, or none. Given
            // something to hold the way a group is, below.
            9 => Box::new(Node::artboard(
                &name,
                self.between(12.0, 36.0),
                self.between(10.0, 28.0),
                self.chance(2).then(|| self.color(1.0)),
            )),
            _ => {
                let mut node = Node::vector(&name, self.shape());
                let alpha = self.between(0.4, 1.0);
                let fill = self.color(alpha);
                if let NodeKind::Vector { fill: f, .. } = &mut node.kind {
                    *f = Some(fill);
                }
                Box::new(node)
            }
        }
    }

    /// Something for a group or a frame to hold: one or two layers,
    /// placed inside it.
    fn hold(&mut self, doc: &mut Document, id: NodeId) {
        for k in 0..1 + self.upto(2) {
            let child = self.node(doc, &[], 90 + k as usize);
            doc.apply(Command::AddNode {
                parent: id,
                index: k as usize,
                node: child,
            })
            .unwrap();
            let cid = doc.children_of(id).unwrap()[k as usize];
            let ct = Transform::translation(self.between(0.0, 16.0), self.between(0.0, 12.0));
            doc.apply(Command::SetTransform {
                id: cid,
                transform: ct,
            })
            .unwrap();
        }
    }

    /// What is done to a layer after it is put down: where it sits, how
    /// it composites, and now and then a mask, a clip, a child or an
    /// effect.
    fn dress(&mut self, doc: &mut Document, id: NodeId, made: &[NodeId]) {
        let (e, f) = (self.between(-4.0, 34.0), self.between(-4.0, 26.0));
        // Every layer on these pages used to be *placed* and never
        // turned: a translation, and the two scales left at one. So a
        // turned layer met a mask, a clip, a blend or an effect nowhere
        // here, and the combinations these pages exist to find were
        // being drawn from a space with one of the axes missing. A
        // third of them stand turned now, by an angle that is not a
        // quarter of anything and a scale either side of one, so a box
        // that is not the shape's box and a length that is not the
        // length on the page are ordinary rather than exceptional.
        let t = if self.chance(3) {
            let angle = self.between(-0.7, 0.7);
            let scale = self.between(0.7, 1.4);
            let (sin, cos) = angle.sin_cos();
            Transform {
                a: scale * cos,
                b: scale * sin,
                c: -scale * sin,
                d: scale * cos,
                e,
                f,
            }
        } else {
            Transform {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e,
                f,
            }
        };
        doc.apply(Command::SetTransform { id, transform: t })
            .unwrap();
        if self.chance(4) {
            let opacity = self.between(0.3, 0.95);
            doc.apply(Command::SetOpacity { id, opacity }).unwrap();
        }
        if self.chance(3) {
            let blend = BLENDS[self.upto(BLENDS.len() as u64) as usize];
            doc.apply(Command::SetBlendMode { id, blend }).unwrap();
        }
        if self.chance(5) {
            let at = Transform::translation(self.between(0.0, 20.0), self.between(0.0, 16.0));
            // Every kind of mask rather than an ellipse. A mask of another
            // shape is the same code with another outline; a brushed one
            // is a plane worked out stroke by stroke, and an image one is
            // a picture's brightness read through a transform — two
            // readings of coverage no page here had ever asked for.
            let kind = match self.upto(4) {
                0 => MaskKind::Vector {
                    shape: VectorShape::Ellipse {
                        rx: self.between(4.0, 14.0),
                        ry: self.between(4.0, 12.0),
                    },
                    transform: at,
                },
                1 => MaskKind::Vector {
                    shape: self.shape(),
                    transform: at,
                },
                2 => MaskKind::Painted {
                    strokes: (0..1 + self.upto(3))
                        .map(|_| PaintStroke {
                            points: (0..2)
                                .map(|_| [self.between(0.0, 40.0), self.between(0.0, 30.0)])
                                .collect(),
                            radii: vec![self.between(2.0, 6.0)],
                            color: self.color(1.0),
                            softness: self.between(0.0, 0.8),
                            erase: self.chance(2),
                            source: [0.0, 0.0],
                            heal: false,
                            clip: None,
                        })
                        .collect(),
                },
                _ => {
                    let (w, h) = (3u32, 3u32);
                    let bytes: Vec<u8> = (0..w * h)
                        .flat_map(|_| {
                            let v = (self.unit() * 255.0) as u8;
                            [v, v, v, 255]
                        })
                        .collect();
                    let resource_id = doc.add_resource(w, h, bytes);
                    let scale = self.between(3.0, 8.0);
                    MaskKind::Raster {
                        resource_id,
                        width: w,
                        height: h,
                        transform: at.compose(Transform {
                            a: scale,
                            b: 0.0,
                            c: 0.0,
                            d: scale,
                            e: 0.0,
                            f: 0.0,
                        }),
                    }
                }
            };
            let mask = Mask {
                kind,
                invert: self.chance(3),
                feather: if self.chance(2) {
                    self.between(0.0, 2.0)
                } else {
                    0.0
                },
            };
            doc.apply(Command::SetMask {
                id,
                mask: Some(Box::new(mask)),
            })
            .unwrap();
        }
        if self.chance(5) {
            doc.apply(Command::SetEffects {
                id,
                effects: vec![match self.upto(3) {
                    0 => Effect::DropShadow {
                        dx: self.between(-3.0, 3.0),
                        dy: self.between(-3.0, 3.0),
                        blur: self.between(0.0, 2.5),
                        color: self.color(1.0),
                        opacity: self.between(0.4, 1.0),
                    },
                    1 => Effect::Outline {
                        width: self.between(0.5, 3.0),
                        color: self.color(1.0),
                        opacity: self.between(0.4, 1.0),
                    },
                    _ => Effect::InnerShadow {
                        dx: self.between(-2.0, 2.0),
                        dy: self.between(-2.0, 2.0),
                        blur: self.between(0.0, 2.0),
                        color: self.color(1.0),
                        opacity: self.between(0.4, 1.0),
                    },
                }],
            })
            .unwrap();
        }
        // Held to the layer below, when there is one and this is not it.
        if self.chance(5) && made.len() > 1 {
            doc.apply(Command::SetClipped { id, clipped: true })
                .unwrap();
        }
        // A group with nothing in it draws nothing, so one gets a child —
        // and so does a frame, which is a group with a box.
        if doc
            .node(id)
            .map(|n| matches!(n.kind, NodeKind::Group | NodeKind::Artboard { .. }))
            .unwrap_or(false)
        {
            self.hold(doc, id);
        }
        // A copy that differs from what it follows. Every copy on these
        // pages drew its original entire, so a layer of the copy's own
        // standing in for one of the original's had met a mask, a blend,
        // a turn, an effect or a layer held to it only on the one page
        // somebody wrote. Where the original is a plain group — the only
        // thing a copy can stand in for parts of — half the copies take
        // a stand-in: the original's layer moved and, for a shape, given
        // another fill, as `Session::override_child` starts one and a
        // person then changes it; a group is stood in for by a copy of
        // it, as there. Now and then a layer of the copy's own stands in
        // for nothing and is drawn after the rest.
        let copied = match doc.node(id).map(|n| &n.kind) {
            Ok(NodeKind::Instance { of, .. }) => Some(*of),
            _ => None,
        };
        if let Some(of) = copied {
            let plain = doc
                .node(of)
                .map(|o| {
                    matches!(o.kind, NodeKind::Group)
                        && o.opacity >= 1.0
                        && o.blend == BlendMode::Normal
                        && o.mask.is_none()
                        && o.effects.is_empty()
                })
                .unwrap_or(false);
            let theirs = doc.children_of(of).map(|c| c.to_vec()).unwrap_or_default();
            if plain && !theirs.is_empty() && self.chance(2) {
                let original = theirs[self.upto(theirs.len() as u64) as usize];
                let o = doc.node(original).unwrap();
                let mut own = if o.kind.holds_children() {
                    let mut n = Node::instance(&o.name, original);
                    n.transform = o.transform;
                    n
                } else {
                    o.clone()
                };
                own.transform.e += self.between(-4.0, 4.0);
                own.transform.f += self.between(-4.0, 4.0);
                if let NodeKind::Vector { fill: Some(f), .. } = &mut own.kind {
                    let alpha = self.between(0.5, 1.0);
                    *f = self.color(alpha);
                }
                if self.chance(3) {
                    own.blend = BLENDS[self.upto(BLENDS.len() as u64) as usize];
                }
                doc.apply(Command::AddNode {
                    parent: id,
                    index: 0,
                    node: Box::new(own),
                })
                .unwrap();
                doc.apply(Command::SetKind {
                    id,
                    kind: Box::new(NodeKind::Instance {
                        of,
                        replaces: vec![original],
                    }),
                })
                .unwrap();
                if self.chance(4) {
                    let extra = self.node(doc, &[], 95);
                    doc.apply(Command::AddNode {
                        parent: id,
                        index: 1,
                        node: extra,
                    })
                    .unwrap();
                    let eid = doc.children_of(id).unwrap()[1];
                    let et =
                        Transform::translation(self.between(0.0, 16.0), self.between(0.0, 12.0));
                    doc.apply(Command::SetTransform {
                        id: eid,
                        transform: et,
                    })
                    .unwrap();
                }
            }
        }
    }
}

/// The blend modes a page nobody wrote draws from.
///
/// The last four are not more of the first six. A separable blend is one
/// function of one channel and its opposite number, run three times; the
/// non-separable ones take brightness, hue or saturation from the whole
/// colour and the rest from what is under it, which is its own code in
/// both renderers and its own name in both exporters. Six separable ones
/// stood here for a long while, so these pages had never drawn one
/// either; and five more separable ones — a dodge, a burn, the two
/// lights and an exclusion — stood nowhere at all until the shared
/// fixture took a soft light, so they are here now too.
const BLENDS: [BlendMode; 15] = [
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::Difference,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

fn shape_node(name: &str, shape: VectorShape, fill: AuthoredColor) -> Box<Node> {
    let mut node = Node::vector(name, shape);
    if let NodeKind::Vector { fill: f, .. } = &mut node.kind {
        *f = Some(fill);
    }
    Box::new(node)
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
        pierced,
        graded,
        arc,
        leveled,
        soft,
        tilted,
        again,
        spaced,
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
        // The pierced path: moved, so its dirty region has to cover where
        // its curves bulge past its anchors; faded; and given handles of
        // another length, which is the one edit that changes a curve
        // without moving an anchor.
        Command::SetTransform {
            id: *pierced,
            transform: Transform::translation(48.0, 36.0),
        },
        Command::SetOpacity {
            id: *pierced,
            opacity: 0.6,
        },
        Command::SetKind {
            id: *pierced,
            kind: Box::new(NodeKind::Vector {
                shape: VectorShape::Path {
                    points: vec![[0.0, 8.0], [20.0, 8.0]],
                    closed: true,
                    smooth: false,
                    handles: vec![[0.0, 4.0, 0.0, -4.0], [0.0, -4.0, 0.0, 4.0]],
                    subpaths: vec![vec![[7.0, 5.0], [13.0, 5.0], [13.0, 11.0], [7.0, 11.0]]],
                },
                fill: Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.85,
                    g: 0.55,
                    b: 0.15,
                    a: 1.0,
                }),
                stroke: None,
                gradient: None,
            }),
        },
        // The grade re-pointed: the master straightened, green given a
        // curve where it had none, blue's list emptied — every list of
        // the four changed in a different way.
        Command::SetKind {
            id: *graded,
            kind: Box::new(NodeKind::Adjustment(crate::Adjustment::Curves {
                points: vec![[0.0, 0.0], [1.0, 1.0]],
                red: vec![[0.0, 0.12], [1.0, 1.0]],
                green: vec![[0.0, 0.0], [0.5, 0.4], [1.0, 1.0]],
                blue: Vec::new(),
            })),
        },
        Command::SetOpacity {
            id: *graded,
            opacity: 0.7,
        },
        // The guided text: its guide closed into a ring, which is the
        // other branch of the walk — a closed guide wraps what runs off
        // its end rather than dropping it — and the text slid further
        // along; and faded.
        Command::SetKind {
            id: *arc,
            kind: Box::new(NodeKind::Text({
                let mut spec = TextSpec::new(
                    "curve",
                    7.0,
                    chitrakar_color::AuthoredColor::Srgb {
                        r: 0.05,
                        g: 0.35,
                        b: 0.3,
                        a: 1.0,
                    },
                );
                spec.along = Some(VectorShape::Ellipse { rx: 9.0, ry: 7.0 });
                spec.along_offset = 5.0;
                spec
            })),
        },
        Command::SetOpacity {
            id: *arc,
            opacity: 0.8,
        },
        // The levels re-pointed: every one of the five moved, the gamma
        // across one — a lift where there was a drop.
        Command::SetKind {
            id: *leveled,
            kind: Box::new(NodeKind::Adjustment(crate::Adjustment::Levels {
                in_black: 0.0,
                in_white: 0.7,
                gamma: 0.8,
                out_black: 0.15,
                out_white: 1.0,
            })),
        },
        Command::SetOpacity {
            id: *leveled,
            opacity: 0.6,
        },
        // The soft light swapped for a colour dodge — a division where
        // there was a curve — and faded.
        Command::SetBlendMode {
            id: *soft,
            blend: BlendMode::ColorDodge,
        },
        Command::SetOpacity {
            id: *soft,
            opacity: 0.7,
        },
        // The tilted rectangle turned further and stretched unevenly:
        // a forty-five degree turn with the two axes scaled apart, so
        // the largest and smallest a length can come out at differ and
        // anything taking one for the other is wrong about the stroke.
        Command::SetTransform {
            id: *tilted,
            transform: Transform {
                a: 1.131_37,
                b: 1.131_37,
                c: -0.565_685,
                d: 0.565_685,
                e: 28.0,
                f: 26.0,
            },
        },
        // The second picture turned the other way and shrunk, so the
        // sampling runs the other way round the clock and a device pixel
        // covers more than one of its own.
        Command::SetTransform {
            id: *again,
            transform: Transform {
                a: 0.636_4,
                b: -0.636_4,
                c: 0.636_4,
                d: 0.636_4,
                e: 32.0,
                f: 10.0,
            },
        },
        Command::SetOpacity {
            id: *again,
            opacity: 0.85,
        },
        // The spacing taken back out: tight, the two words share a line
        // again, so the block's box changes shape as well as size — two
        // lines to one — which is the edit whose dirty region is easiest
        // to get wrong.
        Command::SetKind {
            id: *spaced,
            kind: Box::new(NodeKind::Text({
                let mut spec = TextSpec::new(
                    "Agile mark",
                    12.0,
                    chitrakar_color::AuthoredColor::Srgb {
                        r: 0.15,
                        g: 0.1,
                        b: 0.3,
                        a: 1.0,
                    },
                );
                spec.width = 60.0;
                spec
            })),
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
        // And the same three said of the layer's *mask* rather than the
        // layer. A mask brushed by hand is brushed with the same tool and
        // the same strokes, and the commands say which of the two they
        // mean with a flag — so a list that only ever said `false` was
        // asking half the question of three commands while looking like
        // it asked all of it. Every audit over this list now says both.
        Command::AddStroke {
            id: painted,
            index: 1,
            stroke: Box::new(PaintStroke {
                points: vec![[18.0, 30.0], [44.0, 48.0]],
                radii: vec![5.0],
                erase: false,
                ..stroke.clone()
            }),
            on_mask: true,
        },
        Command::RemoveStroke {
            id: painted,
            index: 0,
            on_mask: true,
        },
        Command::SetStroke {
            id: painted,
            index: 0,
            stroke: Box::new(PaintStroke {
                softness: 0.0,
                erase: false,
                ..stroke.clone()
            }),
            on_mask: true,
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
        Command::ScaleCanvas {
            factor: 1.5,
            width: 120,
            height: 90,
        },
        Command::SetStyles {
            styles: vec![KeptStyle {
                name: "cool".into(),
                look: Look {
                    fill: Some(AuthoredColor::Srgb {
                        r: 0.2,
                        g: 0.3,
                        b: 0.9,
                        a: 1.0,
                    }),
                    stroke: None,
                    gradient: None,
                    effects: Vec::new(),
                    opacity: 1.0,
                    blend: BlendMode::Normal,
                },
            }],
        },
        Command::MoveNode {
            id: under,
            parent: root,
            index: 0,
        },
        // And a layer reordered *within* its own parent, both ways.
        // That is what dragging a layer up or down the list does, much
        // the commonest move there is, and it is the one whose inverse
        // index has a reservation on it: the old index is read while
        // the layer is still in the list, so putting it back only lands
        // if the undo takes it out first — which it does, and which
        // nothing here said. Both moves above cross into another group,
        // where the question cannot come up; an inverse index off by
        // one in exactly the same-parent downward case passed every
        // test in this workspace until these two went in.
        Command::MoveNode {
            id: over,
            parent: group,
            index: 0,
        },
        Command::MoveNode {
            id: under,
            parent: group,
            index: 1,
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
        // A batch of nothing but field edits, on more than one layer:
        // what the app sends when several layers are picked and one
        // slider moves. Every other batch here carries something
        // structural, which is a different question — and it left the
        // plainest batch there is, the one a person makes every day,
        // out of every audit that asks about batches.
        Command::Batch(vec![
            Command::SetOpacity {
                id: under,
                opacity: 0.15,
            },
            Command::SetOpacity {
                id: over,
                opacity: 0.45,
            },
            Command::SetTransform {
                id: painted,
                transform: Transform::translation(4.0, -3.0),
            },
        ]),
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
        Command::SetStyles { .. } => "SetStyles",
        Command::SetSelection { .. } => "SetSelection",
        Command::SetRegions { .. } => "SetRegions",
        Command::ResizeCanvas { .. } => "ResizeCanvas",
        Command::TurnCanvas { .. } => "TurnCanvas",
        Command::ScaleCanvas { .. } => "ScaleCanvas",
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
    "SetStyles",
    "SetSelection",
    "SetRegions",
    "ResizeCanvas",
    "TurnCanvas",
    "ScaleCanvas",
    "MirrorCanvas",
    "StraightenCanvas",
    "Batch",
];

/// Whether a command can be undone bit for bit.
///
/// A page turned by anything but a quarter cannot be turned back
/// exactly — a sine and its cosine do not multiply out to one — and an
/// axis-aligned guide cannot record a tilt at all, so it comes back on
/// its own axis where it crosses the middle of the page. A page scaled
/// by a ratio that is not a power of two cannot be scaled back exactly
/// either. Those are the two commands allowed to be near rather than
/// exact.
pub fn exact(cmd: &Command) -> bool {
    !matches!(
        cmd,
        Command::StraightenCanvas { .. } | Command::ScaleCanvas { .. }
    )
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
