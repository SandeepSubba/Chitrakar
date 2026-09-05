//! The `.chitra` document container: a ZIP with a versioned JSON manifest
//! and (as they arrive) embedded source resources, ICC profiles, and
//! thumbnails (docs/PLAN.md §2).

use chitrakar_doc::Document;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Bumped on breaking manifest-schema changes; readers refuse newer majors.
pub const FORMAT_VERSION: u32 = 1;

const MANIFEST_PATH: &str = "manifest.json";

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest error: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("unsupported format version {found} (this build reads up to {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("that file says its page is {width}x{height}, which is more than can be drawn")]
    BadCanvas { width: u32, height: u32 },
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    document: Document,
    /// Faces the document's text is set in, carried under `fonts/` so it
    /// reads the same wherever it is opened. Additive: an older reader
    /// ignores the field and the entries alike.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fonts: Vec<EmbeddedFont>,
}

/// One face inside the container: the name text blocks set themselves in,
/// and the entry holding its file. The entry is numbered rather than named
/// after the face so any name, however it is spelled, makes a valid path.
#[derive(Serialize, Deserialize)]
struct EmbeddedFont {
    name: String,
    file: String,
}

/// A font to carry: the name text blocks know it by, and its file.
pub type FontFile<'a> = (&'a str, &'a [u8]);

/// What comes out of a container: the document, and the fonts that came
/// with it, for the caller to make available before rendering.
pub struct Opened {
    pub doc: Document,
    pub fonts: Vec<(String, Vec<u8>)>,
}

/// Serialize a document to `.chitra` bytes. Resource pixels are stored as
/// PNG entries under `resources/`; the manifest carries only their metadata.
pub fn save_chitra(doc: &Document) -> Result<Vec<u8>, ContainerError> {
    save_chitra_with_fonts(doc, &[])
}

/// [`save_chitra`], carrying `fonts` inside the container as well.
pub fn save_chitra_with_fonts(
    doc: &Document,
    fonts: &[FontFile],
) -> Result<Vec<u8>, ContainerError> {
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        document: doc.clone(),
        fonts: fonts
            .iter()
            .enumerate()
            .map(|(i, (name, _))| EmbeddedFont {
                name: name.to_string(),
                file: format!("fonts/{i}.ttf"),
            })
            .collect(),
    };
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(MANIFEST_PATH, SimpleFileOptions::default())?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    for (entry, (_, bytes)) in manifest.fonts.iter().zip(fonts) {
        zip.start_file(&entry.file, SimpleFileOptions::default())?;
        zip.write_all(bytes)?;
    }
    for (id, res) in doc.resources() {
        if res.rgba8.is_empty() {
            continue; // metadata-only entry (bytes were never restored)
        }
        let png = crate::encode_png(res.width, res.height, &res.rgba8)
            .map_err(|e| ContainerError::Io(std::io::Error::other(e.to_string())))?;
        // PNG is already compressed; recompressing wastes time.
        zip.start_file(
            format!("resources/{id}.png"),
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )?;
        zip.write_all(&png)?;
    }
    if let Some(icc) = doc.cmyk_profile_bytes() {
        zip.start_file(CMYK_PROFILE_PATH, SimpleFileOptions::default())?;
        zip.write_all(icc)?;
    }
    Ok(zip.finish()?.into_inner())
}

const CMYK_PROFILE_PATH: &str = "profiles/cmyk.icc";

/// Load a document from `.chitra` bytes, leaving any fonts it carries
/// inside.
pub fn load_chitra(bytes: &[u8]) -> Result<Document, ContainerError> {
    load_chitra_with_fonts(bytes).map(|o| o.doc)
}

/// Load a document and the fonts it carries from `.chitra` bytes.
pub fn load_chitra_with_fonts(bytes: &[u8]) -> Result<Opened, ContainerError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let mut manifest_json = String::new();
    zip.by_name(MANIFEST_PATH)?
        .read_to_string(&mut manifest_json)?;
    let manifest: Manifest = serde_json::from_str(&manifest_json)?;
    if manifest.format_version > FORMAT_VERSION {
        return Err(ContainerError::UnsupportedVersion {
            found: manifest.format_version,
            supported: FORMAT_VERSION,
        });
    }
    let mut doc = manifest.document;
    // A page that opens has to be one the engine could draw: the surface
    // is sixteen bytes a pixel, so a file claiming an enormous one would
    // ask for memory nobody has rather than fail honestly here.
    if !chitrakar_doc::canvas_fits(doc.meta.width, doc.meta.height) {
        return Err(ContainerError::BadCanvas {
            width: doc.meta.width,
            height: doc.meta.height,
        });
    }
    let entries: Vec<String> = zip.file_names().map(String::from).collect();
    for name in entries {
        let Some(id) = name
            .strip_prefix("resources/")
            .and_then(|n| n.strip_suffix(".png"))
            .map(String::from)
        else {
            continue;
        };
        let mut png = Vec::new();
        zip.by_name(&name)?.read_to_end(&mut png)?;
        let img = crate::decode(&png)
            .map_err(|e| ContainerError::Io(std::io::Error::other(e.to_string())))?;
        // Silently ignores entries the manifest doesn't reference or whose
        // size disagrees — the manifest is the source of truth.
        doc.restore_resource_bytes(&id, img.rgba8);
    }
    if let Ok(mut entry) = zip.by_name(CMYK_PROFILE_PATH) {
        let mut icc = Vec::new();
        entry.read_to_end(&mut icc)?;
        // Best effort: a profile that no longer parses is dropped.
        let _ = doc.set_cmyk_profile(icc);
    }
    let mut fonts = Vec::new();
    for font in manifest.fonts {
        // A listed face whose file is missing is left out rather than
        // failing the open; its text falls back to the bundled face.
        let Ok(mut entry) = zip.by_name(&font.file) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        fonts.push((font.name, bytes));
    }
    Ok(Opened { doc, fonts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::ColorMode;
    use chitrakar_doc::{Command, Node, VectorShape};

    #[test]
    fn chitra_roundtrip_preserves_document() {
        let mut doc = Document::new(320, 240, ColorMode::Cmyk);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::vector(
                "shape",
                VectorShape::Ellipse { rx: 20.0, ry: 10.0 },
            )),
        })
        .unwrap();

        let bytes = save_chitra(&doc).unwrap();
        assert_eq!(&bytes[0..2], b"PK", "must be a zip container");

        let restored = load_chitra(&bytes).unwrap();
        assert_eq!(restored.node_count(), 2);
        assert_eq!(restored.meta.color_mode, ColorMode::Cmyk);
        assert_eq!(restored.meta.width, 320);
    }

    /// Everything the document model can hold, saved and opened again,
    /// and the two rendered side by side.
    ///
    /// Each kind of layer, mask, adjustment and effect has its own test
    /// somewhere; what this one is for is the gap between them — a kind
    /// added to the model and not to the file, which no per-kind test
    /// would notice because each of those builds its document in memory.
    /// A file that is damaged, truncated, or simply not one of ours must
    /// be refused rather than bring the editor down with it: a save cut
    /// short by a full disk is exactly the file someone will try to open.
    #[test]
    fn a_damaged_file_is_refused_not_survived() {
        assert!(load_chitra(b"").is_err(), "nothing at all");
        assert!(load_chitra(b"not a zip, just words").is_err(), "not a zip");
        let mut doc = Document::new(64, 48, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::vector(
                "r",
                VectorShape::Rect {
                    width: 10.0,
                    height: 10.0,
                    radius: 0.0,
                },
            )),
        })
        .unwrap();
        let good = save_chitra(&doc).unwrap();
        // Cut short at every tenth of its length.
        for cut in 1..10 {
            let at = good.len() * cut / 10;
            let _ = load_chitra(&good[..at]);
        }
        // A byte flipped anywhere in the first kilobyte.
        for i in (0..good.len().min(1024)).step_by(7) {
            let mut bent = good.clone();
            bent[i] ^= 0xff;
            let _ = load_chitra(&bent);
        }
        // And the whole of it still opens.
        assert_eq!(load_chitra(&good).unwrap().meta.width, 64);
    }

    /// A page that opens is one the engine could draw. A file claiming an
    /// enormous one is refused here, where it can be said, rather than
    /// asking for memory nobody has on the first render.
    #[test]
    fn a_page_too_big_to_draw_is_refused_on_the_way_in() {
        for (w, h) in [(100_000u32, 100_000u32), (40_000, 10), (20_000, 20_000)] {
            let doc = Document::new(w, h, ColorMode::Rgb);
            let saved = save_chitra(&doc).unwrap();
            assert!(
                matches!(load_chitra(&saved), Err(ContainerError::BadCanvas { .. })),
                "{w}x{h} should have been refused"
            );
        }
        // And a page anyone would actually work on still opens.
        for (w, h) in [(1u32, 1u32), (2480, 3508), (9000, 9000)] {
            let saved = save_chitra(&Document::new(w, h, ColorMode::Rgb)).unwrap();
            assert_eq!(load_chitra(&saved).unwrap().meta.height, h, "{w}x{h}");
        }
    }

    /// The palette is document state like the guides are: it saves with
    /// the file and comes back, and setting it is its own inverse.
    #[test]
    fn the_palette_saves_with_the_document() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let swatches = vec![
            chitrakar_doc::Swatch {
                name: "ink".into(),
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.1,
                    g: 0.1,
                    b: 0.12,
                    a: 1.0,
                },
            },
            chitrakar_doc::Swatch {
                name: "paper".into(),
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.98,
                    g: 0.97,
                    b: 0.92,
                    a: 1.0,
                },
            },
        ];
        doc.apply(Command::SetSwatches {
            swatches: swatches.clone(),
        })
        .unwrap();
        let back = load_chitra(&save_chitra(&doc).unwrap()).unwrap();
        assert_eq!(back.swatches(), swatches.as_slice());
        // A document written before there was a palette still reads.
        let bare = Document::new(8, 8, ColorMode::Rgb);
        assert!(load_chitra(&save_chitra(&bare).unwrap())
            .unwrap()
            .swatches()
            .is_empty());
    }

    #[test]
    fn a_document_of_everything_survives_the_round_trip() {
        let mut doc = Document::new(120, 120, ColorMode::Rgb);
        let root = doc.root();
        let red = chitrakar_color::AuthoredColor::Srgb {
            r: 0.9,
            g: 0.2,
            b: 0.1,
            a: 1.0,
        };
        let blue = chitrakar_color::AuthoredColor::Srgb {
            r: 0.1,
            g: 0.3,
            b: 0.9,
            a: 1.0,
        };
        let mut at = 0;
        let mut add = |doc: &mut Document, node: Box<Node>| {
            doc.apply(Command::AddNode {
                parent: root,
                index: at,
                node,
            })
            .unwrap();
            at += 1;
            doc.children_of(root).unwrap()[at - 1]
        };

        // A shape with a gradient and a stroke.
        let mut shape = Node::vector(
            "shape",
            VectorShape::Rect {
                width: 40.0,
                height: 30.0,
                radius: 5.0,
            },
        );
        if let chitrakar_doc::NodeKind::Vector {
            fill,
            stroke,
            gradient,
            ..
        } = &mut shape.kind
        {
            *fill = Some(red);
            *stroke = Some(chitrakar_doc::Stroke {
                color: blue,
                width: 3.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: chitrakar_doc::StrokeCap::Square,
                join: chitrakar_doc::StrokeJoin::Bevel,
                align: None,
                start_marker: Default::default(),
                end_marker: Default::default(),
            });
            *gradient = Some(chitrakar_doc::Gradient::Linear {
                from: [0.0, 0.0],
                to: [1.0, 1.0],
                stops: vec![
                    chitrakar_doc::GradientStop {
                        offset: 0.0,
                        color: red,
                    },
                    chitrakar_doc::GradientStop {
                        offset: 1.0,
                        color: blue,
                    },
                ],
            });
        }
        let shape_id = add(&mut doc, Box::new(shape));
        doc.apply(Command::SetEffects {
            id: shape_id,
            effects: vec![chitrakar_doc::Effect::DropShadow {
                dx: 3.0,
                dy: 3.0,
                blur: 2.0,
                color: blue,
                opacity: 0.7,
            }],
        })
        .unwrap();

        // A painted layer, with a painted mask over it.
        let paint_id = add(&mut doc, Box::new(Node::paint("brush")));
        doc.apply(Command::AddStroke {
            id: paint_id,
            index: 0,
            stroke: Box::new(chitrakar_doc::PaintStroke {
                points: vec![[10.0, 90.0], [60.0, 100.0], [110.0, 90.0]],
                radii: vec![9.0, 5.0, 7.0],
                color: blue,
                softness: 0.4,
                erase: false,
                source: [0.0, 0.0],
                heal: false,
            }),
            on_mask: false,
        })
        .unwrap();
        doc.apply(Command::SetMask {
            id: paint_id,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Painted {
                    strokes: vec![chitrakar_doc::PaintStroke {
                        points: vec![[60.0, 95.0]],
                        radii: vec![8.0],
                        color: red,
                        softness: 0.0,
                        erase: true,
                        source: [0.0, 0.0],
                        heal: false,
                    }],
                },
                invert: false,
            })),
        })
        .unwrap();

        // Text, and the two newest adjustments over everything.
        add(
            &mut doc,
            Box::new(Node::text(
                "words",
                chitrakar_doc::TextSpec::new("Chitrakar", 14.0, red),
            )),
        );
        add(
            &mut doc,
            Box::new(Node::adjustment(
                "balance",
                chitrakar_doc::Adjustment::WhiteBalance {
                    temperature: 0.3,
                    tint: -0.2,
                },
            )),
        );
        add(
            &mut doc,
            Box::new(Node::adjustment(
                "vibrance",
                chitrakar_doc::Adjustment::Vibrance { amount: 0.5 },
            )),
        );
        // A gradient map carries a ramp of its own, which is the only
        // adjustment whose settings have any shape to lose.
        add(
            &mut doc,
            Box::new(Node::adjustment(
                "duotone",
                chitrakar_doc::Adjustment::GradientMap {
                    stops: vec![
                        chitrakar_doc::GradientStop {
                            offset: 0.0,
                            color: blue,
                        },
                        chitrakar_doc::GradientStop {
                            offset: 0.6,
                            color: red,
                        },
                    ],
                },
            )),
        );

        // A frame with something pinned inside it, a live copy of the
        // shape, a layer confined to the one below it, and a curve with
        // a channel of its own — everything the newest node kinds and
        // fields carry, so a file written today still reads as itself.
        let frame = add(
            &mut doc,
            Box::new(Node::artboard("frame", 40.0, 40.0, Some(blue))),
        );
        doc.apply(Command::SetTransform {
            id: frame,
            transform: chitrakar_doc::Transform::translation(70.0, 10.0),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: frame,
            index: 0,
            node: Box::new(Node::vector(
                "in the frame",
                VectorShape::Rect {
                    width: 15.0,
                    height: 15.0,
                    radius: 0.0,
                },
            )),
        })
        .unwrap();
        let inside = doc.children_of(frame).unwrap()[0];
        doc.apply(Command::SetKind {
            id: inside,
            kind: Box::new(chitrakar_doc::NodeKind::Vector {
                shape: VectorShape::Rect {
                    width: 15.0,
                    height: 15.0,
                    radius: 0.0,
                },
                fill: Some(red),
                stroke: None,
                gradient: None,
            }),
        })
        .unwrap();
        doc.apply(Command::SetPinning {
            id: inside,
            pinned: chitrakar_doc::Pinning {
                x: chitrakar_doc::Pin::End,
                y: chitrakar_doc::Pin::Middle,
            },
        })
        .unwrap();

        let copy = add(&mut doc, Box::new(Node::instance("copy", shape_id)));
        doc.apply(Command::SetTransform {
            id: copy,
            transform: chitrakar_doc::Transform::translation(5.0, 70.0),
        })
        .unwrap();
        doc.apply(Command::SetOpacity {
            id: copy,
            opacity: 0.6,
        })
        .unwrap();

        let over = add(
            &mut doc,
            Box::new(Node::vector(
                "clipped to the copy",
                VectorShape::Ellipse { rx: 30.0, ry: 30.0 },
            )),
        );
        doc.apply(Command::SetKind {
            id: over,
            kind: Box::new(chitrakar_doc::NodeKind::Vector {
                shape: VectorShape::Ellipse { rx: 30.0, ry: 30.0 },
                fill: Some(blue),
                stroke: None,
                gradient: None,
            }),
        })
        .unwrap();
        doc.apply(Command::SetClipped {
            id: over,
            clipped: true,
        })
        .unwrap();

        add(
            &mut doc,
            Box::new(Node::adjustment(
                "graded",
                chitrakar_doc::Adjustment::Curves {
                    points: vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]],
                    red: vec![[0.0, 0.1], [1.0, 0.9]],
                    green: Vec::new(),
                    blue: Vec::new(),
                },
            )),
        );

        let before = chitrakar_render::render(&doc).unwrap();
        let back = load_chitra(&save_chitra(&doc).unwrap()).unwrap();
        // The same pixels can come from a document that lost what it was
        // made of, so check the shape of it too.
        assert!(
            matches!(
                back.node(frame).unwrap().kind,
                chitrakar_doc::NodeKind::Artboard {
                    width: 40.0,
                    height: 40.0,
                    background: Some(_),
                    ..
                }
            ),
            "the frame came back a frame"
        );
        assert_eq!(back.children_of(frame).unwrap().len(), 1, "holding its own");
        assert_eq!(
            back.node(inside).unwrap().pinned,
            chitrakar_doc::Pinning {
                x: chitrakar_doc::Pin::End,
                y: chitrakar_doc::Pin::Middle,
            },
            "and what is pinned in it is still pinned"
        );
        assert!(
            matches!(
                back.node(copy).unwrap().kind,
                chitrakar_doc::NodeKind::Instance { of, .. } if of == shape_id
            ),
            "the copy still follows what it followed"
        );
        assert!(back.node(over).unwrap().clipped, "and the clip survived");
        let chitrakar_doc::NodeKind::Vector {
            stroke: Some(line), ..
        } = &back.node(shape_id).unwrap().kind
        else {
            panic!("a stroked shape")
        };
        assert_eq!(
            (line.cap, line.join),
            (
                chitrakar_doc::StrokeCap::Square,
                chitrakar_doc::StrokeJoin::Bevel
            ),
            "and the line still ends and turns the way it was drawn"
        );
        let after = chitrakar_render::render(&back).unwrap();
        assert_eq!((before.width, before.height), (after.width, after.height));
        let mut worst = 0.0f32;
        for (p, q) in before.pixels.iter().zip(&after.pixels) {
            for (u, v) in [(p.r, q.r), (p.g, q.g), (p.b, q.b), (p.a, q.a)] {
                worst = worst.max((u - v).abs());
            }
        }
        assert!(worst < 1e-6, "the page came back different by {worst}");
        // And it is a page with something on it, not two blank ones.
        let ink = before.pixels.iter().filter(|p| p.a > 0.01).count();
        assert!(ink > 1000, "there was something to compare ({ink} pixels)");
    }

    /// A painting is strokes, not pixels, so it saves as what it is and
    /// comes back still editable.
    #[test]
    fn a_painting_saves_as_its_strokes() {
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::paint("brush")),
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddStroke {
            id,
            index: 0,
            on_mask: false,
            stroke: Box::new(chitrakar_doc::PaintStroke {
                points: vec![[4.0, 4.0], [20.0, 30.0]],
                radii: vec![3.0, 6.0],
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.2,
                    g: 0.4,
                    b: 0.9,
                    a: 1.0,
                },
                softness: 0.5,
                erase: false,
                source: [0.0, 0.0],
                heal: false,
            }),
        })
        .unwrap();

        let restored = load_chitra(&save_chitra(&doc).unwrap()).unwrap();
        let node = restored
            .node(restored.children_of(restored.root()).unwrap()[0])
            .unwrap();
        let chitrakar_doc::NodeKind::Paint { strokes } = &node.kind else {
            panic!("not a paint layer: {:?}", node.kind);
        };
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].points, vec![[4.0, 4.0], [20.0, 30.0]]);
        assert_eq!(strokes[0].radii, vec![3.0, 6.0]);
        assert_eq!(strokes[0].softness, 0.5);
    }

    #[test]
    fn resources_roundtrip_through_container() {
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let rgba8 = vec![
            10, 20, 30, 255, /**/ 40, 50, 60, 255, //
            70, 80, 90, 200, /**/ 0, 0, 0, 0,
        ];
        let id = doc.add_resource(2, 2, rgba8.clone());

        let bytes = save_chitra(&doc).unwrap();
        let restored = load_chitra(&bytes).unwrap();
        let res = restored.resource(&id).unwrap();
        assert_eq!((res.width, res.height), (2, 2));
        assert_eq!(res.rgba8, rgba8, "pixel bytes survive the PNG roundtrip");
    }

    /// Needs a real CMYK press profile; see CHITRAKAR_TEST_CMYK_ICC in
    /// chitrakar-color's cms tests.
    #[test]
    fn cmyk_profile_roundtrips_through_container() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let mut doc = Document::new(8, 8, ColorMode::Cmyk);
        doc.set_cmyk_profile(icc.clone()).unwrap();

        let bytes = save_chitra(&doc).unwrap();
        let restored = load_chitra(&bytes).unwrap();
        assert_eq!(restored.cmyk_profile_bytes(), Some(icc.as_slice()));
        assert!(restored.cmyk_cms().is_some(), "transform rebuilt on load");
    }

    #[test]
    fn fonts_travel_inside_the_container() {
        let mut doc = Document::new(8, 8, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::text(
                "t",
                chitrakar_doc::TextSpec::new(
                    "hi",
                    12.0,
                    chitrakar_color::AuthoredColor::Srgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                ),
            )),
        })
        .unwrap();
        let face = b"not really a font, but the container does not care".to_vec();
        let bytes = save_chitra_with_fonts(&doc, &[("A Face / with odd name", &face)]).unwrap();

        let opened = load_chitra_with_fonts(&bytes).unwrap();
        assert_eq!(opened.doc.node_count(), 2);
        assert_eq!(opened.fonts.len(), 1);
        assert_eq!(opened.fonts[0].0, "A Face / with odd name");
        assert_eq!(opened.fonts[0].1, face, "the file comes back byte for byte");
        assert!(
            load_chitra(&bytes).is_ok(),
            "the plain loader reads the same file"
        );

        let plain = save_chitra(&doc).unwrap();
        assert!(load_chitra_with_fonts(&plain).unwrap().fonts.is_empty());
        assert!(
            !String::from_utf8_lossy(&plain).contains("\"fonts\""),
            "a document without fonts writes no fonts field"
        );
    }

    #[test]
    fn newer_major_version_is_refused() {
        let doc = Document::new(8, 8, ColorMode::Rgb);
        let bytes = save_chitra(&doc).unwrap();
        let tampered = String::from_utf8({
            let mut zip = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
            let mut s = Vec::new();
            zip.by_name(MANIFEST_PATH)
                .unwrap()
                .read_to_end(&mut s)
                .unwrap();
            s
        })
        .unwrap()
        .replace("\"format_version\": 1", "\"format_version\": 99");

        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(MANIFEST_PATH, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(tampered.as_bytes()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();

        assert!(matches!(
            load_chitra(&bytes),
            Err(ContainerError::UnsupportedVersion { found: 99, .. })
        ));
    }
}
