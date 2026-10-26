//! Reading a PDF — and an Illustrator file, which since Illustrator 9 is
//! a PDF with Illustrator's own data beside it — as a picture of a page.
//!
//! The page is drawn by `hayro`, a PDF renderer in Rust that runs where
//! the engine does, at the resolution asked for, on a clear ground so a
//! page's own white is the only white there is. It comes in as pixels,
//! not shapes: a PDF is a program for drawing a page, and turning its
//! drawing back into layers an editor can change — which of its hundred
//! paths are one letter — is a different and much larger thing. What a
//! page is drawn at is asked for, so a page wanted for print comes in
//! with the pixels print needs.
//!
//! The fourteen fonts every PDF reader is expected to have (Helvetica,
//! Times, Courier, Symbol and the rest) come with `hayro` itself — Foxit's
//! CFF versions, BSD-licensed — so a page that names one and does not
//! carry it still has its text.

use crate::{CodecError, SourceImage};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

/// The most pixels a page is drawn at on a side: past it the renderer's
/// own surface cannot go.
pub const SIDE_MOST: u32 = 16_384;

/// Whether the bytes are a PDF (or an Illustrator file saved with its PDF
/// inside). The header may sit a little way in, after junk some tools
/// write first.
pub fn is_pdf(b: &[u8]) -> bool {
    b.len() >= 5 && b[..b.len().min(1024)].windows(5).any(|w| w == b"%PDF-")
}

fn bad(what: impl std::fmt::Display) -> CodecError {
    CodecError::Format(format!("PDF: {what}"))
}

fn open(bytes: &[u8]) -> Result<Pdf, CodecError> {
    if !is_pdf(bytes) {
        // An Illustrator file from before Illustrator 9 is PostScript.
        return Err(if bytes.starts_with(b"%!PS") {
            bad("an Illustrator or PostScript file from before PDF; save it again as PDF-compatible")
        } else {
            bad("not a PDF")
        });
    }
    Pdf::new(bytes.to_vec()).map_err(|e| match e {
        hayro::hayro_syntax::LoadPdfError::Decryption(_) => bad("protected by a password"),
        hayro::hayro_syntax::LoadPdfError::Invalid => bad("could not be read"),
    })
}

/// Each page's size, in points (72 to the inch), as it would be shown.
pub fn pdf_page_sizes(bytes: &[u8]) -> Result<Vec<(f32, f32)>, CodecError> {
    let pdf = open(bytes)?;
    let sizes: Vec<(f32, f32)> = pdf.pages().iter().map(|p| p.render_dimensions()).collect();
    if sizes.is_empty() {
        return Err(bad("has no pages"));
    }
    Ok(sizes)
}

/// Draw one page (counted from 0) at `dpi`, as 8-bit sRGB with straight
/// alpha — clear wherever the page draws nothing.
pub fn render_pdf_page(bytes: &[u8], page: usize, dpi: f32) -> Result<SourceImage, CodecError> {
    let pdf = open(bytes)?;
    let pages = pdf.pages();
    let p = pages
        .get(page)
        .ok_or_else(|| bad(format!("has no page {}", page + 1)))?;
    let (w, h) = p.render_dimensions();
    if !(dpi.is_finite() && dpi > 0.0) {
        return Err(bad("a resolution that is not one"));
    }
    // Taken down to fit the renderer when a resolution would pass it.
    let mut scale = dpi / 72.0;
    let longest = w.max(h) * scale;
    if longest > SIDE_MOST as f32 {
        scale *= SIDE_MOST as f32 / longest;
    }
    if (w * scale).round() < 1.0 || (h * scale).round() < 1.0 {
        return Err(bad("a page with no size"));
    }
    let pixmap = render(
        p,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            ..Default::default()
        },
    );
    let (width, height) = (pixmap.width() as u32, pixmap.height() as u32);
    let rgba8 = pixmap.take_rgba8(hayro::vello_cpu::peniko::ImageAlphaType::Alpha);
    Ok(SourceImage {
        width,
        height,
        rgba8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF written by hand: one page, 200 × 100 points, a red square
    /// from (20, 20) to (70, 70) in PDF's own upward coordinates, and a
    /// line of Helvetica the file does not carry.
    fn one_page() -> Vec<u8> {
        let content = b"1 0 0 rg 20 20 50 50 re f BT /F1 24 Tf 100 40 Td (Hi) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn a_page_is_drawn_at_the_resolution_asked() {
        let pdf = one_page();
        assert!(is_pdf(&pdf));
        assert_eq!(pdf_page_sizes(&pdf).unwrap(), vec![(200.0, 100.0)]);
        let img = render_pdf_page(&pdf, 0, 144.0).unwrap();
        assert_eq!((img.width, img.height), (400, 200));
        let px = |x: u32, y: u32| {
            &img.rgba8[((y * img.width + x) * 4) as usize..((y * img.width + x) * 4 + 4) as usize]
        };
        // The square, the right way up: PDF counts up from the bottom.
        assert_eq!(px(90, 200 - 90), &[255, 0, 0, 255]);
        // Clear where the page draws nothing.
        assert_eq!(px(10, 10)[3], 0);
    }

    #[test]
    fn a_standard_font_the_file_does_not_carry_still_draws() {
        let img = render_pdf_page(&one_page(), 0, 72.0).unwrap();
        // "Hi" at (100, 40) up from the bottom, 24 points: some ink there.
        let mut inked = 0;
        for y in 40..60 {
            for x in 100..130 {
                if img.rgba8[((y * img.width + x) * 4 + 3) as usize] > 128 {
                    inked += 1;
                }
            }
        }
        assert!(inked > 20, "the letters are drawn: {inked} pixels");
    }

    #[test]
    fn the_engines_own_pdf_comes_back_as_its_page() {
        // A page the engine wrote — a blue box on clear, 100 × 50 at 72
        // dpi — read back at its own size: the box where it was put.
        use chitrakar_color::LinearRgba;
        let blue = LinearRgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        let pixels: Vec<LinearRgba> = (0..100 * 50)
            .map(|i| {
                let (x, y) = (i % 100, i / 100);
                if (10..50).contains(&x) && (10..30).contains(&y) {
                    blue
                } else {
                    LinearRgba::TRANSPARENT
                }
            })
            .collect();
        let bytes = crate::export_pdf(&pixels, 100, 50, 72.0, None).unwrap();
        assert_eq!(pdf_page_sizes(&bytes).unwrap(), vec![(100.0, 50.0)]);
        let img = render_pdf_page(&bytes, 0, 72.0).unwrap();
        assert_eq!((img.width, img.height), (100, 50));
        let at = |x: u32, y: u32| {
            &img.rgba8[((y * 100 + x) * 4) as usize..((y * 100 + x) * 4 + 4) as usize]
        };
        assert_eq!(
            at(30, 20),
            &[0, 0, 255, 255],
            "the box, where the engine put it"
        );
        // The pixel export lays the page on white (a printed page is
        // paper), so where it put nothing reads back as white.
        assert_eq!(
            at(80, 40),
            &[255, 255, 255, 255],
            "and the page's white where it put nothing"
        );
    }

    #[test]
    fn what_is_not_a_pdf_is_said_so() {
        assert!(pdf_page_sizes(b"hello").is_err());
        let ps = render_pdf_page(b"%!PS-Adobe-3.0 EPSF", 0, 72.0)
            .err()
            .unwrap()
            .to_string();
        assert!(ps.contains("PDF-compatible"), "{ps}");
        assert!(render_pdf_page(&one_page(), 3, 72.0).is_err());
    }
}
