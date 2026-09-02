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
    let font = font().as_scaled(spec.size.max(0.1));
    let line_height = font.ascent() - font.descent() + font.line_gap();
    let mut widest = 0f32;
    let mut lines = 0u32;
    for line in spec.text.split('\n') {
        lines += 1;
        widest = widest.max(line_width(&font, line));
    }
    (
        widest.max(1.0),
        (lines.max(1) as f32 * line_height).max(1.0),
    )
}

fn line_width(font: &impl ScaleFont<&'static FontRef<'static>>, line: &str) -> f32 {
    let mut width = 0f32;
    let mut prev = None;
    for ch in line.chars() {
        let id = font.glyph_id(ch);
        if let Some(prev) = prev {
            width += font.kern(prev, id);
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
    let (w, h) = measure(spec);
    let (width, height) = (
        (w * scale).ceil().max(1.0) as u32,
        (h * scale).ceil().max(1.0) as u32,
    );
    let mut coverage = vec![0f32; (width * height) as usize];
    // Every metric below comes from the scaled font, so advances, kerning
    // and line height are all in raster pixels already.
    let font = font().as_scaled(spec.size.max(0.1) * scale);
    let line_height = font.ascent() - font.descent() + font.line_gap();

    for (line_no, line) in spec.text.split('\n').enumerate() {
        let baseline = line_no as f32 * line_height + font.ascent();
        let mut pen_x = 0f32;
        let mut prev = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = prev {
                pen_x += font.kern(prev, id);
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
        TextSpec {
            text: text.into(),
            size,
            fill: AuthoredColor::Srgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        }
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
    fn empty_text_is_harmless() {
        let raster = rasterize(&spec("", 32.0));
        assert!(raster.width >= 1 && raster.height >= 1);
        assert!(raster.coverage.iter().all(|c| *c == 0.0));
    }
}
