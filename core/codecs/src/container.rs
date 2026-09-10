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
    /// The layers a file names and what it says each group holds are two
    /// separate lists, and they can disagree in ways no document this
    /// editor makes ever does.
    #[error("that file's layers do not make sense together: {0}")]
    BadStructure(chitrakar_doc::DocError),
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
    // The id counter, if the file says a number behind the ids it holds.
    // A file can say that, and the next layer added to such a document
    // would take an id that is already somebody's and overwrite the node
    // under it. Bookkeeping rather than artwork, so it is put right rather
    // than being grounds to refuse the file.
    doc.settle_next_id();
    // And that the layers are a tree at all. A file names the layers and
    // names what each group holds as two separate lists, and nothing about
    // the format stops one of them naming a layer that is not in the other
    // or naming one of its own ancestors — which is a walk that never
    // ends, so opening such a file and drawing it took the process with
    // it. Refused rather than repaired: there is no version of a cycle
    // that is what somebody meant.
    doc.check_structure()
        .map_err(ContainerError::BadStructure)?;
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

    /// A document written out in full, in a fixed order: every field of
    /// every node, the page's own settings, its guides, its swatches,
    /// its resources and what is picked out.
    ///
    /// Spelled out rather than serialized, because serializing is the
    /// thing under test — two documents compared as JSON agree about
    /// every field the JSON has, and say nothing at all about a field it
    /// has lost. `Debug` prints what is there. The order is the tree's
    /// own, since the nodes live in a hash map and its order says
    /// nothing.
    ///
    /// Every accessor the document has, and a new piece of document
    /// state has to be added here — the same rule the shared fixture
    /// keeps for commands. Anything left out is invisible to the audit,
    /// which is exactly as bad as not having written the audit: the
    /// regions a document keeps by name were added a chunk after this
    /// and slipped through until they were named here too.
    fn spelled_out(doc: &Document) -> String {
        fn walk(doc: &Document, id: chitrakar_doc::NodeId, out: &mut String) {
            out.push_str(&format!("{id:?} {:#?}\n", doc.node(id).unwrap()));
            for child in doc.children_of(id).unwrap_or_default() {
                walk(doc, *child, out);
            }
        }
        let mut out = format!(
            // On one line each: a difference is reported as the line it
            // fell on, and a pretty-printed list puts its own bracket on
            // a line of its own, which says nothing about what changed.
            "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\nprofile {:?} bytes\n",
            doc.meta,
            doc.selection(),
            doc.guides(),
            doc.swatches(),
            doc.regions(),
            doc.cmyk_profile_bytes().map(<[u8]>::len)
        );
        for (id, resource) in doc.resources() {
            out.push_str(&format!(
                "resource {id} {}x{} {} bytes\n",
                resource.width,
                resource.height,
                resource.rgba8.len()
            ));
        }
        walk(doc, doc.root(), &mut out);
        out
    }

    /// The first line of two written-out documents that differs, for a
    /// message that says something without printing either of them.
    fn first_difference(a: &str, b: &str) -> String {
        a.lines()
            .zip(b.lines())
            .find(|(x, y)| x != y)
            .map(|(x, y)| format!("{} became {}", x.trim(), y.trim()))
            .unwrap_or_else(|| "one is longer than the other".into())
    }

    /// A file can say anything.
    ///
    /// A `.chitra` is a thing a person opens, and what it says about
    /// itself is the only account of it there is: the page's size, and
    /// every resource's, come out of a manifest, while the pixels come
    /// out of entries beside it. So the sizes are numbers arriving from
    /// outside exactly as the ones the app sends are, and they end the
    /// same two ways — an allocation nobody can serve, or arithmetic
    /// that overflows before anything is checked. Refused is the
    /// answer; opening it and falling over is not.
    #[test]
    fn a_file_that_says_anything_is_refused_rather_than_believed() {
        // A document with a picture in it, to have something real to
        // take apart.
        let mut doc = Document::new(20, 16, ColorMode::Rgb);
        let id = doc.add_resource(2, 2, vec![255u8; 16]);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster(
                "picture",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: 2,
                    height: 2,
                },
            )),
        })
        .unwrap();
        let good = save_chitra(&doc).unwrap();
        assert!(load_chitra(&good).is_ok(), "the honest one opens");

        // Nothing, noise, and a truncation: each a file somebody could
        // hand over, none of them one of ours.
        for bytes in [
            Vec::new(),
            b"not a zip at all".to_vec(),
            good[..good.len() / 2].to_vec(),
            good[..8].to_vec(),
        ] {
            assert!(
                load_chitra(&bytes).is_err(),
                "a file that is not one of ours is refused"
            );
        }

        // A manifest is JSON in a zip, so a hostile one is a zip we can
        // write ourselves. The sizes are what it can lie about.
        let rewrite = |patch: &dyn Fn(&mut serde_json::Value)| -> Vec<u8> {
            let mut zip = ZipArchive::new(Cursor::new(good.clone())).unwrap();
            let mut manifest = String::new();
            zip.by_name(MANIFEST_PATH)
                .unwrap()
                .read_to_string(&mut manifest)
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
            patch(&mut value);
            // Everything else carried over, the resource entries above
            // all: without them the manifest's account of a picture is
            // never held against any bytes, which is the check being
            // aimed at.
            let others: Vec<String> = zip
                .file_names()
                .filter(|n| *n != MANIFEST_PATH)
                .map(String::from)
                .collect();
            let mut out = Vec::new();
            {
                let mut w = ZipWriter::new(Cursor::new(&mut out));
                w.start_file(MANIFEST_PATH, SimpleFileOptions::default())
                    .unwrap();
                w.write_all(value.to_string().as_bytes()).unwrap();
                for name in others {
                    let mut body = Vec::new();
                    zip.by_name(&name).unwrap().read_to_end(&mut body).unwrap();
                    w.start_file(&name, SimpleFileOptions::default()).unwrap();
                    w.write_all(&body).unwrap();
                }
                w.finish().unwrap();
            }
            out
        };

        // A page bigger than can be drawn. Sixteen bytes a pixel, so
        // believing it is asking for memory nobody has.
        let huge_page = rewrite(&|v| {
            v["document"]["meta"]["width"] = 400_000.into();
            v["document"]["meta"]["height"] = 400_000.into();
        });
        assert!(load_chitra(&huge_page).is_err(), "a page too big to draw");

        // A resource whose two sides multiplied pass what a `u32` holds.
        // The bytes for it live outside the manifest, so the two are
        // made to agree on the way in — and that check is arithmetic
        // that has to hold for any pair the file names.
        let huge_resource = rewrite(&|v| {
            let resources = v["document"]["resources"].as_object_mut().unwrap();
            for (_, r) in resources.iter_mut() {
                r["width"] = 65_536.into();
                r["height"] = 65_536.into();
            }
        });
        // Opened or refused, either is an answer; falling over is not.
        let _ = load_chitra(&huge_resource);

        // And one that says its format is from the future.
        let ahead = rewrite(&|v| v["format_version"] = 9_999.into());
        assert!(
            load_chitra(&ahead).is_err(),
            "a file written by something newer says so"
        );
    }

    /// A file whose id counter is behind the ids in it opens, and the next
    /// layer added does not overwrite one that is already there.
    ///
    /// The counter is bookkeeping: nothing looks at it and it is only ever
    /// handed out. But it is written into the file with everything else,
    /// and a file saying a smaller number than the ids it holds is a file
    /// where the next `AddNode` takes an id that is already somebody's —
    /// the node under it is replaced, and the tree is left with two places
    /// claiming the same layer. Before this, adding one layer to such a
    /// document silently ate another: the layer named "layer 0" came back
    /// as the new one.
    ///
    /// Put right rather than refused, and the difference is the point:
    /// where a file's account of its artwork contradicts the artwork — a
    /// resource whose size does not match its bytes — there is nothing to
    /// do but refuse it, which the audit above checks. A counter is not
    /// the artwork, and throwing somebody's work away over a number nobody
    /// sees would be the wrong trade.
    #[test]
    fn a_file_whose_id_counter_is_behind_does_not_eat_a_layer() {
        let mut doc = Document::new(20, 16, ColorMode::Rgb);
        let root = doc.root();
        for i in 0..3 {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(Node::group(&format!("layer {i}"))),
            })
            .unwrap();
        }
        let good = save_chitra(&doc).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(good.clone())).unwrap();
        let mut manifest = String::new();
        zip.by_name(MANIFEST_PATH)
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(
            value["document"]["next_id"], 4,
            "the honest file counts past the ids it holds"
        );

        // Every number a file could say, including honest ones: none of
        // them may cost a layer.
        for behind in [0u64, 1, 2, 3, 4, 9] {
            value["document"]["next_id"] = behind.into();
            let mut out = Vec::new();
            {
                let mut w = ZipWriter::new(Cursor::new(&mut out));
                w.start_file(MANIFEST_PATH, SimpleFileOptions::default())
                    .unwrap();
                w.write_all(value.to_string().as_bytes()).unwrap();
                w.finish().unwrap();
            }
            let mut back = load_chitra(&out)
                .unwrap_or_else(|e| panic!("a file counting from {behind} opens: {e}"));
            let names = |d: &Document| -> Vec<String> {
                let mut out: Vec<String> = d.nodes().map(|(_, n)| n.name.clone()).collect();
                out.sort();
                out
            };
            let was = names(&back);
            let root = back.root();
            back.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("new")),
            })
            .unwrap_or_else(|e| panic!("counting from {behind}: a layer can still be added: {e}"));
            let now = names(&back);
            assert_eq!(
                now.len(),
                was.len() + 1,
                "counting from {behind}: adding a layer added one ({was:?} -> {now:?})"
            );
            for name in &was {
                assert!(
                    now.contains(name),
                    "counting from {behind}: {name} is still there ({now:?})"
                );
            }
            // And the tree agrees with itself: every child a group names
            // is a node, and no id is claimed twice.
            let mut seen = std::collections::BTreeSet::new();
            for (id, _) in back.nodes() {
                assert!(seen.insert(*id), "counting from {behind}: {id:?} twice");
            }
            for (id, _) in back.nodes() {
                if let Ok(kids) = back.children_of(*id) {
                    for kid in kids {
                        assert!(
                            back.node(*kid).is_ok(),
                            "counting from {behind}: {id:?} names {kid:?}, which is not there"
                        );
                    }
                }
            }
        }
    }

    /// A file that says its layers are not a tree is refused, rather than
    /// opened and then drawn until the stack runs out.
    ///
    /// A file names the layers there are and names what each group holds as
    /// two separate lists, and nothing about the format stops one of them
    /// naming a layer that is not in the other, naming the same layer
    /// twice, or naming one of its own ancestors. Every command in this
    /// editor keeps the layers a tree, so nothing that has been *applied*
    /// can be in that state — which is exactly why nothing looked.
    ///
    /// A group holding its own ancestor is a walk that never ends. Such a
    /// file opened, and drawing it overflowed the stack and took the
    /// process with it: a crash from being handed a file, which is the
    /// worst way for "a file that says anything is refused rather than
    /// believed" to be untrue.
    ///
    /// Refused where the id counter beside it is repaired, and the
    /// difference is the same one: a counter is bookkeeping and there is a
    /// right answer to put in it, where a cycle is not something anybody
    /// meant and has no reading that keeps their work.
    #[test]
    fn a_file_whose_layers_are_not_a_tree_is_refused() {
        let mut doc = Document::new(20, 16, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("outer")),
        })
        .unwrap();
        let outer = doc.children_of(root).unwrap()[0];
        doc.apply(Command::AddNode {
            parent: outer,
            index: 0,
            node: Box::new(Node::group("inner")),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::vector(
                "shape",
                chitrakar_doc::VectorShape::Rect {
                    width: 6.0,
                    height: 4.0,
                    radius: 0.0,
                },
            )),
        })
        .unwrap();
        let shape = doc.children_of(root).unwrap()[1];
        let good = save_chitra(&doc).unwrap();
        assert!(load_chitra(&good).is_ok(), "the honest one opens");
        let (outer, inner, shape) = (outer.0, doc.children_of(outer).unwrap()[0].0, shape.0);

        let rewrite = |patch: &dyn Fn(&mut serde_json::Value)| -> Vec<u8> {
            let mut zip = ZipArchive::new(Cursor::new(good.clone())).unwrap();
            let mut manifest = String::new();
            zip.by_name(MANIFEST_PATH)
                .unwrap()
                .read_to_string(&mut manifest)
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
            patch(&mut value);
            let others: Vec<String> = zip
                .file_names()
                .filter(|n| *n != MANIFEST_PATH)
                .map(String::from)
                .collect();
            let mut out = Vec::new();
            {
                let mut w = ZipWriter::new(Cursor::new(&mut out));
                w.start_file(MANIFEST_PATH, SimpleFileOptions::default())
                    .unwrap();
                w.write_all(value.to_string().as_bytes()).unwrap();
                for name in others {
                    let mut body = Vec::new();
                    zip.by_name(&name).unwrap().read_to_end(&mut body).unwrap();
                    w.start_file(&name, SimpleFileOptions::default()).unwrap();
                    w.write_all(&body).unwrap();
                }
                w.finish().unwrap();
            }
            out
        };

        for (what, patch) in [
            (
                "a group holding one of its own ancestors",
                Box::new(move |v: &mut serde_json::Value| {
                    v["document"]["children"][inner.to_string()] = serde_json::json!([outer]);
                }) as Box<dyn Fn(&mut serde_json::Value)>,
            ),
            (
                "a group holding itself",
                Box::new(move |v: &mut serde_json::Value| {
                    v["document"]["children"][outer.to_string()] = serde_json::json!([outer]);
                }),
            ),
            (
                "the same layer named twice",
                Box::new(move |v: &mut serde_json::Value| {
                    v["document"]["children"]["0"] = serde_json::json!([outer, outer]);
                }),
            ),
            (
                "two groups both holding one layer",
                Box::new(move |v: &mut serde_json::Value| {
                    v["document"]["children"][outer.to_string()] =
                        serde_json::json!([inner, shape]);
                }),
            ),
            (
                "a child that is not in the file",
                Box::new(|v: &mut serde_json::Value| {
                    v["document"]["children"]["0"] = serde_json::json!([9999]);
                }),
            ),
            (
                "a root that is not in the file",
                Box::new(|v: &mut serde_json::Value| {
                    v["document"]["root"] = serde_json::json!(4242);
                }),
            ),
            (
                "a copy of itself",
                Box::new(move |v: &mut serde_json::Value| {
                    v["document"]["nodes"][shape.to_string()]["kind"] =
                        serde_json::json!({ "Instance": { "of": shape, "replaces": [] } });
                }),
            ),
        ] {
            let bytes = rewrite(&*patch);
            let refused = load_chitra(&bytes);
            assert!(refused.is_err(), "{what} is refused rather than opened");
            let said = refused.unwrap_err().to_string();
            assert!(
                said.contains("do not make sense together"),
                "{what} is refused for the right reason: {said}"
            );
        }
    }

    /// Every field a `.chitra` was ever given, taken back out again.
    ///
    /// The one rule this format has is that an old file keeps opening:
    /// a new node kind or a new field is additive, written with
    /// `#[serde(default)]` so that a manifest from before it existed
    /// reads as whatever that default is. Nothing checked it. A field
    /// added without the attribute makes every file anybody has ever
    /// saved unopenable, and the way that is found out is somebody's
    /// work refusing to open.
    ///
    /// So: save the document of everything, then take each key of the
    /// manifest out on its own and see whether the file still opens. The
    /// ones it cannot do without are named here — they are the fields
    /// the format has had since the beginning, and by the rule above
    /// that list is finished. A field added without a default joins it,
    /// and this test says which one.
    ///
    /// An enum's tag is not one of these questions: it is a single-key
    /// object whose one key says which variant, so removing it is not
    /// "a file from before this field" but a file that says nothing at
    /// all. Those are skipped, and the hostile-file test above is where
    /// nonsense belongs.
    #[test]
    fn a_file_written_before_a_field_existed_still_opens() {
        /// The three objects whose keys are data rather than field names:
        /// a node id, a node id, a content address. Taking a key out of
        /// one of them is not "a file from before this field existed", it
        /// is a file with a layer missing.
        const KEYED_BY_DATA: &[&str] = &["nodes", "children", "resources"];

        /// Every (object path, key) pair worth removing. `named` is
        /// whether the object we are in is one whose keys are field
        /// names, which is the only kind a removal means anything in.
        fn keys(v: &serde_json::Value, at: String, named: bool, out: &mut Vec<(String, String)>) {
            match v {
                serde_json::Value::Object(map) => {
                    for (k, child) in map {
                        // A single-key object is an enum's tag: its one
                        // key says which variant, so removing it is not a
                        // question about additive fields either.
                        if named && map.len() > 1 {
                            out.push((at.clone(), k.clone()));
                        }
                        keys(
                            child,
                            format!("{at}/{k}"),
                            !KEYED_BY_DATA.contains(&k.as_str()),
                            out,
                        );
                    }
                }
                serde_json::Value::Array(items) => {
                    for (i, child) in items.iter().enumerate() {
                        keys(child, format!("{at}/{i}"), named, out);
                    }
                }
                _ => {}
            }
        }
        fn at_path<'a>(
            v: &'a mut serde_json::Value,
            path: &str,
        ) -> Option<&'a mut serde_json::Value> {
            let mut cur = v;
            for step in path.split('/').filter(|s| !s.is_empty()) {
                cur = match cur {
                    serde_json::Value::Object(m) => m.get_mut(step)?,
                    serde_json::Value::Array(a) => a.get_mut(step.parse::<usize>().ok()?)?,
                    _ => return None,
                };
            }
            Some(cur)
        }

        // Where the format has always had a field, and which. Each entry
        // is a place in the manifest with its numbers taken out and the
        // field that place cannot do without — the fields that were there
        // from the beginning, which by the rule above is a finished list.
        // It is asserted in both directions: a field added without a
        // default joins it and the test names the place, and an entry that
        // has stopped being needed has to leave.
        const ALWAYS: &[&str] = &[
            "/document/meta:color_mode",
            "/document/meta:dpi",
            "/document/meta:height",
            "/document/meta:width",
            "/document/nodes/*/effects/*/DropShadow/color/Srgb:a",
            "/document/nodes/*/effects/*/DropShadow/color/Srgb:b",
            "/document/nodes/*/effects/*/DropShadow/color/Srgb:g",
            "/document/nodes/*/effects/*/DropShadow/color/Srgb:r",
            "/document/nodes/*/effects/*/DropShadow:blur",
            "/document/nodes/*/effects/*/DropShadow:color",
            "/document/nodes/*/effects/*/DropShadow:dx",
            "/document/nodes/*/effects/*/DropShadow:dy",
            "/document/nodes/*/effects/*/DropShadow:opacity",
            "/document/nodes/*/effects/*/InnerShadow/color/Srgb:a",
            "/document/nodes/*/effects/*/InnerShadow/color/Srgb:b",
            "/document/nodes/*/effects/*/InnerShadow/color/Srgb:g",
            "/document/nodes/*/effects/*/InnerShadow/color/Srgb:r",
            "/document/nodes/*/effects/*/InnerShadow:blur",
            "/document/nodes/*/effects/*/InnerShadow:color",
            "/document/nodes/*/effects/*/InnerShadow:dx",
            "/document/nodes/*/effects/*/InnerShadow:dy",
            "/document/nodes/*/effects/*/InnerShadow:opacity",
            "/document/nodes/*/effects/*/Outline/color/Srgb:a",
            "/document/nodes/*/effects/*/Outline/color/Srgb:b",
            "/document/nodes/*/effects/*/Outline/color/Srgb:g",
            "/document/nodes/*/effects/*/Outline/color/Srgb:r",
            "/document/nodes/*/effects/*/Outline:color",
            "/document/nodes/*/effects/*/Outline:opacity",
            "/document/nodes/*/effects/*/Outline:width",
            "/document/nodes/*/kind/Artboard/background/Srgb:a",
            "/document/nodes/*/kind/Artboard/background/Srgb:b",
            "/document/nodes/*/kind/Artboard/background/Srgb:g",
            "/document/nodes/*/kind/Artboard/background/Srgb:r",
            "/document/nodes/*/kind/Artboard:height",
            "/document/nodes/*/kind/Artboard:width",
            "/document/nodes/*/kind/Clone/strokes/*/color/Srgb:a",
            "/document/nodes/*/kind/Clone/strokes/*/color/Srgb:b",
            "/document/nodes/*/kind/Clone/strokes/*/color/Srgb:g",
            "/document/nodes/*/kind/Clone/strokes/*/color/Srgb:r",
            "/document/nodes/*/kind/Clone/strokes/*:color",
            "/document/nodes/*/kind/Clone/strokes/*:points",
            "/document/nodes/*/kind/Clone/strokes/*:radii",
            "/document/nodes/*/kind/Instance:of",
            "/document/nodes/*/kind/Paint/strokes/*/color/Srgb:a",
            "/document/nodes/*/kind/Paint/strokes/*/color/Srgb:b",
            "/document/nodes/*/kind/Paint/strokes/*/color/Srgb:g",
            "/document/nodes/*/kind/Paint/strokes/*/color/Srgb:r",
            "/document/nodes/*/kind/Paint/strokes/*:color",
            "/document/nodes/*/kind/Paint/strokes/*:points",
            "/document/nodes/*/kind/Paint/strokes/*:radii",
            "/document/nodes/*/kind/Raster:height",
            "/document/nodes/*/kind/Raster:resource_id",
            "/document/nodes/*/kind/Raster:width",
            "/document/nodes/*/kind/Text/fill/Srgb:a",
            "/document/nodes/*/kind/Text/fill/Srgb:b",
            "/document/nodes/*/kind/Text/fill/Srgb:g",
            "/document/nodes/*/kind/Text/fill/Srgb:r",
            "/document/nodes/*/kind/Text:fill",
            "/document/nodes/*/kind/Text:size",
            "/document/nodes/*/kind/Text:text",
            "/document/nodes/*/kind/Paint/strokes/*/color/Cmyk:a",
            "/document/nodes/*/kind/Paint/strokes/*/color/Cmyk:c",
            "/document/nodes/*/kind/Paint/strokes/*/color/Cmyk:k",
            "/document/nodes/*/kind/Paint/strokes/*/color/Cmyk:m",
            "/document/nodes/*/kind/Paint/strokes/*/color/Cmyk:y",
            "/document/nodes/*/kind/Text/runs/*/fill/Srgb:a",
            "/document/nodes/*/kind/Text/runs/*/fill/Srgb:b",
            "/document/nodes/*/kind/Text/runs/*/fill/Srgb:g",
            "/document/nodes/*/kind/Text/runs/*/fill/Srgb:r",
            "/document/nodes/*/kind/Text/runs/*:end",
            "/document/nodes/*/kind/Text/runs/*:start",
            "/document/nodes/*/kind/Vector/fill/Srgb:a",
            "/document/nodes/*/kind/Vector/fill/Srgb:b",
            "/document/nodes/*/kind/Vector/fill/Srgb:g",
            "/document/nodes/*/kind/Vector/fill/Srgb:r",
            "/document/nodes/*/kind/Vector/gradient/Linear/stops/*/color/Srgb:a",
            "/document/nodes/*/kind/Vector/gradient/Linear/stops/*/color/Srgb:b",
            "/document/nodes/*/kind/Vector/gradient/Linear/stops/*/color/Srgb:g",
            "/document/nodes/*/kind/Vector/gradient/Linear/stops/*/color/Srgb:r",
            "/document/nodes/*/kind/Vector/gradient/Linear/stops/*:color",
            "/document/nodes/*/kind/Vector/gradient/Linear/stops/*:offset",
            "/document/nodes/*/kind/Vector/gradient/Linear:from",
            "/document/nodes/*/kind/Vector/gradient/Linear:stops",
            "/document/nodes/*/kind/Vector/gradient/Linear:to",
            "/document/nodes/*/kind/Vector/shape/Rect:height",
            "/document/nodes/*/kind/Vector/shape/Rect:width",
            "/document/nodes/*/kind/Vector:shape",
            "/document/nodes/*/kind/Vector/stroke:color",
            "/document/nodes/*/kind/Vector/stroke:width",
            "/document/nodes/*/kind/Vector/stroke/color/Srgb:a",
            "/document/nodes/*/kind/Vector/stroke/color/Srgb:b",
            "/document/nodes/*/kind/Vector/stroke/color/Srgb:g",
            "/document/nodes/*/kind/Vector/stroke/color/Srgb:r",
            "/document/nodes/*/mask/kind/Raster/transform:a",
            "/document/nodes/*/mask/kind/Raster/transform:b",
            "/document/nodes/*/mask/kind/Raster/transform:c",
            "/document/nodes/*/mask/kind/Raster/transform:d",
            "/document/nodes/*/mask/kind/Raster/transform:e",
            "/document/nodes/*/mask/kind/Raster/transform:f",
            "/document/nodes/*/mask/kind/Raster:height",
            "/document/nodes/*/mask/kind/Raster:resource_id",
            "/document/nodes/*/mask/kind/Raster:transform",
            "/document/nodes/*/mask/kind/Raster:width",
            "/document/nodes/*/mask/kind/Vector/shape/Rect:height",
            "/document/nodes/*/mask/kind/Vector/shape/Rect:width",
            "/document/nodes/*/mask/kind/Vector/transform:a",
            "/document/nodes/*/mask/kind/Vector/transform:b",
            "/document/nodes/*/mask/kind/Vector/transform:c",
            "/document/nodes/*/mask/kind/Vector/transform:d",
            "/document/nodes/*/mask/kind/Vector/transform:e",
            "/document/nodes/*/mask/kind/Vector/transform:f",
            "/document/nodes/*/mask/kind/Vector:shape",
            "/document/nodes/*/mask/kind/Vector:transform",
            "/document/nodes/*/mask:invert",
            "/document/nodes/*/mask:kind",
            "/document/nodes/*/pinned:x",
            "/document/nodes/*/pinned:y",
            "/document/nodes/*/transform:a",
            "/document/nodes/*/transform:b",
            "/document/nodes/*/transform:c",
            "/document/nodes/*/transform:d",
            "/document/nodes/*/transform:e",
            "/document/nodes/*/transform:f",
            "/document/nodes/*:blend",
            "/document/nodes/*:kind",
            "/document/nodes/*:name",
            "/document/nodes/*:opacity",
            "/document/nodes/*:transform",
            "/document/nodes/*:visible",
            "/document/resources/*:height",
            "/document/resources/*:width",
            "/document:children",
            "/document:meta",
            "/document:next_id",
            "/document:nodes",
            "/document:root",
            ":document",
            ":format_version",
        ];

        let f = chitrakar_doc::fixture::everything();
        let good = save_chitra(&f.doc).unwrap();
        assert!(load_chitra(&good).is_ok(), "the file itself opens");
        let mut zip = ZipArchive::new(Cursor::new(good.clone())).unwrap();
        let mut manifest = String::new();
        zip.by_name(MANIFEST_PATH)
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        let base: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        // Everything but the manifest carried over untouched, resources
        // included: a file missing a field is still a file with its
        // pictures in it.
        let rest: Vec<(String, Vec<u8>)> = zip
            .file_names()
            .filter(|n| *n != MANIFEST_PATH)
            .map(String::from)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|n| {
                let mut b = Vec::new();
                zip.by_name(&n).unwrap().read_to_end(&mut b).unwrap();
                (n, b)
            })
            .collect();
        let repack = |value: &serde_json::Value| -> Vec<u8> {
            let mut out = Vec::new();
            {
                let mut w = ZipWriter::new(Cursor::new(&mut out));
                w.start_file(MANIFEST_PATH, SimpleFileOptions::default())
                    .unwrap();
                w.write_all(value.to_string().as_bytes()).unwrap();
                for (name, body) in &rest {
                    w.start_file(name, SimpleFileOptions::default()).unwrap();
                    w.write_all(body).unwrap();
                }
                w.finish().unwrap();
            }
            out
        };

        let mut all = Vec::new();
        keys(&base, String::new(), true, &mut all);
        assert!(
            all.len() > 200,
            "the manifest of everything has fields to take out: {}",
            all.len()
        );
        /// A path with its numbers taken out, so an entry stands for
        /// "an effect of a node" rather than for node seven's second
        /// effect: node ids and array indices are both numbers and both
        /// are data. Naming the place as well as the field is what makes
        /// the failure say where to go — two structs can each have a
        /// `blur`, and only one of them need be at fault.
        fn shapely(path: &str) -> String {
            let mut out: Vec<&str> = Vec::new();
            let mut after_data = false;
            for seg in path.split('/') {
                // A segment is data when it is an array index, or when it
                // is the key of one of the objects keyed by data — a node
                // id, a content address. A content address is not a number
                // and would otherwise be baked into the list, which would
                // then have to be rewritten every time a fixture's pixels
                // changed.
                let data =
                    after_data || (!seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()));
                after_data = KEYED_BY_DATA.contains(&seg);
                out.push(if data { "*" } else { seg });
            }
            out.join("/")
        }
        let mut cannot: std::collections::BTreeSet<String> = Default::default();
        for (path, key) in &all {
            let mut value = base.clone();
            let Some(serde_json::Value::Object(m)) = at_path(&mut value, path) else {
                continue;
            };
            m.remove(key);
            if load_chitra(&repack(&value)).is_err() {
                cannot.insert(format!("{}:{key}", shapely(path)));
            }
        }
        let added: Vec<&String> = cannot
            .iter()
            .filter(|k| !ALWAYS.contains(&k.as_str()))
            .collect();
        assert!(
            added.is_empty(),
            "a file written before these existed will not open — they want #[serde(default)]: {added:?}"
        );
        // And the other way round, so the list cannot quietly keep names
        // that stopped being needed.
        let gone: Vec<&&str> = ALWAYS.iter().filter(|k| !cannot.contains(**k)).collect();
        assert!(
            gone.is_empty(),
            "these are no longer needed and can leave the list: {gone:?}"
        );

        // The whole claim in one file: every additive field taken out at
        // once, which is as near as this can come to a manifest written
        // before any of them existed. It opens, and it draws.
        let mut old = base.clone();
        for (path, key) in &all {
            if ALWAYS.contains(&format!("{}:{key}", shapely(path)).as_str()) {
                continue;
            }
            if let Some(serde_json::Value::Object(m)) = at_path(&mut old, path) {
                m.remove(key);
            }
        }
        let doc = load_chitra(&repack(&old)).expect("a manifest of only the oldest fields opens");
        assert!(
            doc.nodes().count() > 5,
            "with its layers still in it: {}",
            doc.nodes().count()
        );
        assert!(
            chitrakar_render::render(&doc).is_ok(),
            "and the page it describes draws"
        );
    }

    /// Every command there is, and then the file.
    ///
    /// A `.chitra` is the only thing between a session and the next one,
    /// so a field that does not survive it is work quietly lost — and it
    /// is lost silently, since nothing complains about a number that
    /// came back as its default. Written per feature, the check is one
    /// somebody has to remember to write; written over the shared
    /// fixture's every command, it is one a new `Command` runs into by
    /// itself. It found the softness of a picked region, which went out
    /// and came back as a hard edge before the field was added to the
    /// manifest's own test.
    ///
    /// A document, saved and loaded, is the same document: that is the
    /// whole claim, and it holds for the state each command leaves
    /// behind, not only for the one a test happened to build.
    #[test]
    fn every_command_survives_the_file() {
        let f = chitrakar_doc::fixture::everything();
        let mut checked = 0usize;
        for command in chitrakar_doc::fixture::every_command(&f) {
            let what = format!("{command:?}");
            let what = what
                .split_once(" {")
                .map_or(what.clone(), |(k, _)| k.into());
            let mut doc = f.doc.clone();
            if doc.apply(command).is_err() {
                continue;
            }
            let bytes = save_chitra(&doc).unwrap_or_else(|e| panic!("saving after {what}: {e}"));
            let back = load_chitra(&bytes).unwrap_or_else(|e| panic!("loading after {what}: {e}"));
            let (want, got) = (spelled_out(&doc), spelled_out(&back));
            assert!(
                want == got,
                "after {what} the file gave back a different document: {}",
                first_difference(&want, &got)
            );
            // And the page it describes, which the account above cannot
            // see: a resource is spelled out as *how many* bytes it has,
            // not as which, so pixels that came back changed — a picture
            // written in a colour type that loses something, an alpha
            // premultiplied on the way out and not on the way back —
            // would pass every line of it. The page is where that shows.
            let (there, here) = (
                chitrakar_render::render(&doc).unwrap(),
                chitrakar_render::render(&back).unwrap(),
            );
            assert_eq!(
                (there.width, there.height),
                (here.width, here.height),
                "after {what} the page came back a different size"
            );
            let mut worst = (0.0f32, 0usize);
            for (i, (p, q)) in there.pixels.iter().zip(&here.pixels).enumerate() {
                let d = (p.r - q.r)
                    .abs()
                    .max((p.g - q.g).abs())
                    .max((p.b - q.b).abs())
                    .max((p.a - q.a).abs());
                if d > worst.0 {
                    worst = (d, i);
                }
            }
            assert!(
                worst.0 < 1e-6,
                "after {what} the page came back different, by {} at pixel {}",
                worst.0,
                worst.1
            );
            checked += 1;
        }
        assert!(
            checked > 25,
            "only {checked} commands made it as far as the file"
        );
    }

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

    /// A page turned round and back saves to the bytes it started with.
    ///
    /// The whole point of writing a document in an order is that the same
    /// work is the same file: nothing can compare two saves otherwise,
    /// and a version control system reports a change where there is none.
    /// Turning the page had been quietly making one. Multiplying a
    /// transform by a quarter turn gives negative zeros where there were
    /// zeros, so a page turned right and then left came back to
    /// transforms *equal* to the ones it started with and written
    /// differently — every layer of it, in a file nothing had really
    /// changed. `Transform::compose` adds zero to each component now,
    /// which is a no-op on every value a transform can hold except that
    /// one.
    ///
    /// A mirror does the same thing for the same reason, so it is asked
    /// here too, and so is a straighten of nothing.
    #[test]
    fn a_page_turned_round_and_back_saves_to_the_same_bytes() {
        let f = chitrakar_doc::fixture::everything();
        let first = save_chitra(&f.doc).unwrap();
        for (what, there_and_back) in [
            (
                "turned a quarter right and a quarter left",
                vec![
                    Command::TurnCanvas { quarters: 1 },
                    Command::TurnCanvas { quarters: 3 },
                ],
            ),
            (
                "turned all the way round",
                vec![
                    Command::TurnCanvas { quarters: 2 },
                    Command::TurnCanvas { quarters: 2 },
                ],
            ),
            (
                "mirrored twice",
                vec![
                    Command::MirrorCanvas { across_x: true },
                    Command::MirrorCanvas { across_x: true },
                ],
            ),
            (
                "mirrored twice the other way",
                vec![
                    Command::MirrorCanvas { across_x: false },
                    Command::MirrorCanvas { across_x: false },
                ],
            ),
        ] {
            let mut doc = f.doc.clone();
            for cmd in there_and_back {
                doc.apply(cmd).unwrap();
            }
            assert_eq!(
                save_chitra(&doc).unwrap(),
                first,
                "a page {what} is the same work, so it is the same file"
            );
        }
    }

    /// The same document saves to the same bytes.
    ///
    /// A `.chitra` is a manifest and one file per resource, and both used
    /// to come out in whatever order a hash map handed them over — which
    /// is a fresh order every run, so a document saved twice was two
    /// different files holding the same work. Nothing read one wrongly;
    /// nothing could compare two either, and a version control system
    /// saw a change where there was none.
    ///
    /// A hash map's order is stable *within* one run, so this cannot
    /// catch the symptom by saving twice here. It asks the thing that
    /// makes the symptom impossible instead: that what goes on the page
    /// is in order.
    #[test]
    fn a_saved_document_is_written_in_an_order() {
        let mut doc = Document::new(60, 40, ColorMode::Rgb);
        let root = doc.root();
        // Enough nodes that a hash map would have something to shuffle,
        // and ids well past ten so that "in order" and "in the order the
        // strings sort" are different answers.
        for i in 0..14 {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(Node::group(&format!("layer {i}"))),
            })
            .unwrap();
        }
        for i in 0..4u8 {
            let px: Vec<u8> = (0..16).map(|n| n as u8 ^ i).collect();
            doc.add_resource(2, 2, px);
        }

        let text = serde_json::to_string(&doc).unwrap();
        // The keys of an object, in the order they were written.
        let order = |key: &str| -> Vec<String> {
            let at = text
                .find(&format!("\"{key}\":{{"))
                .expect("the object is there");
            let body = &text[at + key.len() + 4..];
            let mut depth = 0i32;
            let mut keys = Vec::new();
            let mut chars = body.char_indices();
            while let Some((i, c)) = chars.next() {
                match c {
                    '{' => depth += 1,
                    '}' if depth == 0 => break,
                    '}' => depth -= 1,
                    '"' if depth == 0 => {
                        let rest = &body[i + 1..];
                        let end = rest.find('"').expect("a closing quote");
                        keys.push(rest[..end].to_string());
                        for _ in 0..end + 1 {
                            chars.next();
                        }
                    }
                    _ => {}
                }
            }
            keys
        };

        let nodes = order("nodes");
        assert_eq!(nodes.len(), 15, "the root and fourteen layers: {nodes:?}");
        let mut want: Vec<u64> = nodes.iter().map(|k| k.parse().unwrap()).collect();
        let written = want.clone();
        want.sort();
        assert_eq!(written, want, "the nodes are written in order of their ids");
        assert!(
            written.contains(&12)
                && written.iter().position(|&n| n == 2) < written.iter().position(|&n| n == 12),
            "in order of the number rather than of the text: {written:?}"
        );

        let kids = order("children");
        let mut sorted: Vec<u64> = kids.iter().map(|k| k.parse().unwrap()).collect();
        let as_written = sorted.clone();
        sorted.sort();
        assert_eq!(as_written, sorted, "and so are their children");

        // The resources, and with them the files the container writes.
        let names: Vec<&String> = doc.resources().map(|(id, _)| id).collect();
        let mut want = names.clone();
        want.sort();
        assert_eq!(names, want, "the resources are handed out in order");
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
                clip: None,
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
                        clip: None,
                    }],
                },
                invert: false,
                feather: 0.0,
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

        // A region picked out of the page, and a stroke confined to one.
        // Both are document state that nothing else carries, and a file
        // that loses either loses work: the region silently, the stroke
        // by letting paint out of where it was held.
        doc.apply(Command::SetSelection {
            selection: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: chitrakar_doc::VectorShape::Ellipse { rx: 22.0, ry: 14.0 },
                    transform: chitrakar_doc::Transform::translation(50.0, 40.0),
                },
                invert: true,
                feather: 2.5,
            })),
        })
        .unwrap();
        let held = add(&mut doc, Box::new(Node::paint("held")));
        doc.apply(Command::AddStroke {
            id: held,
            index: 0,
            stroke: Box::new(chitrakar_doc::PaintStroke {
                points: vec![[10.0, 20.0], [100.0, 30.0]],
                radii: vec![6.0],
                color: red,
                softness: 0.0,
                erase: false,
                source: [0.0, 0.0],
                heal: false,
                clip: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Vector {
                        shape: chitrakar_doc::VectorShape::Rect {
                            width: 40.0,
                            height: 60.0,
                            radius: 0.0,
                        },
                        transform: chitrakar_doc::Transform::default(),
                    },
                    invert: false,
                    feather: 0.0,
                })),
            }),
            on_mask: false,
        })
        .unwrap();

        let before = chitrakar_render::render(&doc).unwrap();
        let back = load_chitra(&save_chitra(&doc).unwrap()).unwrap();
        assert!(
            matches!(
                back.selection().map(|m| (&m.kind, m.invert)),
                Some((chitrakar_doc::MaskKind::Vector { .. }, true))
            ),
            "what was picked out came back, inside out as it went"
        );
        assert_eq!(
            back.selection().map(|m| m.feather),
            Some(2.5),
            "with its edge as soft as it was: a softness is a number \
             rather than a shape, and the easiest thing in a region to \
             drop on the way through a file"
        );
        assert!(
            matches!(
                &back.node(held).unwrap().kind,
                chitrakar_doc::NodeKind::Paint { strokes }
                    if strokes[0].clip.is_some()
            ),
            "and the stroke is still held to the region it was painted in"
        );
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
                clip: None,
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
