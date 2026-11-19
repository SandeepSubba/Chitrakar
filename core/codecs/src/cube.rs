//! `.cube` colour lookup tables — the format a grade made in Resolve, a
//! film emulation or a camera's log-to-display conversion is handed over
//! in.
//!
//! A file is a header and then rows of three numbers. A 3D table
//! (`LUT_3D_SIZE n`) is n³ rows, red changing fastest, each the colour
//! that grid point goes to. A 1D table (`LUT_1D_SIZE n`) is n rows, one
//! curve per channel. Either may say the range its input runs over
//! (`DOMAIN_MIN`/`DOMAIN_MAX`, or Resolve's own `LUT_*_INPUT_RANGE`);
//! where that is not nought to one, or the table is 1D, it is resampled
//! here onto a 3D grid over nought to one, so what the document keeps is
//! always one shape and the renderers read one thing.

/// A 3D table over nought to one: `size`³ entries, red fastest, then
/// green, then blue.
#[derive(Debug, Clone, PartialEq)]
pub struct Cube {
    /// What the file calls itself, or nothing.
    pub title: String,
    pub size: u32,
    pub table: Vec<[f32; 3]>,
}

/// The largest grid kept. 65 is the largest in common use; past it the
/// table only costs memory, and a file that says more is resampled.
pub const MOST: u32 = 65;

/// The grid a 1D table is spread over: enough that a smooth curve read
/// back through trilinear steps is within a hair of the curve itself.
const FROM_ONE_D: u32 = 33;

/// Read a `.cube` file. Anything it cannot make sense of is refused with
/// a sentence saying what, rather than read as something else.
pub fn parse_cube(text: &str) -> Result<Cube, String> {
    let mut title = String::new();
    let mut three: Option<u32> = None;
    let mut one: Option<u32> = None;
    let mut lo = [0.0f32; 3];
    let mut hi = [1.0f32; 3];
    let mut rows: Vec<[f32; 3]> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        let numbers = |words: std::str::SplitWhitespace<'_>| -> Result<Vec<f32>, String> {
            words
                .map(|w| {
                    w.parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| format!("line {}: {w:?} is not a number", n + 1))
                })
                .collect()
        };
        let size = |words: std::str::SplitWhitespace<'_>| -> Result<u32, String> {
            let v = numbers(words)?;
            match v.as_slice() {
                [s] if *s >= 2.0 && s.fract() == 0.0 => Ok(*s as u32),
                _ => Err(format!(
                    "line {}: a table's size is one whole number, two or more",
                    n + 1
                )),
            }
        };
        let triple = |words: std::str::SplitWhitespace<'_>| -> Result<[f32; 3], String> {
            match numbers(words)?.as_slice() {
                [a, b, c] => Ok([*a, *b, *c]),
                _ => Err(format!("line {}: wants three numbers", n + 1)),
            }
        };
        match first {
            "TITLE" => {
                title = line["TITLE".len()..].trim().trim_matches('"').to_string();
            }
            "LUT_3D_SIZE" => {
                let s = size(words)?;
                if s > 256 {
                    return Err(format!("a {s}-point grid is more than this reads"));
                }
                three = Some(s);
            }
            "LUT_1D_SIZE" => {
                let s = size(words)?;
                if s > 65536 {
                    return Err(format!("a {s}-point curve is more than this reads"));
                }
                one = Some(s);
            }
            "DOMAIN_MIN" => lo = triple(words)?,
            "DOMAIN_MAX" => hi = triple(words)?,
            "LUT_3D_INPUT_RANGE" | "LUT_1D_INPUT_RANGE" => match numbers(words)?.as_slice() {
                [a, b] => {
                    lo = [*a; 3];
                    hi = [*b; 3];
                }
                _ => return Err(format!("line {}: an input range is two numbers", n + 1)),
            },
            w if w
                .starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.' || c == '+') =>
            {
                rows.push(triple(line.split_whitespace())?);
            }
            // Other keywords (a camera's notes, a vendor's own) say
            // nothing about the colours.
            _ => {}
        }
    }
    if (0..3).any(|c| hi[c] <= lo[c]) {
        return Err("the input range ends before it starts".into());
    }
    let (size, table) = match (three, one) {
        (Some(_), Some(_)) => return Err("a file is a 3D table or a 1D one, not both".into()),
        (None, None) => return Err("no LUT_3D_SIZE or LUT_1D_SIZE: not a .cube table".into()),
        (Some(s), None) => {
            let want = (s as usize).pow(3);
            if rows.len() != want {
                return Err(format!(
                    "a {s}-point grid is {want} rows, and this has {}",
                    rows.len()
                ));
            }
            let plain = lo == [0.0; 3] && hi == [1.0; 3];
            if plain && s <= MOST {
                (s, rows)
            } else {
                let to = s.min(MOST);
                (
                    to,
                    resample(to, |p| {
                        chitrakar_doc::lut::trilinear(s, &rows, along(p, lo, hi))
                    }),
                )
            }
        }
        (None, Some(s)) => {
            if rows.len() != s as usize {
                return Err(format!(
                    "a {s}-point curve is {s} rows, and this has {}",
                    rows.len()
                ));
            }
            let curve = |c: usize, v: f32| {
                let x = ((v - lo[c]) / (hi[c] - lo[c])).clamp(0.0, 1.0) * (s - 1) as f32;
                let i = (x.floor() as usize).min(s as usize - 2);
                let f = x - i as f32;
                rows[i][c] + (rows[i + 1][c] - rows[i][c]) * f
            };
            (
                FROM_ONE_D,
                resample(FROM_ONE_D, |p| {
                    [curve(0, p[0]), curve(1, p[1]), curve(2, p[2])]
                }),
            )
        }
    };
    Ok(Cube { title, size, table })
}

/// A grid over nought to one, each point what `f` says of it.
fn resample(size: u32, f: impl Fn([f32; 3]) -> [f32; 3]) -> Vec<[f32; 3]> {
    let n = size as usize;
    let step = 1.0 / (size - 1) as f32;
    let mut out = Vec::with_capacity(n * n * n);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                out.push(f([r as f32 * step, g as f32 * step, b as f32 * step]));
            }
        }
    }
    out
}

/// Where a colour over nought to one falls in a table whose input runs
/// from `lo` to `hi`, as a share of that range, held to it.
fn along(p: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|c| ((p[c] - lo[c]) / (hi[c] - lo[c])).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_doc::lut::trilinear;

    /// A 3D table written out the way a grading program writes one: red
    /// fastest, a title, comments, a keyword this does not know.
    fn written(size: u32, f: impl Fn([f32; 3]) -> [f32; 3], header: &str) -> String {
        let mut out = format!(
            "# made by hand\nTITLE \"Swap\"\n{header}LUT_3D_SIZE {size}\nLUT_3D_VENDOR whoever\n\n"
        );
        let step = 1.0 / (size - 1) as f32;
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let [x, y, z] = f([r as f32 * step, g as f32 * step, b as f32 * step]);
                    out.push_str(&format!("{x:.6} {y:.6} {z:.6}\n"));
                }
            }
        }
        out
    }

    #[test]
    fn a_3d_table_reads_red_fastest() {
        let swap = |[r, g, b]: [f32; 3]| [b, g * 0.5, r];
        let cube = parse_cube(&written(5, swap, "")).unwrap();
        assert_eq!((cube.title.as_str(), cube.size), ("Swap", 5));
        // Red alone becomes blue alone: red is the fastest index.
        let q = trilinear(cube.size, &cube.table, [1.0, 0.0, 0.0]);
        assert!(q[0].abs() < 1e-6 && q[2] > 0.999, "{q:?}");
        let q = trilinear(cube.size, &cube.table, [0.2, 0.6, 0.9]);
        assert!(
            (q[0] - 0.9).abs() < 1e-5 && (q[1] - 0.3).abs() < 1e-5 && (q[2] - 0.2).abs() < 1e-5
        );
    }

    /// A table over another range is resampled onto nought to one, so a
    /// colour asked of the document's table is the colour the file's
    /// table gives where that colour falls in the file's range.
    #[test]
    fn a_table_over_another_range_is_resampled_onto_nought_to_one() {
        // Over 0..2 on every channel: a grid point at p means the colour 2p.
        let doubled = |[r, g, b]: [f32; 3]| [r * 2.0 / 4.0, g * 2.0 / 4.0, b * 2.0 / 4.0];
        let text = written(9, doubled, "DOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n");
        let cube = parse_cube(&text).unwrap();
        // The file says colour c goes to c/4; so does this.
        let q = trilinear(cube.size, &cube.table, [0.5, 1.0, 0.25]);
        for (got, want) in q.iter().zip([0.125, 0.25, 0.0625]) {
            assert!((got - want).abs() < 1e-4, "{q:?}");
        }
        // And Resolve's spelling of the same thing.
        let text = written(9, doubled, "LUT_3D_INPUT_RANGE 0 2\n");
        assert_eq!(parse_cube(&text).unwrap(), cube);
    }

    /// A 1D table is a curve a channel, spread over a 3D grid.
    #[test]
    fn a_1d_table_becomes_a_grid_of_its_curves() {
        let mut text = String::from("LUT_1D_SIZE 3\n");
        // Red inverted, green squared at the middle, blue kept.
        for (r, g, b) in [(1.0, 0.0, 0.0), (0.5, 0.25, 0.5), (0.0, 1.0, 1.0)] {
            text.push_str(&format!("{r} {g} {b}\n"));
        }
        let cube = parse_cube(&text).unwrap();
        assert_eq!(cube.size, FROM_ONE_D);
        let q = trilinear(cube.size, &cube.table, [0.25, 0.5, 0.75]);
        for (got, want) in q.iter().zip([0.75, 0.25, 0.75]) {
            assert!((got - want).abs() < 1e-5, "{q:?}");
        }
    }

    /// A larger grid than is kept is resampled down rather than refused.
    #[test]
    fn a_grid_past_the_largest_kept_is_resampled() {
        let swap = |[r, g, b]: [f32; 3]| [b, g, r];
        let cube = parse_cube(&written(MOST + 2, swap, "")).unwrap();
        assert_eq!(cube.size, MOST);
        let q = trilinear(cube.size, &cube.table, [0.1, 0.4, 0.8]);
        assert!(
            (q[0] - 0.8).abs() < 1e-4 && (q[2] - 0.1).abs() < 1e-4,
            "{q:?}"
        );
    }

    /// What is not a table says so, rather than becoming one.
    #[test]
    fn what_is_not_a_table_is_refused() {
        for (text, says) in [
            ("", "no LUT_3D_SIZE"),
            ("LUT_3D_SIZE 2\n0 0 0\n", "is 8 rows"),
            ("LUT_3D_SIZE 1\n0 0 0\n", "two or more"),
            ("LUT_3D_SIZE 2\nLUT_1D_SIZE 2\n", "not both"),
            ("LUT_1D_SIZE 2\n0 0 0\n1 x 1\n", "is not a number"),
            ("LUT_1D_SIZE 2\n0 0\n1 1 1\n", "three numbers"),
            (
                "LUT_1D_SIZE 2\nDOMAIN_MIN 1 1 1\nDOMAIN_MAX 0 0 0\n0 0 0\n1 1 1\n",
                "ends before",
            ),
            ("LUT_3D_SIZE 1000\n", "more than this reads"),
        ] {
            let err = parse_cube(text).unwrap_err();
            assert!(err.contains(says), "{text:?} said {err:?}");
        }
    }
}
