//! Color foundations for Chitrakar.
//!
//! The working pixel format everywhere in the engine is 32-bit float,
//! premultiplied alpha, linear light ([`LinearRgba`]). Conversion to and from
//! encoded spaces (sRGB now; ICC-profile-driven transforms in Phase 3) happens
//! only at the pipeline edges: import, display, and export.

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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
    pub fn to_srgb8(self) -> [u8; 4] {
        let unpremul = |v: f32| if self.a > 0.0 { v / self.a } else { 0.0 };
        let enc = |v: f32| (linear_to_srgb(unpremul(v).clamp(0.0, 1.0)) * 255.0).round() as u8;
        [
            enc(self.r),
            enc(self.g),
            enc(self.b),
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
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
pub fn to_working(color: AuthoredColor) -> LinearRgba {
    match color {
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

    #[test]
    fn cmyk_black_maps_to_black() {
        let px = to_working(AuthoredColor::Cmyk {
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 1.0,
            a: 1.0,
        });
        assert_eq!(px.to_srgb8(), [0, 0, 0, 255]);
    }
}
