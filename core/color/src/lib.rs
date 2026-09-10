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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LinearRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
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
        let alpha = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
        if alpha == 0 {
            return [0, 0, 0, 0];
        }
        let unpremul = |v: f32| v / self.a;
        let enc = |v: f32| (linear_to_srgb(unpremul(v).clamp(0.0, 1.0)) * 255.0).round() as u8;
        [enc(self.r), enc(self.g), enc(self.b), alpha]
    }
}

/// sRGB EOTF inverse: encoded 0..=1 → linear 0..=1.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear 0..=1 → sRGB-encoded 0..=1.
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
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
}
