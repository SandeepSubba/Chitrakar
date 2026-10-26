//! Reading a Photoshop file (PSD, and the large-document PSB) as layers.
//!
//! Written here rather than taken from a crate: the one there is reads
//! 8 bits only, mistakes CMYK for RGB, and has places it can panic — and a
//! panic in the engine, which runs as WASM, takes the whole editor with
//! it. Every read here is bounds-checked and answers with an error.
//!
//! What comes across: every pixel layer, at its place, with its name
//! (the Unicode one where there is one), its opacity (times its fill),
//! whether it shows, its blend mode, whether it is clipped to the layer
//! under it, and its layer mask — as a mask, still editable, not burnt
//! into the pixels. Groups come across as groups. Text, smart objects and
//! shapes come across as the pixels Photoshop drew of them, which is what
//! the file keeps for a reader that is not Photoshop. What cannot come
//! across — an adjustment layer, a layer's effects, a blend mode this
//! editor has no equal of — is said (`lost`), and Photoshop's own picture
//! of the whole file comes in beside the layers, hidden, so that nothing
//! is silently different.
//!
//! Depths 1, 8, 16 and 32 bits; RGB, grey, CMYK, Lab, indexed, bitmap,
//! duotone (as its grey) and multichannel (its first channels);
//! uncompressed, run-length, and ZIP with and without prediction; an
//! embedded colour profile honoured as a photograph's is.

use crate::CodecError;
use chitrakar_doc::BlendMode;

/// Whether the bytes are a Photoshop file.
pub fn is_psd(b: &[u8]) -> bool {
    b.len() >= 6 && &b[..4] == b"8BPS" && matches!(b[4..6], [0, 1] | [0, 2])
}

/// A Photoshop file, read.
#[derive(Debug)]
pub struct PsdFile {
    pub width: u32,
    pub height: u32,
    /// Bottom first, as the editor stacks them.
    pub items: Vec<PsdItem>,
    /// Photoshop's picture of the whole file, 8-bit sRGB, when it keeps
    /// one (it almost always does).
    pub composite: Option<Vec<u8>>,
    /// What could not come across, in words, one entry each.
    pub lost: Vec<String>,
}

#[derive(Debug)]
pub enum PsdItem {
    Pixels(PsdPixels),
    Group {
        name: String,
        visible: bool,
        opacity: f32,
        blend: BlendMode,
        /// Bottom first.
        children: Vec<PsdItem>,
    },
}

#[derive(Debug)]
pub struct PsdPixels {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// 8-bit sRGB, straight alpha.
    pub rgba: Vec<u8>,
    pub opacity: f32,
    pub visible: bool,
    pub blend: BlendMode,
    pub clipped: bool,
    pub mask: Option<PsdMask>,
}

/// A layer mask, as the editor's raster masks read one: grey whose
/// luminance in linear light is the coverage, over a rectangle of the
/// document, nothing outside it.
#[derive(Debug)]
pub struct PsdMask {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn bad(what: impl std::fmt::Display) -> CodecError {
    CodecError::Format(format!("Photoshop file: {what}"))
}

/// A cursor that answers running off the end with an error.
struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| bad("ends early"))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, CodecError> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Result<i16, CodecError> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, CodecError> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, CodecError> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> Result<u64, CodecError> {
        let s = self.take(8)?;
        Ok(u64::from_be_bytes(s.try_into().unwrap_or([0; 8])))
    }
    /// A length that is 8 bytes in a PSB and 4 in a PSD.
    fn len(&mut self, big: bool) -> Result<usize, CodecError> {
        let n = if big { self.u64()? } else { self.u32()? as u64 };
        usize::try_from(n).map_err(|_| bad("a length past what can be held"))
    }
    fn sub(&mut self, n: usize) -> Result<R<'a>, CodecError> {
        Ok(R {
            b: self.take(n)?,
            p: 0,
        })
    }
    fn left(&self) -> usize {
        self.b.len() - self.p
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Bitmap,
    Grey,
    Indexed,
    Rgb,
    Cmyk,
    Multichannel,
    Duotone,
    Lab,
}

struct Header {
    big: bool,
    channels: usize,
    width: u32,
    height: u32,
    depth: u16,
    mode: Mode,
}

/// Read a Photoshop file into its layers.
pub fn read_psd(bytes: &[u8]) -> Result<PsdFile, CodecError> {
    if !is_psd(bytes) {
        return Err(bad("not a Photoshop file"));
    }
    let mut r = R { b: bytes, p: 4 };
    let big = r.u16()? == 2;
    r.take(6)?;
    let channels = r.u16()? as usize;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    let mode = match r.u16()? {
        0 => Mode::Bitmap,
        1 => Mode::Grey,
        2 => Mode::Indexed,
        3 => Mode::Rgb,
        4 => Mode::Cmyk,
        7 => Mode::Multichannel,
        8 => Mode::Duotone,
        9 => Mode::Lab,
        m => return Err(bad(format!("colour mode {m} is not read"))),
    };
    if !matches!(depth, 1 | 8 | 16 | 32) {
        return Err(bad(format!("{depth} bits a channel are not read")));
    }
    if width == 0 || height == 0 || (width as u64 * height as u64) > 1 << 31 {
        return Err(bad("a size the editor cannot hold"));
    }
    let h = Header {
        big,
        channels,
        width,
        height,
        depth,
        mode,
    };

    let n = r.u32()? as usize;
    let palette = r.take(n)?.to_vec();
    // Image resources: the colour profile (1039) is the one read.
    let n = r.u32()? as usize;
    let mut res = r.sub(n)?;
    let mut icc: Option<Vec<u8>> = None;
    while res.left() >= 12 {
        if res.take(4)? != b"8BIM" {
            break;
        }
        let id = res.u16()?;
        let name_len = res.u8()? as usize;
        res.take(name_len + (name_len + 1) % 2)?;
        let size = res.u32()? as usize;
        let data = res.take(size)?;
        if size % 2 == 1 {
            res.take(1).ok();
        }
        if id == 1039 {
            icc = Some(data.to_vec());
        }
    }
    let colour = Colour::new(&h, &palette, icc.as_deref());

    let n = r.len(big)?;
    let mut lm = r.sub(n)?;
    let mut lost: Vec<String> = Vec::new();
    let mut items = Vec::new();
    if lm.left() > 0 {
        let n = lm.len(big)?;
        let mut info = lm.sub(n)?;
        if info.left() > 0 {
            items = read_layers(&mut info, &h, &colour, &mut lost)?;
        }
        // 16- and 32-bit files keep their layers in a block at the end of
        // the section — and may keep a placeholder list where an 8-bit
        // file keeps its own — so a block found there is the file's.
        let n = lm.u32().unwrap_or(0) as usize;
        lm.take(n).ok();
        while lm.left() >= 12 {
            let sig = lm.take(4)?;
            if sig != b"8BIM" && sig != b"8B64" {
                break;
            }
            let key: [u8; 4] = lm.take(4)?.try_into().unwrap_or([0; 4]);
            let wide = big
                && matches!(
                    &key,
                    b"Lr16"
                        | b"Lr32"
                        | b"Layr"
                        | b"LMsk"
                        | b"Mt16"
                        | b"Mt32"
                        | b"Mtrn"
                        | b"Alph"
                        | b"FMsk"
                        | b"lnk2"
                        | b"FEid"
                        | b"FXid"
                        | b"PxSD"
                );
            let n = if wide {
                lm.len(true)?
            } else {
                lm.u32()? as usize
            };
            let mut block = lm.sub(n)?;
            while lm.left() > 0 && lm.p % 4 != 0 && lm.b[lm.p] == 0 {
                lm.p += 1;
            }
            if matches!(&key, b"Lr16" | b"Lr32" | b"Layr") {
                let mut said = Vec::new();
                items = read_layers(&mut block, &h, &colour, &mut said)?;
                lost = said;
            }
        }
    }

    // Photoshop's own picture of the whole file.
    let composite = read_merged(&mut r, &h, &colour).ok();
    if items.is_empty() {
        if let Some(px) = &composite {
            items.push(PsdItem::Pixels(PsdPixels {
                name: "Background".into(),
                left: 0,
                top: 0,
                width,
                height,
                rgba: px.clone(),
                opacity: 1.0,
                visible: true,
                blend: BlendMode::Normal,
                clipped: false,
                mask: None,
            }));
        } else {
            return Err(bad("holds no picture that could be read"));
        }
    }
    Ok(PsdFile {
        width,
        height,
        items,
        composite,
        lost,
    })
}

/// A layer record's place in the stack: a group opening or closing, or
/// a layer of pixels.
enum Section {
    Pixels,
    Opens,
    Closes,
}

struct Record {
    name: String,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    channels: Vec<(i16, usize)>,
    blend: Option<BlendMode>,
    blend_key: [u8; 4],
    opacity: f32,
    clipped: bool,
    visible: bool,
    section: Section,
    mask: Option<(i32, i32, i32, i32, u8, bool)>,
    /// The second box, for channel -3, when the layer has a vector mask too.
    real_mask: Option<(i32, i32, i32, i32, u8, bool)>,
    adjustment: bool,
    effects: bool,
    /// A shape or a fill: kept as a drawing, its pixels possibly not at all.
    drawn: bool,
}

fn read_layers(
    r: &mut R,
    h: &Header,
    colour: &Colour,
    lost: &mut Vec<String>,
) -> Result<Vec<PsdItem>, CodecError> {
    let count = r.i16()?.unsigned_abs() as usize;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(read_record(r, h.big)?);
    }
    // The channels' pixels follow every record, in the records' order.
    let mut pixels: Vec<Option<PsdPixels>> = Vec::with_capacity(count);
    for rec in &records {
        let (w, hh) = (
            (rec.right - rec.left).max(0) as usize,
            (rec.bottom - rec.top).max(0) as usize,
        );
        let mut planes: Vec<(i16, Vec<f32>)> = Vec::new();
        for &(id, len) in &rec.channels {
            let mut ch = r.sub(len)?;
            let boxed = match id {
                -2 => rec.mask,
                -3 => rec.real_mask,
                _ => None,
            };
            let (cw, chh) = match (id, boxed) {
                (-2 | -3, Some((t, l, b, rr, _, _))) => {
                    ((rr - l).max(0) as usize, (b - t).max(0) as usize)
                }
                (-2 | -3, None) => (0, 0),
                _ => (w, hh),
            };
            if cw == 0 || chh == 0 || ch.left() < 2 {
                continue;
            }
            let compression = ch.u16()?;
            // One channel that will not read costs that channel, not the
            // file: the rest of the layers are still worth having.
            match decode_plane(&mut ch, compression, cw, chh, h.depth, h.big) {
                Ok(plane) => planes.push((id, plane)),
                Err(e) => lost.push(format!("a channel of \"{}\" ({e})", rec.name)),
            }
        }
        if !matches!(rec.section, Section::Pixels) || w == 0 || hh == 0 {
            pixels.push(None);
            continue;
        }
        let get = |id: i16| {
            planes
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, p)| p.as_slice())
        };
        let rgba = colour.layer_rgba(&get, w * hh);
        // The layer's pixel mask: the "real" one when the layer has a
        // vector mask too, else the only one.
        let (mask_box, mask_id) = match (rec.real_mask, get(-3).is_some()) {
            (Some(m), true) => (Some(m), -3),
            _ => (rec.mask, -2),
        };
        let mask = match mask_box {
            Some((t, l, b, rr, default, false)) if get(mask_id).is_some() => {
                // Over the union of the layer and the mask's own box, the
                // mask's default where it says nothing.
                let (x0, y0) = (rec.left.min(l), rec.top.min(t));
                let (x1, y1) = (rec.right.max(rr), rec.bottom.max(b));
                let (mw, mh) = ((x1 - x0) as usize, (y1 - y0) as usize);
                let plane = get(mask_id).unwrap_or(&[]);
                let (pw, _) = ((rr - l) as usize, (b - t) as usize);
                let mut out = vec![0u8; mw * mh * 4];
                for y in 0..mh {
                    for x in 0..mw {
                        let (dx, dy) = (x0 + x as i32 - l, y0 + y as i32 - t);
                        let v = if dx >= 0 && dy >= 0 && dx < rr - l && dy < b - t {
                            plane
                                .get(dy as usize * pw + dx as usize)
                                .copied()
                                .unwrap_or(0.0)
                        } else {
                            default as f32 / 255.0
                        };
                        // Coverage is read as linear luminance, so the
                        // grey is stored encoded: v of coverage, v again.
                        let g = chitrakar_color::linear_to_srgb8(v.clamp(0.0, 1.0));
                        out[(y * mw + x) * 4..(y * mw + x) * 4 + 4]
                            .copy_from_slice(&[g, g, g, 255]);
                    }
                }
                Some(PsdMask {
                    left: x0,
                    top: y0,
                    width: mw as u32,
                    height: mh as u32,
                    rgba: out,
                })
            }
            _ => None,
        };
        pixels.push(Some(PsdPixels {
            name: rec.name.clone(),
            left: rec.left,
            top: rec.top,
            width: w as u32,
            height: hh as u32,
            rgba,
            opacity: rec.opacity,
            visible: rec.visible,
            blend: rec.blend.unwrap_or_default(),
            clipped: rec.clipped,
            mask,
        }));
    }

    // Records run bottom to top: a group's closing record comes first
    // (Photoshop's "bounding section divider"), then what is in it, then
    // the record that names it.
    let mut stack: Vec<Vec<PsdItem>> = vec![Vec::new()];
    for (rec, px) in records.into_iter().zip(pixels) {
        if rec.adjustment {
            lost.push(format!("the adjustment layer \"{}\"", rec.name));
        }
        if rec.effects {
            lost.push(format!("the effects on \"{}\"", rec.name));
        }
        if rec.drawn && px.is_none() && matches!(rec.section, Section::Pixels) {
            lost.push(format!(
                "the shape \"{}\", which the file keeps as a drawing and not as pixels",
                rec.name
            ));
        }
        if rec.blend.is_none() && !matches!(&rec.blend_key, b"pass") {
            lost.push(format!(
                "the blend mode of \"{}\" ({})",
                rec.name,
                String::from_utf8_lossy(&rec.blend_key).trim()
            ));
        }
        match rec.section {
            Section::Closes => stack.push(Vec::new()),
            Section::Opens => {
                let children = if stack.len() > 1 {
                    stack.pop().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let group = PsdItem::Group {
                    name: rec.name,
                    visible: rec.visible,
                    opacity: rec.opacity,
                    blend: rec.blend.unwrap_or_default(),
                    children,
                };
                if let Some(top) = stack.last_mut() {
                    top.push(group);
                }
            }
            Section::Pixels => {
                if let (Some(p), Some(top)) = (px, stack.last_mut()) {
                    top.push(PsdItem::Pixels(p));
                }
            }
        }
    }
    // A group opened and never named folds into what holds it.
    while stack.len() > 1 {
        let inner = stack.pop().unwrap_or_default();
        if let Some(top) = stack.last_mut() {
            top.extend(inner);
        }
    }
    Ok(stack.pop().unwrap_or_default())
}

fn read_record(r: &mut R, big: bool) -> Result<Record, CodecError> {
    let top = r.i32()?;
    let left = r.i32()?;
    let bottom = r.i32()?;
    let right = r.i32()?;
    if bottom < top || right < left || (right - left) as u64 * (bottom - top) as u64 > 1 << 31 {
        return Err(bad("a layer of impossible size"));
    }
    let n = r.u16()? as usize;
    let mut channels = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.i16()?;
        let len = r.len(big)?;
        channels.push((id, len));
    }
    if r.take(4)? != b"8BIM" {
        return Err(bad("a layer record without its signature"));
    }
    let blend_key: [u8; 4] = r.take(4)?.try_into().unwrap_or([0; 4]);
    let opacity = r.u8()? as f32 / 255.0;
    let clipped = r.u8()? != 0;
    let flags = r.u8()?;
    r.u8()?;
    let n = r.u32()? as usize;
    let mut extra = r.sub(n)?;
    // The layer mask.
    let n = extra.u32()? as usize;
    let mut m = extra.sub(n)?;
    let mut real_mask = None;
    let mask = if n >= 18 {
        let (t, l, b, rr) = (m.i32()?, m.i32()?, m.i32()?, m.i32()?);
        let default = m.u8()?;
        let mflags = m.u8()?;
        // A layer with a vector mask as well keeps a second box: the
        // "real" user mask, whose pixels are channel -3. Its parameters
        // (densities and feathers) come first when the flags say so.
        if mflags & 0x10 != 0 {
            if let Ok(which) = m.u8() {
                for (bit, size) in [(1u8, 1usize), (2, 8), (4, 1), (8, 8)] {
                    if which & bit != 0 {
                        m.take(size).ok();
                    }
                }
            }
        }
        if m.left() >= 18 {
            let rflags = m.u8()?;
            let rdefault = m.u8()?;
            let (rt, rl, rb, rr2) = (m.i32()?, m.i32()?, m.i32()?, m.i32()?);
            real_mask = Some((rt, rl, rb, rr2, rdefault, rflags & 2 != 0));
        }
        Some((t, l, b, rr, default, mflags & 2 != 0))
    } else {
        None
    };
    // Blending ranges: not carried.
    let n = extra.u32()? as usize;
    extra.take(n)?;
    let n = extra.u8()? as usize;
    let mut name = String::from_utf8_lossy(extra.take(n)?).into_owned();
    let pad = (4 - (n + 1) % 4) % 4;
    extra.take(pad.min(extra.left())).ok();
    let mut section = Section::Pixels;
    let mut fill = 1.0f32;
    let mut adjustment = false;
    let mut effects = false;
    let mut drawn = false;
    let mut group_blend: Option<[u8; 4]> = None;
    while extra.left() >= 12 {
        let sig = extra.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let key: [u8; 4] = extra.take(4)?.try_into().unwrap_or([0; 4]);
        let n = extra.u32()? as usize;
        let mut d = extra.sub(n.min(extra.left()))?;
        if n % 2 == 1 && extra.left() > 0 {
            extra.p += 1;
        }
        match &key {
            b"luni" => {
                let chars = d.u32().unwrap_or(0) as usize;
                if let Ok(raw) = d.take(chars * 2) {
                    let units: Vec<u16> = raw
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    let s = String::from_utf16_lossy(&units);
                    let s = s.trim_end_matches('\0').to_string();
                    if !s.is_empty() {
                        name = s;
                    }
                }
            }
            b"lsct" | b"lsdk" => {
                section = match d.u32().unwrap_or(0) {
                    1 | 2 => Section::Opens,
                    3 => Section::Closes,
                    _ => Section::Pixels,
                };
                if d.left() >= 8 && d.take(4).ok() == Some(b"8BIM") {
                    group_blend = d.take(4).ok().and_then(|k| k.try_into().ok());
                }
            }
            b"iOpa" => fill = d.u8().unwrap_or(255) as f32 / 255.0,
            b"lfx2" | b"lrFX" | b"lmfx" => effects = true,
            b"SoCo" | b"GdFl" | b"PtFl" | b"vmsk" | b"vsms" | b"vogk" => drawn = true,
            b"levl" | b"curv" | b"brit" | b"blnc" | b"hue2" | b"hue " | b"selc" | b"mixr"
            | b"grdm" | b"phfl" | b"expA" | b"vibA" | b"thrs" | b"post" | b"nvrt" | b"blwh"
            | b"clrL" => adjustment = true,
            _ => {}
        }
    }
    let key = group_blend.unwrap_or(blend_key);
    let blend = blend_of(&key);
    Ok(Record {
        name,
        top,
        left,
        bottom,
        right,
        channels,
        blend,
        blend_key: key,
        opacity: opacity * fill,
        clipped,
        visible: flags & 2 == 0,
        section,
        mask,
        real_mask,
        adjustment,
        effects,
        drawn,
    })
}

/// Photoshop's blend mode, where the editor has its equal.
fn blend_of(key: &[u8; 4]) -> Option<BlendMode> {
    Some(match key {
        b"norm" | b"pass" => BlendMode::Normal,
        b"mul " => BlendMode::Multiply,
        b"scrn" => BlendMode::Screen,
        b"over" => BlendMode::Overlay,
        b"dark" => BlendMode::Darken,
        b"lite" => BlendMode::Lighten,
        b"div " => BlendMode::ColorDodge,
        b"idiv" => BlendMode::ColorBurn,
        b"hLit" => BlendMode::HardLight,
        b"sLit" => BlendMode::SoftLight,
        b"diff" => BlendMode::Difference,
        b"smud" => BlendMode::Exclusion,
        b"hue " => BlendMode::Hue,
        b"sat " => BlendMode::Saturation,
        b"colr" => BlendMode::Color,
        b"lum " => BlendMode::Luminosity,
        _ => return None,
    })
}

/// One channel's samples, 0..=1, decompressed.
fn decode_plane(
    r: &mut R,
    compression: u16,
    w: usize,
    h: usize,
    depth: u16,
    big: bool,
) -> Result<Vec<f32>, CodecError> {
    let row = (w * depth as usize).div_ceil(8);
    let raw: Vec<u8> = match compression {
        0 => r.take(row * h)?.to_vec(),
        1 => {
            let mut counts = Vec::with_capacity(h);
            for _ in 0..h {
                counts.push(if big {
                    r.u32()? as usize
                } else {
                    r.u16()? as usize
                });
            }
            let mut out = Vec::with_capacity(row * h);
            for n in counts {
                unpackbits(r.take(n)?, row, &mut out)?;
            }
            out
        }
        2 | 3 => {
            use std::io::Read;
            let mut out = Vec::with_capacity(row * h);
            flate2::read::ZlibDecoder::new(r.take(r.left())?)
                .take((row * h) as u64)
                .read_to_end(&mut out)
                .map_err(|e| bad(format!("ZIP: {e}")))?;
            if out.len() < row * h {
                return Err(bad("ZIP data shorter than its channel"));
            }
            if compression == 3 {
                unpredict(&mut out, row, h, depth);
            }
            out
        }
        c => return Err(bad(format!("compression {c} is not read"))),
    };
    samples(&raw, w, h, depth)
}

/// Run-length (PackBits) bytes of one row onto `out`.
fn unpackbits(mut src: &[u8], row: usize, out: &mut Vec<u8>) -> Result<(), CodecError> {
    let start = out.len();
    while out.len() - start < row && !src.is_empty() {
        let n = src[0] as i8;
        src = &src[1..];
        if n >= 0 {
            let k = n as usize + 1;
            let lit = src.get(..k).ok_or_else(|| bad("a run past its row"))?;
            out.extend_from_slice(lit);
            src = &src[k..];
        } else if n != -128 {
            let v = *src.first().ok_or_else(|| bad("a run past its row"))?;
            out.extend(std::iter::repeat_n(v, 1 - n as isize as usize));
            src = &src[1..];
        }
    }
    out.resize(start + row, 0);
    Ok(())
}

/// ZIP with prediction: each row held as differences along it.
fn unpredict(data: &mut [u8], row: usize, h: usize, depth: u16) {
    for y in 0..h {
        let r = &mut data[y * row..(y + 1) * row];
        match depth {
            16 => {
                let mut prev = 0u16;
                for c in r.chunks_exact_mut(2) {
                    let v = u16::from_be_bytes([c[0], c[1]]).wrapping_add(prev);
                    c.copy_from_slice(&v.to_be_bytes());
                    prev = v;
                }
            }
            32 => {
                for i in 1..r.len() {
                    r[i] = r[i].wrapping_add(r[i - 1]);
                }
                // The bytes of each float were laid out as four planes.
                let w = r.len() / 4;
                let planes = r.to_vec();
                for x in 0..w {
                    for b in 0..4 {
                        r[x * 4 + b] = planes[b * w + x];
                    }
                }
            }
            _ => {
                for i in 1..r.len() {
                    r[i] = r[i].wrapping_add(r[i - 1]);
                }
            }
        }
    }
}

fn samples(raw: &[u8], w: usize, h: usize, depth: u16) -> Result<Vec<f32>, CodecError> {
    let row = (w * depth as usize).div_ceil(8);
    if raw.len() < row * h {
        return Err(bad("a channel shorter than its picture"));
    }
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let r = &raw[y * row..(y + 1) * row];
        for x in 0..w {
            out.push(match depth {
                1 => {
                    // A set bit is black.
                    if r[x / 8] & (0x80 >> (x % 8)) != 0 {
                        0.0
                    } else {
                        1.0
                    }
                }
                8 => r[x] as f32 / 255.0,
                16 => u16::from_be_bytes([r[x * 2], r[x * 2 + 1]]) as f32 / 65535.0,
                _ => {
                    // 32 bits a channel is light, linear: encoded here so
                    // every depth hands on the same kind of value.
                    let v =
                        f32::from_be_bytes([r[x * 4], r[x * 4 + 1], r[x * 4 + 2], r[x * 4 + 3]]);
                    chitrakar_color::linear_to_srgb(if v.is_finite() {
                        v.clamp(0.0, 1.0)
                    } else {
                        0.0
                    })
                }
            });
        }
    }
    Ok(out)
}

/// Photoshop's picture of the whole file: every channel's plane, one
/// after another, under one compression.
fn read_merged(r: &mut R, h: &Header, colour: &Colour) -> Result<Vec<u8>, CodecError> {
    let compression = r.u16()?;
    let (w, hh) = (h.width as usize, h.height as usize);
    let n = h.channels.min(5);
    let mut planes: Vec<Vec<f32>> = Vec::with_capacity(n);
    if compression == 1 {
        // The run lengths of every row of every channel come first.
        let mut counts = Vec::with_capacity(h.channels * hh);
        for _ in 0..h.channels * hh {
            counts.push(if h.big {
                r.u32()? as usize
            } else {
                r.u16()? as usize
            });
        }
        let row = (w * h.depth as usize).div_ceil(8);
        for c in 0..n {
            let mut raw = Vec::with_capacity(row * hh);
            for y in 0..hh {
                unpackbits(r.take(counts[c * hh + y])?, row, &mut raw)?;
            }
            planes.push(samples(&raw, w, hh, h.depth)?);
        }
    } else if compression == 0 {
        for _ in 0..n {
            let row = (w * h.depth as usize).div_ceil(8);
            let raw = r.take(row * hh)?;
            planes.push(samples(raw, w, hh, h.depth)?);
        }
    } else {
        // ZIP: all channels in one stream.
        let row = (w * h.depth as usize).div_ceil(8);
        use std::io::Read;
        let mut all = Vec::new();
        flate2::read::ZlibDecoder::new(r.take(r.left())?)
            .take((row * hh * h.channels) as u64)
            .read_to_end(&mut all)
            .map_err(|e| bad(format!("ZIP: {e}")))?;
        for c in 0..n {
            let mut one = all
                .get(c * row * hh..(c + 1) * row * hh)
                .ok_or_else(|| bad("ZIP data shorter than the picture"))?
                .to_vec();
            if compression == 3 {
                unpredict(&mut one, row, hh, h.depth);
            }
            planes.push(samples(&one, w, hh, h.depth)?);
        }
    }
    let alpha_index = match h.mode {
        Mode::Rgb | Mode::Lab => 3,
        Mode::Cmyk => 4,
        Mode::Grey | Mode::Duotone => 1,
        _ => usize::MAX,
    };
    let get = |id: i16| -> Option<&[f32]> {
        let i = if id == -1 { alpha_index } else { id as usize };
        planes.get(i).map(|p| p.as_slice())
    };
    Ok(colour.layer_rgba(&get, w * hh))
}

/// How the file's channels make a colour.
struct Colour {
    mode: Mode,
    palette: Vec<u8>,
    icc_rgb: Option<Vec<u8>>,
    cmyk: Option<chitrakar_color::cms::CmykCms>,
}

impl Colour {
    fn new(h: &Header, palette: &[u8], icc: Option<&[u8]>) -> Colour {
        Colour {
            mode: h.mode,
            palette: palette.to_vec(),
            icc_rgb: icc.filter(|_| h.mode == Mode::Rgb).map(<[u8]>::to_vec),
            cmyk: icc
                .filter(|_| h.mode == Mode::Cmyk)
                .and_then(|p| chitrakar_color::cms::CmykCms::new(p).ok()),
        }
    }

    /// Channels 0.. and -1 (transparency) to 8-bit sRGB, straight alpha.
    fn layer_rgba<'p>(&self, get: &dyn Fn(i16) -> Option<&'p [f32]>, n: usize) -> Vec<u8> {
        let at = |id: i16, i: usize, or: f32| get(id).and_then(|p| p.get(i)).copied().unwrap_or(or);
        let b8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut out = vec![0u8; n * 4];
        for i in 0..n {
            let a = at(-1, i, 1.0);
            let px = match self.mode {
                Mode::Rgb => [b8(at(0, i, 0.0)), b8(at(1, i, 0.0)), b8(at(2, i, 0.0))],
                Mode::Grey | Mode::Duotone | Mode::Bitmap => {
                    let g = b8(at(0, i, 0.0));
                    [g, g, g]
                }
                Mode::Multichannel => {
                    let g = b8(at(0, i, 0.0));
                    match (get(1), get(2)) {
                        (Some(_), Some(_)) => [g, b8(at(1, i, 0.0)), b8(at(2, i, 0.0))],
                        _ => [g, g, g],
                    }
                }
                Mode::Indexed => {
                    // The value is the index, read back from 0..=1.
                    let k = (at(0, i, 0.0) * 255.0).round() as usize;
                    let p = |c: usize| self.palette.get(c * 256 + k).copied().unwrap_or(0);
                    [p(0), p(1), p(2)]
                }
                Mode::Cmyk => {
                    // Photoshop keeps CMYK inverted: 1 is no ink.
                    let ink = |c: i16| 1.0 - at(c, i, 1.0);
                    let (c, m, y, k) = (ink(0), ink(1), ink(2), ink(3));
                    match &self.cmyk {
                        Some(t) => {
                            let w = t.to_working(c, m, y, k, 1.0);
                            [
                                chitrakar_color::linear_to_srgb8(w.r),
                                chitrakar_color::linear_to_srgb8(w.g),
                                chitrakar_color::linear_to_srgb8(w.b),
                            ]
                        }
                        None => [
                            b8((1.0 - c) * (1.0 - k)),
                            b8((1.0 - m) * (1.0 - k)),
                            b8((1.0 - y) * (1.0 - k)),
                        ],
                    }
                }
                Mode::Lab => lab_to_srgb8(at(0, i, 0.0), at(1, i, 0.5), at(2, i, 0.5)),
            };
            out[i * 4..i * 4 + 4].copy_from_slice(&[px[0], px[1], px[2], b8(a)]);
        }
        if let Some(icc) = &self.icc_rgb {
            chitrakar_color::cms::normalize_rgba8_to_srgb(icc, &mut out);
        }
        out
    }
}

/// Lab as Photoshop stores it (L 0..=1 for 0..=100, a and b 0..=1 for
/// -128..=127) to sRGB, under D50 adapted to D65.
fn lab_to_srgb8(l: f32, a: f32, b: f32) -> [u8; 3] {
    let (l, a, b) = (l * 100.0, a * 255.0 - 128.0, b * 255.0 - 128.0);
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let f = |t: f32| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    // D50 white.
    let (x, y, z) = (0.9642 * f(fx), f(fy), 0.8249 * f(fz));
    // Bradford D50 → D65, then XYZ → linear sRGB, as one matrix.
    let r = 3.134_136 * x - 1.617_386 * y - 0.490_662 * z;
    let g = -0.978_795 * x + 1.916_254 * y + 0.033_443 * z;
    let bl = 0.071_955 * x - 0.228_977 * y + 1.405_386 * z;
    [r, g, bl].map(|v| chitrakar_color::linear_to_srgb8(v.clamp(0.0, 1.0)))
}

impl PsdFile {
    /// The commands that put the file's layers into a document under
    /// `parent` at `index`, numbered on from `first` — and Photoshop's
    /// own picture of the whole, hidden on top, when anything could not
    /// come across.
    pub fn into_commands(
        self,
        doc: &mut chitrakar_doc::Document,
        parent: chitrakar_doc::NodeId,
        index: usize,
        first: chitrakar_doc::NodeId,
    ) -> Vec<chitrakar_doc::Command> {
        use chitrakar_doc::{Command, Mask, MaskKind, Node, NodeId, RasterRef, Transform};
        let mut next = first.0;
        let mut cmds = Vec::new();
        fn add(
            item: PsdItem,
            parent: NodeId,
            index: usize,
            doc: &mut chitrakar_doc::Document,
            next: &mut u64,
            cmds: &mut Vec<Command>,
        ) {
            match item {
                PsdItem::Group {
                    name,
                    visible,
                    opacity,
                    blend,
                    children,
                } => {
                    let mut node = Node::group(&name);
                    node.visible = visible;
                    node.opacity = opacity;
                    node.blend = blend;
                    let id = NodeId(*next);
                    *next += 1;
                    cmds.push(Command::AddNode {
                        parent,
                        index,
                        node: Box::new(node),
                    });
                    for (i, child) in children.into_iter().enumerate() {
                        add(child, id, i, doc, next, cmds);
                    }
                }
                PsdItem::Pixels(p) => {
                    let resource_id = doc.add_resource(p.width, p.height, p.rgba);
                    let mut node = Node::raster(
                        &p.name,
                        RasterRef {
                            resource_id,
                            width: p.width,
                            height: p.height,
                            corners: None,
                            develop: None,
                        },
                    );
                    node.transform = Transform::translation(p.left as f32, p.top as f32);
                    node.opacity = p.opacity;
                    node.visible = p.visible;
                    node.blend = p.blend;
                    node.clipped = p.clipped;
                    node.mask = p.mask.map(|m| Mask {
                        kind: MaskKind::Raster {
                            resource_id: doc.add_resource(m.width, m.height, m.rgba),
                            width: m.width,
                            height: m.height,
                            transform: Transform::translation(m.left as f32, m.top as f32),
                        },
                        invert: false,
                        feather: 0.0,
                    });
                    *next += 1;
                    cmds.push(Command::AddNode {
                        parent,
                        index,
                        node: Box::new(node),
                    });
                }
            }
        }
        let count = self.items.len();
        for (i, item) in self.items.into_iter().enumerate() {
            add(item, parent, index + i, doc, &mut next, &mut cmds);
        }
        if !self.lost.is_empty() {
            if let Some(px) = self.composite {
                let resource_id = doc.add_resource(self.width, self.height, px);
                let mut node = Node::raster(
                    "As Photoshop drew it",
                    RasterRef {
                        resource_id,
                        width: self.width,
                        height: self.height,
                        corners: None,
                        develop: None,
                    },
                );
                node.visible = false;
                cmds.push(Command::AddNode {
                    parent,
                    index: index + count,
                    node: Box::new(node),
                });
            }
        }
        cmds
    }
}

/// Photoshop files written by hand, for tests here and in the engine:
/// 8-bit RGB, uncompressed or run-length. Not a writer anybody exports
/// with — the editor saves its own format.
#[doc(hidden)]
pub mod sample {

    /// A Photoshop file written by hand: 8-bit RGB, uncompressed, the
    /// layers given bottom first as (name, rect, rgba, blend, extra
    /// blocks), and a composite. Enough of the format to be read back.
    pub struct Layer {
        pub name: &'static str,
        pub rect: [i32; 4], // top, left, bottom, right
        pub rgba: Vec<u8>,
        pub blend: [u8; 4],
        pub opacity: u8,
        pub hidden: bool,
        pub clipped: bool,
        pub section: Option<u32>,
        pub mask: Option<([i32; 4], u8, Vec<u8>)>,
        pub rle: bool,
    }

    pub fn layer(name: &'static str, rect: [i32; 4], rgba: Vec<u8>) -> Layer {
        Layer {
            name,
            rect,
            rgba,
            blend: *b"norm",
            opacity: 255,
            hidden: false,
            clipped: false,
            section: None,
            mask: None,
            rle: false,
        }
    }

    fn packbits(row: &[u8]) -> Vec<u8> {
        // Literal runs of up to 128 — valid PackBits, if not the shortest.
        let mut out = Vec::new();
        for chunk in row.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
        out
    }

    pub fn write(width: u32, height: u32, layers: &[Layer], composite: &[u8]) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"8BPS");
        f.extend_from_slice(&1u16.to_be_bytes());
        f.extend_from_slice(&[0; 6]);
        f.extend_from_slice(&4u16.to_be_bytes());
        f.extend_from_slice(&height.to_be_bytes());
        f.extend_from_slice(&width.to_be_bytes());
        f.extend_from_slice(&8u16.to_be_bytes());
        f.extend_from_slice(&3u16.to_be_bytes());
        f.extend_from_slice(&0u32.to_be_bytes()); // colour mode data
        f.extend_from_slice(&0u32.to_be_bytes()); // resources
        let mut info = Vec::new();
        info.extend_from_slice(&(layers.len() as i16).to_be_bytes());
        let mut data = Vec::new();
        for l in layers {
            let [t, le, b, r] = l.rect;
            let (w, h) = ((r - le) as usize, (b - t) as usize);
            for v in [t, le, b, r] {
                info.extend_from_slice(&v.to_be_bytes());
            }
            let mut chans: Vec<(i16, Vec<u8>)> = (0..4)
                .map(|c| {
                    let id = if c == 3 { -1 } else { c as i16 };
                    let plane: Vec<u8> = (0..w * h).map(|i| l.rgba[i * 4 + c]).collect();
                    (id, plane)
                })
                .collect();
            if let Some((mr, _, mp)) = &l.mask {
                let _ = mr;
                chans.push((-2, mp.clone()));
            }
            info.extend_from_slice(&(chans.len() as u16).to_be_bytes());
            let mut chan_data = Vec::new();
            for (id, plane) in &chans {
                let (cw, ch) = if *id == -2 {
                    let [mt, ml, mb, mrr] = l.mask.as_ref().unwrap().0;
                    ((mrr - ml) as usize, (mb - mt) as usize)
                } else {
                    (w, h)
                };
                let mut d = Vec::new();
                if l.rle && cw > 0 {
                    d.extend_from_slice(&1u16.to_be_bytes());
                    let rows: Vec<Vec<u8>> = plane.chunks(cw).map(packbits).collect();
                    for row in &rows {
                        d.extend_from_slice(&(row.len() as u16).to_be_bytes());
                    }
                    for row in rows {
                        d.extend(row);
                    }
                } else {
                    d.extend_from_slice(&0u16.to_be_bytes());
                    d.extend_from_slice(&plane[..cw * ch]);
                }
                info.extend_from_slice(&id.to_be_bytes());
                info.extend_from_slice(&(d.len() as u32).to_be_bytes());
                chan_data.extend(d);
            }
            info.extend_from_slice(b"8BIM");
            info.extend_from_slice(&l.blend);
            info.push(l.opacity);
            info.push(l.clipped as u8);
            info.push(if l.hidden { 2 } else { 0 });
            info.push(0);
            let mut extra = Vec::new();
            match &l.mask {
                Some((mr, default, _)) => {
                    extra.extend_from_slice(&20u32.to_be_bytes());
                    for v in mr {
                        extra.extend_from_slice(&v.to_be_bytes());
                    }
                    extra.push(*default);
                    extra.push(0);
                    extra.extend_from_slice(&[0, 0]);
                }
                None => extra.extend_from_slice(&0u32.to_be_bytes()),
            }
            extra.extend_from_slice(&0u32.to_be_bytes()); // blending ranges
            let name = l.name.as_bytes();
            extra.push(name.len() as u8);
            extra.extend_from_slice(name);
            extra.extend(std::iter::repeat_n(0u8, (4 - (name.len() + 1) % 4) % 4));
            if let Some(kind) = l.section {
                extra.extend_from_slice(b"8BIMlsct");
                extra.extend_from_slice(&4u32.to_be_bytes());
                extra.extend_from_slice(&kind.to_be_bytes());
            }
            info.extend_from_slice(&(extra.len() as u32).to_be_bytes());
            info.extend(extra);
            data.extend(chan_data);
        }
        info.extend(data);
        if info.len() % 2 == 1 {
            info.push(0);
        }
        let mut lm = Vec::new();
        lm.extend_from_slice(&(info.len() as u32).to_be_bytes());
        lm.extend(info);
        lm.extend_from_slice(&0u32.to_be_bytes()); // global mask
        f.extend_from_slice(&(lm.len() as u32).to_be_bytes());
        f.extend(lm);
        f.extend_from_slice(&0u16.to_be_bytes());
        for c in 0..4 {
            f.extend((0..(width * height) as usize).map(|i| composite[i * 4 + c]));
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::sample::{layer, write};
    use super::*;

    fn solid(w: usize, h: usize, px: [u8; 4]) -> Vec<u8> {
        px.repeat(w * h)
    }

    #[test]
    fn layers_come_in_with_their_places_names_and_looks() {
        let mut top = layer("Top", [2, 3, 6, 7], solid(4, 4, [255, 0, 0, 255]));
        top.blend = *b"mul ";
        top.opacity = 128;
        top.clipped = true;
        let mut hid = layer("Hidden", [0, 0, 1, 1], solid(1, 1, [0, 0, 255, 255]));
        hid.hidden = true;
        let bytes = write(
            10,
            8,
            &[
                layer("Back", [0, 0, 8, 10], solid(10, 8, [10, 20, 30, 255])),
                top,
                hid,
            ],
            &solid(10, 8, [1, 2, 3, 255]),
        );
        assert!(is_psd(&bytes));
        let f = read_psd(&bytes).unwrap();
        assert_eq!((f.width, f.height), (10, 8));
        assert_eq!(f.items.len(), 3);
        let PsdItem::Pixels(back) = &f.items[0] else {
            panic!()
        };
        assert_eq!(
            (back.name.as_str(), back.width, back.height),
            ("Back", 10, 8)
        );
        assert_eq!(&back.rgba[..4], &[10, 20, 30, 255]);
        let PsdItem::Pixels(t) = &f.items[1] else {
            panic!()
        };
        assert_eq!((t.left, t.top, t.width, t.height), (3, 2, 4, 4));
        assert_eq!(t.blend, BlendMode::Multiply);
        assert!((t.opacity - 128.0 / 255.0).abs() < 1e-6 && t.clipped && t.visible);
        let PsdItem::Pixels(hd) = &f.items[2] else {
            panic!()
        };
        assert!(!hd.visible);
        assert!(f.lost.is_empty(), "{:?}", f.lost);
        assert_eq!(&f.composite.unwrap()[..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn groups_come_in_as_groups() {
        // Bottom first: the group's closing divider, what is in it, the
        // record naming it, then a layer above the group.
        let mut close = layer("</Layer group>", [0, 0, 0, 0], vec![]);
        close.section = Some(3);
        let mut open = layer("Badge", [0, 0, 0, 0], vec![]);
        open.section = Some(1);
        open.opacity = 200;
        let bytes = write(
            4,
            4,
            &[
                close,
                layer("Inside 1", [0, 0, 2, 2], solid(2, 2, [255, 255, 255, 255])),
                layer("Inside 2", [2, 2, 4, 4], solid(2, 2, [0, 0, 0, 255])),
                open,
                layer("Above", [0, 0, 1, 1], solid(1, 1, [9, 9, 9, 255])),
            ],
            &solid(4, 4, [0, 0, 0, 255]),
        );
        let f = read_psd(&bytes).unwrap();
        assert_eq!(f.items.len(), 2, "the group, then the layer above it");
        let PsdItem::Group {
            name,
            children,
            opacity,
            ..
        } = &f.items[0]
        else {
            panic!("{:?}", f.items[0])
        };
        assert_eq!(name, "Badge");
        assert!((opacity - 200.0 / 255.0).abs() < 1e-6);
        let names: Vec<&str> = children
            .iter()
            .map(|c| match c {
                PsdItem::Pixels(p) => p.name.as_str(),
                PsdItem::Group { name, .. } => name.as_str(),
            })
            .collect();
        assert_eq!(names, ["Inside 1", "Inside 2"]);
    }

    #[test]
    fn a_layer_mask_comes_in_as_a_mask_with_its_default_round_it() {
        // A 4×4 layer masked by a 2×2 box at its corner that shows half;
        // outside the box the mask's default (white) shows all.
        let mut l = layer("Masked", [0, 0, 4, 4], solid(4, 4, [0, 255, 0, 255]));
        l.mask = Some(([0, 0, 2, 2], 255, vec![128; 4]));
        let bytes = write(4, 4, &[l], &solid(4, 4, [0, 0, 0, 255]));
        let f = read_psd(&bytes).unwrap();
        let PsdItem::Pixels(p) = &f.items[0] else {
            panic!()
        };
        let m = p.mask.as_ref().expect("a mask");
        assert_eq!((m.left, m.top, m.width, m.height), (0, 0, 4, 4));
        let cov = |x: usize, y: usize| {
            chitrakar_color::srgb_to_linear(m.rgba[(y * 4 + x) * 4] as f32 / 255.0)
        };
        assert!(
            (cov(0, 0) - 128.0 / 255.0).abs() < 0.01,
            "inside the box, half: {}",
            cov(0, 0)
        );
        assert!(
            (cov(3, 3) - 1.0).abs() < 0.01,
            "outside it, the default: {}",
            cov(3, 3)
        );
    }

    #[test]
    fn run_length_channels_read_as_raw_ones_do() {
        let mut l = layer(
            "RLE",
            [0, 0, 3, 200],
            (0..600)
                .flat_map(|i| [(i % 256) as u8, 7, 9, 255])
                .collect(),
        );
        l.rle = true;
        let bytes = write(200, 3, &[l], &solid(200, 3, [0, 0, 0, 255]));
        let f = read_psd(&bytes).unwrap();
        let PsdItem::Pixels(p) = &f.items[0] else {
            panic!()
        };
        assert_eq!(&p.rgba[..8], &[0, 7, 9, 255, 1, 7, 9, 255]);
        assert_eq!(&p.rgba[599 * 4..], &[(599 % 256) as u8, 7, 9, 255]);
    }

    #[test]
    fn what_cannot_come_across_is_said_and_photoshops_picture_kept() {
        let mut odd = layer("Dissolved", [0, 0, 2, 2], solid(2, 2, [1, 1, 1, 255]));
        odd.blend = *b"diss";
        let bytes = write(2, 2, &[odd], &solid(2, 2, [5, 6, 7, 255]));
        let f = read_psd(&bytes).unwrap();
        assert_eq!(f.lost.len(), 1, "{:?}", f.lost);
        assert!(f.lost[0].contains("Dissolved"));
        let mut doc = chitrakar_doc::Document::new(2, 2, chitrakar_color::ColorMode::Rgb);
        let root = doc.root();
        let first = doc.peek_next_id();
        let cmds = f.into_commands(&mut doc, root, 0, first);
        assert_eq!(
            cmds.len(),
            2,
            "the layer, and Photoshop's picture beside it"
        );
        let chitrakar_doc::Command::AddNode { node, .. } = &cmds[1] else {
            panic!()
        };
        assert!(!node.visible && node.name == "As Photoshop drew it");
    }

    #[test]
    fn a_flat_file_comes_in_as_its_one_picture() {
        let bytes = write(3, 2, &[], &solid(3, 2, [40, 50, 60, 255]));
        let f = read_psd(&bytes).unwrap();
        assert_eq!(f.items.len(), 1);
        let PsdItem::Pixels(p) = &f.items[0] else {
            panic!()
        };
        assert_eq!(
            (p.width, p.height, &p.rgba[..4]),
            (3, 2, &[40u8, 50, 60, 255][..])
        );
    }

    #[test]
    fn cmyk_is_read_inverted_as_photoshop_keeps_it() {
        let h = Header {
            big: false,
            channels: 4,
            width: 1,
            height: 1,
            depth: 8,
            mode: Mode::Cmyk,
        };
        let colour = Colour::new(&h, &[], None);
        // Stored: cyan full (0), the rest none (1).
        let planes = [vec![0.0f32], vec![1.0], vec![1.0], vec![1.0]];
        let get = |id: i16| (id >= 0).then(|| planes[id as usize].as_slice());
        assert_eq!(colour.layer_rgba(&get, 1), vec![0, 255, 255, 255]);
    }

    #[test]
    fn lab_white_and_black_are_white_and_black() {
        assert_eq!(
            lab_to_srgb8(1.0, 128.0 / 255.0, 128.0 / 255.0),
            [255, 255, 255]
        );
        assert_eq!(lab_to_srgb8(0.0, 128.0 / 255.0, 128.0 / 255.0), [0, 0, 0]);
    }

    #[test]
    fn sixteen_bit_prediction_is_undone() {
        // Differences 1000, +24, +1: 1000, 1024, 1025.
        let mut row: Vec<u8> = [1000u16, 24, 1]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        unpredict(&mut row, 6, 1, 16);
        let back: Vec<u16> = row
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(back, [1000, 1024, 1025]);
    }

    #[test]
    fn a_truncated_file_is_refused_not_panicked_over() {
        let bytes = write(
            4,
            4,
            &[layer("L", [0, 0, 4, 4], solid(4, 4, [1, 2, 3, 255]))],
            &solid(4, 4, [0, 0, 0, 255]),
        );
        for cut in [10, 30, 60, 100, bytes.len() / 2, bytes.len() - 3] {
            let _ = read_psd(&bytes[..cut]);
        }
        assert!(read_psd(b"8BPS\0\x01junk").is_err());
    }
}
