//! A picture kept again at a half, a quarter, an eighth … of its size,
//! for drawing it shrunk.
//!
//! A shrunk picture is drawn by averaging the texels each device pixel
//! covers (`picture_footprint`). At its own size that is one tap; shrunk
//! to a quarter it is a grid of taps over sixteen texels, and every frame
//! reads the whole picture again — for a 12-megapixel photograph fitted
//! to a phone's screen, all twelve million texels a frame, and a drag
//! that repaints at a few frames a second. The cost is the source, not
//! the screen: halving the screen's resolution only doubled each pixel's
//! footprint. So the averaging is done once, here, into levels each half
//! the last, and a pixel is drawn from the level whose texels are about
//! its own size — where the same averaging is one tap again.
//!
//! A level's texel is the plain average of the two by two below it, in
//! linear light and premultiplied, which is the arithmetic the averaging
//! taps do; at an exact half the level *is* the answer
//! (`a_picture_at_half_its_size_is_each_two_texels_averaged`). Kept at
//! sixteen bits a channel: a 12-megapixel photograph's levels are 32 MB
//! together, a third of the picture's own size at twice its depth.
//!
//! Pictures are content-addressed and never change, so a level made once
//! is right for as long as anything asks for that picture: they are kept
//! by the picture's id, and the least recently used go first once the
//! whole lot passes a bound.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chitrakar_doc::Resource;

/// One level: `width` by `height` texels, linear light, premultiplied,
/// a channel in sixteen bits.
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<[u16; 4]>,
}

impl Level {
    /// One texel, as the renderer reads it.
    #[inline]
    pub fn texel(&self, x: u32, y: u32) -> [f32; 4] {
        let p = self.texels[(y * self.width + x) as usize];
        const K: f32 = 1.0 / 65535.0;
        [
            p[0] as f32 * K,
            p[1] as f32 * K,
            p[2] as f32 * K,
            p[3] as f32 * K,
        ]
    }

    fn bytes(&self) -> usize {
        self.texels.len() * 8
    }
}

/// Every level below the picture's own size: `levels[0]` is the half,
/// `levels[1]` the quarter, down to a single texel.
pub struct Pyramid {
    pub levels: Vec<Level>,
    /// Of the picture these were made from (`fingerprint`).
    of: u64,
}

impl Pyramid {
    fn bytes(&self) -> usize {
        self.levels.iter().map(Level::bytes).sum()
    }

    /// The level `k` steps down — 1 is the half — or the deepest there is.
    pub fn level(&self, k: u32) -> Option<&Level> {
        if k == 0 || self.levels.is_empty() {
            return None;
        }
        let i = (k as usize - 1).min(self.levels.len() - 1);
        self.levels.get(i)
    }

    /// How many steps down the deepest level is.
    pub fn depth(&self) -> u32 {
        self.levels.len() as u32
    }
}

/// How many steps down a picture shrunk to cover `foot` texels a device
/// pixel (the smaller of its two axes) is drawn from: none below a
/// footprint of two, where the averaging is already one or two taps a
/// side and the picture is drawn from itself as it always was; the level
/// whose texels the footprint is between one and two of above that.
pub fn level_for(foot: f32) -> u32 {
    if !foot.is_finite() || foot < 2.0 {
        return 0;
    }
    foot.log2().floor() as u32
}

/// The whole lot kept may not pass this. A phone's tab is the reason it
/// is a bound at all — memory a page has taken is never given back — and
/// a picture whose levels alone pass it is not kept. It holds three
/// 12-megapixel photographs' levels.
const KEPT_MOST: usize = 96 << 20;

#[derive(Default)]
struct Kept {
    by_id: HashMap<String, Arc<Pyramid>>,
    /// Least recently used first.
    order: Vec<String>,
    bytes: usize,
}

/// One for the whole process rather than one a thread: the desktop
/// shell draws on the graphics card from whichever worker a request
/// lands on, and a copy a worker would be a photograph's levels kept
/// once for each.
static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

/// The levels of the picture `id`, made the first time they are asked
/// for and kept. `None` for a picture too small to have a half.
pub fn of(id: &str, res: &Resource) -> Option<Arc<Pyramid>> {
    if res.width < 2 && res.height < 2 || res.rgba8.len() != (res.width * res.height * 4) as usize {
        return None;
    }
    let kept = || KEPT.lock().unwrap_or_else(|e| e.into_inner());
    {
        let mut guard = kept();
        let kept = guard.get_or_insert_with(Kept::default);
        if let Some(p) = kept.by_id.get(id).cloned() {
            if p.of == fingerprint(res) {
                kept.order.retain(|k| k != id);
                kept.order.push(id.to_string());
                return Some(p);
            }
            // Another picture under the same id: these levels are not
            // its, and are let go.
            kept.order.retain(|k| k != id);
            kept.bytes -= p.bytes();
            kept.by_id.remove(id);
        }
    }
    // Made without holding the others up: a frame of another picture
    // need not wait for this one's levels.
    let p = Arc::new(build(res));
    let size = p.bytes();
    if size > KEPT_MOST {
        return Some(p);
    }
    let mut guard = kept();
    let kept = guard.get_or_insert_with(Kept::default);
    if let Some(theirs) = kept.by_id.get(id).filter(|t| t.of == p.of).cloned() {
        return Some(theirs);
    }
    if let Some(stale) = kept.by_id.remove(id) {
        kept.order.retain(|k| k != id);
        kept.bytes -= stale.bytes();
    }
    while kept.bytes + size > KEPT_MOST && !kept.order.is_empty() {
        let gone = kept.order.remove(0);
        if let Some(g) = kept.by_id.remove(&gone) {
            kept.bytes -= g.bytes();
        }
    }
    kept.bytes += size;
    kept.order.push(id.to_string());
    kept.by_id.insert(id.to_string(), p.clone());
    Some(p)
}

/// Every level of a picture, the half from its own texels and each one
/// after from the one before.
pub fn build(res: &Resource) -> Pyramid {
    let mut lut = [0f32; 256];
    for (v, out) in lut.iter_mut().enumerate() {
        *out = chitrakar_color::srgb_to_linear(v as f32 / 255.0);
    }
    let w0 = res.width;
    let own = |x: u32, y: u32| -> [f32; 4] {
        let s = ((y * w0 + x) * 4) as usize;
        let a = res.rgba8[s + 3] as f32 / 255.0;
        [
            lut[res.rgba8[s] as usize] * a,
            lut[res.rgba8[s + 1] as usize] * a,
            lut[res.rgba8[s + 2] as usize] * a,
            a,
        ]
    };
    let mut levels: Vec<Level> = Vec::new();
    let first = first_level(res, &lut, own);
    levels.push(first);
    while let Some(last) = levels.last() {
        if last.width <= 1 && last.height <= 1 {
            break;
        }
        let next = halve(last.width, last.height, |x, y| last.texel(x, y));
        levels.push(next);
    }
    Pyramid {
        levels,
        of: fingerprint(res),
    }
}

/// A picture's size and a few thousand of its bytes, spread across it.
/// An id is a hash of the whole picture when the engine makes it, but one
/// read out of a file is the file's word for it and is not hashed again
/// on the way in — that would be a pass over every byte of every
/// photograph opened — so before levels kept under an id are handed out
/// for a picture, they are checked to have been made from one like it.
/// Cheap enough to ask every frame.
fn fingerprint(res: &Resource) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for b in res
        .width
        .to_le_bytes()
        .into_iter()
        .chain(res.height.to_le_bytes())
    {
        eat(b);
    }
    let step = (res.rgba8.len() / 4096).max(1);
    for &b in res.rgba8.iter().step_by(step) {
        eat(b);
    }
    hash
}

/// The half, from the picture's own bytes: [`halve`] over them, with the
/// whole two by twos — every texel but an odd edge's — read two rows at a
/// time as slices rather than a texel at a time by its coordinates. The
/// same arithmetic in the same order (`the_quick_two_by_two_is_the_loops_
/// two_by_two`); without an index worked out and checked for every byte,
/// which in a browser was most of making a photograph's levels.
fn first_level(res: &Resource, lut: &[f32; 256], own: impl Fn(u32, u32) -> [f32; 4]) -> Level {
    let (w, h) = (res.width, res.height);
    let (hw, hh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
    let q = |v: f32| chitrakar_color::round_unsigned(v.clamp(0.0, 1.0) * 65535.0) as u16;
    // The alpha a byte stands for, entry for entry what the division
    // gives; and an opaque texel — nearly every one of a photograph — is
    // its curve's values, since multiplying by one changes nothing.
    let alpha_of: [f32; 256] = std::array::from_fn(|v| v as f32 / 255.0);
    let lit = |b: &[u8]| {
        if b[3] == 255 {
            return [
                lut[b[0] as usize],
                lut[b[1] as usize],
                lut[b[2] as usize],
                1.0,
            ];
        }
        let a = alpha_of[b[3] as usize];
        [
            lut[b[0] as usize] * a,
            lut[b[1] as usize] * a,
            lut[b[2] as usize] * a,
            a,
        ]
    };
    let row = (w * 4) as usize;
    let whole = (w / 2) as usize;
    let mut texels = Vec::with_capacity((hw * hh) as usize);
    for y in 0..hh {
        if 2 * y + 1 < h {
            let top = &res.rgba8[(2 * y) as usize * row..][..row];
            let bottom = &res.rgba8[(2 * y + 1) as usize * row..][..row];
            for (t, b) in top.chunks_exact(8).zip(bottom.chunks_exact(8)).take(whole) {
                let (a, bb) = (lit(&t[..4]), lit(&t[4..]));
                let (c, d) = (lit(&b[..4]), lit(&b[4..]));
                let avg = |k: usize| (((0.0 + a[k]) + bb[k]) + c[k] + d[k]) * 0.25;
                texels.push([q(avg(0)), q(avg(1)), q(avg(2)), q(avg(3))]);
            }
            for x in whole as u32..hw {
                texels.push(partial(w, h, x, y, &own));
            }
        } else {
            for x in 0..hw {
                texels.push(partial(w, h, x, y, &own));
            }
        }
    }
    Level {
        width: hw,
        height: hh,
        texels,
    }
}

/// A texel of the level below one `w` by `h` at an odd edge, where fewer
/// than four are over it: the plain average of those there are.
fn partial(w: u32, h: u32, x: u32, y: u32, read: &impl Fn(u32, u32) -> [f32; 4]) -> [u16; 4] {
    let mut sum = [0f32; 4];
    let mut n = 0.0f32;
    for sy in (2 * y)..(2 * y + 2).min(h) {
        for sx in (2 * x)..(2 * x + 2).min(w) {
            let p = read(sx, sy);
            for c in 0..4 {
                sum[c] += p[c];
            }
            n += 1.0;
        }
    }
    let q = |v: f32| chitrakar_color::round_unsigned(v.clamp(0.0, 1.0) * 65535.0) as u16;
    [q(sum[0] / n), q(sum[1] / n), q(sum[2] / n), q(sum[3] / n)]
}

/// The level below one `w` by `h`: each texel the plain average of the
/// two by two above it — or of as many of them as there are, at an odd
/// edge.
fn halve(w: u32, h: u32, read: impl Fn(u32, u32) -> [f32; 4]) -> Level {
    let (hw, hh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
    let mut texels = Vec::with_capacity((hw * hh) as usize);
    // `round` to the bit, without the call it is in WebAssembly, where
    // it was most of making a photograph's levels.
    let q = |v: f32| chitrakar_color::round_unsigned(v.clamp(0.0, 1.0) * 65535.0) as u16;
    for y in 0..hh {
        for x in 0..hw {
            // A whole two by two, which is every texel but an odd edge's:
            // the four added in the order the loop below adds them, and a
            // quarter taken, which is a division by four to the bit.
            if 2 * x + 1 < w && 2 * y + 1 < h {
                let (a, b) = (read(2 * x, 2 * y), read(2 * x + 1, 2 * y));
                let (c, d) = (read(2 * x, 2 * y + 1), read(2 * x + 1, 2 * y + 1));
                let avg = |k: usize| (((0.0 + a[k]) + b[k]) + c[k] + d[k]) * 0.25;
                texels.push([q(avg(0)), q(avg(1)), q(avg(2)), q(avg(3))]);
                continue;
            }
            texels.push(partial(w, h, x, y, &read));
        }
    }
    Level {
        width: hw,
        height: hh,
        texels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Resource {
        let mut rgba8 = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba8.extend_from_slice(&f(x, y));
            }
        }
        Resource {
            width: w,
            height: h,
            rgba8,
        }
    }

    /// Each level is half the last, rounded up, down to one texel; a
    /// texel is the average of the two by two over it in premultiplied
    /// linear light, and at an odd edge of only those there are — not
    /// the edge's own colour dragged in from a texel past the picture.
    #[test]
    fn each_level_is_the_last_averaged_two_by_two() {
        let res = picture(5, 3, |x, y| {
            if (x + y) % 2 == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let p = build(&res);
        let sizes: Vec<(u32, u32)> = p.levels.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, vec![(3, 2), (2, 1), (1, 1)]);
        let half = &p.levels[0];
        // Two white of four, premultiplied: a half of everything.
        assert!((half.texel(0, 0)[0] - 0.5).abs() < 1e-4);
        assert!((half.texel(0, 0)[3] - 0.5).abs() < 1e-4);
        // The last column is one texel wide: (4,0) white over (4,1) clear.
        assert!((half.texel(2, 0)[3] - 0.5).abs() < 1e-4);
        // The last row is one texel high: (0,2) white beside (1,2) clear.
        assert!((half.texel(0, 1)[3] - 0.5).abs() < 1e-4);
        // And the corner one texel alone: (4,2), white.
        assert!((half.texel(2, 1)[3] - 1.0).abs() < 1e-4);
    }

    /// The whole two-by-twos taken the quick way are the same texels, to
    /// the bit, as the loop every texel used to go through.
    #[test]
    fn the_quick_two_by_two_is_the_loops_two_by_two() {
        let res = picture(37, 23, |x, y| {
            let v = (x * 53 + y * 29) as u8;
            [v, v.wrapping_mul(3), 255 - v, (x * 11 + y * 7) as u8]
        });
        let quick = build(&res);
        // The first level the slow way.
        let lut: Vec<f32> = (0..256)
            .map(|v| chitrakar_color::srgb_to_linear(v as f32 / 255.0))
            .collect();
        let own = |x: u32, y: u32| {
            let s = ((y * 37 + x) * 4) as usize;
            let a = res.rgba8[s + 3] as f32 / 255.0;
            [
                lut[res.rgba8[s] as usize] * a,
                lut[res.rgba8[s + 1] as usize] * a,
                lut[res.rgba8[s + 2] as usize] * a,
                a,
            ]
        };
        let first = &quick.levels[0];
        for y in 0..first.height {
            for x in 0..first.width {
                let mut sum = [0f32; 4];
                let mut n = 0.0f32;
                for sy in (2 * y)..(2 * y + 2).min(23) {
                    for sx in (2 * x)..(2 * x + 2).min(37) {
                        let p = own(sx, sy);
                        for c in 0..4 {
                            sum[c] += p[c];
                        }
                        n += 1.0;
                    }
                }
                let want = sum.map(|v| ((v / n).clamp(0.0, 1.0) * 65535.0).round() as u16);
                assert_eq!(
                    first.texels[(y * first.width + x) as usize],
                    want,
                    "({x}, {y})"
                );
            }
        }
    }

    /// A level made once is the one handed out again, and a picture too
    /// small to have a half has none.
    #[test]
    fn a_pyramid_is_made_once_and_kept() {
        let res = picture(8, 8, |x, _| [x as u8 * 30, 0, 0, 255]);
        let a = of("kept-once", &res).unwrap();
        let b = of("kept-once", &res).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.depth(), 3);
        assert!(of("a-dot", &picture(1, 1, |_, _| [0, 0, 0, 255])).is_none());
        // A file's word for which picture an id is is not taken on trust:
        // another picture under the same id gets levels of its own.
        let other = picture(8, 8, |x, _| [0, x as u8 * 30, 0, 255]);
        let c = of("kept-once", &other).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(c.levels[0].texel(0, 0)[1] > c.levels[0].texel(0, 0)[0]);
        assert_eq!(level_for(1.9), 0);
        assert_eq!(level_for(2.0), 1);
        assert_eq!(level_for(7.9), 2);
        assert_eq!(level_for(f32::NAN), 0);
    }
}
