//! PDF export for print handoff.
//!
//! One page sized from the document's pixel dimensions and dpi, carrying the
//! composite as a single embedded image. Two flavors, chosen by whether the
//! document has a press profile:
//!
//! - **CMYK**: the composite is separated into ink through the profile and
//!   embedded in an ICCBased (N=4) color space carrying that same profile —
//!   so a RIP reproduces what soft proofing showed.
//! - **RGB**: sRGB composite over white in DeviceRGB.
//!
//! Image data is Flate-compressed (lossless — this is print output, so DCT
//! is not an option). The writer is deliberately small and explicit rather
//! than a PDF library: the document we emit is one page with one image, and
//! hand-rolling it keeps the dependency surface honest.

use chitrakar_color::LinearRgba;
use flate2::{write::ZlibEncoder, Compression};
use std::io::Write;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("color conversion failed: {0}")]
    Color(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Flatten a premultiplied linear composite to non-linear sRGB over white
/// paper. PDF images here have no alpha; unprinted areas are the page.
fn flatten_to_srgb(pixels: &[LinearRgba]) -> Vec<f32> {
    let mut out = Vec::with_capacity(pixels.len() * 3);
    for px in pixels {
        let over_white = |v: f32| (v + (1.0 - px.a)).clamp(0.0, 1.0);
        out.push(chitrakar_color::linear_to_srgb(over_white(px.r)));
        out.push(chitrakar_color::linear_to_srgb(over_white(px.g)));
        out.push(chitrakar_color::linear_to_srgb(over_white(px.b)));
    }
    out
}

fn deflate(data: &[u8]) -> Result<Vec<u8>, PdfError> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

/// Write a one-page PDF containing the composite.
///
/// `dpi` sets the physical page size (pixels / dpi * 72pt). When `cmyk_icc`
/// is given, the page is separated into that profile's ink and the profile
/// travels with the file.
pub fn export_pdf(
    pixels: &[LinearRgba],
    width: u32,
    height: u32,
    dpi: f32,
    cmyk_icc: Option<&[u8]>,
) -> Result<Vec<u8>, PdfError> {
    let srgb = flatten_to_srgb(pixels);

    // Sample data and the color space that describes it.
    let (samples, components) = match cmyk_icc {
        Some(icc) => {
            let sep = chitrakar_color::cms::RgbToCmyk::new(icc).map_err(PdfError::Color)?;
            (sep.separate(&srgb).map_err(PdfError::Color)?, 4)
        }
        None => (
            srgb.iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect(),
            3,
        ),
    };
    let image_stream = deflate(&samples)?;
    let icc_stream = cmyk_icc.map(deflate).transpose()?;

    let pt = 72.0 / dpi.max(1.0);
    let (page_w, page_h) = (width as f32 * pt, height as f32 * pt);

    // Objects are appended in order, each recording its byte offset for the
    // cross-reference table.
    let mut out: Vec<u8> = Vec::with_capacity(image_stream.len() + 4096);
    let mut offsets: Vec<usize> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");

    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    // 1 catalog, 2 pages, 3 page, 4 contents, 5 image, [6 icc]
    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    );
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.3} {page_h:.3}] \
             /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        )
        .as_bytes(),
    );

    // Content stream: scale the unit image to fill the page.
    let content = format!("q\n{page_w:.3} 0 0 {page_h:.3} 0 0 cm\n/Im0 Do\nQ\n");
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        )
        .as_bytes(),
    );

    // ICCBased must be an array holding an *indirect* reference to the
    // profile stream — a stream cannot live inline inside an array.
    let color_space = if icc_stream.is_some() {
        "6 0 R".to_string()
    } else {
        "/DeviceRGB".to_string()
    };
    let mut image_obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace {color_space} /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
        image_stream.len()
    )
    .into_bytes();
    image_obj.extend_from_slice(&image_stream);
    image_obj.extend_from_slice(b"\nendstream");
    obj(&mut out, &mut offsets, &image_obj);

    if let Some(icc) = icc_stream {
        // 6: the color space array, referring to 7: the profile stream.
        obj(&mut out, &mut offsets, b"[/ICCBased 7 0 R]");
        let mut icc_obj = format!(
            "<< /N {components} /Filter /FlateDecode /Length {} >>\nstream\n",
            icc.len()
        )
        .into_bytes();
        icc_obj.extend_from_slice(&icc);
        icc_obj.extend_from_slice(b"\nendstream");
        obj(&mut out, &mut offsets, &icc_obj);
    }

    // Cross-reference table and trailer.
    let xref_at = out.len();
    let count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn red_and_clear() -> Vec<LinearRgba> {
        vec![
            LinearRgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            LinearRgba::TRANSPARENT,
        ]
    }

    /// Payload of the nth `stream ... endstream` in the file (0-based).
    /// Matches "\nstream\n" rather than "stream\n": the latter also occurs
    /// inside "endstream\n", which would land on the wrong boundary.
    fn nth_stream(pdf: &[u8], n: usize) -> Vec<u8> {
        let mut cursor = 0usize;
        for _ in 0..n {
            let rel = pdf[cursor..]
                .windows(8)
                .position(|w| w == b"\nstream\n")
                .expect("enough streams");
            cursor += rel + 8;
        }
        let rel = pdf[cursor..]
            .windows(8)
            .position(|w| w == b"\nstream\n")
            .expect("stream marker");
        let start = cursor + rel + 8;
        let end = pdf[start..]
            .windows(9)
            .position(|w| w == b"endstream")
            .expect("endstream marker");
        // Streams are written with a trailing newline before endstream.
        pdf[start..start + end - 1].to_vec()
    }

    #[test]
    fn rgb_pdf_has_valid_structure_and_lossless_pixels() {
        let pdf = export_pdf(&red_and_clear(), 2, 1, 72.0, None).unwrap();

        assert!(pdf.starts_with(b"%PDF-1.7"), "header");
        assert!(pdf.ends_with(b"%%EOF\n"), "trailer");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Catalog"));
        assert!(
            text.contains("/MediaBox [0 0 2.000 1.000]"),
            "72dpi -> 1pt/px"
        );
        assert!(text.contains("/ColorSpace /DeviceRGB"));
        assert!(text.contains("/Filter /FlateDecode"));

        // The xref offsets must actually point at their objects, or readers
        // reject the file.
        let xref_at: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(&pdf[xref_at..xref_at + 4], b"xref");
        for (i, line) in text[xref_at..]
            .lines()
            .skip(2) // "xref", "0 N"
            .take_while(|l| l.ends_with(" n "))
            .enumerate()
        {
            let off: usize = line.split_whitespace().next().unwrap().parse().unwrap();
            let expect = format!("{} 0 obj", i + 1);
            assert_eq!(
                &pdf[off..off + expect.len()],
                expect.as_bytes(),
                "xref entry {i} points at its object"
            );
        }

        // Image samples round-trip losslessly: red, then white paper.
        let mut raw = Vec::new();
        ZlibDecoder::new(&nth_stream(&pdf, 1)[..])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw.len(), 6, "two RGB pixels");
        assert_eq!(&raw[0..3], &[255, 0, 0], "opaque red survives");
        assert_eq!(
            &raw[3..6],
            &[255, 255, 255],
            "transparent flattens to paper"
        );
    }

    #[test]
    fn cmyk_pdf_embeds_the_profile_and_separates_ink() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let pdf = export_pdf(&red_and_clear(), 2, 1, 300.0, Some(&icc)).unwrap();
        let text = String::from_utf8_lossy(&pdf);

        assert!(text.contains("[/ICCBased 7 0 R]"), "colorspace is indirect");
        assert!(text.contains("/N 4"), "four ink components");
        assert!(
            text.contains("/MediaBox [0 0 0.480 0.240]"),
            "300dpi page size"
        );

        // Ink data: red separates to magenta+yellow, paper stays near bare.
        let mut ink = Vec::new();
        ZlibDecoder::new(&nth_stream(&pdf, 1)[..])
            .read_to_end(&mut ink)
            .unwrap();
        assert_eq!(ink.len(), 8, "two CMYK pixels");
        assert!(
            ink[1] > 100 && ink[2] > 100 && ink[0] < 90,
            "red ink {:?}",
            &ink[0..4]
        );
        assert!(ink[4..8].iter().all(|v| *v < 40), "paper {:?}", &ink[4..8]);

        // The profile itself travels with the file.
        let mut embedded = Vec::new();
        ZlibDecoder::new(&nth_stream(&pdf, 2)[..])
            .read_to_end(&mut embedded)
            .unwrap();
        assert_eq!(embedded, icc, "profile embedded verbatim");
    }
}
