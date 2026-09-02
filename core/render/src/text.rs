//! Text rasterization: live [`TextSpec`] objects render to a coverage
//! bitmap at their natural size, then blit through the node transform like
//! any raster. Built on ab_glyph with a bundled DejaVu Sans (Bitstream Vera
//! license — see assets/DejaVuSans-LICENSE.txt). Font selection and shaping
//! (rustybuzz/parley) come later; this is deliberate MVP layout: per-glyph
//! advances with kerning, `\n` line breaks.

use ab_glyph::{Font, FontRef, ScaleFont};
use chitrakar_doc::TextSpec;
use std::sync::OnceLock;

const FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_BYTES).expect("bundled font must parse"))
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
    let font = font().as_scaled(spec.size.max(0.1) * scale);
    let step = line_step(&font, spec);
    let mut widest = 0f32;
    let mut lines = 0u32;
    for line in spec.text.split('\n') {
        lines += 1;
        widest = widest.max(line_width(&font, line, spec, scale));
    }
    (widest.max(1.0), (lines.max(1) as f32 * step).max(1.0))
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

fn line_width(
    font: &impl ScaleFont<&'static FontRef<'static>>,
    line: &str,
    spec: &TextSpec,
    scale: f32,
) -> f32 {
    let track = tracking(spec, scale);
    let mut width = 0f32;
    let mut prev = None;
    for ch in line.chars() {
        let id = font.glyph_id(ch);
        if let Some(prev) = prev {
            width += font.kern(prev, id) + track;
        }
        width += font.h_advance(id);
        prev = Some(id);
    }
    width
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
    let (w, h) = measure_at(spec, scale);
    let (width, height) = (w.ceil().max(1.0) as u32, h.ceil().max(1.0) as u32);
    let mut coverage = vec![0f32; (width * height) as usize];
    // Every metric below comes from the scaled font, so advances, kerning
    // and line height are all in raster pixels already.
    let font = font().as_scaled(spec.size.max(0.1) * scale);
    let step = line_step(&font, spec);
    let track = tracking(spec, scale);

    for (line_no, line) in spec.text.split('\n').enumerate() {
        let baseline = line_no as f32 * step + font.ascent();
        // Alignment is within the block's own width, which is the widest
        // line: a short line is pushed right by the slack it leaves.
        let slack = w - line_width(&font, line, spec, scale);
        let mut pen_x = match spec.align {
            chitrakar_doc::TextAlign::Left => 0.0,
            chitrakar_doc::TextAlign::Center => slack / 2.0,
            chitrakar_doc::TextAlign::Right => slack,
        };
        let mut prev = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = prev {
                pen_x += font.kern(prev, id) + track;
            }
            let glyph = id.with_scale_and_position(
                spec.size.max(0.1) * scale,
                ab_glyph::point(pen_x, baseline),
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
            pen_x += font.h_advance(id);
            prev = Some(id);
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
    fn empty_text_is_harmless() {
        let raster = rasterize(&spec("", 32.0));
        assert!(raster.width >= 1 && raster.height >= 1);
        assert!(raster.coverage.iter().all(|c| *c == 0.0));
    }
}
