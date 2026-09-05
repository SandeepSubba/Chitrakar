//! A TrueType subsetter for embedding: keep the glyphs a document uses
//! (and the components any of them are built from), empty the rest, and
//! drop the tables a PDF reader does not need. Glyph ids stay what they
//! were — the PDF addresses glyphs by id through an identity map — so
//! the glyph count is kept and unused slots simply have no outline.
//!
//! Hand-parsed: the sfnt directory and the six tables a `glyf` font
//! needs (`head`, `hhea`, `maxp`, `hmtx`, `loca`, `glyf`), plus the
//! hinting tables carried along unchanged when present.

use std::collections::BTreeSet;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

/// The tables of an sfnt file, by tag.
fn tables(font: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let count = u16_at(font, 4)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 12 + i * 16;
        let tag: [u8; 4] = font.get(at..at + 4)?.try_into().ok()?;
        let offset = u32_at(font, at + 8)? as usize;
        let length = u32_at(font, at + 12)? as usize;
        out.push((tag, font.get(offset..offset + length)?));
    }
    Some(out)
}

/// The sum of a table's big-endian words, as sfnt checksums are.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |acc, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        acc.wrapping_add(u32::from_be_bytes(w))
    })
}

/// A TrueType file holding only `glyphs` (and what they are composed
/// of), or `None` when the file is not a `glyf` font this can read — an
/// OpenType CFF face, say — in which case the whole file should travel.
pub fn subset_ttf(font: &[u8], glyphs: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let tables = tables(font)?;
    let table = |tag: &[u8; 4]| tables.iter().find(|(t, _)| t == tag).map(|(_, d)| *d);
    let head = table(b"head")?;
    let maxp = table(b"maxp")?;
    let loca = table(b"loca")?;
    let glyf = table(b"glyf")?;
    let count = u16_at(maxp, 4)? as usize;
    let long_loca = u16_at(head, 50)? != 0;
    let offset = |g: usize| -> Option<usize> {
        if long_loca {
            u32_at(loca, g * 4).map(|v| v as usize)
        } else {
            u16_at(loca, g * 2).map(|v| v as usize * 2)
        }
    };
    let glyph_data = |g: usize| -> Option<&[u8]> {
        let (start, end) = (offset(g)?, offset(g + 1)?);
        glyf.get(start..end.max(start))
    };

    // Close over composites: a glyph drawn from others needs them too.
    let mut keep: BTreeSet<u16> = glyphs
        .iter()
        .copied()
        .filter(|&g| (g as usize) < count)
        .collect();
    let mut pending: Vec<u16> = keep.iter().copied().collect();
    while let Some(g) = pending.pop() {
        let data = glyph_data(g as usize)?;
        if data.len() < 10 || (u16_at(data, 0)? as i16) >= 0 {
            continue;
        }
        let mut at = 10;
        loop {
            let flags = u16_at(data, at)?;
            let component = u16_at(data, at + 2)?;
            if keep.insert(component) {
                pending.push(component);
            }
            at += 4 + if flags & 1 != 0 { 4 } else { 2 };
            if flags & 8 != 0 {
                at += 2;
            } else if flags & 0x40 != 0 {
                at += 4;
            } else if flags & 0x80 != 0 {
                at += 8;
            }
            if flags & 0x20 == 0 {
                break;
            }
        }
    }

    // New glyf and a long loca over it: kept glyphs as they were, four-
    // byte aligned; the rest empty.
    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((count + 1) * 4);
    for g in 0..count {
        new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
        if keep.contains(&(g as u16)) {
            new_glyf.extend_from_slice(glyph_data(g)?);
            while new_glyf.len() % 4 != 0 {
                new_glyf.push(0);
            }
        }
    }
    new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
    let mut new_head = head.to_vec();
    new_head[8..12].copy_from_slice(&[0; 4]); // checksumAdjustment, set below
    new_head[50..52].copy_from_slice(&1u16.to_be_bytes()); // long loca

    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"head", new_head),
        (*b"hhea", table(b"hhea")?.to_vec()),
        (*b"maxp", maxp.to_vec()),
        (*b"hmtx", table(b"hmtx")?.to_vec()),
        (*b"loca", new_loca),
        (*b"glyf", new_glyf),
    ];
    for tag in [b"cvt ", b"fpgm", b"prep"] {
        if let Some(data) = table(tag) {
            out_tables.push((*tag, data.to_vec()));
        }
    }
    out_tables.sort_by_key(|(tag, _)| *tag);

    // The directory, then the tables, each padded to four bytes.
    let n = out_tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    let range = (1usize << (usize::BITS - 1 - n.leading_zeros())) * 16;
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&(range as u16).to_be_bytes());
    out.extend_from_slice(&((range / 16).trailing_zeros() as u16).to_be_bytes());
    out.extend_from_slice(&((n * 16 - range) as u16).to_be_bytes());
    let mut at = 12 + n * 16;
    let mut head_at = 0;
    for (tag, data) in &out_tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(at as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = at;
        }
        at += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &out_tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::Font;

    const SANS: &[u8] = include_bytes!("../../render/assets/DejaVuSans.ttf");

    #[test]
    fn a_subset_keeps_the_used_glyphs_as_they_were_and_nothing_else() {
        let full = ab_glyph::FontRef::try_from_slice(SANS).unwrap();
        // 'é' is composed of 'e' and an accent in DejaVu: the components
        // come along without being asked for.
        let used: BTreeSet<u16> = "Hé".chars().map(|c| full.glyph_id(c).0).collect();
        let subset = subset_ttf(SANS, &used).expect("a glyf font subsets");
        assert!(
            subset.len() * 10 < SANS.len(),
            "{} bytes from {}",
            subset.len(),
            SANS.len()
        );
        let small = ab_glyph::FontRef::try_from_slice(&subset).expect("the subset parses");
        assert_eq!(small.glyph_count(), full.glyph_count(), "ids stay put");
        assert_eq!(small.units_per_em(), full.units_per_em());
        for c in ['H', 'é', 'e'] {
            let id = full.glyph_id(c);
            assert_eq!(
                small.glyph_id(c).0,
                0,
                "no cmap travels; glyphs are addressed by id"
            );
            let (a, b) = (full.outline(id).unwrap(), small.outline(id).unwrap());
            assert_eq!(a.curves.len(), b.curves.len(), "{c:?} keeps its outline");
            assert_eq!(
                small.h_advance_unscaled(id),
                full.h_advance_unscaled(id),
                "and its advance"
            );
        }
        let unused = full.glyph_id('Z');
        assert!(full.outline(unused).is_some());
        assert!(small.outline(unused).is_none(), "an unused glyph is empty");
        assert!(subset_ttf(b"not a font", &used).is_none());
    }
}
