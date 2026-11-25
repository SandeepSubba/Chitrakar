//! The resolution a picture file says it was made at, so a picture
//! opened as a document keeps its printed size. Read for PNG (`pHYs`),
//! JPEG (JFIF's density), TIFF (`XResolution`) and Photoshop's own
//! (resource 1005). Left unread, the editor's own 300-dpi exports came
//! back at 72 and went out to a PDF four times too large.

/// Dots an inch, when the file says, within what a page may be set to.
pub fn picture_dpi(bytes: &[u8]) -> Option<f32> {
    let dpi = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg(bytes)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        tiff(bytes)
    } else if bytes.starts_with(b"8BPS") {
        psd(bytes)
    } else {
        None
    }?;
    if !(dpi.is_finite() && (1.0..=2400.0).contains(&dpi)) {
        return None;
    }
    // A metre's worth of pixels said whole is a hair off a round dpi.
    let near = dpi.round();
    Some(if (dpi - near).abs() < 0.05 {
        near
    } else {
        (dpi * 100.0).round() / 100.0
    })
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn png(b: &[u8]) -> Option<f32> {
    let mut at = 8;
    while at + 8 <= b.len() {
        let len = be32(b, at)? as usize;
        let kind = b.get(at + 4..at + 8)?;
        if kind == b"IDAT" {
            return None;
        }
        if kind == b"pHYs" && len >= 9 {
            let x = be32(b, at + 8)?;
            // Unit 1 is the metre; 0 says only how wide a pixel is
            // against how tall.
            return (*b.get(at + 16)? == 1).then_some(x as f32 * 0.0254);
        }
        at = at.checked_add(12)?.checked_add(len)?;
    }
    None
}

fn jpeg(b: &[u8]) -> Option<f32> {
    let mut at = 2;
    while at + 4 <= b.len() {
        if b[at] != 0xff {
            return None;
        }
        let marker = b[at + 1];
        // Where the picture begins, nothing more is said of its size.
        if marker == 0xda || marker == 0xd9 {
            return None;
        }
        let len = be16(b, at + 2)? as usize;
        if marker == 0xe0 && b.get(at + 4..at + 9) == Some(b"JFIF\0") {
            let units = *b.get(at + 11)?;
            let x = be16(b, at + 12)? as f32;
            return match units {
                1 => Some(x),
                2 => Some(x * 2.54),
                _ => None,
            };
        }
        at = at.checked_add(2)?.checked_add(len)?;
    }
    None
}

fn tiff(b: &[u8]) -> Option<f32> {
    let little = b.starts_with(b"II");
    let u16_at = |at: usize| -> Option<u16> {
        let s: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(s)
        } else {
            u16::from_be_bytes(s)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let s: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(s)
        } else {
            u32::from_be_bytes(s)
        })
    };
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    let (mut x, mut unit) = (None, 2u16);
    for k in 0..count.min(512) {
        let entry = ifd + 2 + k * 12;
        match u16_at(entry)? {
            282 => {
                let off = u32_at(entry + 8)? as usize;
                let (n, d) = (u32_at(off)?, u32_at(off + 4)?);
                if d > 0 {
                    x = Some(n as f32 / d as f32);
                }
            }
            296 => unit = u16_at(entry + 8)?,
            _ => {}
        }
    }
    match unit {
        2 => x,
        3 => x.map(|v| v * 2.54),
        _ => None,
    }
}

fn psd(b: &[u8]) -> Option<f32> {
    let colour = be32(b, 26)? as usize;
    let at = 30usize.checked_add(colour)?;
    let len = be32(b, at)? as usize;
    let (mut p, end) = (at + 4, at.checked_add(4)?.checked_add(len)?.min(b.len()));
    while p + 12 <= end {
        if b.get(p..p + 4)? != b"8BIM" {
            return None;
        }
        let id = be16(b, p + 4)?;
        let name = *b.get(p + 6)? as usize;
        // The name's length byte and the name, made even.
        let named = (1 + name + 1) & !1;
        let size_at = p + 6 + named;
        let size = be32(b, size_at)? as usize;
        let data = size_at + 4;
        if id == 1005 && size >= 16 {
            let h = be32(b, data)? as f32 / 65536.0;
            return match be16(b, data + 4)? {
                1 => Some(h),
                2 => Some(h * 2.54),
                _ => None,
            };
        }
        p = data.checked_add((size + 1) & !1)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_says_its_resolution() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&9u32.to_be_bytes());
        png.extend_from_slice(b"pHYs");
        png.extend_from_slice(&11811u32.to_be_bytes());
        png.extend_from_slice(&11811u32.to_be_bytes());
        png.push(1);
        png.extend_from_slice(&[0; 4]);
        assert_eq!(picture_dpi(&png), Some(300.0));
    }

    #[test]
    fn a_jpeg_says_its_density() {
        let mut j = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        j.extend_from_slice(b"JFIF\0");
        j.extend_from_slice(&[1, 1, 1]);
        j.extend_from_slice(&240u16.to_be_bytes());
        j.extend_from_slice(&240u16.to_be_bytes());
        j.extend_from_slice(&[0, 0]);
        assert_eq!(picture_dpi(&j), Some(240.0));
        // Units of nothing: only the shape of a pixel.
        j[13] = 0;
        assert_eq!(picture_dpi(&j), None);
    }

    #[test]
    fn nothing_is_made_of_junk() {
        for junk in [
            &b""[..],
            b"\x89PNG\r\n\x1a\n\xff\xff\xff\xff",
            b"8BPS",
            b"II*\0\xff\xff\xff\xff",
            &[0xff, 0xd8, 0xff],
        ] {
            assert_eq!(picture_dpi(junk), None);
        }
    }
}
