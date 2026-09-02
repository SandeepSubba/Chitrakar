//! Text rasterization: live [`TextSpec`] objects render to a coverage
//! bitmap at their natural size, then blit through the node transform like
//! any raster. A bundled DejaVu Sans (Bitstream Vera license — see
//! assets/DejaVuSans-LICENSE.txt) is shaped by rustybuzz — so the font's
//! own kerning, ligatures and mark positioning apply, and a script that
//! reorders or joins will work the day a font for it is bundled — and its
//! glyphs are rasterized by ab_glyph. Lines break on `\n`, and a block
//! given a width wraps words to it.

use ab_glyph::{Font, FontRef, ScaleFont};
use chitrakar_doc::{TextSpec, VectorShape};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// The bundled face's name, and what any name nothing answers to means.
pub const DEFAULT_FONT: &str = "DejaVu Sans";

/// One face, as both of its readers see it: ab_glyph for the outlines,
/// rustybuzz for the shaping. Both borrow the same bytes for the life of
/// the process — a font, once registered, is never taken away, which is
/// what lets the readers be handed out as plain references.
pub struct Fonts {
    bytes: &'static [u8],
    font: FontRef<'static>,
    face: rustybuzz::Face<'static>,
}

fn parse(bytes: &'static [u8]) -> Result<Fonts, String> {
    Ok(Fonts {
        bytes,
        font: FontRef::try_from_slice(bytes).map_err(|e| e.to_string())?,
        face: rustybuzz::Face::from_slice(bytes, 0).ok_or("not a font the shaper can read")?,
    })
}

fn bundled() -> &'static Fonts {
    static BUNDLED: OnceLock<Fonts> = OnceLock::new();
    BUNDLED.get_or_init(|| parse(FONT_BYTES).expect("bundled font must parse"))
}

fn registry() -> &'static RwLock<HashMap<String, &'static Fonts>> {
    static REGISTRY: OnceLock<RwLock<HashMap<String, &'static Fonts>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Make a face available under `name` for the rest of the process. The
/// bytes are kept for good: a registered font is referenced from every
/// text block that names it, so there is no moment it could be dropped.
pub fn register_font(name: &str, bytes: Vec<u8>) -> Result<(), String> {
    // The bundled face answers to its own name and to none: a face
    // registered under either would be listed and never drawn.
    if name.trim().is_empty() || name == DEFAULT_FONT {
        return Err(format!(
            "\"{name}\" is the bundled face's name; register the file under another"
        ));
    }
    let fonts: &'static Fonts = Box::leak(Box::new(parse(Box::leak(bytes.into_boxed_slice()))?));
    registry()
        .write()
        .map_err(|_| "font registry poisoned")?
        .insert(name.to_string(), fonts);
    Ok(())
}

/// Whether a face answers to `name` — the bundled one always does.
pub fn has_font(name: &str) -> bool {
    name.is_empty()
        || name == DEFAULT_FONT
        || registry()
            .read()
            .map(|r| r.contains_key(name))
            .unwrap_or(false)
}

/// The file a registered face was read from, so a document can carry it.
/// The bundled face is not offered: every build has it, so there is
/// nothing to carry.
pub fn font_bytes(name: &str) -> Option<&'static [u8]> {
    if name.is_empty() || name == DEFAULT_FONT {
        return None;
    }
    registry().read().ok()?.get(name).map(|f| f.bytes)
}

/// Every face that can be named: the bundled one first, then the rest in
/// the order they sort.
pub fn font_names() -> Vec<String> {
    let mut names: Vec<String> = registry()
        .read()
        .map(|r| r.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names.insert(0, DEFAULT_FONT.to_string());
    names
}

/// The lean of a synthesized italic: horizontal run per unit of rise,
/// about 11°, which is where most oblique faces sit.
const SLANT: f32 = 0.2;

/// The face a block is set in — the one it names, or the bundled one when
/// it names nothing or something this process has not been given. An
/// italic block gets that face's oblique twin when one is registered;
/// otherwise the upright face, which the rasterizer leans itself.
fn fonts_for(spec: &TextSpec) -> &'static Fonts {
    if spec.italic {
        if let Some(oblique) = oblique_for(spec) {
            return oblique;
        }
    }
    upright_for(spec)
}

fn upright_for(spec: &TextSpec) -> &'static Fonts {
    if spec.font.is_empty() || spec.font == DEFAULT_FONT {
        return bundled();
    }
    registry()
        .read()
        .ok()
        .and_then(|r| r.get(&spec.font).copied())
        .unwrap_or_else(bundled)
}

/// The registered oblique twin of the block's face — "… Oblique" or
/// "… Italic" — or the face itself when it already is one, so an italic
/// set directly in an oblique face is not leaned a second time.
fn oblique_for(spec: &TextSpec) -> Option<&'static Fonts> {
    let name = oblique_name(spec)?;
    registry().read().ok()?.get(&name).copied()
}

/// The registered name of the block's oblique twin, if there is one.
fn oblique_name(spec: &TextSpec) -> Option<String> {
    let base = if spec.font.is_empty() {
        DEFAULT_FONT
    } else {
        spec.font.as_str()
    };
    let registry = registry().read().ok()?;
    if base.ends_with(" Oblique") || base.ends_with(" Italic") {
        return registry.contains_key(base).then(|| base.to_string());
    }
    ["Oblique", "Italic"]
        .iter()
        .map(|suffix| format!("{base} {suffix}"))
        .find(|name| registry.contains_key(name))
}

/// The registered names a block draws with: the face it names, and the
/// oblique twin an italic block is set in when one is registered — what
/// a file has to carry for the block to read the same elsewhere.
pub fn faces_used(spec: &TextSpec) -> Vec<String> {
    let mut names = Vec::new();
    if !spec.font.is_empty() {
        names.push(spec.font.clone());
    }
    if spec.italic {
        if let Some(twin) = oblique_name(spec) {
            if !names.contains(&twin) {
                names.push(twin);
            }
        }
    }
    names
}

/// Whether the block leans by the rasterizer's hand rather than the font's.
fn synthesized_lean(spec: &TextSpec) -> bool {
    spec.italic && oblique_for(spec).is_none()
}

/// How far a synthesized lean pushes ink past the block's edges: the
/// descenders lean left of the first glyph's origin, the ascenders right
/// of the last one's. Nothing, when the lean is the font's own.
fn lean_room(spec: &TextSpec, font: &impl ScaleFont<&'static FontRef<'static>>) -> (f32, f32) {
    if synthesized_lean(spec) {
        (SLANT * -font.descent(), SLANT * font.ascent())
    } else {
        (0.0, 0.0)
    }
}

/// A glyph's outline leaned by `slant` around its baseline, then scaled
/// and placed like any other: a synthesized italic. Font units have y up,
/// so the top of a glyph leans furthest right and a descender leans left.
fn leaned(
    font: &FontRef<'static>,
    glyph: ab_glyph::Glyph,
    slant: f32,
    factor: ab_glyph::PxScaleFactor,
) -> Option<ab_glyph::OutlinedGlyph> {
    use ab_glyph::{point, OutlineCurve, Point};
    let lean = |p: Point| point(p.x + slant * p.y, p.y);
    let mut outline = font.outline(glyph.id)?;
    for curve in &mut outline.curves {
        match curve {
            OutlineCurve::Line(a, b) => {
                *a = lean(*a);
                *b = lean(*b);
            }
            OutlineCurve::Quad(a, b, c) => {
                *a = lean(*a);
                *b = lean(*b);
                *c = lean(*c);
            }
            OutlineCurve::Cubic(a, b, c, d) => {
                *a = lean(*a);
                *b = lean(*b);
                *c = lean(*c);
                *d = lean(*d);
            }
        }
    }
    // The lowest point of the box leans furthest left and the highest
    // furthest right — whichever of the box's corners holds each, since
    // ab_glyph keeps the top edge in `min` and the bottom in `max`.
    let b = outline.bounds;
    let (low, high) = (b.min.y.min(b.max.y), b.min.y.max(b.max.y));
    outline.bounds = ab_glyph::Rect {
        min: point(b.min.x + slant * low, b.min.y),
        max: point(b.max.x + slant * high, b.max.y),
    };
    Some(ab_glyph::OutlinedGlyph::new(glyph, outline, factor))
}

/// One positioned glyph of a shaped line: which glyph, where its origin
/// sits along the baseline, and how far the pen moves past it — all in
/// raster pixels at the scale the line was shaped for.
struct Shaped {
    id: ab_glyph::GlyphId,
    x: f32,
    y: f32,
    /// How far the pen moves past this glyph, tracking aside.
    advance: f32,
    /// Byte offset into the line of the text this glyph came from.
    cluster: usize,
}

/// Shape one line: the font decides which glyphs the text becomes and where
/// each goes, which is what turns "fi" into a ligature, tightens "AV", and
/// puts an accent over its base. Tracking is added after each glyph, the
/// way a typesetter letter-spaces: between the glyphs, never inside a
/// ligature's own shape.
fn shape_line(line: &str, spec: &TextSpec, scale: f32) -> (Vec<Shaped>, f32) {
    let fonts = fonts_for(spec);
    let face = &fonts.face;
    let px = spec.size.max(0.1) * scale;
    // `size` is ab_glyph's scale — pixels per ascent-to-descent height, not
    // per em — so the shaper's font units are converted by that same
    // height. Using the em here instead sets every advance and offset 16%
    // adrift of the outlines they position in DejaVu Sans.
    let unit = px / fonts.font.height_unscaled();
    let track = tracking(spec, scale);
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(line);
    let out = rustybuzz::shape(face, &[], buffer);
    let mut glyphs = Vec::with_capacity(out.len());
    let mut pen = 0f32;
    for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
        glyphs.push(Shaped {
            id: ab_glyph::GlyphId(info.glyph_id as u16),
            x: pen + pos.x_offset as f32 * unit,
            y: -(pos.y_offset as f32) * unit,
            advance: pos.x_advance as f32 * unit,
            cluster: info.cluster as usize,
        });
        pen += pos.x_advance as f32 * unit + track;
    }
    // The last glyph's tracking is not part of the line's width: nothing
    // follows it to be spaced from.
    if !glyphs.is_empty() {
        pen -= track;
    }
    (glyphs, pen.max(0.0))
}

/// Where a line's start sits, given the block's alignment and the room a
/// lean takes: the x its first glyph's origin lands on.
fn line_start(spec: &TextSpec, l: &Layout, line_no: usize) -> f32 {
    let slack = l.width - l.room.0 - l.room.1 - l.widths[line_no];
    l.room.0
        + match spec.align {
            chitrakar_doc::TextAlign::Left => 0.0,
            chitrakar_doc::TextAlign::Center => slack / 2.0,
            chitrakar_doc::TextAlign::Right => slack,
        }
}

/// The bands an underline and a strike-through paint, `[x0, y0, x1, y1]`
/// in raster pixels at `scale`: one per line of text that has any
/// width, under its baseline or through its x-height, as thick as a
/// twentieth of the em and never thinner than a pixel.
fn decoration_bands(spec: &TextSpec, l: &Layout, scale: f32) -> Vec<[f32; 4]> {
    if !spec.underline && !spec.strike {
        return Vec::new();
    }
    let fonts = fonts_for(spec);
    let px = spec.size.max(0.1) * scale;
    let font = fonts.font.as_scaled(px);
    let em = px * fonts.font.units_per_em().unwrap_or(1000.0) / fonts.font.height_unscaled();
    let thickness = (em * 0.05).max(1.0);
    let step = line_step(&font, spec);
    let mut bands = Vec::new();
    for (line_no, width) in l.widths.iter().enumerate() {
        if *width <= 0.0 {
            continue;
        }
        let baseline = line_no as f32 * step + font.ascent();
        let x0 = line_start(spec, l, line_no);
        let x1 = x0 + width;
        if spec.underline {
            let y0 = baseline + em * 0.1;
            bands.push([x0, y0, x1, y0 + thickness]);
        }
        if spec.strike {
            let y0 = baseline - em * 0.3;
            bands.push([x0, y0, x1, y0 + thickness]);
        }
    }
    bands
}

/// A guide flattened to the polyline the text walks: the points and
/// whether it closes. An open path keeps its ends; a rectangle or an
/// ellipse is a ring.
pub fn guide_points(spec: &TextSpec) -> Option<(Vec<[f32; 2]>, bool)> {
    let shape = spec.along.as_ref()?;
    match shape {
        VectorShape::Path { closed, .. } => {
            let flat = crate::flatten_shape(shape);
            let VectorShape::Path { points, .. } = flat.as_ref() else {
                return None;
            };
            (points.len() >= 2).then(|| (points.clone(), *closed))
        }
        _ => crate::shape_rings(shape)
            .into_iter()
            .next()
            .filter(|ring| ring.len() >= 2)
            .map(|ring| (ring, true)),
    }
}

/// A guide with its arc length tabulated, so a distance along it finds
/// a point and the direction there.
struct Guide {
    points: Vec<[f32; 2]>,
    /// Arc length at each point, and at the closing segment's end.
    at: Vec<f32>,
    closed: bool,
}

impl Guide {
    fn new(points: Vec<[f32; 2]>, closed: bool, scale: f32) -> Guide {
        let points: Vec<[f32; 2]> = points
            .iter()
            .map(|p| [p[0] * scale, p[1] * scale])
            .collect();
        let mut at = vec![0.0];
        let n = points.len();
        let segments = if closed { n } else { n - 1 };
        for i in 0..segments {
            let (a, b) = (points[i], points[(i + 1) % n]);
            at.push(at[i] + ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt());
        }
        Guide { points, at, closed }
    }

    fn length(&self) -> f32 {
        *self.at.last().unwrap_or(&0.0)
    }

    /// The point `s` along the guide and the unit direction there, or
    /// nothing when an open guide has ended.
    fn at(&self, s: f32) -> Option<([f32; 2], [f32; 2])> {
        let total = self.length();
        if total <= 0.0 {
            return None;
        }
        let s = if self.closed {
            s.rem_euclid(total)
        } else if s < 0.0 || s > total {
            return None;
        } else {
            s
        };
        let n = self.points.len();
        let mut i = self.at.partition_point(|&a| a <= s).saturating_sub(1);
        i = i.min(self.at.len() - 2);
        let (a, b) = (self.points[i], self.points[(i + 1) % n]);
        let len = (self.at[i + 1] - self.at[i]).max(1e-6);
        let t = ((s - self.at[i]) / len).clamp(0.0, 1.0);
        let dir = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        Some(([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], dir))
    }
}

/// One glyph placed along a guide: where its origin lands, which way
/// its baseline runs, and the text it stands for.
struct AlongGlyph {
    id: ab_glyph::GlyphId,
    origin: [f32; 2],
    angle: f32,
    cluster: usize,
}

/// The text as one run of glyphs along its guide, at `scale`: lines are
/// joined with spaces, since a guide has no lines. Each glyph's middle
/// sits on the guide, `along_offset` in from its start, turned to the
/// direction there; glyphs past an open guide's end are left out.
fn along_glyphs(spec: &TextSpec, scale: f32) -> (Vec<AlongGlyph>, String) {
    let line = spec.text.replace('\n', " ");
    let Some((points, closed)) = guide_points(spec) else {
        return (Vec::new(), line);
    };
    let guide = Guide::new(points, closed, scale);
    let (shaped, _) = shape_line(&line, spec, scale);
    let mut out = Vec::with_capacity(shaped.len());
    for g in shaped {
        let s = spec.along_offset * scale + g.x + g.advance / 2.0;
        let Some((p, t)) = guide.at(s) else {
            continue;
        };
        // "Up" for the glyph is the left of the direction of travel, in
        // a y-down space; a mark's offset rides along that.
        let n = [t[1], -t[0]];
        let half = g.advance / 2.0;
        out.push(AlongGlyph {
            id: g.id,
            origin: [
                p[0] - t[0] * half - n[0] * g.y,
                p[1] - t[1] * half - n[1] * g.y,
            ],
            angle: t[1].atan2(t[0]),
            cluster: g.cluster,
        });
    }
    (out, line)
}

/// A glyph's outline leaned by `slant` and turned by `angle`, then scaled
/// and placed like any other. The outline is in font units with y up, so
/// a turn of `angle` on the page is a turn of `-angle` here.
fn turned(
    font: &FontRef<'static>,
    glyph: ab_glyph::Glyph,
    slant: f32,
    angle: f32,
    factor: ab_glyph::PxScaleFactor,
) -> Option<ab_glyph::OutlinedGlyph> {
    use ab_glyph::{point, OutlineCurve, Point};
    let (sin, cos) = angle.sin_cos();
    let turn = |p: Point| {
        let x = p.x + slant * p.y;
        point(x * cos + p.y * sin, -x * sin + p.y * cos)
    };
    let mut outline = font.outline(glyph.id)?;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut note = |p: Point| {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
        p
    };
    for curve in &mut outline.curves {
        match curve {
            OutlineCurve::Line(a, b) => {
                *a = note(turn(*a));
                *b = note(turn(*b));
            }
            OutlineCurve::Quad(a, b, c) => {
                *a = note(turn(*a));
                *b = note(turn(*b));
                *c = note(turn(*c));
            }
            OutlineCurve::Cubic(a, b, c, d) => {
                *a = note(turn(*a));
                *b = note(turn(*b));
                *c = note(turn(*c));
                *d = note(turn(*d));
            }
        }
    }
    if x0 > x1 {
        return None;
    }
    // ab_glyph keeps the top edge in `min` and the bottom in `max`.
    outline.bounds = ab_glyph::Rect {
        min: point(x0, y1),
        max: point(x1, y0),
    };
    Some(ab_glyph::OutlinedGlyph::new(glyph, outline, factor))
}

/// The glyphs of a block along its guide as outlines ready to draw, at
/// `scale`, with the box they cover: `(outlines, [x0, y0, x1, y1])`.
fn along_outlines(spec: &TextSpec, scale: f32) -> (Vec<ab_glyph::OutlinedGlyph>, [f32; 4]) {
    let fonts = fonts_for(spec);
    let px = spec.size.max(0.1) * scale;
    let font = fonts.font.as_scaled(px);
    let slant = if synthesized_lean(spec) { SLANT } else { 0.0 };
    let (glyphs, _) = along_glyphs(spec, scale);
    let mut out = Vec::with_capacity(glyphs.len());
    let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for g in glyphs {
        let glyph =
            g.id.with_scale_and_position(px, ab_glyph::point(g.origin[0], g.origin[1]));
        if let Some(outlined) = turned(font.font, glyph, slant, g.angle, font.scale_factor()) {
            let b = outlined.px_bounds();
            bounds = [
                bounds[0].min(b.min.x),
                bounds[1].min(b.min.y),
                bounds[2].max(b.max.x),
                bounds[3].max(b.max.y),
            ];
            out.push(outlined);
        }
    }
    if out.is_empty() {
        bounds = [0.0, 0.0, 0.0, 0.0];
    }
    (out, bounds)
}

/// The box a block covers in its own space, `[x0, y0, x1, y1]`: from the
/// origin for text in lines, wherever the glyphs land for text along a
/// guide.
pub fn bounds(spec: &TextSpec) -> [f32; 4] {
    if spec.along.is_some() {
        let (_, b) = along_outlines(spec, 1.0);
        [
            b[0].floor(),
            b[1].floor(),
            b[2].ceil().max(b[0].floor() + 1.0),
            b[3].ceil().max(b[1].floor() + 1.0),
        ]
    } else {
        let (w, h) = measure(spec);
        [0.0, 0.0, w, h]
    }
}

/// The file behind the face a block draws with, for an exporter that
/// embeds it, and what an embedder needs to know: the name the face was
/// registered under, its units per em, the ascent-to-descent height in
/// those units — what `size` scales — and the ascent and descent.
pub struct FaceFile {
    pub name: String,
    pub bytes: &'static [u8],
    pub units_per_em: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
    pub glyph_count: usize,
}

impl FaceFile {
    /// A glyph's advance, in thousandths of an em — a PDF width.
    pub fn advance(&self, glyph: u16) -> f32 {
        fonts_named(&self.name)
            .font
            .h_advance_unscaled(ab_glyph::GlyphId(glyph))
            * 1000.0
            / self.units_per_em
    }
}

/// One glyph as placed: which, where its origin sits (document pixels
/// from the block's top-left, on its line's baseline), and the text it
/// stands for — empty for a glyph that shares its text with the one
/// before it, as the pieces of a decomposed character do.
pub struct PlacedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    /// Which way the baseline runs, in radians on the page; zero for
    /// text in lines.
    pub angle: f32,
    pub text: String,
}

/// A block as glyphs, for an exporter that sets type itself: the face,
/// the em it is scaled to, the lean the rasterizer would add (zero when
/// the face leans by itself), and every glyph placed as the raster
/// places it.
pub struct PlacedText {
    pub face: FaceFile,
    pub em: f32,
    pub lean: f32,
    pub glyphs: Vec<PlacedGlyph>,
    /// Underline and strike-through bands, `[x0, y0, x1, y1]` in document
    /// pixels, to draw in the block's colour.
    pub decorations: Vec<[f32; 4]>,
}

/// The name of the face a block draws with.
fn face_name(spec: &TextSpec) -> String {
    if spec.italic {
        if let Some(twin) = oblique_name(spec) {
            return twin;
        }
    }
    if !spec.font.is_empty() && spec.font != DEFAULT_FONT && has_font(&spec.font) {
        spec.font.clone()
    } else {
        DEFAULT_FONT.to_string()
    }
}

fn fonts_named(name: &str) -> &'static Fonts {
    if name == DEFAULT_FONT {
        return bundled();
    }
    registry()
        .read()
        .ok()
        .and_then(|r| r.get(name).copied())
        .unwrap_or_else(bundled)
}

/// The text each glyph of a shaped run stands for: from its cluster to
/// the next that differs, so a ligature keeps both its letters and the
/// pieces of a decomposed character share one (the later ones get none).
fn cluster_texts(line: &str, clusters: &[usize]) -> Vec<String> {
    clusters
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            if i > 0 && clusters[i - 1] == c {
                return String::new();
            }
            let end = clusters[i..]
                .iter()
                .copied()
                .find(|&d| d != c)
                .unwrap_or(line.len())
                .max(c);
            line.get(c..end).unwrap_or("").to_string()
        })
        .collect()
}

pub fn placed(spec: &TextSpec) -> PlacedText {
    let l = layout(spec, 1.0);
    let fonts = fonts_for(spec);
    let px = spec.size.max(0.1);
    let font = fonts.font.as_scaled(px);
    let step = line_step(&font, spec);
    let mut glyphs = Vec::new();
    if spec.along.is_some() {
        let (along, line) = along_glyphs(spec, 1.0);
        let clusters: Vec<usize> = along.iter().map(|g| g.cluster).collect();
        for (g, text) in along.iter().zip(cluster_texts(&line, &clusters)) {
            glyphs.push(PlacedGlyph {
                id: g.id.0,
                x: g.origin[0],
                y: g.origin[1],
                angle: g.angle,
                text,
            });
        }
    }
    for (line_no, line) in l.lines.iter().enumerate() {
        if spec.along.is_some() {
            break;
        }
        let baseline = line_no as f32 * step + font.ascent();
        let (shaped, _) = shape_line(line, spec, 1.0);
        let start = line_start(spec, &l, line_no);
        let clusters: Vec<usize> = shaped.iter().map(|g| g.cluster).collect();
        for (g, text) in shaped.iter().zip(cluster_texts(line, &clusters)) {
            glyphs.push(PlacedGlyph {
                id: g.id.0,
                x: start + g.x,
                y: baseline + g.y,
                angle: 0.0,
                text,
            });
        }
    }
    let name = face_name(spec);
    let upem = fonts.font.units_per_em().unwrap_or(1000.0);
    PlacedText {
        face: FaceFile {
            bytes: fonts.bytes,
            units_per_em: upem,
            height: fonts.font.height_unscaled(),
            ascent: fonts.font.ascent_unscaled(),
            descent: fonts.font.descent_unscaled(),
            glyph_count: fonts.font.glyph_count(),
            name,
        },
        em: px * upem / fonts.font.height_unscaled(),
        lean: if synthesized_lean(spec) { SLANT } else { 0.0 },
        decorations: if spec.along.is_some() {
            Vec::new()
        } else {
            decoration_bands(spec, &l, 1.0)
        },
        glyphs,
    }
}

/// A rasterized text block: alpha coverage (0..=1) at natural size.
pub struct TextRaster {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<f32>,
    /// Where the raster's top-left sits in the block's own space, in
    /// natural (unscaled) pixels: the origin for text in lines, and the
    /// glyphs' extent for text along a guide, which can run anywhere.
    pub origin: (f32, f32),
}

impl TextRaster {
    pub fn sample(&self, x: u32, y: u32) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.coverage[(y * self.width + x) as usize]
    }

    /// Coverage at a fractional position, interpolated between the four
    /// texels around it. Off the edge reads as no ink, so glyphs fade out
    /// rather than smearing their border row.
    pub fn sample_at(&self, x: f32, y: f32) -> f32 {
        let (u, v) = (x - 0.5, y - 0.5);
        let (u0, v0) = (u.floor(), v.floor());
        let (fx, fy) = (u - u0, v - v0);
        let at = |i: f32, j: f32| {
            if i < 0.0 || j < 0.0 {
                0.0
            } else {
                self.sample(i as u32, j as u32)
            }
        };
        let top = at(u0, v0) * (1.0 - fx) + at(u0 + 1.0, v0) * fx;
        let bottom = at(u0, v0 + 1.0) * (1.0 - fx) + at(u0 + 1.0, v0 + 1.0) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// A block as it is set, for an exporter that places text itself: each
/// line as the wrapping left it, with its shaped width; where the first
/// baseline sits below the block's top and how far each next one is
/// below it; the block's size, the width lines are aligned within and
/// where that starts (a synthesized lean takes room on the left); and the
/// em size the face is scaled to — all in document pixels.
pub struct SetBlock {
    pub lines: Vec<(String, f32)>,
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub step: f32,
    pub inset: f32,
    pub inner: f32,
    pub em: f32,
}

pub fn set(spec: &TextSpec) -> SetBlock {
    let l = layout(spec, 1.0);
    let fonts = fonts_for(spec);
    let px = spec.size.max(0.1);
    let font = fonts.font.as_scaled(px);
    SetBlock {
        inner: l.width - l.room.0 - l.room.1,
        inset: l.room.0,
        lines: l.lines.into_iter().zip(l.widths).collect(),
        width: l.width,
        height: l.height,
        ascent: font.ascent(),
        step: line_step(&font, spec),
        // `size` is the ascent-to-descent height; the em is what a font
        // size means everywhere else.
        em: px * fonts.font.units_per_em().unwrap_or(1000.0) / fonts.font.height_unscaled(),
    }
}

/// Natural (untransformed) size of a text block in document pixels.
pub fn measure(spec: &TextSpec) -> (f32, f32) {
    measure_at(spec, 1.0)
}

/// The same, for a block rasterized at `scale`. Metrics are linear in the
/// font size, so this is the natural measure times the scale — but taken
/// from the scaled font, so it agrees with the raster to the pixel.
fn measure_at(spec: &TextSpec, scale: f32) -> (f32, f32) {
    let l = layout(spec, scale);
    (l.width, l.height)
}

/// A block set in lines: the lines themselves, each one's shaped width,
/// and the block's size. Computed once and used by measure and raster
/// alike, so a rasterize does not shape everything twice over.
struct Layout {
    lines: Vec<String>,
    widths: Vec<f32>,
    width: f32,
    height: f32,
    /// Room a synthesized lean takes inside `width`, left and right of
    /// the lines: where the first glyph's origin sits, and what the block
    /// holds past the last one's.
    room: (f32, f32),
}

fn layout(spec: &TextSpec, scale: f32) -> Layout {
    let font = fonts_for(spec).font.as_scaled(spec.size.max(0.1) * scale);
    let step = line_step(&font, spec);
    let lines = lines_of(spec, scale);
    let widths: Vec<f32> = lines.iter().map(|l| line_width(l, spec, scale)).collect();
    let widest = widths.iter().cloned().fold(0f32, f32::max);
    // A wrapping block is as wide as it was told to be, so its right edge
    // holds still while the words inside it change; only a word too long
    // to fit pushes past that.
    let width = if spec.width > 0.0 {
        widest.max(spec.width * scale)
    } else {
        widest
    };
    let room = lean_room(spec, &font);
    Layout {
        height: (lines.len().max(1) as f32 * step).max(1.0),
        lines,
        widths,
        width: (width + room.0 + room.1).max(1.0),
        room,
    }
}

/// The lines the block is set in: each paragraph (a run between newlines)
/// wrapped to the block's width when it has one, greedily, at spaces.
///
/// Each word is shaped once and the break decided from the words' widths
/// plus a space's, which ignores kerning across a space — a fraction of a
/// pixel, and only for the decision; the line is then shaped whole, so the
/// kerning that shows is the font's. Lines are cut out of the paragraph as
/// it was typed, so a run of spaces stays a run of spaces, an indent stays
/// an indent, and only the single space at a cut is dropped.
fn lines_of(spec: &TextSpec, scale: f32) -> Vec<String> {
    let limit = spec.width * scale;
    let mut out = Vec::new();
    for paragraph in spec.text.split('\n') {
        if limit <= 0.0 {
            out.push(paragraph.to_string());
            continue;
        }
        let space = line_width(" ", spec, scale);
        let mut start = 0usize; // where the current line begins, in bytes
        let mut used = 0f32; // its width so far
        let mut first = true;
        let mut pos = 0usize; // where the current word begins
        for word in paragraph.split(' ') {
            let w = line_width(word, spec, scale);
            if first {
                used = w;
                first = false;
            } else if used + space + w <= limit {
                used += space + w;
            } else {
                out.push(paragraph[start..pos - 1].to_string());
                start = pos;
                used = w;
            }
            pos += word.len() + 1;
        }
        out.push(paragraph[start..].to_string());
    }
    out
}

/// Baseline-to-baseline distance: the font's own line height, scaled by
/// whatever the block asks for.
fn line_step(font: &impl ScaleFont<&'static FontRef<'static>>, spec: &TextSpec) -> f32 {
    (font.ascent() - font.descent() + font.line_gap()) * spec.line_scale()
}

/// Tracking in raster pixels. Quoted in ems, so it follows the size.
fn tracking(spec: &TextSpec, scale: f32) -> f32 {
    spec.letter_spacing * spec.size.max(0.1) * scale
}

fn line_width(line: &str, spec: &TextSpec, scale: f32) -> f32 {
    shape_line(line, spec, scale).1
}

/// Rasterize the block at natural size.
pub fn rasterize(spec: &TextSpec) -> TextRaster {
    rasterize_at(spec, 1.0)
}

/// Rasterize the block at `scale` times its natural size.
///
/// Text is outlines, not pixels, so it should be rasterized at the size it
/// will actually be seen at: rendering at natural size and then magnifying
/// the bitmap is the one thing that makes vector type look like a scanned
/// letter. Callers pass the scale their transform imposes and index the
/// result by natural-size coordinates times the same scale.
pub fn rasterize_at(spec: &TextSpec, scale: f32) -> TextRaster {
    let scale = scale.max(0.01);
    if spec.along.is_some() {
        return rasterize_along(spec, scale);
    }
    let l = layout(spec, scale);
    let (w, h) = (l.width, l.height);
    let (width, height) = (w.ceil().max(1.0) as u32, h.ceil().max(1.0) as u32);
    let mut coverage = vec![0f32; (width * height) as usize];
    // Every metric below comes from the scaled font, so advances, kerning
    // and line height are all in raster pixels already.
    let font = fonts_for(spec).font.as_scaled(spec.size.max(0.1) * scale);
    let step = line_step(&font, spec);
    let slant = if synthesized_lean(spec) { SLANT } else { 0.0 };

    for (line_no, line) in l.lines.iter().enumerate() {
        let baseline = line_no as f32 * step + font.ascent();
        let (glyphs, _) = shape_line(line, spec, scale);
        // Alignment is within the block's own width, which is the widest
        // line: a short line is pushed right by the slack it leaves.
        let start = line_start(spec, &l, line_no);
        for g in glyphs {
            let glyph = g.id.with_scale_and_position(
                spec.size.max(0.1) * scale,
                ab_glyph::point(start + g.x, baseline + g.y),
            );
            let outlined = if slant > 0.0 {
                leaned(font.font, glyph, slant, font.scale_factor())
            } else {
                font.font.outline_glyph(glyph)
            };
            if let Some(outlined) = outlined {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, c| {
                    let x = bounds.min.x as i32 + gx as i32;
                    let y = bounds.min.y as i32 + gy as i32;
                    if x >= 0 && y >= 0 && (x as u32) < width && (y as u32) < height {
                        let i = (y as u32 * width + x as u32) as usize;
                        coverage[i] = coverage[i].max(c);
                    }
                });
            }
        }
    }
    // Underline and strike-through: solid bands, clipped to the raster,
    // that also draw the space a line is spaced with.
    for [x0, y0, x1, y1] in decoration_bands(spec, &l, scale) {
        let (cx0, cx1) = (
            (x0.round().max(0.0)) as u32,
            (x1.round().max(0.0) as u32).min(width),
        );
        let (cy0, cy1) = (
            (y0.round().max(0.0)) as u32,
            (y1.round().max(0.0) as u32).min(height),
        );
        for y in cy0..cy1 {
            for x in cx0..cx1 {
                coverage[(y * width + x) as usize] = 1.0;
            }
        }
    }
    TextRaster {
        width,
        height,
        coverage,
        origin: (0.0, 0.0),
    }
}

/// Text along its guide: the turned outlines, drawn into a raster just
/// big enough for where they land.
fn rasterize_along(spec: &TextSpec, scale: f32) -> TextRaster {
    let (outlines, b) = along_outlines(spec, scale);
    let (x0, y0) = (b[0].floor(), b[1].floor());
    let (width, height) = (
        (b[2].ceil() - x0).max(1.0) as u32,
        (b[3].ceil() - y0).max(1.0) as u32,
    );
    let mut coverage = vec![0f32; (width * height) as usize];
    for outlined in outlines {
        let bounds = outlined.px_bounds();
        outlined.draw(|gx, gy, c| {
            let x = (bounds.min.x - x0) as i32 + gx as i32;
            let y = (bounds.min.y - y0) as i32 + gy as i32;
            if x >= 0 && y >= 0 && (x as u32) < width && (y as u32) < height {
                let i = (y as u32 * width + x as u32) as usize;
                coverage[i] = coverage[i].max(c);
            }
        });
    }
    TextRaster {
        width,
        height,
        coverage,
        origin: (x0 / scale, y0 / scale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::AuthoredColor;

    fn spec(text: &str, size: f32) -> TextSpec {
        TextSpec::new(
            text,
            size,
            AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        )
    }

    #[test]
    fn measure_grows_with_content_and_size() {
        let (w1, h1) = measure(&spec("Hi", 24.0));
        let (w2, _) = measure(&spec("Hi there", 24.0));
        let (w3, h3) = measure(&spec("Hi", 48.0));
        let (_, h4) = measure(&spec("Hi\nHo", 24.0));
        assert!(w2 > w1, "longer text is wider");
        assert!(w3 > w1 && h3 > h1, "bigger size scales both axes");
        assert!(h4 > h1 * 1.8, "two lines roughly double the height");
    }

    #[test]
    fn rasterized_glyphs_produce_ink() {
        let raster = rasterize(&spec("I", 32.0));
        let total: f32 = raster.coverage.iter().sum();
        assert!(total > 10.0, "the glyph left ink, got {total}");
        // Ink stays inside the measured box.
        assert!(raster.width >= 2 && raster.height >= 20);
    }

    #[test]
    fn alignment_moves_the_short_line_not_the_long_one() {
        // Alignment is within the block's own width, so the widest line
        // never moves and a shorter one shifts by its slack.
        let ink_columns = |raster: &TextRaster| {
            // Where the second line's ink starts, in raster columns.
            let start = raster.height / 2;
            (0..raster.width)
                .find(|&x| (start..raster.height).any(|y| raster.sample(x, y) > 0.1))
                .unwrap_or(raster.width)
        };
        let mut left = spec("Wide line here\nx", 24.0);
        let block = measure(&left).0;
        let at_left = ink_columns(&rasterize(&left));
        left.align = chitrakar_doc::TextAlign::Right;
        let at_right = ink_columns(&rasterize(&left));
        left.align = chitrakar_doc::TextAlign::Center;
        let at_center = ink_columns(&rasterize(&left));
        assert!(
            at_left < at_center && at_center < at_right,
            "{at_left} {at_center} {at_right}"
        );
        assert!(
            (at_right as f32) > block * 0.8,
            "right-aligned, the short line ends up against the far edge"
        );
        // The block is the same size whichever way it is aligned.
        left.align = chitrakar_doc::TextAlign::Right;
        assert_eq!(measure(&left).0, block);
    }

    #[test]
    fn line_height_and_tracking_stretch_the_block() {
        let plain = spec("ab\ncd", 24.0);
        let (w0, h0) = measure(&plain);

        let mut tall = plain.clone();
        tall.line_height = 2.0;
        let (w1, h1) = measure(&tall);
        assert_eq!(w1, w0, "line spacing does not change the width");
        assert!(
            (h1 - h0 * 2.0).abs() < 1.0,
            "double spacing doubles the height: {h0} -> {h1}"
        );

        let mut tracked = plain.clone();
        tracked.letter_spacing = 0.25;
        let (w2, h2) = measure(&tracked);
        assert_eq!(h2, h0, "tracking does not change the height");
        // Two glyphs a line, so one gap a line: a quarter em at 24px.
        assert!(
            (w2 - w0 - 6.0).abs() < 0.5,
            "a quarter-em of tracking widens each line by six pixels: {w0} -> {w2}"
        );
        assert!(rasterize(&tracked).width >= w2.ceil() as u32 - 1);

        // A nonsense line height cannot collapse the block onto one line.
        let mut broken = plain.clone();
        broken.line_height = 0.0;
        assert!(measure(&broken).1 > 1.0);
    }

    #[test]
    fn a_minified_raster_keeps_every_line_and_every_alignment() {
        // Rasterizing below natural size is what happens whenever the
        // canvas is zoomed out, and it must not lose the parts of the
        // block that sit against its right and bottom edges.
        let mut block = spec("Hello there!\nhi", 48.0);
        for scale in [1.0f32, 0.779] {
            for align in [
                chitrakar_doc::TextAlign::Left,
                chitrakar_doc::TextAlign::Right,
            ] {
                block.align = align;
                let r = rasterize_at(&block, scale);
                let ink: f32 = (r.height / 2..r.height)
                    .flat_map(|y| (0..r.width).map(move |x| (x, y)))
                    .map(|(x, y)| r.sample(x, y))
                    .sum();
                assert!(
                    ink > 5.0,
                    "scale {scale}, {align:?}: the second line has {ink} ink"
                );
            }
        }
    }

    #[test]
    fn the_font_shapes_the_line_rather_than_the_characters() {
        // Shaping is the font's say over what the text becomes: two
        // letters can be one glyph, a pair can sit closer than their
        // advances add up to, and a base plus a combining mark is the same
        // as the precomposed letter. None of that falls out of walking
        // characters one at a time.
        let at = |t: &str| shape_line(t, &spec(t, 40.0), 1.0);
        assert_eq!(at("fi").0.len(), 1, "fi is a ligature in DejaVu Sans");
        assert_eq!(at("office").0.len(), 4, "and so is ffi");
        let (_, av) = at("AV");
        let apart = at("A").1 + at("V").1;
        assert!(av < apart - 1.0, "AV is kerned closer: {av} vs {apart}");
        let composed = at("\u{e9}");
        let decomposed = at("e\u{301}");
        assert_eq!(decomposed.0.len(), 1, "e + combining acute is one glyph");
        assert_eq!(
            decomposed.0[0].id, composed.0[0].id,
            "and it is the same glyph as the precomposed letter"
        );
        // Tracking spaces glyphs, not the letters inside a ligature, so a
        // ligature still counts as one.
        let mut tracked = spec("fi", 40.0);
        tracked.letter_spacing = 0.5;
        assert_eq!(shape_line("fi", &tracked, 1.0).0.len(), 1);
    }

    #[test]
    fn the_shaper_and_the_rasterizer_agree_on_the_scale() {
        // Both read the same font, but each has its own idea of what one
        // pixel of "size" is. Set the shaper's advance against ab_glyph's
        // for the same glyph at the same size: they have to be the same
        // number, or every line is set loose against its own ink.
        let s = spec("", 40.0);
        let scaled = bundled().font.as_scaled(40.0);
        for ch in ['H', 'i', 'W', '.'] {
            let (glyphs, w) = shape_line(&ch.to_string(), &s, 1.0);
            assert_eq!(glyphs.len(), 1);
            let expected = scaled.h_advance(scaled.glyph_id(ch));
            assert!(
                (w - expected).abs() < 0.05,
                "{ch}: shaped {w} vs ab_glyph {expected}"
            );
        }
    }

    #[test]
    fn wrapping_keeps_the_spaces_that_were_typed() {
        // Wrapping only ever moves words to the next line. An indent, or a
        // run of spaces inside a line, is what was typed and stays.
        let mut s = spec("    indented start", 20.0);
        s.width = 1000.0;
        assert_eq!(lines_of(&s, 1.0), vec!["    indented start"]);
        let mut runs = spec("a  b c", 20.0);
        runs.width = 1000.0;
        assert_eq!(lines_of(&runs, 1.0), vec!["a  b c"]);
        // At a cut, exactly the one separating space goes.
        let mut cut = spec("aa bb", 20.0);
        cut.width = line_width("aa", &cut, 1.0) + 1.0;
        assert_eq!(lines_of(&cut, 1.0), vec!["aa", "bb"]);
    }

    #[test]
    fn a_width_wraps_words_and_holds_the_block_to_it() {
        let loose = spec("the quick brown fox jumps over the lazy dog", 20.0);
        let (w0, h0) = measure(&loose);
        let mut wrapped = loose.clone();
        wrapped.width = w0 / 3.0;
        let (w1, h1) = measure(&wrapped);
        assert!(
            (w1 - w0 / 3.0).abs() < 0.01,
            "the block is exactly as wide as it was told: {w1} vs {}",
            w0 / 3.0
        );
        assert!(
            h1 >= h0 * 3.0,
            "and three times as tall, or more: {h0} -> {h1}"
        );
        assert_eq!(lines_of(&wrapped, 1.0).len() as f32 * h0, h1);
        // Every wrapped line fits, and no word was cut in half.
        for line in lines_of(&wrapped, 1.0) {
            assert!(
                line_width(&line, &wrapped, 1.0) <= w1 + 0.01,
                "{line:?} overflows"
            );
            assert!(!line.starts_with(' ') && !line.ends_with(' '));
        }
        assert_eq!(
            lines_of(&wrapped, 1.0).join(" "),
            loose.text,
            "the words are all still there, in order"
        );
        // A word longer than the width stands alone and overhangs.
        let mut narrow = spec("antidisestablishmentarianism ok", 20.0);
        narrow.width = 30.0;
        let lines = lines_of(&narrow, 1.0);
        assert_eq!(lines, vec!["antidisestablishmentarianism", "ok"]);
        assert!(measure(&narrow).0 > 30.0, "the block overhangs for it");
        // Explicit newlines still break, inside a wrapping block too.
        let mut both = spec("a b\nc", 20.0);
        both.width = 1000.0;
        assert_eq!(lines_of(&both, 1.0), vec!["a b", "c"]);
        // And ink lands on the later lines.
        let r = rasterize(&wrapped);
        let lower: f32 = (r.height * 2 / 3..r.height)
            .flat_map(|y| (0..r.width).map(move |x| (x, y)))
            .map(|(x, y)| r.sample(x, y))
            .sum();
        assert!(lower > 5.0, "the last third of the block has ink");
    }

    #[test]
    fn a_registered_face_sets_the_blocks_that_name_it() {
        // Bold is wider than regular for the same words; a name nothing
        // answers to falls back to the bundled face rather than failing.
        let bold = include_bytes!("../../../app/public/fonts/DejaVuSans-Bold.ttf");
        register_font("Test Bold", bold.to_vec()).unwrap();
        assert!(font_names().contains(&"Test Bold".to_string()));
        let regular = spec("Hello there", 32.0);
        let mut heavy = regular.clone();
        heavy.font = "Test Bold".into();
        let (w0, _) = measure(&regular);
        let (w1, _) = measure(&heavy);
        assert!(w1 > w0 * 1.05, "bold is wider: {w0} -> {w1}");
        let mut missing = regular.clone();
        missing.font = "No Such Face".into();
        assert_eq!(measure(&missing), measure(&regular), "unknown falls back");
        assert!(register_font("junk", vec![1, 2, 3]).is_err());
    }

    /// Where a raster's ink sits horizontally, as its centre of mass over
    /// the rows `y0..y1`.
    fn ink_centre(r: &TextRaster, y0: u32, y1: u32) -> f32 {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in y0..y1 {
            for x in 0..r.width {
                let c = r.sample(x, y);
                sum += c * (x as f32 + 0.5);
                n += c;
            }
        }
        sum / n.max(1e-6)
    }

    /// The rows that carry ink, as `first..last + 1`.
    fn ink_rows(r: &TextRaster) -> (u32, u32) {
        let inked = |y: u32| (0..r.width).any(|x| r.sample(x, y) > 0.1);
        let first = (0..r.height).find(|&y| inked(y)).unwrap_or(0);
        let last = (0..r.height).rev().find(|&y| inked(y)).unwrap_or(0);
        (first, last + 1)
    }

    #[test]
    fn italic_leans_the_glyphs_when_no_oblique_face_exists() {
        let upright = spec("l", 48.0);
        let mut italic = upright.clone();
        italic.italic = true;
        let (up, lean) = (rasterize(&upright), rasterize(&italic));
        assert!(
            lean.width > up.width,
            "the block makes room for the lean ({} > {})",
            lean.width,
            up.width
        );
        assert_eq!(
            measure(&italic).0.ceil() as u32,
            lean.width,
            "and measures as wide as it rasterizes"
        );
        let tilt = |r: &TextRaster| {
            let (y0, y1) = ink_rows(r);
            let third = (y1 - y0) / 3;
            ink_centre(r, y0, y0 + third) - ink_centre(r, y1 - third, y1)
        };
        assert!(
            tilt(&up).abs() < 0.5,
            "a stem stands straight ({})",
            tilt(&up)
        );
        assert!(
            tilt(&lean) > 3.0,
            "and its top sits right of its foot once italic ({})",
            tilt(&lean)
        );
    }

    #[test]
    fn italic_takes_a_registered_oblique_face_over_a_synthesized_lean() {
        const MONO: &[u8] = include_bytes!("../../../app/public/fonts/DejaVuSansMono.ttf");
        const OBLIQUE: &[u8] =
            include_bytes!("../../../app/public/fonts/DejaVuSansMono-Oblique.ttf");
        register_font("Mono Test", MONO.to_vec()).unwrap();
        register_font("Mono Test Oblique", OBLIQUE.to_vec()).unwrap();

        let mut italic = spec("Slant", 32.0);
        italic.font = "Mono Test".into();
        italic.italic = true;
        let mut oblique = italic.clone();
        oblique.font = "Mono Test Oblique".into();
        oblique.italic = false;
        let drawn = rasterize(&oblique);
        assert_eq!(
            rasterize(&italic).coverage,
            drawn.coverage,
            "the oblique face is used as it is"
        );
        let mut twice = oblique.clone();
        twice.italic = true;
        assert_eq!(
            rasterize(&twice).coverage,
            drawn.coverage,
            "an italic set in the oblique face itself is not leaned again"
        );

        let mut upright = oblique.clone();
        upright.font = "Mono Test".into();
        assert_ne!(
            rasterize(&upright).coverage,
            drawn.coverage,
            "while the upright face is its own face"
        );
        assert_eq!(
            measure(&upright).0.ceil(),
            measure(&oblique).0.ceil(),
            "and a real oblique needs no room past the glyphs' own advances"
        );
    }

    #[test]
    fn the_bundled_face_keeps_its_name() {
        let before = font_names();
        assert!(register_font(DEFAULT_FONT, FONT_BYTES.to_vec()).is_err());
        assert!(register_font("  ", FONT_BYTES.to_vec()).is_err());
        assert_eq!(font_names(), before, "and the list is not doubled");
    }

    #[test]
    fn faces_used_names_the_twin_an_italic_block_draws_with() {
        const MONO: &[u8] = include_bytes!("../../../app/public/fonts/DejaVuSansMono.ttf");
        register_font("Twin Test", MONO.to_vec()).unwrap();
        register_font("Twin Test Italic", MONO.to_vec()).unwrap();
        let mut block = spec("x", 12.0);
        assert!(
            faces_used(&block).is_empty(),
            "the bundled face is nobody's to carry"
        );
        block.font = "Twin Test".into();
        assert_eq!(faces_used(&block), ["Twin Test"]);
        block.italic = true;
        assert_eq!(
            faces_used(&block),
            ["Twin Test", "Twin Test Italic"],
            "italic draws with the twin, so the twin is used too"
        );
        block.font = "Twin Test Italic".into();
        assert_eq!(
            faces_used(&block),
            ["Twin Test Italic"],
            "once, when it is the face itself"
        );
        block.font = "Twinless".into();
        assert_eq!(
            faces_used(&block),
            ["Twinless"],
            "a synthesized lean draws with the face alone"
        );
    }

    #[test]
    fn a_set_block_says_where_its_lines_land() {
        let mut block = spec("Hello there!\nhi", 20.0);
        block.align = chitrakar_doc::TextAlign::Center;
        let laid = set(&block);
        assert_eq!(laid.lines.len(), 2);
        assert_eq!(laid.lines[0].0, "Hello there!");
        assert!(laid.lines[0].1 > laid.lines[1].1, "the long line is wider");
        assert!(
            (laid.width - laid.lines[0].1).abs() < 1e-3,
            "the block is as wide as its widest line"
        );
        assert_eq!(laid.inset, 0.0);
        assert!((laid.inner - laid.width).abs() < 1e-3);
        assert!(laid.ascent > 0.0 && laid.ascent < 20.0);
        assert!((laid.height - 2.0 * laid.step).abs() < 1e-3);
        assert!(
            laid.em > 20.0 * 0.8 && laid.em < 20.0,
            "an em is a little under the ascent-to-descent height ({})",
            laid.em
        );
        let (w, h) = measure(&block);
        assert!((w - laid.width).abs() < 1e-3 && (h - laid.height).abs() < 1e-3);
        // A wrap width holds the block to it and folds the words.
        block.width = 60.0;
        let folded = set(&block);
        assert!(folded.lines.len() > 2 && (folded.width - 60.0).abs() < 1e-3);
        // A synthesized lean insets the lines.
        block.italic = true;
        assert!(set(&block).inset > 0.0);
    }

    #[test]
    fn placed_glyphs_carry_their_text_and_land_where_the_raster_puts_them() {
        let mut block = spec("fi AV\nhi", 24.0);
        block.align = chitrakar_doc::TextAlign::Right;
        let typeset = placed(&block);
        assert_eq!(typeset.face.name, DEFAULT_FONT);
        assert!(
            typeset.face.bytes.len() > 100_000,
            "the whole file, to embed"
        );
        assert!(typeset.lean == 0.0);
        let text: String = typeset.glyphs.iter().map(|g| g.text.as_str()).collect();
        assert_eq!(
            text, "fi AVhi",
            "every character is accounted for, ligature or not"
        );
        // Two lines: the second's glyphs sit a line step lower, and being
        // right-aligned the short line starts further right than the long.
        let laid = set(&block);
        let (first, second): (Vec<_>, Vec<_>) =
            typeset.glyphs.iter().partition(|g| g.y < laid.step);
        assert!(!first.is_empty() && !second.is_empty());
        assert!((first[0].y - laid.ascent).abs() < 1e-3);
        assert!((second[0].y - laid.ascent - laid.step).abs() < 1e-3);
        assert!(
            second[0].x > first[0].x,
            "right aligned: {} > {}",
            second[0].x,
            first[0].x
        );
        // Widths are in thousandths of an em and an em is what the block
        // is scaled to.
        let h = typeset.face.advance(typeset.glyphs[3].id);
        assert!(h > 400.0 && h < 900.0, "advance of 'A' in 1/1000 em: {h}");
        assert!((typeset.em - laid.em).abs() < 1e-3);
        // A synthesized italic reports its lean; an oblique face does not.
        block.italic = true;
        assert_eq!(placed(&block).lean, SLANT);
    }

    #[test]
    fn underline_and_strike_through_paint_bands_the_glyphs_do_not() {
        let plain = spec("Hello\nhi", 24.0);
        let base = rasterize(&plain);
        let mut lined = plain.clone();
        lined.underline = true;
        let under = rasterize(&lined);
        assert_eq!(
            (under.width, under.height),
            (base.width, base.height),
            "the block's size is the same"
        );
        assert_eq!(measure(&lined), measure(&plain));
        // Under the first line's baseline, where 'Hello' has no
        // descender, a full-width band appears.
        let laid = set(&plain);
        let y = (laid.ascent + laid.em * 0.1 + 0.5) as u32;
        let inked = |r: &TextRaster, y: u32| (0..r.width).filter(|&x| r.sample(x, y) > 0.5).count();
        let (first_w, second_w) = (laid.lines[0].1, laid.lines[1].1);
        assert!(
            inked(&base, y) < (first_w * 0.2) as usize,
            "little ink there before"
        );
        assert!(
            inked(&under, y) as f32 >= first_w * 0.95,
            "a band as wide as the line ({} of {first_w})",
            inked(&under, y)
        );
        // The second line gets its own, only as wide as itself.
        let y2 = (laid.ascent + laid.step + laid.em * 0.1 + 0.5) as u32;
        let band2 = inked(&under, y2) as f32;
        assert!(
            band2 >= second_w * 0.95 && band2 < first_w * 0.8,
            "{band2} vs {second_w}"
        );
        // Strike-through crosses the x-height instead.
        let mut struck = plain.clone();
        struck.strike = true;
        let through = rasterize(&struck);
        let ys = (laid.ascent - laid.em * 0.3 + 0.5) as u32;
        assert!(inked(&through, ys) as f32 >= first_w * 0.95);
        assert!(
            inked(&through, y) == inked(&base, y),
            "and leaves the underline's row alone"
        );
        // Exporters get the same bands.
        let typeset = placed(&lined);
        assert_eq!(typeset.decorations.len(), 2);
        assert!((typeset.decorations[0][2] - typeset.decorations[0][0] - first_w).abs() < 1e-3);
    }

    #[test]
    fn text_along_a_guide_follows_it_glyph_by_glyph() {
        // A straight guide running down the page: the glyphs turn a
        // quarter turn and the block's box is tall, not wide.
        let mut down = spec("Hello", 20.0);
        down.along = Some(VectorShape::Path {
            points: vec![[10.0, 0.0], [10.0, 200.0]],
            closed: false,
            smooth: false,
            handles: Vec::new(),
            subpaths: Vec::new(),
        });
        let b = bounds(&down);
        assert!(b[3] - b[1] > (b[2] - b[0]) * 2.0, "tall box {b:?}");
        assert!(
            b[0] > 10.0 - 20.0 && b[2] > 10.0,
            "hangs to the left of the guide? {b:?}"
        );
        let r = rasterize(&down);
        assert!(
            r.origin.0 < 10.0 && r.origin.1 >= -1.0,
            "origin {:?}",
            r.origin
        );
        let ink: f32 = r.coverage.iter().sum();
        assert!(ink > 50.0);
        let typeset = placed(&down);
        assert_eq!(typeset.glyphs.len(), 5);
        assert!(
            (typeset.glyphs[0].angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "turned down"
        );
        assert!(
            typeset.glyphs[1].y > typeset.glyphs[0].y,
            "each glyph further down"
        );
        assert!(typeset.decorations.is_empty());

        // Past an open guide's end nothing is drawn; a closed one wraps.
        let mut short = down.clone();
        short.along = Some(VectorShape::Path {
            points: vec![[10.0, 0.0], [10.0, 30.0]],
            closed: false,
            smooth: false,
            handles: Vec::new(),
            subpaths: Vec::new(),
        });
        assert!(placed(&short).glyphs.len() < 5, "the rest runs off the end");
        let mut ring = down.clone();
        ring.along = Some(VectorShape::Ellipse { rx: 30.0, ry: 30.0 });
        ring.text = "Round and round and round".into();
        let on_ring = placed(&ring);
        assert_eq!(
            on_ring.glyphs.len(),
            ring.text.chars().count(),
            "wrapping keeps every glyph"
        );
        let rb = bounds(&ring);
        assert!(
            rb[0] < 0.0 && rb[2] > 60.0,
            "glyphs sit around the ring {rb:?}"
        );
        // An offset slides the text along.
        let mut slid = down.clone();
        slid.along_offset = 40.0;
        assert!((placed(&slid).glyphs[0].y - typeset.glyphs[0].y - 40.0).abs() < 1e-3);
        // The guide reads back flattened for exporters.
        let (pts, closed) = guide_points(&ring).unwrap();
        assert!(closed && pts.len() >= 16);
    }

    #[test]
    fn empty_text_is_harmless() {
        let raster = rasterize(&spec("", 32.0));
        assert!(raster.width >= 1 && raster.height >= 1);
        assert!(raster.coverage.iter().all(|c| *c == 0.0));
    }
}
