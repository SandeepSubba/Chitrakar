//! Color foundations for Chitrakar.
//!
//! The working pixel format everywhere in the engine is 32-bit float,
//! premultiplied alpha, linear light ([`LinearRgba`]). Conversion to and from
//! encoded spaces (sRGB now; ICC-profile-driven transforms in Phase 3) happens
//! only at the pipeline edges: import, display, and export.

pub mod cms;

pub use cms::CmykCms;

use serde::{Deserialize, Serialize};

/// The color mode of a document. CMYK documents composite in a linear RGB
/// proxy space; authored CMYK values are preserved on objects and used at
/// proofing/export time (see docs/PLAN.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    Rgb,
    Cmyk,
}

/// A color value as authored by the user, preserved losslessly in the
/// document. Rendering converts it to [`LinearRgba`] via the document's
/// working space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthoredColor {
    /// Non-linear sRGB components in 0..=1.
    Srgb { r: f32, g: f32, b: f32, a: f32 },
    /// CMYK components in 0..=1 (ink coverage).
    Cmyk {
        c: f32,
        m: f32,
        y: f32,
        k: f32,
        a: f32,
    },
    /// A colour that stands for one of the document's swatches: the name
    /// it goes by, and what that name means at the moment.
    ///
    /// The colour is *carried* rather than looked up. A colour that knows
    /// what it looks like is still a colour away from the document it was
    /// authored in — pasted into another file, read by an exporter, or in
    /// a palette the name has since been taken out of — and nothing in
    /// the renderer has to learn about palettes. What makes the link live
    /// is the other direction: `Document::settle_swatches` re-points every
    /// one of these at what the palette now says, so changing one entry
    /// recolours every layer that reached for it.
    Named {
        name: String,
        means: Box<AuthoredColor>,
    },
}

impl AuthoredColor {
    /// The colour itself, with any names peeled off — what to match on
    /// when what is wanted is the components rather than the reference.
    ///
    /// Iterative on purpose: a name meaning a name is not a thing the
    /// editor makes, but a hand-written file can say it, and a colour is
    /// walked once per layer per frame.
    pub fn flat(&self) -> &AuthoredColor {
        let mut at = self;
        while let AuthoredColor::Named { means, .. } = at {
            at = means;
        }
        at
    }

    /// The alpha the colour was authored with.
    pub fn alpha(&self) -> f32 {
        match *self.flat() {
            AuthoredColor::Srgb { a, .. } | AuthoredColor::Cmyk { a, .. } => a,
            // `flat` returns one of the two above.
            AuthoredColor::Named { .. } => unreachable!(),
        }
    }

    /// The swatch this colour stands for, if it stands for one.
    pub fn swatch_name(&self) -> Option<&str> {
        match self {
            AuthoredColor::Named { name, .. } => Some(name),
            _ => None,
        }
    }

    /// The same colour standing for `name` instead of nothing — or for a
    /// different name, since a colour stands for one swatch at a time.
    pub fn standing_for(&self, name: impl Into<String>) -> AuthoredColor {
        AuthoredColor::Named {
            name: name.into(),
            means: Box::new(self.flat().clone()),
        }
    }
}

/// Premultiplied, linear-light RGBA. The engine's working pixel format.
///
/// `Zeroable` because a surface is a great many of these and the one that
/// matters is [`LinearRgba::TRANSPARENT`], whose bits are all zero: it
/// lets a page-sized run be asked of the allocator as zeroed memory
/// instead of written pixel by pixel. See [`transparent_run`].
#[derive(Debug, Clone, Copy, PartialEq, Default, bytemuck::Zeroable)]
pub struct LinearRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// A run of fully transparent pixels.
///
/// `vec![LinearRgba::TRANSPARENT; n]` writes every element, because the
/// standard library only asks the allocator for zeroed memory when the
/// element is a primitive it knows about — a struct of four zero floats
/// loses that, however zero it is. For a surface the size of an A4 page at
/// three hundred dots an inch that is a hundred and forty megabytes
/// written before anything is drawn, and measured on this machine it costs
/// sixty-five milliseconds: the whole of what an empty page took to
/// render. Zeroed memory from the allocator is pages the system already
/// knows are zero, so the cost falls only on the pixels something actually
/// paints — which on a page with a small drawing on it is almost none of
/// them.
pub fn transparent_run(len: usize) -> Vec<LinearRgba> {
    bytemuck::zeroed_vec(len)
}

impl LinearRgba {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// Source-over compositing (premultiplied).
    pub fn over(self, dst: Self) -> Self {
        let ia = 1.0 - self.a;
        Self {
            r: self.r + dst.r * ia,
            g: self.g + dst.g * ia,
            b: self.b + dst.b * ia,
            a: self.a + dst.a * ia,
        }
    }

    /// Convert an 8-bit non-linear sRGB pixel (straight alpha) into the
    /// working format.
    pub fn from_srgb8(r: u8, g: u8, b: u8, a: u8) -> Self {
        let af = a as f32 / 255.0;
        let lin = |v: u8| srgb_to_linear(v as f32 / 255.0);
        Self {
            r: lin(r) * af,
            g: lin(g) * af,
            b: lin(b) * af,
            a: af,
        }
    }

    /// Convert back to 8-bit non-linear sRGB with straight alpha (display /
    /// export edge). Values are clamped.
    ///
    /// Pixels whose alpha quantizes to zero encode as transparent black:
    /// un-premultiplying by a near-zero alpha amplifies float dust into
    /// arbitrary color, which is both meaningless (nothing is shown) and
    /// non-deterministic — the same invisible pixel could encode differently
    /// depending on rounding upstream.
    pub fn to_srgb8(self) -> [u8; 4] {
        Srgb8::new().encode(self)
    }
}

/// sRGB EOTF inverse: encoded 0..=1 → linear 0..=1.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        power((v + 0.055) / 1.055, Power::Decode)
    }
}

/// The two powers the sRGB curve is made of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Power {
    /// `x^2.4`, out of the encoding into light.
    Decode,
    /// `x^(1/2.4)`, from light into the encoding.
    Encode,
}

/// `x` raised to one of the curve's two powers.
///
/// Natively that is `powf`, which the machine does in a few dozen
/// instructions, and this is the reference every other answer is held
/// to. WebAssembly has no instruction for a power: there `powf` is a
/// routine in software, a channel a pixel crosses the curve once or
/// twice for nearly every adjustment, and in a phone's browser that was
/// more of a frame than anything else in it. So there the power is read
/// from a table (`PowerTable`) — within a few hundred-millionths of
/// `powf`, which is the last bit or two of an `f32`
/// (`the_curve_by_table_is_the_curve_by_powers`) — and `powf` answers
/// only off the table's ends.
#[inline]
fn power(x: f32, which: Power) -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(v) = power_table(which).at(x) {
            return v;
        }
    }
    power_exact(x, which)
}

#[inline]
fn power_exact(x: f32, which: Power) -> f32 {
    match which {
        Power::Decode => x.powf(2.4),
        Power::Encode => x.powf(1.0 / 2.4),
    }
}

/// A power over `[lo, 1]`, at 4096 points an octave — the float's
/// exponent and its leading twelve bits of fraction say which two points
/// a value is between, and the rest of its fraction how far, which within
/// an octave is a straight line in the value itself. A power law bends
/// so little across a four-thousandth of an octave that the line between
/// two points is off by a few parts in a hundred million. (A thousandth
/// was tried first: four parts in ten million, which is several bits.)
#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
struct PowerTable {
    first: u32,
    values: Vec<f32>,
}

/// Bits of fraction below the twelve that index the table.
#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
const POWER_SHIFT: u32 = 11;

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
impl PowerTable {
    fn new(lo: f32, which: Power) -> PowerTable {
        PowerTable::of(lo, |x| power_exact(x, which))
    }

    fn of(lo: f32, f: impl Fn(f32) -> f32) -> PowerTable {
        let first = lo.to_bits() >> POWER_SHIFT << POWER_SHIFT;
        let last = 1.0f32.to_bits();
        let values = (0..=((last - first) >> POWER_SHIFT) + 1)
            .map(|i| f(f32::from_bits(first + (i << POWER_SHIFT))))
            .collect();
        PowerTable { first, values }
    }

    #[inline]
    fn at(&self, x: f32) -> Option<f32> {
        let bits = x.to_bits();
        if x.is_nan() || x > 1.0 || bits < self.first {
            return None;
        }
        let off = bits - self.first;
        let i = (off >> POWER_SHIFT) as usize;
        let t = (off & ((1 << POWER_SHIFT) - 1)) as f32 * (1.0 / (1 << POWER_SHIFT) as f32);
        let (a, b) = (*self.values.get(i)?, *self.values.get(i + 1)?);
        Some(a + (b - a) * t)
    }
}

/// `x.powf(e)`, for a pass that raises every channel of a page to one
/// power — levels with a gamma. Natively that is `powf`. In WebAssembly
/// it is read from a table of `x^e` over `[2^-16, 1]`, made the first
/// time `e` is asked for and kept until another is (a slider being
/// dragged asks for one a frame, and a table is a few milliseconds):
/// within a few parts in a million for any exponent a gamma gives, which
/// is a thousandth of the eight bits a page is shown in
/// (`a_power_by_table_is_the_power`). Off the table's ends — above one,
/// or darker than it reaches — `powf` answers.
///
/// It fetches the table each time, so a pass raising millions of
/// channels to one power holds an [`Exponent`] instead and asks that.
#[inline]
pub fn pow(x: f32, e: f32) -> f32 {
    Exponent::new(e).of(x)
}

/// One power, fetched once for a whole pass: `of(x)` is [`pow`]`(x, e)`
/// to the bit, without finding the table again for every channel —
/// which was a lock and a count up and down each time, and made levels
/// with a gamma slower in WebAssembly built for SIMD than without.
#[derive(Clone)]
pub struct Exponent {
    e: f32,
    #[cfg(target_arch = "wasm32")]
    table: std::sync::Arc<PowerTable>,
}

impl Exponent {
    pub fn new(e: f32) -> Exponent {
        Exponent {
            e,
            #[cfg(target_arch = "wasm32")]
            table: exponent_table(e),
        }
    }

    /// The power it raises to.
    pub fn exponent(&self) -> f32 {
        self.e
    }

    #[inline]
    pub fn of(&self, x: f32) -> f32 {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(v) = self.table.at(x) {
                return v;
            }
        }
        x.powf(self.e)
    }
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn exponent_table(e: f32) -> std::sync::Arc<PowerTable> {
    use std::sync::{Arc, Mutex};
    static KEPT: Mutex<Option<(u32, Arc<PowerTable>)>> = Mutex::new(None);
    let mut kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((bits, table)) = kept.as_ref() {
        if *bits == e.to_bits() {
            return table.clone();
        }
    }
    let table = Arc::new(PowerTable::of(1.0 / 65536.0, |x| x.powf(e)));
    *kept = Some((e.to_bits(), table.clone()));
    table
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn power_table(which: Power) -> &'static PowerTable {
    static DECODE: std::sync::OnceLock<PowerTable> = std::sync::OnceLock::new();
    static ENCODE: std::sync::OnceLock<PowerTable> = std::sync::OnceLock::new();
    match which {
        // Past the curve's straight foot: (0.04045 + 0.055) / 1.055.
        Power::Decode => DECODE.get_or_init(|| PowerTable::new(0.09, Power::Decode)),
        // And past the other end's: 0.003_130_8.
        Power::Encode => ENCODE.get_or_init(|| PowerTable::new(0.003, Power::Encode)),
    }
}

/// `v.round()` for a value that is not negative: half away from zero,
/// which for these is half up. The same number, to the bit, as
/// `f32::round` — a whole part and what is left over are both exact —
/// without the call: WebAssembly has no instruction for rounding half
/// away from zero, so `round` there is a routine in software, and the
/// frame a page shows rounds four times a pixel.
#[inline]
pub fn round_unsigned(v: f32) -> f32 {
    let whole = v.trunc();
    if v - whole >= 0.5 {
        whole + 1.0
    } else {
        whole
    }
}

/// `v.round()` for any `f32`: half away from zero, to the bit, without
/// the call `round` is in WebAssembly ([`round_unsigned`] for each side).
#[inline]
pub fn round_half_away(v: f32) -> f32 {
    if v.is_sign_negative() {
        -round_unsigned(-v)
    } else {
        round_unsigned(v)
    }
}

/// A linear channel as the eight-bit sRGB value a display is sent:
/// `(linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8`, the same
/// answer for every `f32`, without working out the curve.
///
/// The curve is a power law, and WebAssembly has no instruction for a
/// power: there it is a routine in software, and the frame a page shows
/// was three of them a pixel — in a phone's browser, more than the whole
/// of drawing the photograph the frame was of. The answer is one of 256
/// whole numbers, though, and the curve only climbs, so it is the count
/// of the 255 points where it steps up that `v` has reached. Those are
/// found once, by bisection over the floats themselves using the curve
/// as it is, so each is exactly where the curve's own answer changes;
/// an index by a value's leading bits says where to start counting, and
/// from there it is a step or two
/// (`eight_bits_by_table_are_the_curves_own_eight_bits`).
#[inline]
pub fn linear_to_srgb8(v: f32) -> u8 {
    Srgb8::new().channel(v)
}

/// The eight-bit encoder with its steps in hand: [`linear_to_srgb8`] and
/// [`LinearRgba::to_srgb8`] for a caller encoding a whole frame, which
/// asks for the steps once rather than three times a pixel.
#[derive(Clone, Copy)]
pub struct Srgb8 {
    steps: &'static Srgb8Steps,
}

impl Default for Srgb8 {
    fn default() -> Self {
        Self::new()
    }
}

impl Srgb8 {
    pub fn new() -> Self {
        Srgb8 {
            steps: srgb8_steps(),
        }
    }

    /// One linear channel: [`linear_to_srgb8`].
    ///
    /// Read off the value's bits. A positive float's bits are in the
    /// order its value is, so everything at or past one's bits is one or
    /// more — white — unless it is past infinity's, which is a NaN; and a
    /// negative number, nought included, has the sign bit set and so is
    /// past them too. What is left, past the first step's starting point,
    /// is one read of the table and one comparison: no two steps fall
    /// between neighbouring starting points, so the step a slot has
    /// certainly reached is either the answer or one short of it. Every
    /// frame is every pixel of it through this three times, which is why
    /// it is written as straight arithmetic rather than a loop that runs
    /// once.
    #[inline(always)]
    pub fn channel(&self, v: f32) -> u8 {
        let bits = v.to_bits();
        if bits >= ONE_BITS {
            return if bits <= INFINITY_BITS { 255 } else { 0 };
        }
        let Some(from) = bits.checked_sub(self.steps.first) else {
            return 0;
        };
        // In range: below one's bits, so at most the last slot.
        let k = self.steps.start[(from >> STEP_SHIFT) as usize];
        k + (v >= self.steps.at[k as usize]) as u8
    }

    /// One premultiplied pixel: [`LinearRgba::to_srgb8`].
    #[inline(always)]
    pub fn encode(&self, px: LinearRgba) -> [u8; 4] {
        // Opaque, which is nearly all of a page: a division by one is the
        // number itself, to the bit, and three divisions a pixel are what
        // is left of this encode's cost once the curve is a table.
        if px.a == 1.0 {
            return [
                self.channel(px.r),
                self.channel(px.g),
                self.channel(px.b),
                255,
            ];
        }
        let alpha = round_unsigned(px.a.clamp(0.0, 1.0) * 255.0) as u8;
        if alpha == 0 {
            return [0, 0, 0, 0];
        }
        let enc = |v: f32| self.channel(v / px.a);
        [enc(px.r), enc(px.g), enc(px.b), alpha]
    }
}

/// The curve's own eight bits, worked out: what [`linear_to_srgb8`]
/// stands for.
fn srgb8_by_curve(v: f32) -> u8 {
    (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8
}

/// How far a value's bits are shifted to index where to start counting:
/// the exponent and eleven bits of the fraction, so 2048 starting points
/// an octave — finer than the curve's steps anywhere, so no more than one
/// of them falls between two starting points and a channel is one
/// comparison.
const STEP_SHIFT: u32 = 12;

/// One's bits and infinity's: everything a channel reads from the bits
/// alone, before the table.
const ONE_BITS: u32 = 0x3f80_0000;
const INFINITY_BITS: u32 = 0x7f80_0000;

struct Srgb8Steps {
    /// The least value giving each of 1 ..= 255, and past them one that
    /// nothing reaches, so the comparison after the last step is asked of
    /// something and answers no — and an index that is a `u8` is always
    /// in the table.
    at: [f32; 256],
    /// The bits the first starting point stands at: the first step's,
    /// rounded down to a starting point. Anything less is nought.
    first: u32,
    /// How many of those a value at or past each starting point has
    /// certainly reached.
    start: Vec<u8>,
}

fn srgb8_steps() -> &'static Srgb8Steps {
    static STEPS: std::sync::OnceLock<Srgb8Steps> = std::sync::OnceLock::new();
    STEPS.get_or_init(|| {
        let one = 1.0f32.to_bits();
        let mut at = [f32::INFINITY; 256];
        for (k, step) in at.iter_mut().take(255).enumerate() {
            let want = k as u8 + 1;
            // The least bits in (0, one] whose value gives `want` or more.
            let (mut lo, mut hi) = (0u32, one);
            while lo + 1 < hi {
                let mid = lo + (hi - lo) / 2;
                if srgb8_by_curve(f32::from_bits(mid)) >= want {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            *step = f32::from_bits(hi);
        }
        let first = at[0].to_bits() >> STEP_SHIFT << STEP_SHIFT;
        let mut reached = 0usize;
        debug_assert_eq!(one, ONE_BITS);
        let start = (0..=((one - first) >> STEP_SHIFT))
            .map(|b| {
                let lowest = f32::from_bits(first + (b << STEP_SHIFT));
                while reached < 255 && at[reached] <= lowest {
                    reached += 1;
                }
                reached as u8
            })
            .collect();
        Srgb8Steps { at, first, start }
    })
}

/// Linear 0..=1 → sRGB-encoded 0..=1.
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else if v >= 1.0 {
        // The curve's own arithmetic misses the top of the range: at one,
        // `1.055 * 1 - 0.055` is 0.999_999_94 in f32 rather than one, and
        // white that is not quite white is a different thing to anything
        // asking `is this the top`. Colour burn is where it showed —
        // white under a black source is white by the compositing spec and
        // came out black, because the branch that says so is `backdrop
        // == 1` and the backdrop arrived a rounding short of it. Said
        // here rather than guarded at each reader, since every one of
        // them would have to know.
        1.0
    } else {
        1.055 * power(v, Power::Encode) - 0.055
    }
}

/// Naive device conversion of authored colors to linear RGB for compositing.
/// Placeholder until the ICC engine lands in Phase 3: CMYK uses the standard
/// uncalibrated formula, which is good enough for on-screen editing previews.
pub fn to_working(color: &AuthoredColor) -> LinearRgba {
    match *color.flat() {
        AuthoredColor::Srgb { r, g, b, a } => LinearRgba {
            r: srgb_to_linear(r) * a,
            g: srgb_to_linear(g) * a,
            b: srgb_to_linear(b) * a,
            a,
        },
        AuthoredColor::Cmyk { c, m, y, k, a } => {
            let to_lin = |ink: f32| srgb_to_linear((1.0 - ink) * (1.0 - k));
            LinearRgba {
                r: to_lin(c) * a,
                g: to_lin(m) * a,
                b: to_lin(y) * a,
                a,
            }
        }
        // `flat` returns one of the two above.
        AuthoredColor::Named { .. } => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    /// A whole pixel encoded is the formula's bytes: opaque (where the
    /// divisions by one are skipped), see-through, and gone.
    #[test]
    fn a_pixel_encoded_is_the_formulas_bytes() {
        let reference = |p: LinearRgba| -> [u8; 4] {
            let alpha = (p.a.clamp(0.0, 1.0) * 255.0).round() as u8;
            if alpha == 0 {
                return [0, 0, 0, 0];
            }
            let enc = |v: f32| (linear_to_srgb((v / p.a).clamp(0.0, 1.0)) * 255.0).round() as u8;
            [enc(p.r), enc(p.g), enc(p.b), alpha]
        };
        let mut seed = 0x1234_5678u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 100_000) as f32 / 99_999.0
        };
        let encoder = Srgb8::new();
        for _ in 0..200_000 {
            let a = match (next() * 4.0) as u32 {
                0 => 1.0,
                1 => 0.0,
                _ => next(),
            };
            let p = LinearRgba {
                r: next() * 1.2 * a,
                g: next() * a,
                b: next() * a,
                a,
            };
            assert_eq!(encoder.encode(p), reference(p), "{p:?}");
        }
    }

    /// Any power a levels gamma asks for, by table, is the power to a few
    /// parts in a million wherever the table answers, and the same table
    /// is handed back while the exponent stays.
    #[test]
    fn a_power_by_table_is_the_power() {
        for e in [1.0f32 / 20.0, 1.0 / 3.0, 0.77, 1.3, 2.2, 5.0, 10.0] {
            let table = exponent_table(e);
            let mut worst = 0.0f64;
            let mut bits = (1.0f32 / 65536.0).to_bits();
            while bits <= 1.0f32.to_bits() {
                let x = f32::from_bits(bits);
                let exact = x.powf(e);
                let got = table.at(x).expect("on the table");
                // Relatively, down to where the power has run out of
                // float to say it in: x^10 near 2^-16 is denormal, and a
                // denormal has no relative precision to keep.
                if exact > 1e-30 {
                    worst = worst.max(((got - exact) / exact).abs() as f64);
                } else {
                    assert!((got - exact).abs() < 1e-30, "x^{e} at {x:e}");
                }
                bits += 61;
            }
            assert!(
                worst < 1e-6,
                "x^{e} is {worst:e} from the power, relatively"
            );
            assert!(std::sync::Arc::ptr_eq(&table, &exponent_table(e)));
            assert!(table.at(1.5).is_none() && table.at(1e-6).is_none());
        }
    }

    /// The curve as WebAssembly reads it, from a table, is the curve by
    /// powers to within the last bit or two of an `f32`: across every
    /// value the table covers, in both directions, and the table answers
    /// for the whole of the curve past its straight foot.
    #[test]
    fn the_curve_by_table_is_the_curve_by_powers() {
        for (which, lo) in [(Power::Decode, 0.0905f32), (Power::Encode, 0.003_130_9)] {
            let table = power_table(which);
            let mut worst = 0.0f64;
            let mut bits = lo.to_bits();
            while bits <= 1.0f32.to_bits() {
                let x = f32::from_bits(bits);
                let by_table = table
                    .at(x)
                    .expect("the table covers the curve past its foot");
                let exact = power_exact(x, which);
                worst = worst.max(((by_table - exact) / exact).abs() as f64);
                bits += 7;
            }
            assert!(
                worst < 1.2e-7,
                "the table is {worst:e} from the power, relatively"
            );
            // Off its ends it says so, and `powf` answers.
            assert!(table.at(1.5).is_none() && table.at(f32::NAN).is_none());
            assert!(table.at(0.001).is_none());
        }
    }

    /// Eight bits by the table are the curve's own eight bits for every
    /// value tried: every one either side of each step, a spread of a
    /// few million across nothing to one, and what is off the ends — less
    /// than nothing, more than one, not a number.
    #[test]
    fn eight_bits_by_table_are_the_curves_own_eight_bits() {
        let steps = srgb8_steps();
        // No two steps between neighbouring starting points, which is
        // what makes a channel one comparison.
        for pair in steps.start.windows(2) {
            assert!(pair[1] - pair[0] <= 1, "two steps in one slot: {pair:?}");
        }
        for t in steps.at {
            for d in [-2i64, -1, 0, 1, 2] {
                let v = f32::from_bits((t.to_bits() as i64 + d) as u32);
                assert_eq!(linear_to_srgb8(v), srgb8_by_curve(v), "at {v:e}");
            }
        }
        let one = 1.0f32.to_bits();
        let mut bits = 0u32;
        while bits <= one {
            let v = f32::from_bits(bits);
            assert_eq!(linear_to_srgb8(v), srgb8_by_curve(v), "at {v:e}");
            bits += 331;
        }
        for v in [
            -1.0,
            -0.0,
            0.0,
            1.0,
            1.5,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            1e-40,
        ] {
            assert_eq!(linear_to_srgb8(v), srgb8_by_curve(v), "at {v:e}");
        }
        // And the rounding beside it is `round`, to the bit.
        let mut bits = 0u32;
        while bits <= 255.0f32.to_bits() {
            let v = f32::from_bits(bits);
            assert_eq!(round_unsigned(v).to_bits(), v.round().to_bits(), "at {v:e}");
            bits += 97;
        }
        for v in [0.5f32, 1.5, 2.5, 254.5, 0.49999997, 0.5000001] {
            assert_eq!(round_unsigned(v).to_bits(), v.round().to_bits(), "at {v:e}");
        }
        // Either side of nothing, through the whole range and its ends.
        let mut bits = 0u32;
        while bits < 0x7f80_0000 {
            for v in [f32::from_bits(bits), -f32::from_bits(bits)] {
                assert_eq!(
                    round_half_away(v).to_bits(),
                    v.round().to_bits(),
                    "at {v:e}"
                );
            }
            bits += 9973;
        }
        for v in [
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            -0.5,
            -2.5,
            -0.49999997,
            8388609.0,
        ] {
            assert_eq!(
                round_half_away(v).to_bits(),
                v.round().to_bits(),
                "at {v:e}"
            );
        }
        assert!(round_half_away(f32::NAN).is_nan());
    }

    use super::*;

    #[test]
    fn srgb_roundtrip_is_lossless_at_8_bits() {
        for v in [0u8, 1, 17, 128, 200, 254, 255] {
            let px = LinearRgba::from_srgb8(v, v, v, 255);
            assert_eq!(px.to_srgb8(), [v, v, v, 255]);
        }
    }

    #[test]
    fn invisible_pixels_encode_as_transparent_black() {
        // Float dust left by e.g. a blur tail: alpha rounds to 0, so the
        // color channels must not be amplified into junk by unpremultiply.
        let dust = LinearRgba {
            r: 1e-9,
            g: 5e-10,
            b: 2e-9,
            a: 1e-9,
        };
        assert_eq!(dust.to_srgb8(), [0, 0, 0, 0]);
        assert_eq!(LinearRgba::TRANSPARENT.to_srgb8(), [0, 0, 0, 0]);
        // Just-visible alpha still encodes its color.
        let faint = LinearRgba {
            r: 0.5 * 0.01,
            g: 0.0,
            b: 0.0,
            a: 0.01,
        };
        let out = faint.to_srgb8();
        assert!(out[3] > 0 && out[0] > 100, "faint but real pixel: {out:?}");
    }

    /// A run of transparent pixels is asked of the allocator as zeroed
    /// memory, which is only the same thing while every field of
    /// `TRANSPARENT` is a zero float.
    ///
    /// It is the whole of why a page-sized surface costs nothing to make,
    /// and it is the kind of assumption that a field added later with a
    /// sensible non-zero default would break in silence: every surface in
    /// the engine would start life full of something else. So it is
    /// asked, rather than left to the derive.
    #[test]
    fn a_run_of_transparent_pixels_is_transparent() {
        let run = transparent_run(1000);
        assert_eq!(run.len(), 1000);
        assert!(
            run.iter().all(|p| *p == LinearRgba::TRANSPARENT),
            "a zeroed run is not the transparent pixel"
        );
        // Said of the bits rather than of the values, since a negative
        // zero is equal to a zero and is not the same memory.
        let t = LinearRgba::TRANSPARENT;
        assert_eq!(
            (t.r.to_bits(), t.g.to_bits(), t.b.to_bits(), t.a.to_bits()),
            (0, 0, 0, 0),
            "the transparent pixel is not four zero floats"
        );
        // And of the size, which is what catches a field added later:
        // the four above would still be zero and the new one would not
        // be looked at, so every surface in the engine would quietly
        // start life holding something else.
        assert_eq!(
            std::mem::size_of::<LinearRgba>(),
            4 * std::mem::size_of::<f32>(),
            "a pixel has gained a field; is it zero when its bits are?"
        );
        assert_eq!(LinearRgba::default(), LinearRgba::TRANSPARENT);
    }

    #[test]
    fn over_opaque_src_wins() {
        let red = LinearRgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let blue = LinearRgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        assert_eq!(red.over(blue), red);
    }

    #[test]
    fn over_transparent_src_is_identity() {
        let dst = LinearRgba {
            r: 0.25,
            g: 0.5,
            b: 0.75,
            a: 1.0,
        };
        assert_eq!(LinearRgba::TRANSPARENT.over(dst), dst);
    }

    /// A name is a reference and not a colour space: what a named colour
    /// looks like is what it stands for. Including in the case nothing
    /// here makes but a hand-written file can say — a name meaning a name
    /// — which is read through rather than followed one step.
    #[test]
    fn a_named_colour_is_the_colour_it_stands_for() {
        let green = AuthoredColor::Srgb {
            r: 0.0,
            g: 0.6,
            b: 0.2,
            a: 0.5,
        };
        let named = green.standing_for("brand");
        assert_eq!(named.swatch_name(), Some("brand"));
        assert_eq!(named.flat(), &green);
        assert_eq!(named.alpha(), 0.5);
        assert_eq!(to_working(&named), to_working(&green));
        // Standing for a second name replaces the first rather than
        // stacking on it: a colour stands for one swatch at a time.
        let again = named.standing_for("other");
        assert_eq!(again.swatch_name(), Some("other"));
        assert_eq!(again.flat(), &green);
        // And a chain written by hand still reads as the colour at its end.
        let chain = AuthoredColor::Named {
            name: "outer".into(),
            means: Box::new(named.clone()),
        };
        assert_eq!(chain.flat(), &green);
        assert_eq!(to_working(&chain), to_working(&green));
    }

    /// Each ink takes out the light it is named for.
    ///
    /// Without a press profile a CMYK colour goes through the plain
    /// uncalibrated formula, and that is what every document gets until
    /// somebody loads one — so it is the path most CMYK colours are seen
    /// through, not a fallback nobody meets. What it had was black
    /// mapping to black, which is true of the formula with cyan and
    /// magenta traded, with any two channels traded, and with the inks
    /// read in any order at all: at full black every channel is nought
    /// and the arrangement cannot be seen. Trading cyan and magenta did
    /// in fact survive the whole workspace with no profile set.
    ///
    /// The meaning is physical and each ink can be asked for its own:
    /// cyan absorbs red, magenta absorbs green, yellow absorbs blue. So
    /// a full cyan leaves green and blue and takes the red away, and the
    /// other two say the same thing one channel over. Two inks together
    /// leave only the third light, which is what makes cyan over magenta
    /// blue rather than a muddier cyan.
    #[test]
    fn each_ink_takes_out_the_light_it_is_named_for() {
        let ink = |c: f32, m: f32, y: f32, k: f32| {
            to_working(&AuthoredColor::Cmyk { c, m, y, k, a: 1.0 }).to_srgb8()
        };
        // One ink at a time, full strength, no black.
        assert_eq!(
            ink(1.0, 0.0, 0.0, 0.0),
            [0, 255, 255, 255],
            "cyan takes the red"
        );
        assert_eq!(
            ink(0.0, 1.0, 0.0, 0.0),
            [255, 0, 255, 255],
            "magenta takes the green"
        );
        assert_eq!(
            ink(0.0, 0.0, 1.0, 0.0),
            [255, 255, 0, 255],
            "yellow takes the blue"
        );
        // Two at a time: only the light neither absorbs is left.
        assert_eq!(
            ink(1.0, 1.0, 0.0, 0.0),
            [0, 0, 255, 255],
            "cyan and magenta leave blue"
        );
        assert_eq!(
            ink(1.0, 0.0, 1.0, 0.0),
            [0, 255, 0, 255],
            "cyan and yellow leave green"
        );
        assert_eq!(
            ink(0.0, 1.0, 1.0, 0.0),
            [255, 0, 0, 255],
            "magenta and yellow leave red"
        );
        // No ink at all is paper.
        assert_eq!(ink(0.0, 0.0, 0.0, 0.0), [255, 255, 255, 255], "bare paper");
        // And a half-strength ink is partway there, on its own channel
        // only — which says the inks do not leak into one another.
        let half = ink(0.5, 0.0, 0.0, 0.0);
        assert!(
            half[0] > 100 && half[0] < 160,
            "half a cyan takes half the red: {half:?}"
        );
        assert_eq!(&half[1..3], &[255, 255], "and leaves the other two alone");
    }

    #[test]
    fn cmyk_black_maps_to_black() {
        let px = to_working(&AuthoredColor::Cmyk {
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 1.0,
            a: 1.0,
        });
        assert_eq!(px.to_srgb8(), [0, 0, 0, 255]);
    }

    /// White comes back white, and black black.
    ///
    /// The curve is continuous and its ends are exact, which sounds like
    /// nothing until something branches on them. `1.055 * 1 - 0.055` is
    /// 0.999_999_94 in f32, so white encoded and read back was a rounding
    /// short of white — and colour burn asks `is the backdrop one` before
    /// it asks anything else. White paper under a black layer set to
    /// colour burn is white by the compositing spec and came out black.
    #[test]
    fn the_transfer_curve_is_exact_at_both_ends() {
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert_eq!(linear_to_srgb(1.0), 1.0, "white encodes to white");
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert_eq!(srgb_to_linear(1.0), 1.0, "and reads back as white");
        // And round trips at both ends, which is the thing a reader
        // branching on "is this the top" actually depends on.
        assert_eq!(srgb_to_linear(linear_to_srgb(1.0)), 1.0);
        assert_eq!(linear_to_srgb(srgb_to_linear(1.0)), 1.0);
        // Still the curve it was in between: a middling value is
        // unchanged to the last place it had before.
        assert!((linear_to_srgb(0.5) - 0.735_356_9).abs() < 1e-6);
        assert!((linear_to_srgb(0.2) - 0.484_529_2).abs() < 1e-6);
        // And monotone up to the end, so nothing was flattened to get
        // the last point right.
        let mut last = -1.0f32;
        for i in 0..=2048 {
            let v = linear_to_srgb(i as f32 / 2048.0);
            assert!(v > last, "the curve still climbs at {i}");
            last = v;
        }
    }
}
