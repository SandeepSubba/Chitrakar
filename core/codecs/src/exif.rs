//! How a photograph was taken, in and out of a JPEG's EXIF: read from a
//! camera's JPEG when it is placed, and written into a JPEG exported from
//! it, so the file that goes out still says which camera, which lens, at
//! what shutter, aperture and ISO, and when. A raw file's own EXIF comes
//! by the raw reader (`chitrakar-raw-in`), which reads far more of it.
//!
//! Only those fields, in both directions. Nothing of where it was taken
//! goes out: a place is the person's to give away, and an editor that
//! copied a phone's GPS into every file it wrote would be giving it away
//! for them. The pixels written are upright, so the orientation written
//! is the upright one, whatever the camera's was.

use chitrakar_doc::Capture;

/// A JPEG's EXIF, read for how the photograph was taken; `None` where it
/// has none, or none that says anything this keeps.
pub fn capture_of(file: &[u8]) -> Option<Capture> {
    let tiff = exif_of_jpeg(file)?;
    read_tiff(tiff).filter(|c| !c.is_empty())
}

/// The TIFF inside a JPEG's EXIF segment (APP1, `Exif\0\0`), found by
/// walking its markers up to the picture data.
fn exif_of_jpeg(file: &[u8]) -> Option<&[u8]> {
    if file.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut at = 2;
    loop {
        let marker = file.get(at..at + 2)?;
        if marker[0] != 0xFF {
            return None;
        }
        // The picture's own data begins; no EXIF before it.
        if marker[1] == 0xDA || marker[1] == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([*file.get(at + 2)?, *file.get(at + 3)?]) as usize;
        let body = file.get(at + 4..at + 2 + len)?;
        if marker[1] == 0xE1 && body.starts_with(b"Exif\0\0") {
            return Some(&body[6..]);
        }
        at += 2 + len;
    }
}

/// A TIFF's first directory and its EXIF directory, read for the fields
/// a [`Capture`] keeps. Either byte order; anything out of bounds or of
/// an unexpected type is left unread rather than refused.
pub fn read_tiff(t: &[u8]) -> Option<Capture> {
    let little = match t.get(..4)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b = t.get(at..at + 2)?;
        Some(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b = t.get(at..at + 4)?;
        Some(if little {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    // Each entry of a directory: its tag, type, count and where its value
    // is (in the entry when it fits in four bytes, else at the offset).
    let entries = |ifd: usize| -> Vec<(u16, u16, u32, usize)> {
        let Some(n) = u16_at(ifd) else {
            return Vec::new();
        };
        (0..n as usize)
            .filter_map(|i| {
                let e = ifd + 2 + i * 12;
                let (tag, kind, count) = (u16_at(e)?, u16_at(e + 2)?, u32_at(e + 4)?);
                let size = match kind {
                    1 | 2 | 6 | 7 => 1,
                    3 | 8 => 2,
                    4 | 9 => 4,
                    5 | 10 => 8,
                    _ => return None,
                } * count as usize;
                let at = if size <= 4 {
                    e + 8
                } else {
                    u32_at(e + 8)? as usize
                };
                t.get(at..at.checked_add(size)?)?;
                Some((tag, kind, count, at))
            })
            .collect()
    };
    let text = |kind: u16, count: u32, at: usize| -> Option<String> {
        (kind == 2).then(|| {
            let raw = &t[at..at + count as usize];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).trim().to_string()
        })
    };
    let ratio = |kind: u16, at: usize| -> Option<[u32; 2]> {
        (kind == 5)
            .then(|| Some([u32_at(at)?, u32_at(at + 4)?]))
            .flatten()
            .filter(|r| r[1] != 0 && r[0] != 0)
    };
    let mut c = Capture::default();
    let ifd0 = u32_at(4)? as usize;
    let mut exif_ifd = None;
    for (tag, kind, count, at) in entries(ifd0) {
        match tag {
            0x010F => c.make = text(kind, count, at).unwrap_or_default(),
            0x0110 => c.model = text(kind, count, at).unwrap_or_default(),
            0x8769 if kind == 4 => exif_ifd = u32_at(at).map(|v| v as usize),
            _ => {}
        }
    }
    if let Some(ifd) = exif_ifd {
        for (tag, kind, count, at) in entries(ifd) {
            match tag {
                0x829A => c.exposure = ratio(kind, at),
                0x829D => c.aperture = ratio(kind, at),
                0x920A => c.focal = ratio(kind, at),
                0x8827 => {
                    c.iso = match kind {
                        3 => u16_at(at).map_or(0, u32::from),
                        4 => u32_at(at).unwrap_or(0),
                        _ => 0,
                    }
                }
                0x9003 => c.taken = text(kind, count, at).unwrap_or_default(),
                0x9204 if kind == 10 => {
                    let (n, d) = (u32_at(at)? as i32, u32_at(at + 4)? as i32);
                    c.bias = (d != 0).then_some([n, d]);
                }
                0xA433 => c.lens_make = text(kind, count, at).unwrap_or_default(),
                0xA434 => c.lens_model = text(kind, count, at).unwrap_or_default(),
                _ => {}
            }
        }
    }
    Some(c)
}

/// One field of a directory to be written: its tag, type, count and the
/// bytes of its value (little-endian).
struct Field(u16, u16, u32, Vec<u8>);

fn ascii(tag: u16, s: &str) -> Option<Field> {
    let s = s.trim();
    (!s.is_empty()).then(|| {
        // Held to what a name or a date is, and to plain text.
        let mut v: Vec<u8> = s
            .bytes()
            .filter(|b| (0x20..0x7F).contains(b))
            .take(127)
            .collect();
        v.push(0);
        Field(tag, 2, v.len() as u32, v)
    })
}

fn rational(tag: u16, r: Option<[u32; 2]>) -> Option<Field> {
    let [n, d] = r?;
    (d != 0).then(|| {
        let mut v = n.to_le_bytes().to_vec();
        v.extend_from_slice(&d.to_le_bytes());
        Field(tag, 5, 1, v)
    })
}

/// A directory laid out at `start` in the TIFF: its count, its entries,
/// the next directory's offset (none), and the values too long for an
/// entry after it.
fn directory(fields: &[Field], start: usize) -> Vec<u8> {
    let head = 2 + fields.len() * 12 + 4;
    let mut out = Vec::new();
    let mut tail = Vec::new();
    out.extend_from_slice(&(fields.len() as u16).to_le_bytes());
    for Field(tag, kind, count, value) in fields {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        if value.len() <= 4 {
            let mut v = value.clone();
            v.resize(4, 0);
            out.extend_from_slice(&v);
        } else {
            let at = (start + head + tail.len()) as u32;
            out.extend_from_slice(&at.to_le_bytes());
            tail.extend_from_slice(value);
            // Values start on even offsets.
            if tail.len() % 2 == 1 {
                tail.push(0);
            }
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&tail);
    out
}

/// The TIFF an EXIF segment carries, for `c`: the camera and the
/// upright orientation in the first directory, the rest in the EXIF one.
pub fn write_tiff(c: &Capture) -> Vec<u8> {
    let exif: Vec<Field> = [
        rational(0x829A, c.exposure),
        rational(0x829D, c.aperture),
        (c.iso > 0).then(|| {
            if c.iso <= u16::MAX as u32 {
                Field(0x8827, 3, 1, (c.iso as u16).to_le_bytes().to_vec())
            } else {
                Field(0x8827, 4, 1, c.iso.to_le_bytes().to_vec())
            }
        }),
        Some(Field(0x9000, 7, 4, b"0232".to_vec())),
        ascii(0x9003, &c.taken),
        c.bias.filter(|b| b[1] != 0).map(|[n, d]| {
            let mut v = n.to_le_bytes().to_vec();
            v.extend_from_slice(&d.to_le_bytes());
            Field(0x9204, 10, 1, v)
        }),
        rational(0x920A, c.focal),
        ascii(0xA433, &c.lens_make),
        ascii(0xA434, &c.lens_model),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut ifd0: Vec<Field> = [
        ascii(0x010F, &c.make),
        ascii(0x0110, &c.model),
        Some(Field(0x0112, 3, 1, 1u16.to_le_bytes().to_vec())),
        ascii(0x0131, "Chitrakar"),
    ]
    .into_iter()
    .flatten()
    .collect();
    // The EXIF directory follows the first; its offset is known once the
    // first is laid out, and the first's size does not depend on it.
    ifd0.push(Field(0x8769, 4, 1, vec![0; 4]));
    let first_len = directory(&ifd0, 8).len();
    let exif_at = (8 + first_len) as u32;
    ifd0.last_mut().expect("pushed above").3 = exif_at.to_le_bytes().to_vec();
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&directory(&ifd0, 8));
    out.extend_from_slice(&directory(&exif, exif_at as usize));
    out
}

/// A JPEG with `c` as its EXIF: the segment put after the JFIF header
/// (where cameras' and editors' own files have it), any EXIF segment it
/// had taken out. A capture that says nothing leaves the file as it is.
pub fn with_capture(jpeg: &[u8], c: &Capture) -> Vec<u8> {
    if c.is_empty() || jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
        return jpeg.to_vec();
    }
    let tiff = write_tiff(c);
    let len = 2 + 6 + tiff.len();
    if len > u16::MAX as usize {
        return jpeg.to_vec();
    }
    let mut segment = vec![0xFF, 0xE1];
    segment.extend_from_slice(&(len as u16).to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(&tiff);
    // Walk the segments before the picture data: keep each but an old
    // EXIF, and put the new one in after the JFIF header (or first).
    let mut out = jpeg[..2].to_vec();
    let mut at = 2;
    let mut placed = false;
    while let (Some(&0xFF), Some(&kind)) = (jpeg.get(at), jpeg.get(at + 1)) {
        if kind == 0xDA || kind == 0xD9 {
            break;
        }
        let Some(len) = jpeg
            .get(at + 2..at + 4)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
        else {
            break;
        };
        let Some(seg) = jpeg.get(at..at + 2 + len) else {
            break;
        };
        if !placed && kind != 0xE0 {
            out.extend_from_slice(&segment);
            placed = true;
        }
        let old_exif = kind == 0xE1 && seg.get(4..10) == Some(b"Exif\0\0");
        if !old_exif {
            out.extend_from_slice(seg);
        }
        at += 2 + len;
    }
    if !placed {
        out.extend_from_slice(&segment);
    }
    out.extend_from_slice(&jpeg[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x_t5() -> Capture {
        Capture {
            make: "FUJIFILM".into(),
            model: "X-T5".into(),
            lens_make: "FUJIFILM".into(),
            lens_model: "XF16-80mmF4 R OIS WR".into(),
            taken: "2026:10:31 16:42:07".into(),
            exposure: Some([1, 250]),
            aperture: Some([56, 10]),
            focal: Some([230, 10]),
            iso: 400,
            bias: Some([-2, 3]),
        }
    }

    #[test]
    fn what_is_written_is_what_is_read() {
        let c = x_t5();
        assert_eq!(read_tiff(&write_tiff(&c)).unwrap(), c);
        // And an ISO past what a short holds.
        let high = Capture {
            iso: 102_400,
            ..x_t5()
        };
        assert_eq!(read_tiff(&write_tiff(&high)).unwrap().iso, 102_400);
    }

    #[test]
    fn a_jpeg_carries_it_and_still_decodes() {
        let px: Vec<chitrakar_color::LinearRgba> = (0..64)
            .map(|i| chitrakar_color::LinearRgba {
                r: i as f32 / 64.0,
                g: 0.3,
                b: 0.6,
                a: 1.0,
            })
            .collect();
        let plain = crate::encode_jpeg(8, 8, &px, 90).unwrap();
        assert_eq!(capture_of(&plain), None, "the encoder writes none");
        let with = with_capture(&plain, &x_t5());
        assert_eq!(capture_of(&with), Some(x_t5()));
        // Still a JPEG, of the same picture.
        let (a, b) = (
            crate::decode(&plain).unwrap(),
            crate::decode(&with).unwrap(),
        );
        assert_eq!(a.rgba8, b.rgba8);
        // After the JFIF header, which stays first.
        assert_eq!(&with[2..4], &[0xFF, 0xE0]);
        // Written again, it replaces what it had rather than adding.
        let other = Capture {
            model: "X100VI".into(),
            ..x_t5()
        };
        let again = with_capture(&with, &other);
        assert_eq!(capture_of(&again).unwrap().model, "X100VI");
        let segments = again.windows(6).filter(|w| w == b"Exif\0\0").count();
        assert_eq!(segments, 1, "one segment, not two");
        // Nothing to say, nothing written.
        assert_eq!(with_capture(&plain, &Capture::default()), plain);
    }

    #[test]
    fn a_big_endian_tiff_is_read() {
        // A camera's own: Motorola order, the make, and an EXIF directory
        // with a shutter and an ISO.
        let mut t = b"MM\0\x2a\0\0\0\x08".to_vec();
        // IFD0 at 8: two entries.
        t.extend_from_slice(&[0, 2]);
        t.extend_from_slice(&[0x01, 0x0F, 0, 2, 0, 0, 0, 4]);
        t.extend_from_slice(b"Sony");
        t.extend_from_slice(&[0x87, 0x69, 0, 4, 0, 0, 0, 1, 0, 0, 0, 38]);
        t.extend_from_slice(&[0, 0, 0, 0]);
        // EXIF IFD at 38: two entries, the shutter's value after it.
        t.extend_from_slice(&[0, 2]);
        t.extend_from_slice(&[0x82, 0x9A, 0, 5, 0, 0, 0, 1, 0, 0, 0, 68]);
        t.extend_from_slice(&[0x88, 0x27, 0, 3, 0, 0, 0, 1, 0x0C, 0x80, 0, 0]);
        t.extend_from_slice(&[0, 0, 0, 0]);
        t.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 60]);
        let c = read_tiff(&t).unwrap();
        assert_eq!(c.make, "Sony");
        assert_eq!(c.exposure, Some([1, 60]));
        assert_eq!(c.iso, 3200);
    }

    #[test]
    fn nonsense_is_nothing_rather_than_a_panic() {
        for bytes in [
            &b""[..],
            b"\xFF\xD8",
            b"\xFF\xD8\xFF\xE1\xFF\xFFExif\0\0II*\0",
            b"\xFF\xD8\xFF\xE1\0\x10Exif\0\0II*\0\xFF\xFF\xFF\xFF",
        ] {
            assert_eq!(capture_of(bytes), None);
        }
        assert!(read_tiff(b"II*\0\x08\0\0\0\xFF\xFF").is_some_and(|c| c.is_empty()));
    }
}
