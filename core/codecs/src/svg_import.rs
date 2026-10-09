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
    /// The pixels of the soft masks groups wear (`ImportedMask`), which
    /// want pooling just as pictures do; a group's raster mask names one
    /// by its `key` until it is (`into_commands`).
    pub masks: Vec<ImportedMask>,
    /// The groups the file's layers stay in, where flattening them away
    /// would change the picture (`ImportedGroup`).
    pub groups: Vec<ImportedGroup>,
    /// Which of `groups` each of `shapes` is in, if any.
    pub shape_groups: Vec<Option<usize>>,
}

/// A group a file's layers come in inside rather than flattened: one
/// seen through a mask with grey in it, or faded or blended with more
/// than one layer under it. A fade does not distribute over the layers under it
/// the way a clip does — two overlapping layers each faded to half are
/// three quarters where they overlap, one group faded to half is half —
/// so the mask and the fade go on the group, over what its layers make
/// together, as a reader puts them.
pub struct ImportedGroup {
    pub name: String,
    pub mask: Option<chitrakar_doc::Mask>,
    /// How faded the group is, over what its layers make together.
    pub opacity: f32,
    /// How what its layers make together comes down on what is under it.
    pub blend: chitrakar_doc::BlendMode,
    /// The effects drawn from what its layers make together.
    pub effects: Vec<chitrakar_doc::Effect>,
    /// The group it is inside, if any.
    pub within: Option<usize>,
}

/// A greyscale mask a file's layers are seen through — a `<mask>` with
/// real grey in it, drawn — as the coverage it lets through in alpha
/// over white, which is what a raster mask reads as that coverage.
pub struct ImportedMask {
    /// What a layer's `MaskKind::Raster::resource_id` says until the
    /// pixels are pooled.
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl ImportedSvg {
    /// The commands that put the file's layers into `doc` under `parent`
    /// from `index` on, bottom first and in their groups, with every
    /// picture and every soft mask's pixels pooled in `doc` and referred
    /// to by the id it gave them. `first` is the id the first layer added
    /// will be given — `doc.peek_next_id()` unless commands ahead of these
    /// in the same batch add layers of their own.
    pub fn into_commands(
        self,
        doc: &mut chitrakar_doc::Document,
        parent: chitrakar_doc::NodeId,
        index: usize,
        first: chitrakar_doc::NodeId,
    ) -> Vec<chitrakar_doc::Command> {
        Gathered {
            shapes: self.shapes,
            within: self.shape_groups,
            pics: self.images,
            masks: self.masks,
            groups: self.groups,
        }
        .into_commands(doc, parent, index, first)
    }
}

/// What a walk of a file gathers: the shapes in painter's order and the
/// group each is in, the pictures with their places among them, the
/// groups, and the pixels of the soft masks they wear.
#[derive(Default)]
struct Gathered {
    shapes: Vec<Node>,
    within: Vec<Option<usize>>,
    pics: Vec<ImportedImage>,
    masks: Vec<ImportedMask>,
    groups: Vec<ImportedGroup>,
}

impl Gathered {
    fn into_commands(
        self,
        doc: &mut chitrakar_doc::Document,
        parent: chitrakar_doc::NodeId,
        index: usize,
        first: chitrakar_doc::NodeId,
    ) -> Vec<chitrakar_doc::Command> {
        let ids: std::collections::HashMap<String, String> = self
            .masks
            .into_iter()
            .map(|m| (m.key, doc.add_resource(m.width, m.height, m.rgba)))
            .collect();
        let pooled = |mask: &mut Option<chitrakar_doc::Mask>| {
            if let Some(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Raster { resource_id, .. },
                ..
            }) = mask
            {
                if let Some(id) = ids.get(resource_id.as_str()) {
                    *resource_id = id.clone();
                }
            }
        };
        // Every layer in painter's order with the group it is in.
        let mut pictures: Vec<(usize, Option<usize>, Node)> = self
            .pics
            .into_iter()
            .map(|pic| {
                let resource_id = doc.add_resource(pic.width, pic.height, pic.rgba);
                let mut node = Node::raster(
                    &pic.name,
                    chitrakar_doc::RasterRef {
                        resource_id,
                        width: pic.width,
                        height: pic.height,
                    },
                );
                node.transform = pic.transform;
                node.opacity = pic.opacity;
                node.blend = pic.blend;
                node.effects = pic.effects;
                node.mask = pic.clip;
                (pic.below, pic.within, node)
            })
            .collect();
        pictures.reverse();
        let mut order: Vec<(Option<usize>, Node)> = Vec::new();
        for (i, (shape, within)) in self.shapes.into_iter().zip(self.within).enumerate() {
            while pictures.last().is_some_and(|(below, _, _)| *below <= i) {
                order.extend(pictures.pop().map(|(_, w, pic)| (w, pic)));
            }
            order.push((within, shape));
        }
        while let Some((_, w, pic)) = pictures.pop() {
            order.push((w, pic));
        }
        // Each layer goes into its group, the groups made as they are
        // first needed, inside the groups they are in.
        let mut next = first.0;
        let mut cmds = Vec::new();
        let mut top = index;
        // The groups open now, outermost first: which, its id, how many
        // layers it holds so far.
        let mut open: Vec<(usize, chitrakar_doc::NodeId, usize)> = Vec::new();
        let mut add = |cmds: &mut Vec<chitrakar_doc::Command>,
                       open: &mut Vec<(usize, chitrakar_doc::NodeId, usize)>,
                       mut node: Node| {
            pooled(&mut node.mask);
            let (under, at) = match open.last_mut() {
                Some((_, id, n)) => {
                    *n += 1;
                    (*id, *n - 1)
                }
                None => {
                    top += 1;
                    (parent, top - 1)
                }
            };
            cmds.push(chitrakar_doc::Command::AddNode {
                parent: under,
                index: at,
                node: Box::new(node),
            });
            next += 1;
            chitrakar_doc::NodeId(next - 1)
        };
        for (within, node) in order {
            let mut chain = Vec::new();
            let mut at = within;
            while let Some(g) = at {
                chain.push(g);
                at = self.groups[g].within;
            }
            chain.reverse();
            let kept = open
                .iter()
                .zip(&chain)
                .take_while(|((g, _, _), c)| g == *c)
                .count();
            open.truncate(kept);
            for &g in &chain[kept..] {
                let mut group = Node::group(&self.groups[g].name);
                group.mask = self.groups[g].mask.clone();
                group.opacity = self.groups[g].opacity;
                group.blend = self.groups[g].blend;
                group.effects = self.groups[g].effects.clone();
                let id = add(&mut cmds, &mut open, group);
                open.push((g, id, 0));
            }
            add(&mut cmds, &mut open, node);
        }
        cmds
    }
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
    /// Which of the file's groups it is in, if any (`ImportedGroup`).
    pub within: Option<usize>,
    /// How it comes down on what is under it.
    pub blend: chitrakar_doc::BlendMode,
    /// The effects drawn from it.
    pub effects: Vec<chitrakar_doc::Effect>,
    /// What it is seen through, where the file put it inside a clip.
    pub clip: Option<chitrakar_doc::Mask>,
}

/// Bring an SVG in as shape layers.
pub fn import_svg(data: &[u8]) -> Result<ImportedSvg, String> {
    // A compressed file is opened here rather than by the reader, so that
    // what it says is asked about like any other file's, and so that a
    // few kilobytes cannot unpack into more memory than there is.
    let inflated;
    let data = if data.starts_with(&[0x1f, 0x8b]) {
        inflated = inflate_capped(data)?;
        &inflated[..]
    } else {
        data
    };
    if let Some(why) = past_reason(data) {
        return Err(why);
    }
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_font_data(FACE.to_vec());
    // Text in a face the file cannot supply is set in the bundled one.
    opt.font_family = "DejaVu Sans".to_string();
    let tree = usvg::Tree::from_data(data, &opt).map_err(|e| e.to_string())?;
    let mut got = Gathered::default();
    walk(tree.root(), 1.0, None, Place::default(), None, &mut got);
    Ok(ImportedSvg {
        width: tree.size().width(),
        height: tree.size().height(),
        shapes: got.shapes,
        shape_groups: got.within,
        images: got.pics,
        masks: got.masks,
        groups: got.groups,
    })
}

/// The most an SVG file is unpacked to: far past any drawing, and short
/// of what a few kilobytes made to unpack for ever would ask for.
const INFLATED_MOST: u64 = 256 << 20;

fn inflate_capped(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(INFLATED_MOST + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("the compressed file could not be opened: {e}"))?;
    if out.len() as u64 > INFLATED_MOST {
        return Err("the compressed file unpacks to more than any drawing holds".into());
    }
    Ok(out)
}

/// Past this, a number in an SVG is not a coordinate anybody drew: a
/// billion pixels is a drawing a few kilometres across at a screen's
/// resolution. And it is where the reader stops being able to cope — a
/// curve with a point out near 10²⁰ hung its text layout for good — so it
/// is asked about here, before the reader is handed the file.
const SVG_NUMBER_MOST: f64 = 1e9;

/// How lopsided a picture's box may be before the reader cannot place it:
/// one about a billion times wider than tall stopped it outright.
const IMAGE_SHAPE_MOST: f64 = 1e6;

/// Why a file is refused before the reader sees it, if it is.
///
/// A file can say anything, and the reader believes it: a number past
/// anything a drawing holds hung it, and a picture shaped like a thread
/// panicked it — and in the browser there is no catching a panic, the
/// engine stops. What it cannot be handed is asked about here instead:
/// the numbers in every attribute and style sheet, leaving out what is
/// not a number at all — an id, a class, a link, a colour written in
/// hex, a picture's own data, and the attributes other programs keep
/// for themselves.
fn past_reason(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    let options = usvg::roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    // What will not parse here will not parse for the reader either,
    // which says so in its own words.
    let doc = usvg::roxmltree::Document::parse_with_options(text, options).ok()?;
    let not_numbers = [
        "id",
        "class",
        "href",
        "lang",
        "title",
        "font-family",
        "version",
    ];
    for node in doc.descendants().filter(|n| n.is_element()) {
        for a in node.attributes() {
            let name = a.name();
            if a.namespace()
                .is_some_and(|ns| ns != "http://www.w3.org/2000/svg")
                || not_numbers.contains(&name)
                || name.starts_with("aria-")
                || name.starts_with("data-")
                || a.value().trim_start().starts_with("data:")
            {
                continue;
            }
            if let Some(x) =
                numbers_in(a.value()).find(|x| !x.is_finite() || x.abs() > SVG_NUMBER_MOST)
            {
                return Some(format!(
                    "this file says {x:e} in `{name}`, which is past anything a drawing holds"
                ));
            }
        }
        if node.tag_name().name() == "style" {
            if let Some(x) = node
                .descendants()
                .filter_map(|n| n.text())
                .flat_map(numbers_in)
                .find(|x| !x.is_finite() || x.abs() > SVG_NUMBER_MOST)
            {
                return Some(format!(
                    "this file's style sheet says {x:e}, which is past anything a drawing holds"
                ));
            }
        }
        if node.tag_name().name() == "image" {
            let side = |n: &str| node.attribute(n).and_then(|v| numbers_in(v).next());
            if let (Some(w), Some(h)) = (side("width"), side("height")) {
                let (w, h) = (w.abs(), h.abs());
                if w > 0.0 && h > 0.0 && (w / h).max(h / w) > IMAGE_SHAPE_MOST {
                    return Some(format!(
                        "this file has a picture {w} by {h}, which cannot be placed"
                    ));
                }
            }
        }
    }
    None
}

/// The numbers written in an attribute or a style sheet: a sign, digits,
/// a point and an exponent, standing on their own — not the tail of a
/// name, and not a colour in hex.
fn numbers_in(text: &str) -> impl Iterator<Item = f64> + '_ {
    let b = text.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            let starts = b[i].is_ascii_digit()
                || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
                || ((b[i] == b'-' || b[i] == b'+')
                    && b.get(i + 1)
                        .is_some_and(|c| c.is_ascii_digit() || *c == b'.'));
            let stands = i == 0
                || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'#' || b[i - 1] == b'_');
            if !(starts && stands) {
                i += 1;
                continue;
            }
            let s = i;
            if b[i] == b'-' || b[i] == b'+' {
                i += 1;
            }
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            // A run of hex digits after this is a colour or a name, not
            // a number with something after it.
            if i < b.len() && b[i].is_ascii_alphanumeric() && !matches!(b[i], b'e' | b'E') {
                let hexish = b[i..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
                if hexish > 0 && text[s..i].chars().all(|c| c.is_ascii_hexdigit()) {
                    i += hexish;
                    continue;
                }
            }
            if let Ok(x) = text[s..i].parse::<f64>() {
                return Some(x);
            }
        }
        None
    })
}

/// The region a `mask` lets through, where the mask is only a region.
///
/// An SVG mask is greyscale: coverage is the luminance (or the alpha) of
/// whatever is drawn in it, so a mask painted in grey fades what it
/// covers and a mask with a gradient in it fades it unevenly. That comes
/// in as a raster mask on a group of its own (`soft_mask`, `walk`).
///
/// What is carried as a region instead is the common case: a mask drawn
/// as opaque white shapes, which is a region and nothing more. Used that
/// way a mask is a clip with a different spelling, and it stays one — an
/// outline that can be edited, and no group the file did not need.
///
/// Every condition below is a guard rather than a nicety: fail any of
/// them and the answer is `None`, which is exactly what happened before
/// this existed. A fill that is not white, not opaque, or a gradient; a
/// group or shape that is faded; anything with an effect or a stroke on
/// it — each of those is grey somewhere, and a region would be wrong
/// about it in the direction of showing too much.
fn mask_rings(group: &usvg::Group) -> Option<Vec<Vec<[f32; 2]>>> {
    mask_region(group.mask()?, group.abs_transform())
}

/// One mask's region placed by `at`, the referring group's placement, and
/// narrowed by the mask on it, if that is a region too.
///
/// The mask on a mask is usvg's `Mask::mask`, which a reader applies in
/// the same space as the first. It used to be looked for on the mask's
/// *contents* instead, where usvg never puts it, so it was never applied.
fn mask_region(m: &usvg::Mask, at: usvg::Transform) -> Option<Vec<Vec<[f32; 2]>>> {
    if !region_only(m.root(), m.kind()) {
        return None;
    }
    let mut rings: Vec<Vec<[f32; 2]>> = Vec::new();
    collect_clip(m.root(), &mut rings);
    // Nothing drawn in it lets nothing through.
    if rings.is_empty() {
        return Some(Vec::new());
    }
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
    // A mask has a region of its own outside which it lets nothing
    // through, whatever is drawn in it.
    let r = m.rect();
    let bounds = vec![vec![
        [r.left(), r.top()],
        [r.right(), r.top()],
        [r.right(), r.bottom()],
        [r.left(), r.bottom()],
    ]];
    acc = placed(narrowed(acc, bounds)?, at);
    // And a mask on the mask narrows it again.
    if let Some(inner) = m.mask().and_then(|inner| mask_region(inner, at)) {
        acc = narrowed(acc, inner)?;
    }
    Some(acc)
}

/// A clip's or a mask's region carried from the space of the group that
/// refers to it to the document's. usvg leaves what is inside a clip
/// path or a mask in that group's own user space — the one clip path can
/// serve any number of groups, each placed differently — so the region
/// came out where the group would be with no placement at all: a frame
/// exported and brought back in was cut to a rectangle at the page's
/// corner rather than its own.
fn placed(rings: Vec<Vec<[f32; 2]>>, t: usvg::Transform) -> Vec<Vec<[f32; 2]>> {
    rings
        .into_iter()
        .map(|ring| {
            ring.into_iter()
                .map(|[x, y]| {
                    let mut p = usvg::tiny_skia_path::Point::from_xy(x, y);
                    t.map_point(&mut p);
                    [p.x, p.y]
                })
                .collect()
        })
        .collect()
}

/// Whether everything in a mask is fully showing, so its coverage is a
/// yes or a no rather than a shade.
fn region_only(group: &usvg::Group, kind: usvg::MaskType) -> bool {
    if group.opacity().get() < 1.0 || !group.filters().is_empty() {
        return false;
    }
    group.children().iter().all(|child| match child {
        usvg::Node::Group(g) => region_only(g, kind),
        usvg::Node::Path(p) => {
            if !p.is_visible() || p.stroke().is_some() {
                return false;
            }
            let Some(f) = p.fill() else { return false };
            if f.opacity().get() < 1.0 {
                return false;
            }
            match f.paint() {
                // A luminance mask reads how light the paint is, so only
                // white shows everything; an alpha mask does not care
                // what colour it is, only that it is opaque.
                usvg::Paint::Color(c) => {
                    kind == usvg::MaskType::Alpha || (c.red, c.green, c.blue) == (255, 255, 255)
                }
                _ => false,
            }
        }
        // Text and pictures in a mask are shades as far as this is
        // concerned: an outline is antialiased and a photograph is grey
        // nearly everywhere.
        usvg::Node::Text(_) | usvg::Node::Image(_) => false,
    })
}

/// A region narrowed to where another one also is: no rings where the
/// two do not meet — what shows through both is nothing — and left as it
/// was where the meeting cannot be traced, which is the one case it is
/// not known. Always `Some`; an `Option` so a caller can pass it on.
///
/// Two regions that do not meet used to leave the region as it was, so a
/// mask drawn wholly outside its own rectangle — or a clip path cut by a
/// clip it does not meet — let everything through where it lets nothing.
fn narrowed(region: Vec<Vec<[f32; 2]>>, by: Vec<Vec<[f32; 2]>>) -> Option<Vec<Vec<[f32; 2]>>> {
    use chitrakar_render::boolean::{combine_or_nudge, BoolOp};
    if region.is_empty() || by.is_empty() {
        return Some(Vec::new());
    }
    Some(combine_or_nudge(&region, &by, BoolOp::Intersect).unwrap_or(region))
}

/// The region a group is seen through, in document space, or `None` for
/// a group with no clip; an empty region shows nothing.
///
/// A clip path is a set of outlines and the content shows where they
/// cover — usvg has already resolved which outlines and put the whole
/// placement into their absolute transforms, so what comes back here is
/// polygons in the same space the shapes are in. `clipPathUnits`,
/// nesting on the clip itself and a clip referring to another clip are
/// all resolved by then as well; a clip *on* the clip path narrows it,
/// which is an intersection like any other.
fn clip_rings(group: &usvg::Group) -> Option<Vec<Vec<[f32; 2]>>> {
    clip_region(group.clip_path()?, group.abs_transform())
}

/// One clip path's region placed by `at`, the referring group's
/// placement, and narrowed by the clip path on it.
///
/// The clip on a clip path is usvg's `ClipPath::clip_path`, applied by a
/// reader in the referring group's space — without the first clip path's
/// own `transform`. It used to be looked for on the clip path's
/// *contents*, where usvg never puts it, so a clip path cut by another
/// let through everything the first one did.
fn clip_region(cp: &usvg::ClipPath, at: usvg::Transform) -> Option<Vec<Vec<[f32; 2]>>> {
    let mut rings: Vec<Vec<[f32; 2]>> = Vec::new();
    collect_clip(cp.root(), &mut rings);
    // A clip path with no outlines in it shows nothing.
    if rings.is_empty() {
        return Some(Vec::new());
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
    // The clip path's own `transform` goes inside the group's placement.
    acc = placed(acc, at.pre_concat(cp.transform()));
    // And a clip on the clip path narrows what it lets through.
    if let Some(inner) = cp.clip_path().and_then(|inner| clip_region(inner, at)) {
        acc = narrowed(acc, inner)?;
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

/// A filter's effects, said the engine's way, or `None` when any of it is
/// something else. Read off the graph of primitives three ways, which are
/// the three ways a shadow arrives: as `feDropShadow`; as the chain this
/// editor's exporter writes for each of its effects, merged under and
/// over the source (`svg::effect_filter`); and as the chain design tools
/// export — the alpha taken, offset and blurred, given its colour by a
/// colour matrix, and blended in under the source. Anything else, and
/// the filter is passed over as every filter was before: a layer that
/// comes in plainer than the file, never wrong in some other way.
///
/// `at` is where the filtered group stands: offsets and blurs are in its
/// user space, and the engine's are in the page's.
///
/// With them, the fade of the layer they were drawn from, where an
/// outline says what it was. An outline's edge is where its layer is half
/// as covered as its opacity, and the exporter writes that edge into the
/// filter (`svg::effect_filter`) — while a shape's opacity goes to SVG as
/// the fade of each paint, which comes back as the colours' own. Read back
/// from the edge, the fade can be given back to the layer, which is where
/// the engine measures the edge from.
fn effects_of(
    group: &usvg::Group,
    at: usvg::Transform,
) -> Option<(Vec<chitrakar_doc::Effect>, Option<f32>)> {
    use chitrakar_doc::Effect;
    use usvg::filter::{Input, Kind};
    let mut out = Vec::new();
    let page = Page {
        at,
        edge: std::cell::Cell::new(None),
    };
    for filter in group.filters() {
        let prims = filter.primitives();
        let last = prims.last()?;
        let named = |input: &Input| -> Option<&usvg::filter::Primitive> {
            match input {
                Input::Reference(name) => prims.iter().rev().find(|p| p.result() == name),
                _ => None,
            }
        };
        match last.kind() {
            Kind::DropShadow(d) if *d.input() == Input::SourceGraphic => {
                let (dx, dy) = page.offset(d.dx(), d.dy());
                out.push(Effect::DropShadow {
                    dx,
                    dy,
                    blur: page.blur((d.std_dev_x().get() + d.std_dev_y().get()) / 2.0),
                    color: color_of(d.color(), 1.0),
                    opacity: d.opacity().get(),
                });
            }
            // This editor's own: what goes under the source, the source,
            // and what goes over it.
            Kind::Merge(m) => {
                let at_source = m.inputs().iter().position(|i| *i == Input::SourceGraphic)?;
                for (k, input) in m.inputs().iter().enumerate() {
                    if k == at_source {
                        continue;
                    }
                    let fx = page.tinted(named(input)?, &named, k > at_source)?;
                    out.push(fx);
                }
            }
            // A design tool's: the source blended in over the shadows,
            // each blended in over the ones before it, down to nothing.
            Kind::Blend(b)
                if *b.input1() == Input::SourceGraphic && b.mode() == usvg::BlendMode::Normal =>
            {
                let mut shadows = Vec::new();
                let mut at_blend = named(b.input2())?;
                loop {
                    match at_blend.kind() {
                        Kind::Blend(b) if b.mode() == usvg::BlendMode::Normal => {
                            shadows.push(page.tinted(named(b.input1())?, &named, false)?);
                            match named(b.input2()) {
                                Some(below) => at_blend = below,
                                None => break,
                            }
                        }
                        // Where the chain starts: nothing at all.
                        Kind::Flood(f) if f.opacity().get() <= 0.0 => break,
                        // Or a shadow on its own, with nothing under it.
                        _ => {
                            shadows.push(page.tinted(at_blend, &named, false)?);
                            break;
                        }
                    }
                }
                shadows.reverse();
                out.extend(shadows);
            }
            _ => return None,
        }
    }
    // Half covered at full opacity is no fade at all.
    let fade = page
        .edge
        .get()
        .map(|edge| (2.0 * edge).clamp(0.0, 1.0))
        .filter(|fade| *fade > 1e-3 && *fade < 0.999);
    Some((out, fade))
}

/// A filter's own space carried to the page's.
struct Page {
    at: usvg::Transform,
    /// Where an outline among the effects puts its edge, as a coverage.
    edge: std::cell::Cell<Option<f32>>,
}

impl Page {
    fn offset(&self, dx: f32, dy: f32) -> (f32, f32) {
        let t = self.at;
        (t.sx * dx + t.kx * dy, t.ky * dx + t.sy * dy)
    }

    /// The engine's blur for a reader's standard deviation in the
    /// filter's space: the one whose spread — what the exporter writes
    /// for it (`svg::effect_filter`) — is nearest, so an effect that went
    /// out comes back as itself. The engine blurs by three boxes of one
    /// radius, `r`, and spreads by `√(r(r+1))`.
    fn blur(&self, std_dev: f32) -> f32 {
        let t = self.at;
        let scale = (t.sx.hypot(t.ky)).max(t.kx.hypot(t.sy)).max(1e-6);
        let s = std_dev * scale;
        if s <= 0.01 {
            return 0.0;
        }
        let spread = |r: f32| (r * (r + 1.0)).sqrt();
        // `√(r(r+1))` is within a sixteenth of `r + ½` from one up, so
        // the nearest radius is there or beside it. Counted up from one,
        // a blur a file said was a billion wide was a billion steps.
        let guess = (s - 0.5).round().max(1.0);
        let r = [guess - 1.0, guess, guess + 1.0]
            .into_iter()
            .filter(|r| *r >= 1.0)
            .min_by(|a, b| (spread(*a) - s).abs().total_cmp(&(spread(*b) - s).abs()))
            .unwrap_or(1.0);
        // The sigma the engine turns into that radius: its box width is
        // the W3C's, `⌊σ·3√(2π)/4 + ½⌋`, halved; `2r` halves to `r`.
        let k = 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0;
        2.0 * r / k / scale
    }

    /// One effect from the primitive that makes it, coloured: a flood
    /// cut to a silhouette (this editor's), or a colour matrix laying a
    /// colour over one (a design tool's).
    fn tinted<'a>(
        &self,
        p: &'a usvg::filter::Primitive,
        named: &dyn Fn(&usvg::filter::Input) -> Option<&'a usvg::filter::Primitive>,
        over: bool,
    ) -> Option<chitrakar_doc::Effect> {
        use chitrakar_doc::Effect;
        use usvg::filter::{ColorMatrixKind, CompositeOperator, Input, Kind};
        match p.kind() {
            // Cut to the source again: an inner shadow, which sits over.
            Kind::Composite(c)
                if over
                    && c.operator() == CompositeOperator::In
                    && *c.input2() == Input::SourceAlpha =>
            {
                let (color, opacity, shape) = self.flooded(named(c.input1())?, named)?;
                let (dx, dy, blur, inverted, grown) = self.shaped(shape, named)?;
                (inverted && grown.is_none()).then_some(Effect::InnerShadow {
                    dx,
                    dy,
                    blur,
                    color,
                    opacity,
                })
            }
            Kind::Composite(_) if !over => {
                let (color, opacity, shape) = self.flooded(p, named)?;
                let (dx, dy, blur, inverted, grown) = self.shaped(shape, named)?;
                if inverted {
                    return None;
                }
                Some(match grown {
                    Some(width) if dx == 0.0 && dy == 0.0 && blur == 0.0 => Effect::Outline {
                        width,
                        color,
                        opacity,
                    },
                    Some(_) => return None,
                    None => Effect::DropShadow {
                        dx,
                        dy,
                        blur,
                        color,
                        opacity,
                    },
                })
            }
            // A colour matrix laying one colour over a shadow's alpha.
            Kind::ColorMatrix(m) if !over => {
                let ColorMatrixKind::Matrix(v) = m.kind() else {
                    return None;
                };
                let only = |row: usize, keep: usize| {
                    (0..4).all(|c| c == keep || v[row * 5 + c].abs() < 1e-6)
                };
                if !(only(0, 9) && only(1, 9) && only(2, 9) && only(3, 3)) {
                    return None;
                }
                let channel = |c: f32| match p.color_interpolation() {
                    usvg::filter::ColorInterpolation::SRGB => c,
                    usvg::filter::ColorInterpolation::LinearRGB => {
                        chitrakar_color::linear_to_srgb(c.clamp(0.0, 1.0))
                    }
                };
                let color = AuthoredColor::Srgb {
                    r: channel(v[4]).clamp(0.0, 1.0),
                    g: channel(v[9]).clamp(0.0, 1.0),
                    b: channel(v[14]).clamp(0.0, 1.0),
                    a: 1.0,
                };
                let opacity = v[18].clamp(0.0, 1.0);
                let (dx, dy, blur, inverted, grown) = self.shaped(named(m.input())?, named)?;
                (!inverted && grown.is_none()).then_some(Effect::DropShadow {
                    dx,
                    dy,
                    blur,
                    color,
                    opacity,
                })
            }
            _ => None,
        }
    }

    /// A flood cut to a shape — `feFlood` then `feComposite in` with the
    /// shape second — as its colour, its opacity and the shape.
    fn flooded<'a>(
        &self,
        p: &'a usvg::filter::Primitive,
        named: &dyn Fn(&usvg::filter::Input) -> Option<&'a usvg::filter::Primitive>,
    ) -> Option<(AuthoredColor, f32, &'a usvg::filter::Primitive)> {
        use usvg::filter::{CompositeOperator, Kind};
        let Kind::Composite(c) = p.kind() else {
            return None;
        };
        if c.operator() != CompositeOperator::In {
            return None;
        }
        let Kind::Flood(f) = named(c.input1())?.kind() else {
            return None;
        };
        Some((
            color_of(f.color(), 1.0),
            f.opacity().get(),
            named(c.input2())?,
        ))
    }

    /// The silhouette a shadow is made from, walked back to the alpha it
    /// starts at: how far it is moved, how much it is blurred, whether
    /// the alpha was turned inside out (an inner shadow's) and how far it
    /// was grown (an outline's). A step that is not one of those, and the
    /// answer is `None`.
    fn shaped<'a>(
        &self,
        mut p: &'a usvg::filter::Primitive,
        named: &dyn Fn(&usvg::filter::Input) -> Option<&'a usvg::filter::Primitive>,
    ) -> Option<(f32, f32, f32, bool, Option<f32>)> {
        use usvg::filter::{ColorMatrixKind, CompositeOperator, Input, Kind, TransferFunction};
        let (mut dx, mut dy, mut blur, mut inverted, mut grown) = (0.0, 0.0, 0.0, false, None);
        for _ in 0..16 {
            let input = match p.kind() {
                Kind::Offset(o) => {
                    let (x, y) = self.offset(o.dx(), o.dy());
                    dx += x;
                    dy += y;
                    o.input()
                }
                Kind::GaussianBlur(g) => {
                    blur = self.blur((g.std_dev_x().get() + g.std_dev_y().get()) / 2.0);
                    g.input()
                }
                Kind::Morphology(m) if m.operator() == usvg::filter::MorphologyOperator::Dilate => {
                    let t = self.at;
                    let scale = (t.sx.hypot(t.ky)).max(t.kx.hypot(t.sy));
                    grown = Some((m.radius_x().get() + m.radius_y().get()) / 2.0 * scale);
                    m.input()
                }
                // A design tool knocks the shape out of its own shadow,
                // which under an opaque shape is nothing at all.
                Kind::Composite(c) if c.operator() == CompositeOperator::Out => c.input1(),
                Kind::ComponentTransfer(c) => {
                    let identity = |f: &TransferFunction| matches!(f, TransferFunction::Identity);
                    if !(identity(c.func_r()) && identity(c.func_g()) && identity(c.func_b())) {
                        return None;
                    }
                    match c.func_a() {
                        TransferFunction::Table(t)
                            if t.len() == 2 && t[0] == 1.0 && t[1] == 0.0 =>
                        {
                            inverted = true;
                        }
                        // The exporter's hard edge for an outline.
                        TransferFunction::Linear { slope, intercept } if *slope >= 100.0 => {
                            // Half way up the step is the edge.
                            self.edge.set(Some((0.5 - intercept) / slope));
                        }
                        _ => return None,
                    }
                    c.input()
                }
                // A design tool's hard alpha: the alpha alone, scaled up.
                Kind::ColorMatrix(m) => {
                    let ColorMatrixKind::Matrix(v) = m.kind() else {
                        return None;
                    };
                    let alpha_only = (0..15).all(|i| v[i].abs() < 1e-6)
                        && v[15].abs() < 1e-6
                        && v[16].abs() < 1e-6
                        && v[17].abs() < 1e-6
                        && v[18] > 0.0
                        && v[19].abs() < 1e-6;
                    if !alpha_only {
                        return None;
                    }
                    m.input()
                }
                _ => return None,
            };
            match input {
                Input::SourceAlpha => return Some((dx, dy, blur, inverted, grown)),
                Input::SourceGraphic => return None,
                Input::Reference(_) => p = named(input)?,
            }
        }
        None
    }
}

/// Whether anything under a group comes down by a blend of its own,
/// which usvg says on a group around it.
fn holds_a_blend(group: &usvg::Group) -> bool {
    group.children().iter().any(|child| match child {
        usvg::Node::Group(g) => g.blend_mode() != usvg::BlendMode::Normal || holds_a_blend(g),
        _ => false,
    })
}

/// A shape's fade given back to it as its opacity, out of the paint it
/// was folded into on the way to SVG (`Place::fade`).
fn unfade(node: &mut chitrakar_doc::Node, fade: f32) {
    if let chitrakar_doc::NodeKind::Vector {
        fill,
        stroke,
        gradient,
        ..
    } = &mut node.kind
    {
        let lift = |c: &mut AuthoredColor| {
            if let AuthoredColor::Srgb { a, .. } = c {
                *a = (*a / fade).min(1.0);
            }
        };
        if let Some(fill) = fill {
            lift(fill);
        }
        if let Some(stroke) = stroke {
            lift(&mut stroke.color);
        }
        if let Some(gradient) = gradient {
            for stop in gradient.stops_mut() {
                lift(&mut stop.color);
            }
        }
        node.opacity *= fade;
    }
}

/// Whether anything under a group wears a filter of its own. A group's
/// effects are drawn from what is under it as that comes out, its own
/// filters included — a copy outlined round a layer that casts a
/// shadow is outlined round the shadow too — so a group with effects
/// over one is a group, and the two do not land on one layer, where one
/// of them was lost.
fn holds_a_filter(group: &usvg::Group) -> bool {
    group.children().iter().any(|child| match child {
        usvg::Node::Group(g) => !g.filters().is_empty() || holds_a_filter(g),
        _ => false,
    })
}

/// Whether more than one layer would come in from under a group, which
/// is when a fade on it stops being any one layer's.
fn draws_more_than_one(group: &usvg::Group) -> bool {
    fn count(group: &usvg::Group, n: &mut usize) {
        for child in group.children() {
            if *n > 1 {
                return;
            }
            match child {
                usvg::Node::Group(g) => count(g, n),
                usvg::Node::Path(p) if p.is_visible() => *n += 1,
                usvg::Node::Image(i) if i.is_visible() => *n += 1,
                // A block of text comes in as an outline a glyph, so it
                // is more than one layer whatever it says: faded, or
                // casting a shadow, it does so as one.
                usvg::Node::Text(_) => *n += 2,
                _ => {}
            }
        }
    }
    let mut n = 0;
    count(group, &mut n);
    n > 1
}

/// Where what a walk finds goes: into which of the file's groups, if
/// any, and with what blend — a group's own, carried down to the one
/// layer under it when there is only one (`walk`).
#[derive(Clone, Default)]
struct Place {
    within: Option<usize>,
    blend: chitrakar_doc::BlendMode,
    /// A filter's effects, carried down to the one layer under it.
    effects: Vec<chitrakar_doc::Effect>,
    /// The fade the layer those effects belong to had, when an outline
    /// among them says so (`effects_of`): given back to it as its
    /// opacity, out of the paint it was folded into.
    fade: Option<f32>,
    /// Inside a text, whose outlines are in the text's own space and are
    /// placed by the text (`walk`).
    text: bool,
}

/// A blend as the document says it; the two lists are the same sixteen.
fn blend_of(mode: usvg::BlendMode) -> chitrakar_doc::BlendMode {
    use chitrakar_doc::BlendMode as B;
    match mode {
        usvg::BlendMode::Normal => B::Normal,
        usvg::BlendMode::Multiply => B::Multiply,
        usvg::BlendMode::Screen => B::Screen,
        usvg::BlendMode::Overlay => B::Overlay,
        usvg::BlendMode::Darken => B::Darken,
        usvg::BlendMode::Lighten => B::Lighten,
        usvg::BlendMode::ColorDodge => B::ColorDodge,
        usvg::BlendMode::ColorBurn => B::ColorBurn,
        usvg::BlendMode::HardLight => B::HardLight,
        usvg::BlendMode::SoftLight => B::SoftLight,
        usvg::BlendMode::Difference => B::Difference,
        usvg::BlendMode::Exclusion => B::Exclusion,
        usvg::BlendMode::Hue => B::Hue,
        usvg::BlendMode::Saturation => B::Saturation,
        usvg::BlendMode::Color => B::Color,
        usvg::BlendMode::Luminosity => B::Luminosity,
    }
}

/// A transform the way the document says one.
fn as_doc(t: usvg::Transform) -> chitrakar_doc::Transform {
    chitrakar_doc::Transform {
        a: t.sx,
        b: t.ky,
        c: t.kx,
        d: t.sy,
        e: t.tx,
        f: t.ty,
    }
}

/// `truth` is where `group` really stands when usvg's own word for it
/// cannot be taken: inside a pattern whose units it has resolved, it
/// wraps the contents in a group of its own and leaves everything under
/// that group placed as if it were not there (a TODO in usvg's
/// `push_pattern_transform`). Each layer under it is put right by the
/// difference between where it stands and where usvg says it does.
///
/// `place` is the group the layers here go into (`ImportedGroup`) and
/// the blend they come down with.
fn walk(
    group: &usvg::Group,
    opacity: f32,
    clip: Option<&Vec<Vec<[f32; 2]>>>,
    place: Place,
    truth: Option<usvg::Transform>,
    got: &mut Gathered,
) {
    let within = place.within;
    // What a layer standing at `said` is out by: the truth over what usvg
    // said, which a path and a text both take from the group they are in.
    let fix = |said: usvg::Transform| {
        let truth = truth?;
        Some(as_doc(truth.pre_concat(said.invert()?)))
    };
    // A fade on a group with one layer under it is that layer's own, and
    // goes into its colours as it always has; on more than one it is the
    // group's (`ImportedGroup`), and what is under it starts unfaded.
    let faded = group.opacity().get() < 1.0 && draws_more_than_one(group);
    // A blend is the same: on one layer it is that layer's, and on more
    // than one it is how what they make together comes down. (`mix-blend-
    // mode`, which this editor's own exporter writes, came back Normal.)
    let mine = blend_of(group.blend_mode());
    // It is carried down to a lone layer only where nothing under the
    // group blends as well: two blends, one inside the other, are two
    // steps, and one layer can wear only one of them.
    let blended = mine != chitrakar_doc::BlendMode::Normal
        && (draws_more_than_one(group) || holds_a_blend(group));
    // And a group that is isolated — `isolation:isolate`, which this
    // editor's exporter writes on a group whose layers blend, or a fade,
    // a clip or a mask, which isolate in every reader — with a blend
    // inside it keeps the blend inside it: flattened, the blend would
    // reach everything under the group.
    let isolating = group.should_isolate() && holds_a_blend(group) && draws_more_than_one(group);
    // A filter that is a shadow, an outline or an inner shadow comes in as
    // the engine's own effects (`effects_of`); on one layer they are that
    // layer's, and on more — or faded, since a fade takes the shadow with
    // it, or inside another such group — the group's. Any other filter is
    // passed over, as every filter was.
    let (fx, fade) = match (!group.filters().is_empty())
        .then(|| effects_of(group, truth.unwrap_or_else(|| group.abs_transform())))
        .flatten()
    {
        Some((fx, fade)) if !fx.is_empty() => (Some(fx), fade),
        _ => (None, None),
    };
    let effected = fx.is_some()
        && (draws_more_than_one(group)
            || !place.effects.is_empty()
            || group.opacity().get() < 1.0
            || holds_a_filter(group));
    let opacity = if faded {
        opacity
    } else {
        opacity * group.opacity().get()
    };
    // What this group is seen through, and everything under it with it. A
    // clip is the one thing about a group that survives the group being
    // flattened away: it multiplies coverage by nought or one, and that
    // distributes over the children exactly — each child shown only
    // inside the region is the same picture as the group shown only
    // inside it. Opacity and blending do not distribute that way, which
    // is why they are still folded into the colours instead.
    // A clip path and a mask are both "show only here", so they meet as
    // one region when the mask *is* a region (`mask_rings`). A mask with
    // grey in it does not distribute — it fades what the layers make
    // together, not each of them — so the group stays a group, wearing it
    // as a raster mask drawn from it with the region it is cut to as well
    // (`soft_mask`), and what is under it starts uncut.
    //
    // Two regions that do not meet show nothing, and what is under them
    // stays out (`narrowed`). The inner region used to be kept instead, so
    // a frame standing wholly outside the frame it sits in — hidden on the
    // page — came back whole. Only what cannot be traced keeps the inner
    // region, as before.
    let masked = mask_rings(group);
    let soft_here = match (group.mask(), &masked) {
        (Some(m), None) => Some(m),
        _ => None,
    };
    let here = match (clip_rings(group), masked) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => Some(only),
        (Some(a), Some(b)) => narrowed(b, a),
    };
    let clip = match (clip, here) {
        (None, None) => None,
        (Some(outer), None) => Some(outer.clone()),
        (None, Some(inner)) => Some(inner),
        // A clip inside a clip shows only what both show.
        (Some(outer), Some(inner)) => narrowed(inner, outer.clone()),
    };
    if clip.as_ref().is_some_and(|c| c.is_empty()) {
        return;
    }
    // A group kept for its effects, with an outline among them saying what
    // fade its layer had.
    let lifted = if effected { fade } else { None };
    let (clip, within, opacity) =
        if soft_here.is_some() || faded || blended || isolating || effected {
            let at = truth.unwrap_or_else(|| group.abs_transform());
            let mask = soft_here.and_then(|m| soft_mask(m, at, clip.as_ref(), got));
            // A group kept for its fade alone is still cut to its region, by
            // the region on each layer under it, which distributes.
            let clip = if soft_here.is_some() { None } else { clip };
            got.groups.push(ImportedGroup {
                name: if !group.id().is_empty() {
                    group.id().to_string()
                } else if soft_here.is_some() {
                    "Masked".to_string()
                } else if faded {
                    "Faded".to_string()
                } else if blended {
                    "Blended".to_string()
                } else if effected {
                    "Shadowed".to_string()
                } else {
                    "Group".to_string()
                },
                mask,
                // What was folded on the way down comes onto the group too,
                // and the fade its outline says its layer had, which the
                // paint under it was carrying (`effects_of`).
                opacity: if faded {
                    opacity * group.opacity().get()
                } else {
                    opacity
                } * lifted.unwrap_or(1.0),
                blend: if mine != chitrakar_doc::BlendMode::Normal {
                    mine
                } else {
                    place.blend
                },
                effects: fx.clone().unwrap_or_else(|| place.effects.clone()),
                within,
            });
            // What is under it comes in with that fade taken back out of
            // its paint. Exact for text, whose glyphs do not overlap; where
            // a fill and a stroke drawn as a second shape overlap, they
            // were two faded paints and are now one faded group, which is
            // a little lighter where both are — against an outline that
            // was otherwise measured from a fade it could not see.
            (
                clip,
                Some(got.groups.len() - 1),
                1.0 / lifted.unwrap_or(1.0),
            )
        } else {
            (clip, within, opacity)
        };
    // What is under this group goes in there, with the group's blend when
    // it is not a group of its own.
    let place = Place {
        within,
        blend: if within != place.within {
            chitrakar_doc::BlendMode::Normal
        } else if mine != chitrakar_doc::BlendMode::Normal {
            mine
        } else {
            place.blend
        },
        text: place.text,
        fade: if within != place.within {
            None
        } else if fx.is_some() {
            fade
        } else {
            place.fade
        },
        effects: if within != place.within {
            Vec::new()
        } else {
            fx.unwrap_or_else(|| place.effects.clone())
        },
    };
    let clip = clip.as_ref();
    let worn = clip.map(|c| mask_of(c));
    for child in group.children() {
        match child {
            usvg::Node::Group(g) => walk(
                g,
                opacity,
                clip,
                place.clone(),
                truth.map(|t| t.pre_concat(g.transform())),
                got,
            ),
            usvg::Node::Path(p) => {
                if p.is_visible() {
                    // A pattern fill comes in as a picture under the rest
                    // of the path (`pattern_picture`); a path with nothing
                    // else to it is that picture alone.
                    let patterned = match p.fill().map(|f| (f, f.paint())) {
                        Some((f, usvg::Paint::Pattern(pattern))) => {
                            let alpha = f.opacity().get() * opacity;
                            let below = got.shapes.len();
                            if let Some(mut pic) = pattern_picture(p, pattern, alpha, clip, below) {
                                pic.within = within;
                                pic.blend = place.blend;
                                pic.effects = place.effects.clone();
                                got.pics.push(pic);
                            }
                            true
                        }
                        _ => false,
                    };
                    if patterned && p.stroke().is_none() {
                        continue;
                    }
                    let said = if place.text {
                        usvg::Transform::default()
                    } else {
                        p.abs_transform()
                    };
                    let fixed = if place.text { None } else { fix(said) };
                    for (k, mut node) in shapes_of(p, said, opacity).into_iter().enumerate() {
                        node.mask = worn.clone();
                        node.blend = place.blend;
                        // A path that came in as a fill and a stroke over
                        // it casts its shadow once, from the first.
                        if k == 0 {
                            if let Some(fade) = place.fade {
                                unfade(&mut node, fade);
                            }
                            node.effects = place.effects.clone();
                        }
                        if let Some(f) = fixed {
                            node.transform = f.compose(node.transform);
                        }
                        got.shapes.push(node);
                        got.within.push(within);
                    }
                }
            }
            // Text arrives as the outlines usvg set it in — in the text's
            // own space, not the document's: what usvg flattens a text
            // into carries none of the text's placement or its groups'.
            // Every block came in where it would stand with none at all,
            // so text in a moved or turned group, or moved itself, landed
            // at the page's corner. The placement goes onto each outline
            // as its own transform, which carries its gradient and its
            // stroke with it; a clip is in the space the layer sits in and
            // stays where it is.
            usvg::Node::Text(t) => {
                // Walked with the text's own placement as the truth, so
                // each outline is put where the text stands whatever the
                // reader said it was: the reader's glyphs used to come in
                // the text's own space and its newer ones come placed,
                // while a decoration line can say either — and composing
                // the placement onto every one placed the newer glyphs
                // twice, turned text turned again.
                //
                // Every outline a text is set in is read in the text's own
                // space, whatever the reader says its transform is: the
                // reader's glyphs used to say none and its newer ones say
                // the text's, while its underlines still say none — and
                // read through what each said, the newer glyphs were
                // placed twice, a turned caption turned again.
                let before = got.shapes.len();
                let inside = Place {
                    text: true,
                    ..place.clone()
                };
                walk(t.flattened(), opacity, clip, inside, None, got);
                let a = t.abs_transform();
                let placed = match fix(a) {
                    Some(f) => f.compose(as_doc(a)),
                    None => as_doc(a),
                };
                for node in &mut got.shapes[before..] {
                    node.transform = placed.compose(node.transform);
                }
            }
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
                    usvg::ImageKind::SVG(tree) => {
                        walk(tree.root(), opacity, clip, place.clone(), None, got)
                    }
                    kind => {
                        if let Some(mut pic) = picture_of(img, kind, opacity, got.shapes.len()) {
                            pic.clip = worn.clone();
                            pic.within = within;
                            pic.blend = place.blend;
                            pic.effects = place.effects.clone();
                            got.pics.push(pic);
                        }
                    }
                }
            }
        }
    }
}

/// The most pixels a soft mask is drawn with before it is drawn coarser.
const SOFT_MOST: f32 = 16_000_000.0;

/// What a soft mask on a group placed at `at` lets through, with
/// `region` cut out of it as well, as a raster mask over the box both
/// reach — in document units, a pixel a unit unless that is more than
/// `SOFT_MOST` — whose pixels go into `got` to be pooled.
///
/// The mask is drawn as a reader draws it: its contents (brought in by
/// `walk`, like any of the file's) in the space of the group wearing it,
/// cut to the mask's own rectangle, and read as luminance times alpha on
/// the colours a device shows, or alpha alone; a mask on the mask
/// multiplies in.
fn soft_mask(
    m: &usvg::Mask,
    at: usvg::Transform,
    region: Option<&Vec<Vec<[f32; 2]>>>,
    got: &mut Gathered,
) -> Option<chitrakar_doc::Mask> {
    // The box every mask's rectangle and the region all reach.
    let mut reach = [f32::MIN, f32::MIN, f32::MAX, f32::MAX];
    let mut meet = |pts: &mut dyn Iterator<Item = [f32; 2]>| {
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in pts {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
        reach = [
            reach[0].max(b[0]),
            reach[1].max(b[1]),
            reach[2].min(b[2]),
            reach[3].min(b[3]),
        ];
    };
    meet(&mut rect_ring(m.rect(), at).into_iter());
    if let Some(r) = region {
        meet(&mut r.iter().flatten().copied());
    }
    let (x0, y0) = (reach[0].floor(), reach[1].floor());
    let (x1, y1) = (reach[2].ceil(), reach[3].ceil());
    let key = format!("svg-mask:{}", got.masks.len());
    let raster = |w: u32, h: u32, transform: chitrakar_doc::Transform| chitrakar_doc::Mask {
        kind: chitrakar_doc::MaskKind::Raster {
            resource_id: key.clone(),
            width: w,
            height: h,
            transform,
        },
        invert: false,
        feather: 0.0,
    };
    if !(x1 - x0 >= 1.0 && y1 - y0 >= 1.0 && (x1 - x0).is_finite() && (y1 - y0).is_finite()) {
        // Nothing reaches anywhere: a mask that lets nothing through.
        got.masks.push(ImportedMask {
            key: key.clone(),
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 0],
        });
        return Some(raster(1, 1, chitrakar_doc::Transform::default()));
    }
    // As many pixels as `SOFT_MOST` at most, and no side longer than a
    // page can be: held to the total alone, a mask a billion wide and a
    // few tall kept the total and asked for a row no machine has.
    let side = chitrakar_doc::MAX_CANVAS_SIDE as f32;
    let k = (SOFT_MOST / ((x1 - x0) * (y1 - y0)))
        .sqrt()
        .min(side / (x1 - x0))
        .min(side / (y1 - y0))
        .min(1.0);
    let (w, h) = (
        ((x1 - x0) * k).ceil().max(1.0) as u32,
        ((y1 - y0) * k).ceil().max(1.0) as u32,
    );
    // Document → the mask's pixels.
    let grid = usvg::Transform::from_scale(k, k).pre_translate(-x0, -y0);
    let mut cover = vec![1.0f32; (w * h) as usize];
    mask_cover(m, at, grid, w, h, &mut cover)?;
    if let Some(r) = region {
        let shown = draw_coverage(w, h, |doc| {
            let root = doc.root();
            let main = placed(r.clone(), grid);
            let mut node = Node::vector(
                "region",
                VectorShape::Path {
                    handles: vec![[0.0; 4]; main[0].len()],
                    points: main[0].clone(),
                    closed: true,
                    smooth: false,
                    subpaths: main[1..].to_vec(),
                },
            );
            if let NodeKind::Vector { fill, .. } = &mut node.kind {
                *fill = Some(AuthoredColor::Srgb {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                });
            }
            vec![chitrakar_doc::Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(node),
            }]
        })?;
        for (c, px) in cover.iter_mut().zip(&shown.pixels) {
            *c *= px.a.clamp(0.0, 1.0);
        }
    }
    got.masks.push(ImportedMask {
        key: key.clone(),
        width: w,
        height: h,
        rgba: cover
            .iter()
            .flat_map(|c| [255, 255, 255, (c.clamp(0.0, 1.0) * 255.0).round() as u8])
            .collect(),
    });
    Some(raster(
        w,
        h,
        chitrakar_doc::Transform {
            a: 1.0 / k,
            b: 0.0,
            c: 0.0,
            d: 1.0 / k,
            e: x0,
            f: y0,
        },
    ))
}

/// A rectangle in a group's space as the ring it is on the page.
fn rect_ring(r: usvg::NonZeroRect, at: usvg::Transform) -> Vec<[f32; 2]> {
    placed(
        vec![vec![
            [r.left(), r.top()],
            [r.right(), r.top()],
            [r.right(), r.bottom()],
            [r.left(), r.bottom()],
        ]],
        at,
    )
    .remove(0)
}

/// One mask's coverage multiplied into `cover`, a `w`×`h` grid that
/// `grid` carries the document onto; and the mask on it, if any.
fn mask_cover(
    m: &usvg::Mask,
    at: usvg::Transform,
    grid: usvg::Transform,
    w: u32,
    h: u32,
    cover: &mut [f32],
) -> Option<()> {
    let seen = grid.pre_concat(at);
    let drawn = draw_coverage(w, h, |doc| {
        // Its contents, in the space of the group wearing it — usvg's
        // word for where they stand is no better here than in a pattern.
        let mut inside = Gathered::default();
        walk(
            m.root(),
            1.0,
            None,
            Place::default(),
            Some(usvg::Transform::default()),
            &mut inside,
        );
        let root = doc.root();
        let group = doc.peek_next_id();
        let mut g = Node::group("mask");
        g.transform = as_doc(seen);
        // Cut to the mask's own rectangle.
        g.mask = Some(mask_of(&[rect_ring(m.rect(), seen)]));
        let mut cmds = vec![chitrakar_doc::Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(g),
        }];
        let first = chitrakar_doc::NodeId(group.0 + 1);
        cmds.extend(inside.into_commands(doc, group, 0, first));
        cmds
    })?;
    let luminance = m.kind() == usvg::MaskType::Luminance;
    for (c, px) in cover.iter_mut().zip(&drawn.pixels) {
        let [r, g, b, a] = px.to_srgb8();
        let a = a as f32 / 255.0;
        *c *= if luminance {
            (0.2125 * r as f32 + 0.7154 * g as f32 + 0.0721 * b as f32) / 255.0 * a
        } else {
            a
        };
    }
    match m.mask() {
        Some(inner) => mask_cover(inner, at, grid, w, h, cover),
        None => Some(()),
    }
}

/// A page `w`×`h` with what `build` puts on it, drawn by the engine.
fn draw_coverage(
    w: u32,
    h: u32,
    build: impl FnOnce(&mut chitrakar_doc::Document) -> Vec<chitrakar_doc::Command>,
) -> Option<chitrakar_render::Surface> {
    let mut doc = chitrakar_doc::Document::new(w, h, chitrakar_color::ColorMode::Rgb);
    let cmds = build(&mut doc);
    doc.apply(chitrakar_doc::Command::Batch(cmds)).ok()?;
    chitrakar_render::render(&doc).ok()
}

/// The most pixels a pattern's picture is drawn with before it is drawn
/// coarser instead: a pattern behind a whole poster is still one layer.
const PATTERN_MOST: f32 = 16_000_000.0;

/// A path filled with a pattern, as a picture of the pattern laid across
/// the path's box and seen only through its outline.
///
/// The engine has no pattern fill — a tile repeated under a transform of
/// its own — so a hatched or textured shape used to come in with no fill
/// at all, silently. What a reader does is draw the tile once at the
/// scale it will be seen at and repeat it across the page through the
/// pattern's transform; this does the same with the engine's own
/// renderer for the tile (whose contents are more of the same file, and
/// come in through `walk` like any other), and lays the result across the
/// box at a pixel a document unit. The outline goes on as the picture's
/// mask, so it is still the shape it was: a layer can be moved, and its
/// mask edited, where a fill would have been lost.
fn pattern_picture(
    path: &usvg::Path,
    pattern: &usvg::Pattern,
    opacity: f32,
    clip: Option<&Vec<Vec<[f32; 2]>>>,
    below: usize,
) -> Option<ImportedImage> {
    let abs = path.abs_transform();
    let rings = as_even_odd(path, rings_of(path));
    let mut outline: Vec<Vec<[f32; 2]>> = rings
        .iter()
        .map(Ring::flattened)
        .filter(|r| r.len() >= 3)
        .collect();
    if outline.is_empty() {
        return None;
    }
    if let Some(c) = clip {
        outline = narrowed(outline, c.clone())?;
        if outline.is_empty() {
            return None;
        }
    }
    // The tile, drawn at the scale it is seen at, as a reader draws it.
    let seen = abs.pre_concat(pattern.transform());
    let (sx, sy) = seen.get_scale();
    let rect = pattern.rect();
    let (tw, th) = (
        (rect.width() * sx).round().max(1.0) as u32,
        (rect.height() * sy).round().max(1.0) as u32,
    );
    if (tw as f32) * (th as f32) > PATTERN_MOST {
        return None;
    }
    let tile = draw_tile(pattern.root(), tw, th, sx, sy)?;
    // Page → tile pixels: the inverse of where the tile's pixels land.
    let place = seen
        .pre_translate(rect.x(), rect.y())
        .pre_scale(1.0 / sx, 1.0 / sy);
    let back = place.invert()?;
    // The box the outline covers, in document units, whole pixels out.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in outline.iter().flatten() {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let (x0, y0) = (x0.floor(), y0.floor());
    let (x1, y1) = (x1.ceil(), y1.ceil());
    let (bw, bh) = (x1 - x0, y1 - y0);
    if !(bw >= 1.0 && bh >= 1.0) {
        return None;
    }
    let k = (PATTERN_MOST / (bw * bh)).sqrt().min(1.0);
    let (w, h) = (
        (bw * k).ceil().max(1.0) as u32,
        (bh * k).ceil().max(1.0) as u32,
    );
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for j in 0..h {
        for i in 0..w {
            let mut q = usvg::tiny_skia_path::Point::from_xy(
                x0 + (i as f32 + 0.5) / k,
                y0 + (j as f32 + 0.5) / k,
            );
            back.map_point(&mut q);
            rgba.extend_from_slice(&sample_wrapped(&tile, q.x - 0.5, q.y - 0.5).to_srgb8());
        }
    }
    let name = if path.id().is_empty() {
        "Pattern".to_string()
    } else {
        path.id().to_string()
    };
    Some(ImportedImage {
        name,
        rgba,
        width: w,
        height: h,
        transform: chitrakar_doc::Transform {
            a: 1.0 / k,
            b: 0.0,
            c: 0.0,
            d: 1.0 / k,
            e: x0,
            f: y0,
        },
        opacity: opacity.min(1.0),
        below,
        within: None,
        blend: chitrakar_doc::BlendMode::Normal,
        effects: Vec::new(),
        clip: Some(mask_of(&outline)),
    })
}

/// A pattern's tile as the engine draws it: what it holds, brought in as
/// any file's contents are, on a page the tile's size in device pixels.
fn draw_tile(
    root: &usvg::Group,
    w: u32,
    h: u32,
    sx: f32,
    sy: f32,
) -> Option<chitrakar_render::Surface> {
    // The tile's own space is where the pattern's contents stand.
    let mut got = Gathered::default();
    walk(
        root,
        1.0,
        None,
        Place::default(),
        Some(usvg::Transform::default()),
        &mut got,
    );
    draw_coverage(w, h, |doc| {
        let root_id = doc.root();
        let group = doc.peek_next_id();
        let mut g = Node::group("tile");
        g.transform = as_doc(usvg::Transform::from_scale(sx, sy));
        let mut cmds = vec![chitrakar_doc::Command::AddNode {
            parent: root_id,
            index: 0,
            node: Box::new(g),
        }];
        let first = chitrakar_doc::NodeId(group.0 + 1);
        cmds.extend(got.into_commands(doc, group, 0, first));
        cmds
    })
}

/// A tile's colour at a point in its pixels, the tile repeating every way
/// and its neighbours mixed in by how near they are.
fn sample_wrapped(tile: &chitrakar_render::Surface, x: f32, y: f32) -> chitrakar_color::LinearRgba {
    let (w, h) = (tile.width as i64, tile.height as i64);
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let at = |i: i64, j: i64| tile.pixels[(j.rem_euclid(h) * w + i.rem_euclid(w)) as usize];
    let (i, j) = (fx as i64, fy as i64);
    let mix = |a: chitrakar_color::LinearRgba, b: chitrakar_color::LinearRgba, t: f32| {
        chitrakar_color::LinearRgba {
            r: a.r + (b.r - a.r) * t,
            g: a.g + (b.g - a.g) * t,
            b: a.b + (b.b - a.b) * t,
            a: a.a + (b.a - a.a) * t,
        }
    };
    mix(
        mix(at(i, j), at(i + 1, j), tx),
        mix(at(i, j + 1), at(i + 1, j + 1), tx),
        ty,
    )
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
        opacity: opacity.min(1.0),
        below,
        within: None,
        blend: chitrakar_doc::BlendMode::Normal,
        effects: Vec::new(),
        clip: None,
    })
}

/// One subpath as anchors with bezier handles, and whether it closes.
#[derive(Clone)]
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
    rings_in(path, path.abs_transform())
}

/// Where a stroked path is kept: in the page's own space, its pen scaled
/// with it, while the file's transform only moves, turns and scales it
/// evenly — and otherwise in the path's own space with that transform
/// on the layer, since a pen that is round in the file's space is an
/// ellipse on the page under a skew or an uneven scale, leaning with the
/// line. Brought into page space the pen stayed round and took one width
/// for both ways, so a stroke under either came in too thick along one
/// axis and too thin along the other. `None` for page space.
fn own_space(path: &usvg::Path, t: usvg::Transform) -> Option<usvg::Transform> {
    path.stroke()?;
    let (x, y) = ((t.sx, t.ky), (t.kx, t.sy));
    let (xx, yy) = (x.0 * x.0 + x.1 * x.1, y.0 * y.0 + y.1 * y.1);
    let square = (x.0 * y.0 + x.1 * y.1).abs() <= 1e-4 * (xx * yy).sqrt();
    let even = (xx - yy).abs() <= 1e-4 * xx.max(yy);
    (!(square && even)).then_some(t)
}

/// The path's subpaths through `t`.
fn rings_in(path: &usvg::Path, t: usvg::Transform) -> Vec<Ring> {
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
        // A fade handed back to the group a paint sits in is divided out
        // of the paint, which can take it past opaque (`walk`).
        a: alpha.clamp(0.0, 1.0),
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
    abs: usvg::Transform,
    bbox: [f32; 4],
    alpha: f32,
) -> Option<Gradient> {
    let (bw, bh) = ((bbox[2] - bbox[0]).max(1e-6), (bbox[3] - bbox[1]).max(1e-6));
    let norm = |t: usvg::Transform, x: f32, y: f32| -> [f32; 2] {
        let mut p = usvg::tiny_skia_path::Point::from_xy(x, y);
        t.map_point(&mut p);
        abs.map_point(&mut p);
        [(p.x - bbox[0]) / bw, (p.y - bbox[1]) / bh]
    };
    match paint {
        // The engine's line runs across the shape's box, its bands square
        // to it *in the box* — 0..1 each way, however long each way is.
        // A file's bands are square to the line in the gradient's own
        // space, whatever that is mapped to the page by. Both ends taken
        // into the box straight across, a gradient in user space over a
        // box that is not square came in with every band turned, and one
        // under a skew or in box units likewise. So the ramp is carried
        // over rather than the ends: it is `t = (p − p1)·a` on the page,
        // `a = M⁻ᵀ d / |d|²` for the gradient's vector `d` and the matrix
        // `M` taking its space to the page; in the box, `q` with
        // `p = B q + b0`, it is `(q − q1)·B a`; and the line whose ramp
        // that is runs from `q1` along `B a / |B a|²`.
        usvg::Paint::LinearGradient(g) => {
            let from = norm(g.transform(), g.x1(), g.y1());
            let m = abs.pre_concat(g.transform());
            let det = m.sx * m.sy - m.kx * m.ky;
            let d = [g.x2() - g.x1(), g.y2() - g.y1()];
            let dd = d[0] * d[0] + d[1] * d[1];
            let to = if det.abs() > 1e-12 && dd > 1e-12 {
                // M⁻ᵀ d, then over |d|².
                let a = [
                    (m.sy * d[0] - m.ky * d[1]) / det / dd,
                    (-m.kx * d[0] + m.sx * d[1]) / det / dd,
                ];
                let ba = [bw * a[0], bh * a[1]];
                let n = ba[0] * ba[0] + ba[1] * ba[1];
                if n > 1e-20 {
                    [from[0] + ba[0] / n, from[1] + ba[1] / n]
                } else {
                    norm(g.transform(), g.x2(), g.y2())
                }
            } else {
                norm(g.transform(), g.x2(), g.y2())
            };
            Some(Gradient::Linear {
                from,
                to,
                stops: stops_of(g.stops(), alpha),
                spread: spread_of(g.spread_method()),
            })
        }
        // A file's rings are circles in the gradient's own space, which
        // reaches the page through the matrix `M`: the ramp is
        // `|M⁻¹ (p − pc)| / r`. In the shape's box `p − pc = B (q − qc)`,
        // so the rings there are `|M⁻¹ B (q − qc)| / r` — exactly the
        // engine's radial with those axes and a radius of one. They used
        // to come in as a circle in the box's own units with the file's
        // radius over the box's half-diagonal: half as wide again as the
        // file's on a square, and an ellipse on anything else.
        usvg::Paint::RadialGradient(g) => {
            let center = norm(g.transform(), g.cx(), g.cy());
            let m = abs.pre_concat(g.transform());
            let det = m.sx * m.sy - m.kx * m.ky;
            let r = g.r().get();
            if det.abs() < 1e-12 || r <= 0.0 {
                return None;
            }
            // M⁻¹ = [sy −kx; −ky sx] / det, then on the right by B, over r.
            let axes = [
                m.sy / det * bw / r,
                -m.kx / det * bh / r,
                -m.ky / det * bw / r,
                m.sx / det * bh / r,
            ];
            // The focus is a point in the gradient's space like the
            // centre, so it comes into the box the way the centre does,
            // and the axes measure from either alike.
            let focus =
                (g.fx() != g.cx() || g.fy() != g.cy()).then(|| norm(g.transform(), g.fx(), g.fy()));
            Some(Gradient::Radial {
                focus,
                center,
                radius: 1.0,
                stops: stops_of(g.stops(), alpha),
                spread: spread_of(g.spread_method()),
                axes: Some(axes),
            })
        }
        _ => None,
    }
}

/// What a file's gradient does past its ends, said the engine's way.
fn spread_of(m: usvg::SpreadMethod) -> chitrakar_doc::Spread {
    match m {
        usvg::SpreadMethod::Pad => chitrakar_doc::Spread::Pad,
        usvg::SpreadMethod::Reflect => chitrakar_doc::Spread::Reflect,
        usvg::SpreadMethod::Repeat => chitrakar_doc::Spread::Repeat,
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
/// It used to convert only one case — rings all wound the same way,
/// overlapping, as their union — and leave the rest alone. Asked of
/// files nobody wrote, the commonest case it left was the simplest: one
/// outline crossing itself, a star or a looping polyline, which came in
/// with a hole wherever it wound round twice. Now the outline is cut
/// where it crosses itself and kept where it is the edge of what the
/// nonzero rule fills (`boolean::nonzero_as_even_odd`), which is every
/// case; a path whose rules already agree is left with its curves.
fn as_even_odd(path: &usvg::Path, rings: Vec<Ring>) -> Vec<Ring> {
    let nonzero = path
        .fill()
        .is_some_and(|f| f.rule() == usvg::FillRule::NonZero);
    if !nonzero {
        return rings;
    }
    let flat: Vec<Vec<[f32; 2]>> = rings.iter().map(Ring::flattened).collect();
    let edge = chitrakar_render::boolean::nonzero_as_even_odd(&flat)
        // Where that could not be traced — outlines that only touch, as a
        // tapered stroke's bands and discs do — rings all wound one way
        // are still exactly their union, which the shape booleans can say
        // by nudging what only touches apart.
        .or_else(|| union_if_wound_alike(&flat));
    match edge {
        Some(edge) => edge
            .into_iter()
            .map(|points| Ring {
                handles: vec![[0.0; 4]; points.len()],
                points,
                closed: true,
            })
            .collect(),
        // The rules agree — or the edge could not be traced, where the
        // shape as it stands is the honest answer rather than a guess.
        None => rings,
    }
}

/// Rings all wound the same way, overlapping, as their union — which is
/// what the nonzero rule fills for them. `None` for anything else.
fn union_if_wound_alike(flat: &[Vec<[f32; 2]>]) -> Option<Vec<Vec<[f32; 2]>>> {
    if flat.len() < 2 {
        return None;
    }
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
        return None;
    }
    // Rings that do not reach each other already mean the same under
    // either rule, and a union would only flatten their curves.
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
        return None;
    }
    let mut acc: Vec<Vec<[f32; 2]>> = vec![flat[0].clone()];
    for next in &flat[1..] {
        acc = chitrakar_render::boolean::combine_or_nudge(
            &acc,
            std::slice::from_ref(next),
            chitrakar_render::boolean::BoolOp::Union,
        )?;
    }
    Some(acc)
}

/// A path's layers: one, or two where its fill and its stroke need
/// different outlines.
///
/// They do when the fill had to be said for the even-odd rule
/// (`as_even_odd`): the fill's outline is then the edge of the region the
/// file fills, and a stroke laid along *that* goes round the region's
/// edge rather than along the line the file drew — an open path closed
/// up, a crossing turned into a corner. So the fill goes on the region,
/// and the stroke on the file's own path above it, which is the order SVG
/// paints the two in.
fn shapes_of(path: &usvg::Path, abs: usvg::Transform, opacity: f32) -> Vec<Node> {
    let space = own_space(path, abs);
    let mut out = shapes_in(
        path,
        space.map_or(abs, |_| usvg::Transform::default()),
        opacity,
    );
    if let Some(t) = space {
        let placed = chitrakar_doc::Transform {
            a: t.sx,
            b: t.ky,
            c: t.kx,
            d: t.sy,
            e: t.tx,
            f: t.ty,
        };
        for node in &mut out {
            node.transform = placed;
        }
    }
    out
}

/// The layers a path comes in as, its rings through `t` — the page's
/// transform, or none when the layer is to carry it (`own_space`).
fn shapes_in(path: &usvg::Path, t: usvg::Transform, opacity: f32) -> Vec<Node> {
    let rings = rings_in(path, t);
    if rings.is_empty() {
        return Vec::new();
    }
    let edge = as_even_odd(path, rings.clone());
    let corrected =
        edge.len() != rings.len() || edge.iter().zip(&rings).any(|(a, b)| a.points != b.points);
    if !corrected || path.stroke().is_none() || path.fill().is_none() {
        return shape_of(path, edge, t, opacity).into_iter().collect();
    }
    let mut out = Vec::new();
    if let Some(mut fill) = shape_of(path, edge, t, opacity) {
        if let NodeKind::Vector { stroke, .. } = &mut fill.kind {
            *stroke = None;
        }
        out.push(fill);
    }
    if let Some(mut line) = shape_of(path, rings, t, opacity) {
        if let NodeKind::Vector { fill, gradient, .. } = &mut line.kind {
            *fill = None;
            *gradient = None;
        }
        out.push(line);
    }
    out
}

fn shape_of(
    path: &usvg::Path,
    mut rings: Vec<Ring>,
    t: usvg::Transform,
    opacity: f32,
) -> Option<Node> {
    if rings.is_empty() {
        return None;
    }
    // The main ring keeps its curves; the rest, straight-sided, cut holes
    // or add islands. The first subpath is taken as the main one, which
    // is how outlines are usually drawn.
    let main = rings.remove(0);
    let subpaths: Vec<Vec<[f32; 2]>> = rings.iter().map(Ring::flattened).collect();
    let name = if path.id().is_empty() {
        "Path"
    } else {
        path.id()
    };
    let shape = VectorShape::Path {
        points: main.points,
        closed: main.closed,
        smooth: false,
        handles: main.handles,
        subpaths,
    };
    // The box a gradient is laid across is the one the engine will lay it
    // across (`gradient_box`): the curve's own extent. Measured from the
    // anchors alone, a curve bulging past them gave the gradient a
    // smaller box here than it was painted over, and its ramp came in
    // shifted along it.
    let (x0, y0, x1, y1) = chitrakar_render::gradient_box(&shape);
    let bbox = [x0, y0, x1, y1];
    let mut node = Node::vector(name, shape);
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
            *gradient = gradient_of(f.paint(), t, bbox, alpha);
            *fill = solid_of(f.paint(), alpha);
        }
        *stroke = path.stroke().and_then(|s| {
            // In the page's space under an even scale, the pen scales
            // with it; in the path's own, `t` is the identity and the pen
            // is the file's as written.
            let (sx, sy) = t.get_scale();
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
                // walked round and round does that by itself.
                dash: s
                    .dasharray()
                    .map(|d| d.iter().map(|v| v * scale).collect())
                    .unwrap_or_default(),
                // Where the pattern starts along the line, in the same
                // units. It used to have no field to land in and came in
                // as nought, so every dashed line whose file shifted its
                // pattern came in with its dashes where the gaps were.
                dash_offset: s.dashoffset() * scale,
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
        let Some(Gradient::Linear {
            from, to, stops, ..
        }) = gradient
        else {
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
            let ours = chitrakar_render::render(&brought_in(svg, w, h)).unwrap();
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

    /// A mask that is only a region is carried as one.
    ///
    /// An SVG mask is greyscale — coverage is the luminance of whatever is
    /// drawn in it — and nothing here can hold that without pooling
    /// pixels for a raster mask. But a mask drawn as opaque white shapes
    /// is a region and nothing more, which is how most masks in most
    /// files are used, and leaving it out put artwork on the page the
    /// file says is not there: outside the mask, resvg drew nothing and
    /// this drew solid.
    ///
    /// The second half is the guard, and it is the half worth testing.
    /// Every condition in `region_only` fails towards `None`, which is
    /// what happened before any of this existed — so a mask painted grey
    /// must *not* be read as a region, because a region would show what
    /// the file only half shows. That it still arrives whole is a known
    /// loss, written down; being confidently wrong about it would be a
    /// new one.
    #[test]
    fn a_mask_that_is_only_a_region_is_carried_as_one() {
        let drawn = |svg: &str| chitrakar_render::render(&brought_in(svg, 60, 40)).unwrap();
        let theirs = |svg: &str| {
            let mut opt = usvg::Options::default();
            opt.fontdb_mut().load_font_data(FACE.to_vec());
            let tree = usvg::Tree::from_data(svg.as_bytes(), &opt).unwrap();
            let mut pix = resvg::tiny_skia::Pixmap::new(60, 40).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pix.as_mut(),
            );
            pix
        };

        // White and opaque: a region, and it agrees with resvg.
        let hard = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs><mask id="m">
               <rect x="5" y="5" width="20" height="30" fill="#ffffff"/>
             </mask></defs>
             <g mask="url(#m)">
               <rect x="5" y="10" width="50" height="20" fill="#cc0000"/>
             </g>
           </svg>"##;
        let (ours, pix) = (drawn(hard), theirs(hard));
        for (x, y, what) in [
            (12u32, 20u32, "inside the mask"),
            (30, 20, "outside it, where the file shows nothing"),
            (45, 20, "well outside it"),
        ] {
            assert_eq!(
                ours.get(x, y).a > 0.5,
                pix.pixel(x, y).unwrap().alpha() > 128,
                "{what} at {x},{y}"
            );
        }

        // A mask has a region of its own, and what is drawn outside it
        // does not show however white it is. The content here reaches to
        // x=45 and the region stops at x=20, so the difference between
        // reading the region and not reading it is twenty-five columns of
        // artwork.
        let bounded = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs><mask id="m" maskUnits="userSpaceOnUse" x="5" y="5" width="15" height="30">
               <rect x="5" y="5" width="40" height="30" fill="#ffffff"/>
             </mask></defs>
             <g mask="url(#m)">
               <rect x="5" y="10" width="50" height="20" fill="#2266cc"/>
             </g>
           </svg>"##;
        let (ours, pix) = (drawn(bounded), theirs(bounded));
        for (x, y, what) in [
            (12u32, 20u32, "inside the mask's own region"),
            (30, 20, "past it, though the mask is painted white there"),
            (45, 20, "well past it"),
        ] {
            assert_eq!(
                ours.get(x, y).a > 0.5,
                pix.pixel(x, y).unwrap().alpha() > 128,
                "{what} at {x},{y}"
            );
        }

        // Painted grey rather than white: a real shade, so it must not be
        // read as a region. resvg fades the content to about half inside
        // it and hides it outside, and so does this now — as a raster
        // mask on a group of its own (`soft_mask`), not as a region.
        let soft = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
             <defs><mask id="m">
               <rect x="5" y="5" width="20" height="30" fill="#808080"/>
             </mask></defs>
             <g mask="url(#m)">
               <rect x="5" y="10" width="50" height="20" fill="#cc0000"/>
             </g>
           </svg>"##;
        let imported = import_svg(soft.as_bytes()).unwrap();
        assert!(
            imported.shapes.iter().all(|n| n.mask.is_none()),
            "a mask painted grey is not a region"
        );
        assert!(
            matches!(
                imported.groups.first().and_then(|g| g.mask.as_ref()),
                Some(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Raster { .. },
                    ..
                })
            ),
            "it is a raster mask on the group"
        );
        let ours = drawn(soft);
        let half = ours.get(15, 20).a;
        assert!(
            ours.get(45, 20).a < 0.01 && (half - 0.5).abs() < 0.05,
            "hidden outside a grey mask and half inside it, got {} and {half}",
            ours.get(45, 20).a
        );
    }

    /// A page nobody wrote, cut down to what an SVG and this importer
    /// both carry: shapes, paths, strokes, gradients, pictures, groups,
    /// frames, copies, text, masks, blends, faded groups and layers held
    /// to the one below, and effects. Left out: the layers that work on
    /// what is under them. Outlines were left out too until a faded
    /// layer's could come back (`effects_of`'s fade); let in, they found
    /// a hairline the engine itself would not outline
    /// (`chitrakar_render`'s `ridge`) and a group's effects over a layer
    /// with its own losing one of the two (`holds_a_filter`). Masks,
    /// blends, fades and holds were left out too until the importer
    /// kept the groups they need (`ImportedGroup`); let back in, they
    /// found two things the *exporter* had wrong (`isolated`, and the
    /// blend a mask's wrapper takes). Text keeps what a reader can set
    /// exactly — not a synthesized italic or bold, which the bundled face
    /// has no cut for and a reader will not fake, nor text round a closed
    /// guide, which the page wraps past the guide's start and SVG cannot.
    fn portable(seed: u64) -> Document {
        let mut doc = chitrakar_doc::fixture::page(seed);
        let page = doc.clone();
        for (id, n) in page.nodes() {
            let id = *id;
            if page.parent_of(id).is_none() {
                continue;
            }
            let closed_guide = matches!(
                &n.kind,
                NodeKind::Text(t) if matches!(
                    t.along,
                    Some(VectorShape::Ellipse { .. })
                        | Some(VectorShape::Rect { .. })
                        | Some(VectorShape::Path { closed: true, .. })
                )
            );
            let gone = n.name == "ground"
                || closed_guide
                || matches!(
                    n.kind,
                    NodeKind::Adjustment(_) | NodeKind::Filter(_) | NodeKind::Clone { .. }
                )
                || chitrakar_render::copies_a_clone(&page, id)
                || chitrakar_render::rewrites_what_is_under_it(&page, id);
            if gone {
                doc.apply(Command::SetVisible { id, visible: false })
                    .unwrap();
            }

            if let NodeKind::Text(t) = &n.kind {
                let mut t = t.clone();
                t.italic = false;
                t.bold = false;
                for r in &mut t.runs {
                    r.italic = None;
                    r.bold = None;
                }
                doc.apply(Command::SetKind {
                    id,
                    kind: Box::new(NodeKind::Text(t)),
                })
                .unwrap();
            }
        }
        doc
    }

    /// The page exported, and the export brought back in the way
    /// `Session::place_svg` brings a file in: pictures pooled, and put
    /// back among the shapes where each one was.
    fn round_trip(doc: &Document) -> Document {
        let svg = crate::export_svg(doc).unwrap();
        brought_in(&svg, doc.meta.width, doc.meta.height)
    }

    /// An SVG file brought in the way `Session::place_svg` brings one in,
    /// onto a page of the given size.
    fn brought_in(svg: &str, width: u32, height: u32) -> Document {
        let imported = import_svg(svg.as_bytes()).unwrap();
        let mut out = Document::new(width, height, chitrakar_color::ColorMode::Rgb);
        let (root, first) = (out.root(), out.peek_next_id());
        let cmds = imported.into_commands(&mut out, root, 0, first);
        out.apply(Command::Batch(cmds)).unwrap();
        out
    }

    /// A page exported and brought back in covers what it covered.
    ///
    /// The exporter and the importer had each been asked about files
    /// somebody wrote, and never about each other. Asked of four hundred
    /// pages nobody wrote, cut down to what both carry (`portable`), a
    /// third came back wrong, and all of it was placement: a clip path's
    /// region came in where the group it cut would stand with no
    /// placement at all, since usvg leaves a clip's outlines in the space
    /// of the group that refers to it — every frame in a moved group came
    /// back cut to a rectangle at the page's corner; text came in the same
    /// way, usvg's outlines of it carrying none of the text's placement;
    /// and the export itself put a space after every line of text (a
    /// newline between tspans, under `xml:space="preserve"`), wrote its
    /// guide where a reader reads it differently from the page, anchored
    /// text on a guide that the page does not, and left underline and
    /// strike-through to each reader's own idea of them.
    #[test]
    fn a_page_exported_and_brought_back_covers_what_it_covered() {
        let mut touched = 0usize;
        for seed in 0..400u64 {
            let page = portable(seed);
            let before = chitrakar_render::render(&page).unwrap();
            let after = chitrakar_render::render(&round_trip(&page)).unwrap();
            let (mut bad, mut worst, mut at) = (0usize, 0.0f32, 0usize);
            for (i, (p, q)) in before.pixels.iter().zip(&after.pixels).enumerate() {
                let d = (p.a - q.a).abs();
                if d > 0.5 {
                    bad += 1;
                }
                if d > worst {
                    worst = d;
                    at = i;
                }
            }
            let w = page.meta.width as usize;
            assert!(
                bad <= 10,
                "seed {seed}: {bad} pixels are covered differently by more than half, \
                 the worst by {worst:.3} at ({}, {})",
                at % w,
                at / w
            );
            touched += (bad > 0) as usize;
        }
        assert!(
            touched <= 16,
            "{touched} pages came back different somewhere"
        );
    }

    /// A clip or a mask on a group that is placed lands where the group
    /// is, and a clip path's own transform goes inside that.
    #[test]
    fn a_clip_on_a_placed_group_lands_where_the_group_is() {
        let region = |svg: &[u8]| -> Vec<[f32; 2]> {
            let imported = import_svg(svg).unwrap();
            match &imported.shapes[0].mask {
                Some(chitrakar_doc::Mask {
                    kind:
                        chitrakar_doc::MaskKind::Vector {
                            shape: VectorShape::Path { points, .. },
                            ..
                        },
                    ..
                }) => points.clone(),
                other => panic!("a region: {other:?}"),
            }
        };
        let square = |x: f32, y: f32| vec![[x, y], [x + 5.0, y], [x + 5.0, y + 5.0], [x, y + 5.0]];
        assert_eq!(
            region(br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><clipPath id="c"><rect width="5" height="5"/></clipPath></defs><g transform="translate(10 10)" clip-path="url(#c)"><rect width="20" height="20" fill="red"/></g></svg>"#),
            square(10.0, 10.0),
            "a clip on a moved group"
        );
        assert_eq!(
            region(br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><clipPath id="c" transform="translate(2 0)"><rect width="5" height="5"/></clipPath></defs><g transform="translate(10 10)" clip-path="url(#c)"><rect width="20" height="20" fill="red"/></g></svg>"#),
            square(12.0, 10.0),
            "with a transform of its own inside the group's"
        );
        assert_eq!(
            region(br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><mask id="m"><rect width="5" height="5" fill="white"/></mask></defs><g transform="translate(10 10)" mask="url(#m)"><rect width="20" height="20" fill="red"/></g></svg>"#),
            square(10.0, 10.0),
            "and a mask the same"
        );
    }

    /// Text in a placed group comes in where the group puts it.
    #[test]
    fn text_in_a_placed_group_comes_in_where_it_stands() {
        let place = |svg: &[u8]| {
            let imported = import_svg(svg).unwrap();
            let n = &imported.shapes[0];
            (n.transform.e, n.transform.f)
        };
        assert_eq!(
            place(br#"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40"><g transform="translate(20 10)"><text font-family="DejaVu Sans" font-size="10" transform="translate(5 0)" x="0" y="10">Hi</text></g></svg>"#),
            (25.0, 10.0)
        );
    }

    /// A page exported and brought back in is the colour it was.
    ///
    /// Both sides are drawn by the engine, so unlike a reader's picture
    /// nothing here is mixed in another light, and every flat pixel is
    /// held to its colour — translucent ones too. Coverage alone could
    /// not see a layer come back over another opaque one, and that is
    /// what it found: a frame standing wholly outside the frame it sits
    /// in, hidden on the page, came back whole on top of its neighbour,
    /// because two clips that do not meet were read as two clips that
    /// could not be combined (`walk`, `boolean::combine_or_nudge`).
    #[test]
    fn a_page_exported_and_brought_back_is_the_colour_it_was() {
        for seed in 0..400u64 {
            let page = portable(seed);
            let before = chitrakar_render::render(&page).unwrap();
            let after = chitrakar_render::render(&round_trip(&page)).unwrap();
            let (w, h) = (page.meta.width as i32, page.meta.height as i32);
            let px = |x: i32, y: i32| before.get(x as u32, y as u32).to_srgb8();
            let mut off = Vec::new();
            for y in 1..h - 1 {
                for x in 1..w - 1 {
                    let c = px(x, y);
                    // Painted, and flat round about: an edge is the
                    // coverage test's to judge.
                    let flat = c[3] > 0
                        && (-1..=1).all(|j| {
                            (-1..=1).all(|i| {
                                let n = px(x + i, y + j);
                                (0..4).all(|k| n[k].abs_diff(c[k]) <= 3)
                            })
                        });
                    if !flat {
                        continue;
                    }
                    let t = after.get(x as u32, y as u32).to_srgb8();
                    // Premultiplied: what a pixel all but empty holds is
                    // rounding, and a shadow's faint tail is all such
                    // pixels.
                    let pm = |c: [u8; 4], q: usize| {
                        if q == 3 {
                            c[3] as i32
                        } else {
                            c[q] as i32 * c[3] as i32 / 255
                        }
                    };
                    if (0..4).any(|q| (pm(c, q) - pm(t, q)).abs() > 20) {
                        off.push((x, y, c, t));
                    }
                }
            }
            assert!(
                off.len() <= 2,
                "seed {seed}: {} flat pixels came back another colour, the first at \
                 ({}, {}) — {:?} before, {:?} after",
                off.len(),
                off[0].0,
                off[0].1,
                off[0].2,
                off[0].3
            );
        }
    }

    /// What is clipped by two regions that do not meet shows nothing, and
    /// does not come in.
    #[test]
    fn what_two_clips_that_do_not_meet_hide_stays_hidden() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><clipPath id="a"><rect width="10" height="10"/></clipPath><clipPath id="b"><rect x="20" width="10" height="10"/></clipPath></defs><g clip-path="url(#a)"><g clip-path="url(#b)"><rect width="40" height="40" fill="red"/></g></g><rect x="30" y="30" width="5" height="5" fill="blue"/></svg>"#;
        let imported = import_svg(svg).unwrap();
        assert_eq!(imported.shapes.len(), 1, "only the blue square comes in");
        // Every other way a region comes out empty, each held to what a
        // reader draws as well: a red square that shows nowhere.
        for (svg, what) in [
            (
                r#"<defs><clipPath id="c"/></defs><g clip-path="url(#c)"><rect width="40" height="40" fill="red"/></g>"#,
                "a clip path with nothing in it",
            ),
            (
                r#"<defs><clipPath id="i"><rect x="30" width="5" height="5"/></clipPath><clipPath id="c" clip-path="url(#i)"><rect width="10" height="10"/></clipPath></defs><g clip-path="url(#c)"><rect width="40" height="40" fill="red"/></g>"#,
                "a clip path cut by a clip it does not meet",
            ),
            (
                r#"<defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10"><rect x="20" width="10" height="10" fill="white"/></mask></defs><g mask="url(#m)"><rect width="40" height="40" fill="red"/></g>"#,
                "a mask drawn outside its own rectangle",
            ),
        ] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">{svg}</svg>"#
            );
            let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).unwrap();
            let mut pix = resvg::tiny_skia::Pixmap::new(40, 40).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pix.as_mut(),
            );
            assert!(
                pix.data().chunks(4).all(|p| p[3] == 0),
                "{what}: a reader shows nothing"
            );
            assert!(
                import_svg(svg.as_bytes()).unwrap().shapes.is_empty(),
                "{what}: and nothing comes in"
            );
        }
        // And clips that do meet still show where they meet.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><defs><clipPath id="a"><rect width="10" height="10"/></clipPath><clipPath id="b"><rect x="5" width="10" height="10"/></clipPath></defs><g clip-path="url(#a)"><g clip-path="url(#b)"><rect width="40" height="40" fill="red"/></g></g></svg>"#;
        let imported = import_svg(svg).unwrap();
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let root = doc.root();
        for (i, n) in imported.shapes.into_iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: root,
                index: i,
                node: Box::new(n),
            })
            .unwrap();
        }
        let ours = chitrakar_render::render(&doc).unwrap();
        assert!(ours.get(7, 5).a > 0.99, "where both show");
        assert!(
            ours.get(2, 5).a < 0.01 && ours.get(12, 5).a < 0.01,
            "and nowhere else"
        );
    }

    /// A clip on a clip path, and a mask on a mask, narrow it where a
    /// reader narrows it — in the referring group's space, without the
    /// first one's own transform.
    #[test]
    fn a_clip_on_a_clip_path_narrows_it_as_a_reader_does() {
        for (body, what) in [
            (
                r#"<defs><clipPath id="i"><rect x="12" y="0" width="20" height="40"/></clipPath><clipPath id="c" clip-path="url(#i)" transform="translate(4 0)"><rect width="16" height="30"/></clipPath></defs><g transform="translate(2 3)" clip-path="url(#c)"><rect width="40" height="40" fill="red"/></g>"#,
                "a clip on a moved clip path",
            ),
            (
                r#"<defs><mask id="i" maskUnits="userSpaceOnUse" x="0" y="10" width="40" height="40"><rect width="40" height="40" fill="white"/></mask><mask id="m" mask="url(#i)" maskUnits="userSpaceOnUse" x="0" y="0" width="40" height="40"><rect width="20" height="30" fill="white"/></mask></defs><g transform="translate(3 2)" mask="url(#m)"><rect width="40" height="40" fill="red"/></g>"#,
                "a mask on a mask",
            ),
        ] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">{body}</svg>"#
            );
            let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).unwrap();
            let mut pix = resvg::tiny_skia::Pixmap::new(40, 40).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pix.as_mut(),
            );
            let imported = import_svg(svg.as_bytes()).unwrap();
            let mut doc = Document::new(40, 40, ColorMode::Rgb);
            let root = doc.root();
            for (i, n) in imported.shapes.into_iter().enumerate() {
                doc.apply(Command::AddNode {
                    parent: root,
                    index: i,
                    node: Box::new(n),
                })
                .unwrap();
            }
            let ours = chitrakar_render::render(&doc).unwrap();
            let theirs = pix.data();
            let shown = theirs.chunks(4).filter(|p| p[3] > 127).count();
            assert!(shown > 50, "{what}: a reader shows some of it ({shown})");
            for y in 0..40u32 {
                for x in 0..40u32 {
                    let t = theirs[((y * 40 + x) * 4 + 3) as usize] as f32 / 255.0;
                    let o = ours.get(x, y).a;
                    assert!(
                        (t - o).abs() < 0.5,
                        "{what}: at ({x}, {y}) a reader covers {t:.2} and the import {o:.2}"
                    );
                }
            }
        }
    }

    /// A random number source for the files below, fixed by its seed.
    struct Dice(u64);
    impl Dice {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn f(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (self.next() % 10_000) as f32 / 10_000.0 * (hi - lo)
        }
        fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
            &of[(self.next() % of.len() as u64) as usize]
        }
        fn one_in(&mut self, n: u64) -> bool {
            self.next().is_multiple_of(n)
        }
    }

    /// An SVG file nobody wrote, using what files written elsewhere use
    /// and this exporter never writes: every basic shape, paths with
    /// relative commands, quadratics and arcs, transforms of every kind,
    /// gradients in both unit systems with every spread and a transform
    /// of their own, dashes, joins and caps, clip paths, `use`, and a
    /// nested `svg` with a viewBox. `opaque` keeps every paint and every
    /// layer at full strength, so colour can be compared as well.
    #[derive(Clone, Copy)]
    struct Allow {
        gradients: bool,
        strokes: bool,
        dashes: bool,
        transforms: bool,
        wrappers: bool,
        curves: bool,
        many: bool,
        spread: bool,
        radial: bool,
        grad_transform: bool,
        bbox_units: bool,
        focal: bool,
    }
    const ALL: Allow = Allow {
        gradients: true,
        strokes: true,
        dashes: true,
        transforms: true,
        wrappers: true,
        curves: true,
        many: true,
        spread: true,
        radial: true,
        grad_transform: true,
        bbox_units: true,
        focal: true,
    };

    fn foreign_svg_with(seed: u64, opaque: bool, allow: Allow) -> String {
        let mut d = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (w, h) = (64.0f32, 48.0f32);
        let colour = |d: &mut Dice| {
            format!(
                "#{:02x}{:02x}{:02x}",
                d.next() % 256,
                d.next() % 256,
                d.next() % 256
            )
        };
        let mut defs = String::new();
        let mut body = String::new();
        let mut ids = 0usize;
        let transform = |d: &mut Dice| -> String {
            if !allow.transforms {
                return String::new();
            }
            match d.next() % 7 {
                0 => format!(
                    " transform=\"translate({:.2} {:.2})\"",
                    d.f(-10.0, 10.0),
                    d.f(-10.0, 10.0)
                ),
                1 => format!(
                    " transform=\"rotate({:.1} {:.1} {:.1})\"",
                    d.f(-90.0, 90.0),
                    d.f(0.0, 64.0),
                    d.f(0.0, 48.0)
                ),
                2 => format!(
                    " transform=\"scale({:.2} {:.2})\"",
                    d.f(0.6, 1.4),
                    d.f(0.6, 1.4)
                ),
                3 => format!(" transform=\"skewX({:.1})\"", d.f(-25.0, 25.0)),
                4 => format!(
                    " transform=\"matrix({:.2} {:.2} {:.2} {:.2} {:.1} {:.1})\"",
                    d.f(0.7, 1.2),
                    d.f(-0.3, 0.3),
                    d.f(-0.3, 0.3),
                    d.f(0.7, 1.2),
                    d.f(-8.0, 8.0),
                    d.f(-8.0, 8.0)
                ),
                _ => String::new(),
            }
        };
        let paint = |d: &mut Dice, defs: &mut String, ids: &mut usize| -> String {
            if !allow.gradients {
                return colour(d);
            }
            match d.next() % 5 {
                0..=2 => colour(d),
                k => {
                    *ids += 1;
                    let id = format!("g{ids}");
                    let units = if d.one_in(2) || !allow.bbox_units {
                        "userSpaceOnUse"
                    } else {
                        "objectBoundingBox"
                    };
                    let spread = *d.pick(&["pad", "reflect", "repeat"]);
                    let spread = if allow.spread { spread } else { "pad" };
                    let gt = if d.one_in(3) && allow.grad_transform {
                        format!(" gradientTransform=\"rotate({:.1})\"", d.f(-45.0, 45.0))
                    } else {
                        String::new()
                    };
                    let (a, b) = if units == "userSpaceOnUse" {
                        (64.0, 48.0)
                    } else {
                        (1.0, 1.0)
                    };
                    let stops: String = (0..2 + d.next() % 2)
                        .map(|n| {
                            format!(
                                "<stop offset=\"{:.2}\" stop-color=\"{}\"/>",
                                n as f32 / 2.0 + d.f(0.0, 0.2),
                                colour(d)
                            )
                        })
                        .collect();
                    if k == 3 || !allow.radial {
                        let _ = std::fmt::Write::write_fmt(defs, format_args!(
                            "<linearGradient id=\"{id}\" gradientUnits=\"{units}\" spreadMethod=\"{spread}\"{gt} x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\">{stops}</linearGradient>",
                            d.f(0.0, a * 0.5), d.f(0.0, b * 0.5), d.f(a * 0.3, a * 0.8), d.f(b * 0.3, b * 0.8)));
                    } else {
                        // The focus inside the circle: outside it the
                        // gradient is a cone readers do not agree on.
                        let (cx, cy, r) = (
                            d.f(0.3 * a, 0.7 * a),
                            d.f(0.3 * b, 0.7 * b),
                            d.f(0.15 * a, 0.4 * a),
                        );
                        let k = if allow.focal { 1.0 } else { 0.0 };
                        let (fx, fy) = (cx + d.f(-0.5, 0.5) * r * k, cy + d.f(-0.5, 0.5) * r * k);
                        let _ = std::fmt::Write::write_fmt(defs, format_args!(
                            "<radialGradient id=\"{id}\" gradientUnits=\"{units}\" spreadMethod=\"{spread}\"{gt} cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{r:.2}\" fx=\"{fx:.2}\" fy=\"{fy:.2}\">{stops}</radialGradient>"));
                    }
                    format!("url(#{id})")
                }
            }
        };
        let shape = |d: &mut Dice| -> String {
            match d.next() % if allow.curves { 8 } else { 5 } {
                0 => format!(
                    "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" rx=\"{:.2}\"",
                    d.f(0.0, 40.0),
                    d.f(0.0, 30.0),
                    d.f(6.0, 30.0),
                    d.f(6.0, 24.0),
                    if d.one_in(2) { d.f(0.0, 6.0) } else { 0.0 }
                ),
                1 => format!(
                    "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\"",
                    d.f(8.0, 56.0),
                    d.f(8.0, 40.0),
                    d.f(3.0, 14.0)
                ),
                2 => format!(
                    "<ellipse cx=\"{:.2}\" cy=\"{:.2}\" rx=\"{:.2}\" ry=\"{:.2}\"",
                    d.f(8.0, 56.0),
                    d.f(8.0, 40.0),
                    d.f(3.0, 18.0),
                    d.f(3.0, 12.0)
                ),
                3 => format!(
                    "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\"",
                    d.f(0.0, 64.0),
                    d.f(0.0, 48.0),
                    d.f(0.0, 64.0),
                    d.f(0.0, 48.0)
                ),
                4 => {
                    let pts: Vec<String> = (0..3 + d.next() % 4)
                        .map(|_| format!("{:.2},{:.2}", d.f(0.0, 64.0), d.f(0.0, 48.0)))
                        .collect();
                    format!(
                        "<{} points=\"{}\"",
                        if d.one_in(2) { "polygon" } else { "polyline" },
                        pts.join(" ")
                    )
                }
                _ => {
                    let mut p = format!("M{:.2} {:.2}", d.f(5.0, 59.0), d.f(5.0, 43.0));
                    for _ in 0..2 + d.next() % 4 {
                        p += &match d.next() % 6 {
                            0 => format!(" l{:.2} {:.2}", d.f(-15.0, 15.0), d.f(-15.0, 15.0)),
                            1 => format!(
                                " C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
                                d.f(0.0, 64.0),
                                d.f(0.0, 48.0),
                                d.f(0.0, 64.0),
                                d.f(0.0, 48.0),
                                d.f(0.0, 64.0),
                                d.f(0.0, 48.0)
                            ),
                            2 => format!(
                                " q{:.2} {:.2} {:.2} {:.2}",
                                d.f(-15.0, 15.0),
                                d.f(-15.0, 15.0),
                                d.f(-15.0, 15.0),
                                d.f(-15.0, 15.0)
                            ),
                            3 => format!(
                                " A{:.2} {:.2} {:.1} {} {} {:.2} {:.2}",
                                d.f(4.0, 20.0),
                                d.f(4.0, 20.0),
                                d.f(0.0, 90.0),
                                d.next() % 2,
                                d.next() % 2,
                                d.f(0.0, 64.0),
                                d.f(0.0, 48.0)
                            ),
                            4 => format!(" h{:.2} v{:.2}", d.f(-15.0, 15.0), d.f(-15.0, 15.0)),
                            _ => format!(
                                " s{:.2} {:.2} {:.2} {:.2}",
                                d.f(-15.0, 15.0),
                                d.f(-15.0, 15.0),
                                d.f(-15.0, 15.0),
                                d.f(-15.0, 15.0)
                            ),
                        };
                    }
                    if d.one_in(2) {
                        p += " Z";
                    }
                    format!("<path d=\"{p}\"")
                }
            }
        };
        for _ in 0..if allow.many { 3 + d.next() % 5 } else { 1 } {
            let mut el = shape(&mut d);
            let f = paint(&mut d, &mut defs, &mut ids);
            let fill = if allow.strokes && d.one_in(5) {
                "none".to_string()
            } else {
                f
            };
            el += &format!(" fill=\"{fill}\"");
            if d.one_in(3) {
                el += " fill-rule=\"evenodd\"";
            }
            if allow.strokes && (fill == "none" || d.one_in(2)) {
                let st = paint(&mut d, &mut defs, &mut ids);
                el += &format!(" stroke=\"{st}\" stroke-width=\"{:.2}\"", d.f(0.8, 5.0));
                el += &format!(
                    " stroke-linecap=\"{}\" stroke-linejoin=\"{}\"",
                    d.pick(&["butt", "round", "square"]),
                    d.pick(&["miter", "round", "bevel"])
                );
                if allow.dashes && d.one_in(4) {
                    el += &format!(
                        " stroke-dasharray=\"{:.1} {:.1}\" stroke-dashoffset=\"{:.1}\"",
                        d.f(1.0, 6.0),
                        d.f(1.0, 6.0),
                        d.f(0.0, 4.0)
                    );
                }
                if !opaque && d.one_in(3) {
                    el += &format!(" stroke-opacity=\"{:.2}\"", d.f(0.3, 0.9));
                }
            }
            if !opaque && d.one_in(3) {
                el += &format!(" fill-opacity=\"{:.2}\"", d.f(0.3, 0.9));
            }
            if !opaque && d.one_in(4) {
                el += &format!(" opacity=\"{:.2}\"", d.f(0.3, 0.9));
            }
            el += &transform(&mut d);
            el += "/>";
            // Some go inside a clip, a group, a use or a nested viewport.
            el = match if allow.wrappers { d.next() % 9 } else { 8 } {
                0 => {
                    ids += 1;
                    let units = if d.one_in(2) { "userSpaceOnUse" } else { "objectBoundingBox" };
                    let clip = if units == "userSpaceOnUse" {
                        format!("<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\"/>", d.f(10.0, 54.0), d.f(10.0, 38.0), d.f(6.0, 20.0))
                    } else {
                        format!("<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\"/>", d.f(0.0, 0.4), d.f(0.0, 0.4), d.f(0.4, 0.8), d.f(0.4, 0.8))
                    };
                    defs += &format!("<clipPath id=\"c{ids}\" clipPathUnits=\"{units}\">{clip}</clipPath>");
                    format!("<g clip-path=\"url(#c{ids})\">{el}</g>")
                }
                1 => format!("<g{}>{el}</g>", transform(&mut d)),
                2 => {
                    ids += 1;
                    defs += &el.replacen('<', &format!("<g id=\"u{ids}\"><"), 1);
                    defs += "</g>";
                    format!("<use href=\"#u{ids}\" x=\"{:.2}\" y=\"{:.2}\"/>", d.f(-8.0, 8.0), d.f(-8.0, 8.0))
                }
                3 => format!(
                    "<svg x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" viewBox=\"0 0 64 48\" preserveAspectRatio=\"{}\">{el}</svg>",
                    d.f(0.0, 16.0), d.f(0.0, 12.0), d.f(30.0, 60.0), d.f(24.0, 44.0),
                    d.pick(&["xMidYMid meet", "xMinYMin slice", "none", "xMaxYMax meet"])
                ),
                _ => el,
            };
            body += &el;
        }
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><defs>{defs}</defs>{body}</svg>")
    }

    /// What resvg draws of a file, straight RGBA.
    fn reader_draws(svg: &str, w: u32, h: u32) -> Vec<[u8; 4]> {
        let mut opt = usvg::Options::default();
        opt.fontdb_mut().load_font_data(FACE.to_vec());
        let tree = usvg::Tree::from_data(svg.as_bytes(), &opt).unwrap();
        let mut pix = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pix.as_mut(),
        );
        pix.pixels()
            .iter()
            .map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect()
    }

    /// Where a picture first parted from a reader's: the pixel, and the
    /// two colours there.
    type Mismatch = (i32, i32, [u8; 4], [u8; 4]);

    fn foreign_bad(svg: &str, opaque: bool) -> (usize, usize, Option<Mismatch>) {
        let doc = brought_in(svg, 64, 48);
        let ours = chitrakar_render::render(&doc).unwrap();
        let theirs = reader_draws(svg, 64, 48);
        let (mut cov, mut col) = (0, 0);
        let mut first = None;
        for y in 1..47i32 {
            for x in 1..63i32 {
                let o = ours.get(x as u32, y as u32).to_srgb8();
                let t = theirs[(y * 64 + x) as usize];
                if (o[3] as i32 - t[3] as i32).abs() > 127 {
                    cov += 1;
                    first.get_or_insert((x, y, o, t));
                }
                if opaque {
                    let flat = o[3] == 255
                        && (-1..=1).all(|j| {
                            (-1..=1).all(|i| {
                                let q = ours.get((x + i) as u32, (y + j) as u32).to_srgb8();
                                (0..4).all(|k| q[k].abs_diff(o[k]) <= 3)
                            })
                        });
                    if flat && (0..3).any(|k| o[k].abs_diff(t[k]) > 20) {
                        col += 1;
                        first.get_or_insert((x, y, o, t));
                    }
                }
            }
        }
        (cov, col, first)
    }

    /// A file written elsewhere comes in as a reader draws it.
    ///
    /// The round trip only ever hands the importer what this exporter
    /// writes. These files use what files written elsewhere use
    /// (`foreign_svg_with`), and the engine's picture of what came in is
    /// held to resvg's picture of the file, pixel by pixel: coverage
    /// everywhere, and colour where the files are opaque.
    ///
    /// Asked first, a third of single plain shapes came back wrong. An
    /// outline crossing itself — a star, a looping polyline — came in
    /// with a hole wherever it wound round twice, the importer having
    /// converted only rings that overlapped one another
    /// (`boolean::nonzero_as_even_odd` now, for every case); and where it
    /// was stroked as well, its stroke went round the converted edge, so
    /// such a path comes in as its fill and then its stroke along the
    /// file's own line (`shapes_of`). A dashed line's
    /// `stroke-dashoffset` had nowhere to land (`Stroke::dash_offset`).
    /// Dashes along a curve had round, swollen ends, a round join being a
    /// disc at every point a curve is flattened into (`SLIGHT_TURN`). And
    /// an outline crossing itself where the crossing, worked out from
    /// each of its two edges, rounded to two different cells could not
    /// be closed (`chain`).
    ///
    /// And a stroke under a transform that skews or scales unevenly came
    /// in with a round pen of one width, the path brought into the
    /// page's space and its pen with it, where a reader turns and
    /// stretches the pen with the path: 50 of 400 files over ten pixels
    /// out, the worst 125. Such a path is kept in its own space with the
    /// transform on the layer now (`own_space`), and strokes are taken
    /// under every transform as fills are. Gradients have a test of
    /// their own, below.
    #[test]
    fn a_file_written_elsewhere_comes_in_as_a_reader_draws_it() {
        let fills = Allow {
            gradients: false,
            strokes: false,
            dashes: false,
            ..ALL
        };
        let lines = Allow {
            gradients: false,
            ..ALL
        };
        for opaque in [true, false] {
            for seed in 0..400u64 {
                let svg = foreign_svg_with(seed, opaque, fills);
                let (cover, colour, first) = foreign_bad(&svg, opaque);
                assert!(
                    cover + colour <= 2,
                    "filled file {seed} (opaque {opaque}): {cover} pixels covered and {colour} \
                     coloured otherwise, the first {first:?}\n{svg}"
                );
            }
            let mut sizes: Vec<(usize, u64)> = (0..400u64)
                .map(|seed| {
                    let (cover, colour, _) =
                        foreign_bad(&foreign_svg_with(seed, opaque, lines), opaque);
                    (cover + colour, seed)
                })
                .collect();
            sizes.sort();
            let over = sizes.iter().filter(|s| s.0 > 10).count();
            let worst = sizes[sizes.len() - 1];
            assert!(
                over <= 6 && worst.0 <= 50,
                "stroked files (opaque {opaque}): {over} over ten pixels, the worst \
                 {} pixels (file {})",
                worst.0,
                worst.1
            );
        }
    }

    fn grad_bad(svg: &str) -> (usize, Option<Mismatch>) {
        let doc = brought_in(svg, 64, 48);
        let ours = chitrakar_render::render(&doc).unwrap();
        let theirs = reader_draws(svg, 64, 48);
        let (mut n, mut first) = (0, None);
        for y in 1..47i32 {
            for x in 1..63i32 {
                let solid = (-1..=1).all(|j| {
                    (-1..=1).all(|i| {
                        ours.get((x + i) as u32, (y + j) as u32).a > 0.999
                            && theirs[((y + j) * 64 + x + i) as usize][3] == 255
                    })
                });
                if !solid {
                    continue;
                }
                let o = ours.get(x as u32, y as u32).to_srgb8();
                let t = theirs[(y * 64 + x) as usize];
                if (0..3).any(|k| o[k].abs_diff(t[k]) > 20) {
                    n += 1;
                    first.get_or_insert((x, y, o, t));
                }
            }
        }
        (n, first)
    }

    /// A file's gradient comes in painting what a reader paints.
    ///
    /// Colour is compared everywhere the shape is solid rather than only
    /// where the picture is flat — a gradient is nowhere flat, and it was
    /// by being flat-only that every gradient got past the audit above.
    /// One shape a file, so every pixel compared has the gradient under
    /// it; under every transform and wrapper, in both unit systems and
    /// with a gradient transform of its own.
    ///
    /// Asked, every radial gradient was wrong: it came in as a circle in
    /// the box's own units with the file's radius over the box's
    /// half-diagonal, half as wide again as the file's on a square and an
    /// ellipse on anything else — the engine's radial takes axes of its
    /// own now (`Gradient::Radial::axes`) and the file's rings are carried
    /// through them exactly. A linear one in user space over a box that is
    /// not square came in with its bands turned, its ends taken into the
    /// box straight across; its ramp is carried over instead. And a curve
    /// bulging past its anchors had its gradient laid over the anchors'
    /// box rather than the curve's.
    ///
    /// And past its ends, what its `spreadMethod` says — reflect, repeat —
    /// which had no field to land in and came in padded
    /// (`Gradient::spread`). And rings that start from a focus off the
    /// centre, which came in starting from the centre
    /// (`Gradient::Radial::focus`) — a focus inside the outer ring; past
    /// it SVG 2 draws a cone that readers do not agree on, and the
    /// engine holds the focus inside the ring as SVG 1.1 did.
    #[test]
    fn a_gradient_written_elsewhere_comes_in_painting_what_a_reader_paints() {
        let one = Allow {
            strokes: false,
            dashes: false,
            many: false,
            ..ALL
        };
        for seed in 0..400u64 {
            let svg = foreign_svg_with(seed, true, one);
            let (shaded, first) = grad_bad(&svg);
            let (cover, colour, _) = foreign_bad(&svg, true);
            assert!(
                shaded + cover + colour <= 10,
                "file {seed}: {shaded} pixels shaded otherwise, {cover} covered otherwise, \
                 the first {first:?}\n{svg}"
            );
        }
    }

    /// A line under an uneven scale is as thick as a reader draws it —
    /// three times its pen across, where the scale is three — and keeps
    /// the file's pen and transform rather than a round pen of one width
    /// in the page's space; under an even scale it stays in page space.
    #[test]
    fn a_pen_under_an_uneven_scale_stretches_with_its_line() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
  <path d="M8 10 H56" stroke="#000000" stroke-width="4" fill="none" transform="scale(1 3)"/>
</svg>"##;
        let imported = import_svg(svg.as_bytes()).unwrap();
        let line = &imported.shapes[0];
        let NodeKind::Vector {
            stroke: Some(stroke),
            ..
        } = &line.kind
        else {
            panic!("a stroked layer");
        };
        assert_eq!(stroke.width, 4.0, "the file's pen");
        assert_eq!(
            (line.transform.a, line.transform.d),
            (1.0, 3.0),
            "the file's scale"
        );
        let page = chitrakar_render::render(&brought_in(svg, 64, 64)).unwrap();
        let theirs = reader_draws(svg, 64, 64);
        let inked = |a: f32| a > 0.5;
        let ours = (0..64).filter(|&y| inked(page.get(32, y).a)).count();
        let reader = (0..64)
            .filter(|&y| theirs[(y * 64 + 32) as usize][3] > 127)
            .count();
        assert_eq!(reader, 12, "a reader draws it twelve across");
        assert_eq!(ours, reader, "and so does the engine");

        let even = svg.replace("scale(1 3)", "scale(2)");
        let imported = import_svg(even.as_bytes()).unwrap();
        let line = &imported.shapes[0];
        assert_eq!(line.transform, chitrakar_doc::Transform::default());
    }

    /// A shape filled with a pattern comes in painted as a reader paints
    /// it — the tile repeated through the pattern's own transform, in
    /// either unit system, through a viewBox, under the shape's transform
    /// and a group's — as a picture seen through the shape's outline.
    /// It used to come in with no fill at all.
    #[test]
    fn a_pattern_fill_comes_in_painted() {
        let head = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">"##;
        let files = [
            // Stripes in user space.
            r##"<pattern id="p" width="12" height="12" patternUnits="userSpaceOnUse">
  <rect width="6" height="12" fill="#c03020"/><rect x="6" width="6" height="12" fill="#2050d0"/>
</pattern>
<rect x="4" y="4" width="50" height="36" fill="url(#p)"/>"##,
            // A checker turned and scaled by the pattern's own transform.
            r##"<pattern id="p" width="16" height="16" patternUnits="userSpaceOnUse" patternTransform="rotate(30) scale(1.2)">
  <rect width="8" height="8" fill="#20a040"/><rect x="8" y="8" width="8" height="8" fill="#20a040"/>
  <rect x="8" width="8" height="8" fill="#f0e0c0"/><rect y="8" width="8" height="8" fill="#f0e0c0"/>
</pattern>
<circle cx="32" cy="24" r="20" fill="url(#p)"/>"##,
            // In the box's units, through a viewBox, with a stroke over it.
            r##"<pattern id="p" width="0.25" height="0.25" viewBox="0 0 10 10">
  <rect width="10" height="10" fill="#ffffff"/><circle cx="5" cy="5" r="3" fill="#303080"/>
</pattern>
<path d="M6 6 L58 10 L50 42 L10 38 Z" fill="url(#p)" stroke="#000000" stroke-width="2"/>"##,
            // Under a group that moves and scales it, and a skew of its own.
            r##"<pattern id="p" width="10" height="10" patternUnits="userSpaceOnUse">
  <rect width="10" height="10" fill="#f0c020"/><rect width="5" height="5" fill="#402010"/>
</pattern>
<g transform="translate(6 2) scale(0.9)"><ellipse cx="30" cy="26" rx="26" ry="18" fill="url(#p)" transform="skewX(10)"/></g>"##,
        ];
        for (n, body) in files.iter().enumerate() {
            let svg = format!("{head}\n{body}\n</svg>");
            let imported = import_svg(svg.as_bytes()).unwrap();
            assert_eq!(
                imported.images.len(),
                1,
                "file {n}: the pattern as a picture"
            );
            assert!(
                imported.images[0].clip.is_some(),
                "file {n}: seen through its outline"
            );
            let (cover, colour, first) = foreign_bad(&svg, true);
            assert!(
                cover + colour <= 8,
                "file {n}: {cover} pixels covered and {colour} coloured otherwise, the first \
                 {first:?}\n{svg}"
            );
        }
    }

    /// A mask with real grey in it fades what it covers as a reader fades
    /// it — a luminance ramp, an alpha mask of half-opaque content, a grey
    /// in the box's units under a moved and scaled group, and a soft mask
    /// meeting a clip and a mask that is a plain region — as a raster mask
    /// on the group they are in. Such a mask used to be passed over, and
    /// what it should have faded came in whole. (Shades that meet inside
    /// one pixel of the mask are mixed in linear light here and in the
    /// device's values by resvg, so the grey and the white in the third
    /// file are kept a little apart.)
    #[test]
    fn a_soft_mask_fades_what_it_covers() {
        let head = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">"##;
        let files = [
            r##"<linearGradient id="g"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#000000"/></linearGradient>
<mask id="m"><rect x="0" y="0" width="64" height="48" fill="url(#g)"/></mask>
<g mask="url(#m)"><rect x="4" y="4" width="56" height="40" fill="#d02020"/></g>"##,
            r##"<mask id="m" mask-type="alpha"><circle cx="32" cy="24" r="18" fill="#000000" fill-opacity="0.5"/><circle cx="20" cy="20" r="10" fill="#000000"/></mask>
<g mask="url(#m)"><rect x="4" y="4" width="56" height="40" fill="#2040d0"/><circle cx="44" cy="30" r="10" fill="#20a020"/></g>"##,
            r##"<mask id="m" maskContentUnits="objectBoundingBox"><rect x="0.1" y="0.1" width="0.5" height="0.8" fill="#808080"/><rect x="0.65" y="0.1" width="0.25" height="0.8" fill="#ffffff"/></mask>
<g transform="translate(6 4) scale(0.8 1.1)"><rect x="2" y="2" width="60" height="34" fill="#e0a010" mask="url(#m)"/></g>"##,
            r##"<clipPath id="c"><circle cx="32" cy="24" r="20"/></clipPath>
<radialGradient id="r"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#202020"/></radialGradient>
<mask id="soft"><rect width="64" height="48" fill="url(#r)"/></mask>
<mask id="hard"><rect x="10" y="0" width="30" height="48" fill="#ffffff"/></mask>
<g mask="url(#soft)" clip-path="url(#c)"><g mask="url(#hard)"><rect width="64" height="48" fill="#6020a0"/></g><rect x="40" y="10" width="20" height="10" fill="#109090"/></g>"##,
        ];
        for (n, body) in files.iter().enumerate() {
            let svg = format!("{head}\n{body}\n</svg>");
            let (off, asked, first) = faded_bad(&svg);
            assert!(asked > 2000, "file {n}: asked of enough of it ({asked})");
            assert!(
                off <= 2,
                "file {n}: {off} pixels faded otherwise, the first {first:?}\n{svg}"
            );
        }
    }

    /// How many pixels a file comes in faded otherwise than a reader fades
    /// it — premultiplied, so a fade is compared as one — away from the
    /// reader's own edges, where two rasterizers put a partly covered
    /// pixel a shade apart; how many were asked; and the first.
    fn faded_bad(svg: &str) -> (usize, usize, Option<Mismatch>) {
        let ours = chitrakar_render::render(&brought_in(svg, 64, 48)).unwrap();
        let theirs = reader_draws(svg, 64, 48);
        let (mut off, mut asked, mut first) = (0, 0, None);
        for y in 1..47i32 {
            for x in 1..63i32 {
                let o = ours.get(x as u32, y as u32).to_srgb8();
                let t = theirs[(y * 64 + x) as usize];
                let edge = (-1..=1i32).any(|j| {
                    (-1..=1i32).any(|i| {
                        let q = theirs[((y + j) * 64 + x + i) as usize];
                        (0..4).any(|k| (q[k] as i32 - t[k] as i32).abs() > 24)
                    })
                });
                if edge {
                    continue;
                }
                asked += 1;
                let pm = |c: [u8; 4], k: usize| c[k] as i32 * c[3] as i32 / 255;
                let worst = (0..3)
                    .map(|k| (pm(o, k) - pm(t, k)).abs())
                    .chain([(o[3] as i32 - t[3] as i32).abs()])
                    .max()
                    .unwrap();
                if worst > 16 {
                    off += 1;
                    first.get_or_insert((x, y, o, t));
                }
            }
        }
        (off, asked, first)
    }

    /// A group faded with more than one layer under it is faded as one
    /// picture, as a reader fades it — two overlapping layers in a group
    /// at half are half where they overlap, not three quarters — so it
    /// comes in as a group wearing the fade; nested, cut to a clip, and
    /// with a stroke over a fill under it. A fade on a group with one
    /// layer under it stays in that layer, which is exact there. (Nothing
    /// faded here lands on anything opaque: translucent paint over a
    /// colour is mixed in linear light here and in a device's values by
    /// resvg, which is a different question from what is grouped.)
    #[test]
    fn a_faded_group_fades_as_one() {
        let head = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">"##;
        let files = [
            r##"<g opacity="0.5"><rect x="4" y="4" width="36" height="30" fill="#d02020"/><rect x="22" y="14" width="36" height="30" fill="#2040d0"/></g>"##,
            r##"<g opacity="0.7"><rect x="2" y="2" width="10" height="40" fill="#20a040"/>
<g opacity="0.5"><circle cx="30" cy="24" r="15" fill="#e0c020"/><circle cx="44" cy="24" r="14" fill="#8020a0"/></g></g>"##,
            r##"<clipPath id="c"><rect x="8" y="6" width="48" height="36"/></clipPath>
<g opacity="0.6" clip-path="url(#c)"><rect width="64" height="48" fill="#30a0c0"/><path d="M4 40 L32 6 L60 40 Z" fill="#f08020" stroke="#202060" stroke-width="5"/></g>"##,
        ];
        for (n, body) in files.iter().enumerate() {
            let svg = format!("{head}\n{body}\n</svg>");
            assert!(
                !import_svg(svg.as_bytes()).unwrap().groups.is_empty(),
                "file {n}: kept as a group"
            );
            let (off, asked, first) = faded_bad(&svg);
            assert!(asked > 1500, "file {n}: asked of enough of it ({asked})");
            assert!(
                off <= 2,
                "file {n}: {off} pixels faded otherwise, the first {first:?}\n{svg}"
            );
        }
        let one = format!(
            "{head}<g opacity=\"0.5\"><rect x=\"4\" y=\"4\" width=\"30\" height=\"30\" fill=\"#d02020\"/></g></svg>"
        );
        assert!(
            import_svg(one.as_bytes()).unwrap().groups.is_empty(),
            "one layer, no group"
        );
        assert_eq!(faded_bad(&one).0, 0);
    }

    /// A blend comes in as the file says it — a layer's own where the
    /// blended element is one layer, a group's where it is more — and
    /// comes down as a reader brings it down, over an opaque page so the
    /// question is the blend and not how translucent paint mixes. It used
    /// to come in Normal, so a page this editor exported with its blends
    /// came back without them.
    #[test]
    fn a_blend_comes_in_as_the_file_says_it() {
        use chitrakar_doc::BlendMode as B;
        let head = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">
<rect width="64" height="48" fill="#3080c0"/><rect x="32" width="32" height="48" fill="#e0b040"/>"##;
        let files = [
            (
                r##"<circle cx="32" cy="24" r="18" fill="#c04060" style="mix-blend-mode:multiply"/>"##,
                B::Multiply,
                false,
            ),
            (
                r##"<g style="mix-blend-mode:screen"><circle cx="24" cy="24" r="16" fill="#40c060"/><circle cx="40" cy="24" r="16" fill="#a03080"/></g>"##,
                B::Screen,
                true,
            ),
            (
                r##"<rect x="6" y="6" width="52" height="16" fill="#80ff40" style="mix-blend-mode:difference"/><rect x="6" y="26" width="52" height="16" fill="#ff2080" style="mix-blend-mode:hue"/>"##,
                B::Difference,
                false,
            ),
            (
                r##"<rect x="6" y="6" width="52" height="36" fill="#60a0ff" style="mix-blend-mode:color-burn"/>"##,
                B::ColorBurn,
                false,
            ),
        ];
        for (n, (body, mode, grouped)) in files.iter().enumerate() {
            let svg = format!("{head}\n{body}\n</svg>");
            let imported = import_svg(svg.as_bytes()).unwrap();
            let said = if *grouped {
                imported.groups.first().map(|g| g.blend)
            } else {
                imported.shapes.get(2).map(|s| s.blend)
            };
            assert_eq!(said, Some(*mode), "file {n}: the blend it says");
            let (off, asked, first) = faded_bad(&svg);
            assert!(asked > 2000, "file {n}: asked of enough of it ({asked})");
            assert!(
                off <= 2,
                "file {n}: {off} pixels blended otherwise, the first {first:?}\n{svg}"
            );
        }
        // And this editor's own export comes back with what it wrote.
        let doc = brought_in(&format!("{head}\n{}\n</svg>", files[2].0), 64, 48);
        let again = import_svg(crate::export_svg(&doc).unwrap().as_bytes()).unwrap();
        let blends: Vec<B> = again.shapes.iter().map(|s| s.blend).collect();
        assert_eq!(blends, vec![B::Normal, B::Normal, B::Difference, B::Hue]);
    }

    /// A shadow written elsewhere comes in as the engine's own and is
    /// drawn where a reader draws it — `feDropShadow` on a shape and on a
    /// group of two (kept a group, so the shadow is of the two together),
    /// and the chain a design tool exports: the alpha taken hard, moved,
    /// blurred, knocked out of the shape, coloured by a colour matrix and
    /// blended in under it. Every filter used to be passed over, so a card
    /// with a shadow came in without one.
    #[test]
    fn a_shadow_written_elsewhere_comes_in_as_a_shadow() {
        use chitrakar_doc::Effect;
        let head = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">"##;
        let files = [
            r##"<filter id="f" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="3" dy="4" stdDeviation="2" flood-color="#203040" flood-opacity="0.7"/></filter>
<rect x="10" y="8" width="34" height="24" fill="#e04030" filter="url(#f)"/>"##,
            r##"<filter id="f" x="0" y="0" width="64" height="48" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
<feFlood flood-opacity="0" result="BackgroundImageFix"/>
<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
<feOffset dx="2" dy="4"/><feGaussianBlur stdDeviation="3"/>
<feComposite in2="hardAlpha" operator="out"/>
<feColorMatrix type="matrix" values="0 0 0 0 0.1 0 0 0 0 0.2 0 0 0 0 0.5 0 0 0 0.45 0"/>
<feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
<feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow" result="shape"/></filter>
<circle cx="30" cy="22" r="14" fill="#40b060" filter="url(#f)"/>"##,
            r##"<filter id="f" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="-3" dy="3" stdDeviation="1.5" flood-color="#000000" flood-opacity="0.5"/></filter>
<g filter="url(#f)"><rect x="14" y="10" width="26" height="20" fill="#3060e0"/><circle cx="40" cy="28" r="10" fill="#f0c020"/></g>"##,
        ];
        for (n, body) in files.iter().enumerate() {
            let svg = format!("{head}\n{body}\n</svg>");
            let imported = import_svg(svg.as_bytes()).unwrap();
            let shadows = imported
                .shapes
                .iter()
                .flat_map(|s| s.effects.iter())
                .chain(imported.groups.iter().flat_map(|g| g.effects.iter()))
                .filter(|e| matches!(e, Effect::DropShadow { .. }))
                .count();
            assert_eq!(shadows, 1, "file {n}: one shadow, where the file put it");
            let (off, asked, first) = faded_bad(&svg);
            assert!(asked > 1500, "file {n}: asked of enough of it ({asked})");
            assert!(
                off <= 4,
                "file {n}: {off} pixels shadowed otherwise, the first {first:?}\n{svg}"
            );
        }
    }

    /// What the reader cannot be handed is refused before it is.
    ///
    /// A probe set one number at a time in this editor's own SVGs to an
    /// extreme, placing each and drawing it: twenty of four hundred and
    /// eighty stopped the program. Six were the reader's own — a text
    /// laid along a curve with a point out near 10³⁰ hung its layout, a
    /// picture a billion times wider than tall panicked it — which no
    /// change here can reach, and in the browser a panic cannot be
    /// caught. So the file is asked first (`past_reason`).
    #[test]
    fn a_number_past_any_drawing_is_refused_before_the_reader_sees_it() {
        let png = crate::encode_png(4, 4, &[200u8; 64]).unwrap();
        let b64 = crate::svg::base64_for_tests(&png);
        let curve = |m: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="36"><defs><path id="g" d="M0,10 C10,0 20,20 30,{m}"/></defs><text font-size="8"><textPath href="#g">Hello world</textPath></text></svg>"##
            )
        };
        let picture = |w: &str, h: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="36"><image width="{w}" height="{h}" href="data:image/png;base64,{b64}"/></svg>"##
            )
        };
        for (what, svg) in [
            ("a curve out at 1e30", curve("1e30")),
            ("and at -1e20", curve("-1e20")),
            (
                "a picture a billion times taller than wide",
                picture("2", "2e9"),
            ),
            ("or wider than tall", picture("2", "1e-9")),
        ] {
            let refused = import_svg(svg.as_bytes());
            assert!(refused.is_err(), "{what} is refused");
        }
        // Compressed is asked about the same, and a file made to unpack
        // for ever is refused rather than unpacked.
        let gz = |bytes: &[u8]| {
            use std::io::Write;
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(bytes).unwrap();
            e.finish().unwrap()
        };
        assert!(
            import_svg(&gz(curve("1e30").as_bytes())).is_err(),
            "compressed too"
        );
        let bomb = gz(&vec![b' '; (INFLATED_MOST + 1024) as usize]);
        assert!(bomb.len() < 1 << 20, "a small file ({} bytes)", bomb.len());
        assert!(import_svg(&bomb).is_err(), "that unpacks past any drawing");
        // And what is not a number is not asked about: a colour in hex
        // that reads like an exponent, names, and other programs' own
        // attributes. Nor is a billion itself, or a curve near the edge
        // of what a drawing holds.
        let fine = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="48" height="36"><g id="layer1e99" inkscape:label="1e99" class="x2e40"><rect width="20" height="10" fill="#1e3000" data-n="9e99"/><rect x="1e9" width="1" height="1"/></g></svg>"##;
        assert!(
            import_svg(fine.as_bytes()).is_ok(),
            "names and colours open"
        );
        assert!(
            import_svg(curve("1e8").as_bytes()).is_ok(),
            "a curve far out opens"
        );
        assert!(
            import_svg(&gz(fine.as_bytes())).is_ok(),
            "and opens compressed"
        );
    }

    /// What the reader survives, this importer has to as well — and
    /// numbers each within reason still multiply past it, as two groups
    /// each stretched a millionfold do. A shadow's blur was found by
    /// counting up from one, a step for every pixel of its width; and a
    /// soft mask was held to a number of pixels but not to a side, so one
    /// a trillion wide and two tall asked for a row no machine has.
    #[test]
    fn a_drawing_scaled_past_reason_comes_in_without_stopping_anything() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="36">
  <defs>
    <filter id="s"><feDropShadow dx="2" dy="2" stdDeviation="3"/></filter>
    <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="40" height="2"><rect width="40" height="2" fill="#777"/></mask>
  </defs>
  <g transform="scale(1e6 1)"><g transform="scale(1e6 1)">
    <rect width="30" height="20" fill="#c33" filter="url(#s)"/>
    <rect y="22" width="30" height="10" fill="#3c3" mask="url(#m)"/>
  </g></g>
</svg>"##;
        let started = std::time::Instant::now();
        let doc = brought_in(svg, 48, 36);
        let _ = chitrakar_render::render(&doc).unwrap();
        assert!(
            started.elapsed().as_secs() < 30,
            "and in reasonable time ({:?})",
            started.elapsed()
        );
    }

    /// This editor's own effects — a drop shadow, an outline and an inner
    /// shadow — go out as filters and come back as themselves.
    #[test]
    fn an_effect_exported_comes_back_as_itself() {
        use chitrakar_doc::Effect;
        let mut doc = Document::new(64, 48, ColorMode::Rgb);
        let root = doc.root();
        let mut card = Node::vector(
            "card",
            VectorShape::Rect {
                width: 30.0,
                height: 20.0,
                radius: 3.0,
            },
        );
        card.transform = chitrakar_doc::Transform::translation(14.0, 12.0);
        let ink = |r: f32, g: f32, b: f32| AuthoredColor::Srgb { r, g, b, a: 1.0 };
        if let NodeKind::Vector { fill, .. } = &mut card.kind {
            *fill = Some(ink(0.9, 0.5, 0.2));
        }
        card.effects = vec![
            Effect::DropShadow {
                dx: 3.0,
                dy: 4.0,
                blur: 2.5,
                color: ink(0.1, 0.1, 0.3),
                opacity: 0.6,
            },
            Effect::Outline {
                width: 2.0,
                color: ink(0.2, 0.7, 0.3),
                opacity: 1.0,
            },
            Effect::InnerShadow {
                dx: 1.0,
                dy: 2.0,
                blur: 1.5,
                color: ink(0.0, 0.0, 0.0),
                opacity: 0.5,
            },
        ];
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(card),
        })
        .unwrap();
        let svg = crate::export_svg(&doc).unwrap();
        let back = import_svg(svg.as_bytes()).unwrap();
        let fx = &back.shapes[0].effects;
        assert_eq!(fx.len(), 3, "all three came back: {fx:?}");
        let before = chitrakar_render::render(&doc).unwrap();
        let after = chitrakar_render::render(&brought_in(&svg, 64, 48)).unwrap();
        let worst = before
            .pixels
            .iter()
            .zip(&after.pixels)
            .map(|(p, q)| {
                (p.r - q.r)
                    .abs()
                    .max((p.g - q.g).abs())
                    .max((p.b - q.b).abs())
                    .max((p.a - q.a).abs())
            })
            .fold(0.0f32, f32::max);
        assert!(
            worst < 0.02,
            "the page came back as it was, worst {worst}; {fx:?}"
        );
    }

    /// How far apart two pictures of a page are at worst, premultiplied:
    /// a pixel nearly transparent has no colour worth comparing.
    fn worst_apart(a: &Document, b: &Document) -> f32 {
        let (p, q) = (
            chitrakar_render::render(a).unwrap(),
            chitrakar_render::render(b).unwrap(),
        );
        p.pixels
            .iter()
            .zip(&q.pixels)
            .map(|(p, q)| {
                (p.r * p.a - q.r * q.a)
                    .abs()
                    .max((p.g * p.a - q.g * q.a).abs())
                    .max((p.b * p.a - q.b * q.a).abs())
                    .max((p.a - q.a).abs())
            })
            .fold(0.0f32, f32::max)
    }

    fn outlined_card(opacity: f32) -> Document {
        let mut doc = Document::new(64, 48, ColorMode::Rgb);
        let root = doc.root();
        let mut card = Node::vector(
            "card",
            VectorShape::Rect {
                width: 30.0,
                height: 20.0,
                radius: 3.0,
            },
        );
        card.transform = chitrakar_doc::Transform::translation(14.0, 12.0);
        if let NodeKind::Vector { fill, .. } = &mut card.kind {
            *fill = Some(AuthoredColor::Srgb {
                r: 0.9,
                g: 0.5,
                b: 0.2,
                a: 1.0,
            });
        }
        card.opacity = opacity;
        card.effects = vec![chitrakar_doc::Effect::Outline {
            width: 2.0,
            color: AuthoredColor::Srgb {
                r: 0.2,
                g: 0.7,
                b: 0.3,
                a: 1.0,
            },
            opacity: 1.0,
        }];
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(card),
        })
        .unwrap();
        doc
    }

    /// A faded layer's outline comes back. A shape's fade goes to SVG as
    /// the fade of its paint and came back as the colour's own, and the
    /// engine measures an outline's edge from the layer's opacity — half
    /// of it — so the edge came back at half of full cover, which a shape
    /// at a third never reaches. The exporter writes the edge into the
    /// filter; read back from it, the fade is the layer's again.
    #[test]
    fn a_faded_layer_keeps_its_outline_through_svg() {
        for opacity in [0.3f32, 0.6] {
            let doc = outlined_card(opacity);
            let svg = crate::export_svg(&doc).unwrap();
            let back = import_svg(svg.as_bytes()).unwrap();
            let card = &back.shapes[0];
            assert!(
                (card.opacity - opacity).abs() < 1e-3,
                "the fade is the layer's again: {} for {opacity}",
                card.opacity
            );
            let NodeKind::Vector {
                fill: Some(fill), ..
            } = &card.kind
            else {
                panic!("a card with a fill");
            };
            assert!(
                (fill.alpha() - 1.0).abs() < 1e-3,
                "and out of its paint: {fill:?}"
            );
            let worst = worst_apart(&doc, &brought_in(&svg, 64, 48));
            assert!(
                worst < 0.02,
                "at {opacity} it came back as it was, worst {worst}"
            );
        }
        // At full opacity there is no fade to give back.
        let svg = crate::export_svg(&outlined_card(1.0)).unwrap();
        assert_eq!(import_svg(svg.as_bytes()).unwrap().shapes[0].opacity, 1.0);
    }

    /// A group's effects over a layer with effects of its own come back,
    /// both. The layer's filter sits inside the group's in the file, and
    /// the group's effects were carried down to the one layer under it —
    /// which already had its own, and kept those instead.
    #[test]
    fn a_groups_effects_over_a_layers_own_come_back_both() {
        use chitrakar_doc::Effect;
        let mut doc = outlined_card(1.0);
        let root = doc.root();
        let card = doc.children_of(root).unwrap()[0];
        let outline = doc.node(card).unwrap().effects.clone();
        doc.apply(Command::SetEffects {
            id: card,
            effects: vec![Effect::DropShadow {
                dx: 3.0,
                dy: 4.0,
                blur: 1.5,
                color: AuthoredColor::Srgb {
                    r: 0.1,
                    g: 0.1,
                    b: 0.3,
                    a: 1.0,
                },
                opacity: 0.7,
            }],
        })
        .unwrap();
        let group = doc.peek_next_id();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::group("held")),
        })
        .unwrap();
        doc.apply(Command::MoveNode {
            id: card,
            parent: group,
            index: 0,
        })
        .unwrap();
        doc.apply(Command::SetEffects {
            id: group,
            effects: outline,
        })
        .unwrap();
        let svg = crate::export_svg(&doc).unwrap();
        let back = import_svg(svg.as_bytes()).unwrap();
        let kinds = |fx: &[Effect]| {
            fx.iter()
                .map(|e| match e {
                    Effect::DropShadow { .. } => "shadow",
                    Effect::Outline { .. } => "outline",
                    Effect::InnerShadow { .. } => "inner",
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            back.groups
                .iter()
                .map(|g| kinds(&g.effects))
                .collect::<Vec<_>>(),
            vec![vec!["outline"]],
            "the group's outline is the group's"
        );
        assert_eq!(
            kinds(&back.shapes[0].effects),
            vec!["shadow"],
            "the shadow the card's"
        );
        let worst = worst_apart(&doc, &brought_in(&svg, 64, 48));
        assert!(
            worst < 0.03,
            "and the page came back as it was, worst {worst}"
        );
    }

    /// Every blur the engine has comes back at its own radius: what the
    /// exporter writes for it (`√(r(r+1))` at the engine's radius `r`)
    /// is read back to a blur of that radius, in the page's units and in
    /// a space scaled by two.
    #[test]
    fn a_blur_exported_comes_back_at_its_radius() {
        use chitrakar_render::blur::plane_radius;
        for scale in [1.0f32, 2.0] {
            let page = Page {
                at: usvg::Transform::from_scale(scale, scale),
                edge: std::cell::Cell::new(None),
            };
            for k in 2..240 {
                let blur = k as f32 * 0.05;
                let r = plane_radius(blur * scale) as f32;
                let written = (r * (r + 1.0)).sqrt() / scale;
                let back = page.blur(written);
                assert_eq!(
                    plane_radius(back * scale),
                    plane_radius(blur * scale),
                    "a blur of {blur} at scale {scale} wrote {written} and read {back}"
                );
            }
        }
    }
}
