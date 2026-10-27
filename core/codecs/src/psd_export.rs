//! Writing a Photoshop file with layers: what goes back to somebody who
//! works in Photoshop, as its layers rather than one flattened picture.
//!
//! 8-bit RGB with transparency, the PSD format every version of Photoshop
//! since 3 reads (and GIMP, Krita, Affinity and Photopea). Each layer
//! carries its name (in full, as Unicode, beside the 255-byte one the
//! format began with), its place, its opacity, its blend mode, whether it
//! is hidden and whether it is clipped to the layer below; groups are
//! groups. Pixels are run-length compressed (PackBits, the format's own),
//! row by row, and each layer is cut to the box of what it draws, so a
//! small mark on a large page is a small layer. The flattened picture is
//! written after the layers, as the format asks, for readers that show
//! only that — and its transparency with it.
//!
//! What a layer *is* — a shape, a text, an adjustment — is decided by the
//! engine, which hands this pixels (`chitrakar_engine::Session::export_psd`).

use crate::CodecError;
use chitrakar_doc::BlendMode;

/// The largest page the format holds on a side (past it is PSB).
pub const PSD_SIDE_MOST: u32 = 30_000;

/// A layer of pixels, at its place on the page.
#[derive(Debug, Clone)]
pub struct PsdOutLayer {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// 8-bit sRGB, straight alpha, `width × height`.
    pub rgba: Vec<u8>,
    pub opacity: f32,
    pub blend: BlendMode,
    pub visible: bool,
    pub clipped: bool,
}

/// A layer or a group of them, bottom first as the page stacks them.
#[derive(Debug, Clone)]
pub enum PsdOutItem {
    Layer(PsdOutLayer),
    Group {
        name: String,
        opacity: f32,
        blend: BlendMode,
        visible: bool,
        children: Vec<PsdOutItem>,
    },
}

impl PsdOutLayer {
    /// A page-sized picture cut to the box of what it draws; `None` when
    /// it draws nothing at all.
    pub fn trimmed(name: &str, page_w: u32, page_h: u32, rgba: &[u8]) -> PsdOutLayer {
        let (w, h) = (page_w as usize, page_h as usize);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if rgba[(y * w + x) * 4 + 3] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        let mut out = PsdOutLayer {
            name: name.to_string(),
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            rgba: Vec::new(),
            opacity: 1.0,
            blend: BlendMode::Normal,
            visible: true,
            clipped: false,
        };
        if x0 >= x1 {
            return out;
        }
        for y in y0..y1 {
            out.rgba
                .extend_from_slice(&rgba[(y * w + x0) * 4..(y * w + x1) * 4]);
        }
        (out.left, out.top, out.width, out.height) =
            (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32);
        out
    }
}

/// The key the format names a blend mode by.
fn blend_key(b: BlendMode) -> &'static [u8; 4] {
    match b {
        BlendMode::Normal => b"norm",
        BlendMode::Multiply => b"mul ",
        BlendMode::Screen => b"scrn",
        BlendMode::Overlay => b"over",
        BlendMode::Darken => b"dark",
        BlendMode::Lighten => b"lite",
        BlendMode::ColorDodge => b"div ",
        BlendMode::ColorBurn => b"idiv",
        BlendMode::HardLight => b"hLit",
        BlendMode::SoftLight => b"sLit",
        BlendMode::Difference => b"diff",
        BlendMode::Exclusion => b"smud",
        BlendMode::Hue => b"hue ",
        BlendMode::Saturation => b"sat ",
        BlendMode::Color => b"colr",
        BlendMode::Luminosity => b"lum ",
    }
}

/// PackBits: runs of three or more of a byte as a run, the rest as
/// literals, neither longer than 128 — the format's own compression.
fn packbits(row: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 3 {
            out.push((257 - run) as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        // A literal stretch, up to where a run of three begins.
        let start = i;
        while i < row.len() && i - start < 128 {
            if i + 2 < row.len() && row[i] == row[i + 1] && row[i] == row[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
}

/// One plane run-length compressed: the row lengths, then the rows.
fn compressed_plane(plane: &[u8], w: usize, h: usize) -> (Vec<u16>, Vec<u8>) {
    let (mut counts, mut data) = (Vec::with_capacity(h), Vec::new());
    for row in plane.chunks(w.max(1)).take(h) {
        let before = data.len();
        packbits(row, &mut data);
        counts.push((data.len() - before) as u16);
    }
    (counts, data)
}

fn plane_of(rgba: &[u8], c: usize) -> Vec<u8> {
    rgba.chunks_exact(4).map(|p| p[c]).collect()
}

/// A name the 255 bytes of the old field can hold: what is not ASCII
/// becomes `?`; the full name goes in the Unicode block beside it.
fn pascal(name: &str) -> Vec<u8> {
    let ascii: Vec<u8> = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c as u8
            } else {
                b'?'
            }
        })
        .take(255)
        .collect();
    let mut out = vec![ascii.len() as u8];
    out.extend(ascii);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

fn block(key: &[u8; 4], body: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(key);
    let padded = body.len() + body.len() % 2;
    out.extend_from_slice(&(padded as u32).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

fn unicode_name(name: &str) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut out = (units.len() as u32).to_be_bytes().to_vec();
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

/// A layer record as the format lays it down, with its channels' data.
struct Record {
    rect: [i32; 4],
    channels: Vec<(i16, Vec<u8>)>,
    blend: [u8; 4],
    opacity: u8,
    clipped: bool,
    hidden: bool,
    name: String,
    /// `lsct`: 0 a layer, 1 an open group, 3 the end of one.
    section: Option<(u32, [u8; 4])>,
}

fn records(items: &[PsdOutItem], out: &mut Vec<Record>) {
    for item in items {
        match item {
            PsdOutItem::Layer(l) => {
                let (w, h) = (l.width as usize, l.height as usize);
                let channels = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
                    .into_iter()
                    .map(|(id, c)| {
                        let mut data = Vec::new();
                        if w == 0 || h == 0 {
                            data.extend_from_slice(&0u16.to_be_bytes());
                        } else {
                            let (counts, rows) = compressed_plane(&plane_of(&l.rgba, c), w, h);
                            data.extend_from_slice(&1u16.to_be_bytes());
                            for n in counts {
                                data.extend_from_slice(&n.to_be_bytes());
                            }
                            data.extend(rows);
                        }
                        (id, data)
                    })
                    .collect();
                out.push(Record {
                    rect: [
                        l.top,
                        l.left,
                        l.top + l.height as i32,
                        l.left + l.width as i32,
                    ],
                    channels,
                    blend: *blend_key(l.blend),
                    opacity: (l.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                    clipped: l.clipped,
                    hidden: !l.visible,
                    name: l.name.clone(),
                    section: None,
                });
            }
            PsdOutItem::Group {
                name,
                opacity,
                blend,
                visible,
                children,
            } => {
                // Bottom first: the group's end, what it holds, then the
                // record that opens it and carries its look.
                let empty = || -> Vec<(i16, Vec<u8>)> {
                    [-1i16, 0, 1, 2]
                        .into_iter()
                        .map(|id| (id, 0u16.to_be_bytes().to_vec()))
                        .collect()
                };
                out.push(Record {
                    rect: [0; 4],
                    channels: empty(),
                    blend: *b"norm",
                    opacity: 255,
                    clipped: false,
                    hidden: false,
                    name: "</Layer group>".into(),
                    section: Some((3, *b"norm")),
                });
                records(children, out);
                out.push(Record {
                    rect: [0; 4],
                    channels: empty(),
                    blend: *blend_key(*blend),
                    opacity: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                    clipped: false,
                    hidden: !visible,
                    name: name.clone(),
                    section: Some((1, *blend_key(*blend))),
                });
            }
        }
    }
}

/// Write a Photoshop file: `width × height` at `dpi`, the items bottom
/// first, and the flattened picture (8-bit sRGB, straight alpha).
pub fn write_psd(
    width: u32,
    height: u32,
    dpi: f32,
    items: &[PsdOutItem],
    composite: &[u8],
) -> Result<Vec<u8>, CodecError> {
    if width == 0 || height == 0 || width > PSD_SIDE_MOST || height > PSD_SIDE_MOST {
        return Err(CodecError::Format(format!(
            "a Photoshop file holds pages up to {PSD_SIDE_MOST} pixels a side, not {width} by {height}"
        )));
    }
    if composite.len() != width as usize * height as usize * 4 {
        return Err(CodecError::Format(
            "the flattened picture is not the page's size".into(),
        ));
    }
    let mut f = Vec::new();
    f.extend_from_slice(b"8BPS");
    f.extend_from_slice(&1u16.to_be_bytes());
    f.extend_from_slice(&[0; 6]);
    f.extend_from_slice(&4u16.to_be_bytes()); // red, green, blue and transparency
    f.extend_from_slice(&height.to_be_bytes());
    f.extend_from_slice(&width.to_be_bytes());
    f.extend_from_slice(&8u16.to_be_bytes());
    f.extend_from_slice(&3u16.to_be_bytes()); // RGB
    f.extend_from_slice(&0u32.to_be_bytes()); // no colour mode data
                                              // Image resources: the resolution (1005), so the page prints at the
                                              // size it was made for.
    let mut resources = Vec::new();
    let fixed = ((dpi.clamp(1.0, 30_000.0) * 65536.0).round() as u32).to_be_bytes();
    let mut res = Vec::new();
    for _ in 0..2 {
        res.extend_from_slice(&fixed);
        res.extend_from_slice(&1u16.to_be_bytes()); // pixels per inch
        res.extend_from_slice(&1u16.to_be_bytes()); // shown in inches
    }
    resources.extend_from_slice(b"8BIM");
    resources.extend_from_slice(&1005u16.to_be_bytes());
    resources.extend_from_slice(&[0, 0]); // an empty name, padded
    resources.extend_from_slice(&(res.len() as u32).to_be_bytes());
    resources.extend(res);
    f.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    f.extend(resources);

    let mut recs = Vec::new();
    records(items, &mut recs);
    let mut info = Vec::new();
    // Negative: the flattened picture's first extra channel is its
    // transparency.
    info.extend_from_slice(&(-(recs.len() as i16)).to_be_bytes());
    let mut data = Vec::new();
    for r in &recs {
        for v in r.rect {
            info.extend_from_slice(&v.to_be_bytes());
        }
        info.extend_from_slice(&(r.channels.len() as u16).to_be_bytes());
        for (id, d) in &r.channels {
            info.extend_from_slice(&id.to_be_bytes());
            info.extend_from_slice(&(d.len() as u32).to_be_bytes());
        }
        info.extend_from_slice(b"8BIM");
        info.extend_from_slice(&r.blend);
        info.push(r.opacity);
        info.push(r.clipped as u8);
        // Flags: bit 1 hidden, bit 3 set so bit 4 is read (it is clear:
        // the pixels matter to how the layer looks).
        info.push(if r.hidden { 2 } else { 0 } | 8);
        info.push(0);
        let mut extra = Vec::new();
        extra.extend_from_slice(&0u32.to_be_bytes()); // no layer mask
        extra.extend_from_slice(&0u32.to_be_bytes()); // no blending ranges
        extra.extend(pascal(&r.name));
        block(b"luni", &unicode_name(&r.name), &mut extra);
        if let Some((kind, key)) = r.section {
            let mut body = kind.to_be_bytes().to_vec();
            if kind == 1 {
                body.extend_from_slice(b"8BIM");
                body.extend_from_slice(&key);
            }
            block(b"lsct", &body, &mut extra);
        }
        info.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        info.extend(extra);
        for (_, d) in &r.channels {
            data.extend_from_slice(d);
        }
    }
    info.extend(data);
    if info.len() % 2 == 1 {
        info.push(0);
    }
    let mut lm = Vec::new();
    lm.extend_from_slice(&(info.len() as u32).to_be_bytes());
    lm.extend(info);
    lm.extend_from_slice(&0u32.to_be_bytes()); // no global mask
    f.extend_from_slice(&(lm.len() as u32).to_be_bytes());
    f.extend(lm);

    // The flattened picture: every channel's row lengths, then every
    // channel's rows.
    let (w, h) = (width as usize, height as usize);
    f.extend_from_slice(&1u16.to_be_bytes());
    let planes: Vec<(Vec<u16>, Vec<u8>)> = (0..4)
        .map(|c| compressed_plane(&plane_of(composite, c), w, h))
        .collect();
    for (counts, _) in &planes {
        for n in counts {
            f.extend_from_slice(&n.to_be_bytes());
        }
    }
    for (_, rows) in planes {
        f.extend(rows);
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psd::{read_psd, PsdItem};

    fn unpack(mut src: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some((&n, rest)) = src.split_first() {
            src = rest;
            if n < 128 {
                out.extend_from_slice(&src[..n as usize + 1]);
                src = &src[n as usize + 1..];
            } else if n > 128 {
                out.extend(std::iter::repeat_n(src[0], 257 - n as usize));
                src = &src[1..];
            }
        }
        out
    }

    #[test]
    fn packbits_comes_back_as_it_went() {
        let mut seed = 5u32;
        for len in [0, 1, 2, 3, 127, 128, 129, 300, 1000] {
            for kind in 0..3 {
                let row: Vec<u8> = (0..len)
                    .map(|i| {
                        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                        match kind {
                            0 => (seed >> 24) as u8,
                            1 => 7,
                            _ => (i / 5) as u8,
                        }
                    })
                    .collect();
                let mut packed = Vec::new();
                packbits(&row, &mut packed);
                assert_eq!(unpack(&packed), row, "length {len}, kind {kind}");
                if kind == 1 && len > 3 {
                    assert!(
                        packed.len() < len / 10 + 4,
                        "a run is short: {} for {len}",
                        packed.len()
                    );
                }
            }
        }
    }

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        px.repeat((w * h) as usize)
    }

    #[test]
    fn layers_groups_and_their_looks_come_back_through_the_reader() {
        let mut back = PsdOutLayer::trimmed("Back", 8, 6, &solid(8, 6, [10, 20, 30, 255]));
        back.blend = BlendMode::Normal;
        let mut mark = vec![0u8; 8 * 6 * 4];
        for y in 2..4 {
            for x in 3..6 {
                mark[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[250, 0, 0, 255]);
            }
        }
        let mut red = PsdOutLayer::trimmed("Röd märke", 8, 6, &mark);
        assert_eq!(
            (red.left, red.top, red.width, red.height),
            (3, 2, 3, 2),
            "cut to what it draws"
        );
        red.blend = BlendMode::Multiply;
        red.opacity = 0.5;
        red.clipped = true;
        red.visible = false;
        let items = vec![
            PsdOutItem::Layer(back),
            PsdOutItem::Group {
                name: "Marks".into(),
                opacity: 0.75,
                blend: BlendMode::Screen,
                visible: true,
                children: vec![PsdOutItem::Layer(red)],
            },
        ];
        let bytes = write_psd(8, 6, 300.0, &items, &solid(8, 6, [1, 2, 3, 255])).unwrap();
        let f = read_psd(&bytes).unwrap();
        assert_eq!((f.width, f.height), (8, 6));
        assert!(f.lost.is_empty(), "{:?}", f.lost);
        assert_eq!(f.items.len(), 2);
        let PsdItem::Pixels(b) = &f.items[0] else {
            panic!("{:?}", f.items[0])
        };
        assert_eq!(
            (b.name.as_str(), b.width, &b.rgba[..4]),
            ("Back", 8, &[10u8, 20, 30, 255][..])
        );
        let PsdItem::Group {
            name,
            opacity,
            blend,
            children,
            ..
        } = &f.items[1]
        else {
            panic!()
        };
        assert_eq!((name.as_str(), *blend), ("Marks", BlendMode::Screen));
        assert!((opacity - 0.75).abs() < 0.01);
        let PsdItem::Pixels(r) = &children[0] else {
            panic!()
        };
        assert_eq!(r.name, "Röd märke", "the full name, from the Unicode block");
        assert_eq!((r.left, r.top, r.width, r.height), (3, 2, 3, 2));
        assert_eq!(r.blend, BlendMode::Multiply);
        assert!((r.opacity - 0.5).abs() < 0.01 && r.clipped && !r.visible);
        assert_eq!(&r.rgba[..4], &[250, 0, 0, 255]);
        assert_eq!(
            &f.composite.unwrap()[..4],
            &[1, 2, 3, 255],
            "and the flattened picture"
        );
    }

    #[test]
    fn a_layer_that_draws_nothing_is_written_and_read_as_empty() {
        let none = PsdOutLayer::trimmed("Nothing", 4, 4, &[0; 64]);
        assert_eq!((none.width, none.height), (0, 0));
        let bytes = write_psd(4, 4, 72.0, &[PsdOutItem::Layer(none)], &[0; 64]).unwrap();
        assert!(read_psd(&bytes).is_ok());
    }

    #[test]
    fn a_page_too_big_for_the_format_is_said_so() {
        let err = write_psd(40_000, 10, 72.0, &[], &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("30000"), "{err}");
    }
}
