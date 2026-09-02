//! Text rasterization: live [`TextSpec`] objects render to a coverage
//! bitmap at their natural size, then blit through the node transform like
//! any raster. A bundled DejaVu Sans (Bitstream Vera license — see
//! assets/DejaVuSans-LICENSE.txt) is shaped by rustybuzz — so the font's
//! own kerning, ligatures and mark positioning apply, and a script that
//! reorders or joins will work the day a font for it is bundled — and its
//! glyphs are rasterized by ab_glyph. Lines break on `\n`, and a block
//! given a width wraps words to it.

use ab_glyph::{Font, FontRef, ScaleFont};
use chitrakar_doc::TextSpec;
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

/// The face a block is set in — the one it names, or the bundled one when
/// it names nothing or something this process has not been given.
fn fonts_for(spec: &TextSpec) -> &'static Fonts {
    if spec.font.is_empty() || spec.font == DEFAULT_FONT {
        return bundled();
    }
    registry()
        .read()
        .ok()
        .and_then(|r| r.get(&spec.font).copied())
        .unwrap_or_else(bundled)
}

/// One positioned glyph of a shaped line: which glyph, where its origin
/// sits along the baseline, and how far the pen moves past it — all in
/// raster pixels at the scale the line was shaped for.
struct Shaped {
    id: ab_glyph::GlyphId,
    x: f32,
    y: f32,
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

/// A rasterized text block: alpha coverage (0..=1) at natural size.
pub struct TextRaster {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<f32>,
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
    Layout {
        height: (lines.len().max(1) as f32 * step).max(1.0),
        lines,
        widths,
        width: width.max(1.0),
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
    let l = layout(spec, scale);
    let (w, h) = (l.width, l.height);
    let (width, height) = (w.ceil().max(1.0) as u32, h.ceil().max(1.0) as u32);
    let mut coverage = vec![0f32; (width * height) as usize];
    // Every metric below comes from the scaled font, so advances, kerning
    // and line height are all in raster pixels already.
    let font = fonts_for(spec).font.as_scaled(spec.size.max(0.1) * scale);
    let step = line_step(&font, spec);

    for (line_no, line) in l.lines.iter().enumerate() {
        let baseline = line_no as f32 * step + font.ascent();
        let (glyphs, _) = shape_line(line, spec, scale);
        let line_w = l.widths[line_no];
        // Alignment is within the block's own width, which is the widest
        // line: a short line is pushed right by the slack it leaves.
        let slack = w - line_w;
        let start = match spec.align {
            chitrakar_doc::TextAlign::Left => 0.0,
            chitrakar_doc::TextAlign::Center => slack / 2.0,
            chitrakar_doc::TextAlign::Right => slack,
        };
        for g in glyphs {
            let glyph = g.id.with_scale_and_position(
                spec.size.max(0.1) * scale,
                ab_glyph::point(start + g.x, baseline + g.y),
            );
            if let Some(outlined) = font.font.outline_glyph(glyph) {
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
    TextRaster {
        width,
        height,
        coverage,
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

    #[test]
    fn empty_text_is_harmless() {
        let raster = rasterize(&spec("", 32.0));
        assert!(raster.width >= 1 && raster.height >= 1);
        assert!(raster.coverage.iter().all(|c| *c == 0.0));
    }
}
