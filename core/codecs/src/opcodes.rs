//! What a DNG says should be done to its picture on the way to being a
//! photograph: its opcode lists (DNG 1.3 and on). List 2 works on the
//! sensor's values once black is off and white is one — a phone writes
//! its lens shading there, a gain map for each colour of the filter —
//! and list 3 on the picture once it is red, green and blue: a radial
//! vignette, a gain map, and a warp that puts the lens's distortion and
//! colour fringes right. Each is read here, big-endian as the format
//! has it; what this does not do (bad-pixel lists, tables, deltas) is
//! left out rather than half done, as an optional opcode would be.
//!
//! Every position an opcode speaks of is in the DNG's active area, while
//! a developed photograph is its default crop of it: [`Frame`] says
//! where the one sits in the other.

/// Where the developed photograph sits in the active area the opcodes
/// were written for: its offset in it, and the area's size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}

/// A gain map: gains over a grid laid on the area, read between its
/// points, multiplying the sites of `plane..plane + planes` of the
/// rectangle `top..bottom`, `left..right` that fall on its pitch.
#[derive(Debug, Clone, PartialEq)]
pub struct GainMap {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
    pub plane: usize,
    pub planes: usize,
    pub row_pitch: usize,
    pub col_pitch: usize,
    pub points_v: usize,
    pub points_h: usize,
    /// The grid's spacing and origin, as fractions of the area.
    pub spacing_v: f32,
    pub spacing_h: f32,
    pub origin_v: f32,
    pub origin_h: f32,
    /// How many planes the map has gains for (the last serves the rest).
    pub map_planes: usize,
    /// Row by row, point by point, plane by plane.
    pub gains: Vec<f32>,
}

impl GainMap {
    /// Whether the site at `row`, `col` of the area is one the map is for.
    #[inline]
    pub fn covers(&self, row: usize, col: usize) -> bool {
        row >= self.top
            && row < self.bottom
            && col >= self.left
            && col < self.right
            && (row - self.top).is_multiple_of(self.row_pitch)
            && (col - self.left).is_multiple_of(self.col_pitch)
    }

    /// The gain at `row`, `col` of an area `frame` big, for map plane
    /// `p`: between the grid's four nearest points.
    #[inline]
    pub fn gain(&self, row: usize, col: usize, frame: &Frame, p: usize) -> f32 {
        let v = ((row as f32 / frame.height.max(1) as f32 - self.origin_v) / self.spacing_v)
            .clamp(0.0, (self.points_v - 1) as f32);
        let h = ((col as f32 / frame.width.max(1) as f32 - self.origin_h) / self.spacing_h)
            .clamp(0.0, (self.points_h - 1) as f32);
        let (v0, h0) = (v as usize, h as usize);
        let (v1, h1) = (
            (v0 + 1).min(self.points_v - 1),
            (h0 + 1).min(self.points_h - 1),
        );
        let (tv, th) = (v - v0 as f32, h - h0 as f32);
        let p = p.min(self.map_planes - 1);
        let at = |v: usize, h: usize| self.gains[(v * self.points_h + h) * self.map_planes + p];
        let top = at(v0, h0) + (at(v0, h1) - at(v0, h0)) * th;
        let bottom = at(v1, h0) + (at(v1, h1) - at(v1, h0)) * th;
        top + (bottom - top) * tv
    }
}

/// One opcode this reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Gain(GainMap),
    /// Light lifted by `1 + k0 r² + k1 r⁴ + k2 r⁶ + k3 r⁸ + k4 r¹⁰`, `r`
    /// from the centre (a fraction of the area) to the farthest corner.
    Vignette {
        k: [f32; 5],
        centre: [f32; 2],
    },
    /// From where each point lands to where it comes from, per plane
    /// (one serves all three): a radial polynomial `kr0..kr3` in `r²`
    /// and tangential `kt0`, `kt1`, about `centre`.
    Warp {
        planes: Vec<[f32; 6]>,
        centre: [f32; 2],
    },
}

/// The opcodes of a list this reads, in order; an empty or garbled list
/// is none.
pub fn parse(list: &[u8]) -> Vec<Op> {
    struct Cursor<'a>(&'a [u8], usize);
    impl Cursor<'_> {
        fn bytes(&mut self, n: usize) -> Option<&[u8]> {
            let got = self.0.get(self.1..self.1.checked_add(n)?)?;
            self.1 += n;
            Some(got)
        }
        fn u32(&mut self) -> Option<u32> {
            Some(u32::from_be_bytes(self.bytes(4)?.try_into().ok()?))
        }
        fn f64(&mut self) -> Option<f32> {
            Some(f64::from_be_bytes(self.bytes(8)?.try_into().ok()?) as f32)
        }
        fn f32(&mut self) -> Option<f32> {
            Some(f32::from_be_bytes(self.bytes(4)?.try_into().ok()?))
        }
    }
    let mut out = Vec::new();
    let mut c = Cursor(list, 0);
    let Some(count) = c.u32() else {
        return out;
    };
    for _ in 0..count.min(256) {
        let (Some(id), Some(_version), Some(_flags), Some(size)) =
            (c.u32(), c.u32(), c.u32(), c.u32())
        else {
            break;
        };
        let Some(params) = c.bytes(size as usize) else {
            break;
        };
        let mut p = Cursor(params, 0);
        let op = match id {
            1 => (|| {
                let planes = p.u32()? as usize;
                if !(1..=4).contains(&planes) {
                    return None;
                }
                let planes = (0..planes)
                    .map(|_| Some([p.f64()?, p.f64()?, p.f64()?, p.f64()?, p.f64()?, p.f64()?]))
                    .collect::<Option<Vec<_>>>()?;
                Some(Op::Warp {
                    planes,
                    centre: [p.f64()?, p.f64()?],
                })
            })(),
            3 => (|| {
                Some(Op::Vignette {
                    k: [p.f64()?, p.f64()?, p.f64()?, p.f64()?, p.f64()?],
                    centre: [p.f64()?, p.f64()?],
                })
            })(),
            9 => (|| {
                let mut u = || p.u32().map(|v| v as usize);
                let (top, left, bottom, right) = (u()?, u()?, u()?, u()?);
                let (plane, planes, row_pitch, col_pitch) = (u()?, u()?, u()?, u()?);
                let (points_v, points_h) = (u()?, u()?);
                let (spacing_v, spacing_h, origin_v, origin_h) =
                    (p.f64()?, p.f64()?, p.f64()?, p.f64()?);
                let map_planes = p.u32()? as usize;
                let n = points_v.checked_mul(points_h)?.checked_mul(map_planes)?;
                if n == 0 || n > 1 << 20 || row_pitch == 0 || col_pitch == 0 || planes == 0 {
                    return None;
                }
                if !(spacing_v > 0.0 && spacing_h > 0.0) {
                    return None;
                }
                let gains = (0..n).map(|_| p.f32()).collect::<Option<Vec<_>>>()?;
                if gains.iter().any(|g| !g.is_finite()) {
                    return None;
                }
                Some(Op::Gain(GainMap {
                    top,
                    left,
                    bottom,
                    right,
                    plane,
                    planes,
                    row_pitch,
                    col_pitch,
                    points_v,
                    points_h,
                    spacing_v,
                    spacing_h,
                    origin_v,
                    origin_h,
                    map_planes,
                    gains,
                }))
            })(),
            _ => None,
        };
        out.extend(op);
    }
    out
}

/// The distance from `centre` (fractions of a `w` by `h` area) to the
/// area's farthest corner, in pixels: what a radius of one is.
pub fn reach(centre: [f32; 2], w: f32, h: f32) -> f32 {
    let (cx, cy) = (centre[0] * w, centre[1] * h);
    [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
        .iter()
        .map(|&(x, y)| ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt())
        .fold(0.0, f32::max)
        .max(1.0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An opcode list as a DNG writes one, big-endian.
    pub(crate) fn list(ops: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = (ops.len() as u32).to_be_bytes().to_vec();
        for (id, params) in ops {
            for v in [*id, 0x0103_0000, 0, params.len() as u32] {
                out.extend(v.to_be_bytes());
            }
            out.extend(params);
        }
        out
    }

    pub(crate) fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    /// A gain map over `bounds` (top, left, bottom, right) for `plane`
    /// of `planes`, at `pitch`, of a grid `points` (v, h) over the whole
    /// area, its gains given row by row.
    pub(crate) fn gain_map(
        bounds: [u32; 4],
        (plane, planes): (u32, u32),
        pitch: u32,
        points: (u32, u32),
        gains: &[f32],
    ) -> Vec<u8> {
        let mut p = Vec::new();
        for v in bounds
            .into_iter()
            .chain([plane, planes, pitch, pitch, points.0, points.1])
        {
            p.extend(v.to_be_bytes());
        }
        let spacing = |n: u32| 1.0 / (n.max(2) - 1) as f64;
        p.extend(f64s(&[spacing(points.0), spacing(points.1), 0.0, 0.0]));
        p.extend(1u32.to_be_bytes());
        for g in gains {
            p.extend(g.to_be_bytes());
        }
        p
    }

    #[test]
    fn an_opcode_list_is_read_and_what_it_cannot_do_left_out() {
        let bytes = list(&[
            (
                9,
                gain_map([0, 1, 100, 200], (0, 1), 2, (2, 2), &[1.0, 2.0, 3.0, 4.0]),
            ),
            // A bad-pixel list: not read.
            (5, vec![0; 12]),
            (3, f64s(&[0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5])),
            (
                1,
                [
                    1u32.to_be_bytes().to_vec(),
                    f64s(&[1.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5]),
                ]
                .concat(),
            ),
        ]);
        let ops = parse(&bytes);
        assert_eq!(ops.len(), 3);
        let Op::Gain(g) = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!((g.left, g.bottom, g.row_pitch, g.points_h), (1, 100, 2, 2));
        let frame = Frame {
            left: 0,
            top: 0,
            width: 200,
            height: 100,
        };
        // Corners of the grid, and its middle between all four.
        assert_eq!(g.gain(0, 0, &frame, 0), 1.0);
        assert_eq!(g.gain(100, 200, &frame, 0), 4.0);
        assert!((g.gain(50, 100, &frame, 0) - 2.5).abs() < 1e-6);
        assert!(g.covers(0, 1) && !g.covers(0, 2) && !g.covers(1, 1));
        assert_eq!(
            ops[1],
            Op::Vignette {
                k: [0.5, 0.0, 0.0, 0.0, 0.0],
                centre: [0.5, 0.5]
            }
        );
        assert!(
            matches!(&ops[2], Op::Warp { planes, .. } if planes.len() == 1 && planes[0][1] == 0.1)
        );
        // Cut short anywhere: what was whole before the cut, no panic.
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).len() <= 3);
        }
        assert!(parse(&[]).is_empty());
    }

    #[test]
    fn the_reach_is_to_the_farthest_corner() {
        assert!((reach([0.5, 0.5], 8.0, 6.0) - 5.0).abs() < 1e-6);
        assert!((reach([0.0, 0.0], 8.0, 6.0) - 10.0).abs() < 1e-6);
    }
}
