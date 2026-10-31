//! A colour lookup table as the document keeps it.
//!
//! A table is a resource like a picture's pixels — immutable, content
//! addressed, written into a `.chitra` as a PNG beside the manifest — so
//! two layers on one grade share one set of bytes, and nothing about the
//! container had to learn a new kind of thing. A resource is eight bits a
//! channel, which is too coarse for a table read in a float pipeline, so
//! each entry is kept as sixteen: the high bytes in the top half of the
//! image and the low bytes in the bottom half, read back together.
//!
//! The grid is `size`³ entries, red changing fastest, then green, then
//! blue — the order a `.cube` file is written in. Entry (r, g, b) is at
//! column `r + size·g`, row `b` (and row `size + b` for its low bytes).

/// The resource for a table: its width, height and bytes.
pub fn encode(size: u32, table: &[[f32; 3]]) -> (u32, u32, Vec<u8>) {
    let n = size as usize;
    debug_assert_eq!(table.len(), n * n * n);
    let (w, h) = (n * n, 2 * n);
    let mut bytes = vec![255u8; w * h * 4];
    for (i, entry) in table.iter().enumerate() {
        let (x, y) = (i % (n * n), i / (n * n));
        for (c, v) in entry.iter().enumerate() {
            let q = (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
            bytes[(y * w + x) * 4 + c] = (q >> 8) as u8;
            bytes[((n + y) * w + x) * 4 + c] = (q & 0xff) as u8;
        }
    }
    (w as u32, h as u32, bytes)
}

/// The table a resource holds, or nothing where it is not one of
/// `size`³ — the wrong shape, or bytes not loaded.
pub fn decode(size: u32, width: u32, height: u32, rgba8: &[u8]) -> Option<Vec<[f32; 3]>> {
    let n = size as usize;
    if n < 2 || width as usize != n * n || height as usize != 2 * n {
        return None;
    }
    let w = n * n;
    if rgba8.len() != w * 2 * n * 4 {
        return None;
    }
    Some(
        (0..n * n * n)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                [0, 1, 2].map(|c| {
                    let hi = rgba8[(y * w + x) * 4 + c] as u16;
                    let lo = rgba8[((n + y) * w + x) * 4 + c] as u16;
                    ((hi << 8) | lo) as f32 / 65535.0
                })
            })
            .collect(),
    )
}

/// Read a table at a point over nought to one, between its grid points
/// — each channel of the point held to the grid, and the eight entries
/// round it mixed by how near it is to each.
pub fn trilinear(size: u32, table: &[[f32; 3]], p: [f32; 3]) -> [f32; 3] {
    let n = size as usize;
    let last = (n - 1) as f32;
    let mut i = [0usize; 3];
    let mut f = [0.0f32; 3];
    for c in 0..3 {
        let x = p[c].clamp(0.0, 1.0) * last;
        i[c] = (x.floor() as usize).min(n - 2);
        f[c] = x - i[c] as f32;
    }
    let at = |r: usize, g: usize, b: usize| table[r + n * (g + n * b)];
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let mut out = [0.0f32; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let c00 = l(at(i[0], i[1], i[2])[c], at(i[0] + 1, i[1], i[2])[c], f[0]);
        let c10 = l(
            at(i[0], i[1] + 1, i[2])[c],
            at(i[0] + 1, i[1] + 1, i[2])[c],
            f[0],
        );
        let c01 = l(
            at(i[0], i[1], i[2] + 1)[c],
            at(i[0] + 1, i[1], i[2] + 1)[c],
            f[0],
        );
        let c11 = l(
            at(i[0], i[1] + 1, i[2] + 1)[c],
            at(i[0] + 1, i[1] + 1, i[2] + 1)[c],
            f[0],
        );
        *o = l(l(c00, c10, f[1]), l(c01, c11, f[1]), f[2]);
    }
    out
}

/// The table that changes nothing: every grid point goes to itself.
pub fn identity(size: u32) -> Vec<[f32; 3]> {
    let n = size as usize;
    let step = 1.0 / (n - 1) as f32;
    (0..n * n * n)
        .map(|i| {
            [
                (i % n) as f32 * step,
                ((i / n) % n) as f32 * step,
                (i / (n * n)) as f32 * step,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sixteen bits a channel through eight-bit bytes, and back.
    #[test]
    fn a_table_comes_back_from_its_resource_to_sixteen_bits() {
        let size = 5;
        let table: Vec<[f32; 3]> = identity(size)
            .into_iter()
            .map(|[r, g, b]| [r * 0.9 + 0.013, g.powf(1.7), 1.0 - b])
            .collect();
        let (w, h, bytes) = encode(size, &table);
        let back = decode(size, w, h, &bytes).unwrap();
        for (a, b) in table.iter().zip(&back) {
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() <= 0.5 / 65535.0 + 1e-7, "{a:?} {b:?}");
            }
        }
        assert!(
            decode(4, w, h, &bytes).is_none(),
            "the wrong size is refused"
        );
        assert!(decode(size, w, h, &bytes[4..]).is_none(), "and short bytes");
    }

    /// The identity reads back as the point it was asked about, between
    /// grid points as well as on them.
    #[test]
    fn the_identity_table_changes_nothing() {
        let t = identity(9);
        for p in [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.3, 0.71, 0.05],
            [0.999, 0.5, 0.125],
        ] {
            let q = trilinear(9, &t, p);
            for c in 0..3 {
                assert!((p[c] - q[c]).abs() < 1e-5, "{p:?} -> {q:?}");
            }
        }
    }
}
