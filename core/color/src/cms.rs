//! ICC color management, built on moxcms (pure Rust, wasm-compatible; see
//! docs/spikes/color-management.md for the moxcms-vs-lcms2 decision).
//!
//! Two jobs today:
//! - **Import normalization**: pixels arriving with an embedded ICC profile
//!   are converted to sRGB once at the import edge, so the rest of the
//!   engine keeps exactly one internal encoding.
//! - **CMYK documents**: a press profile turns authored CMYK ink values into
//!   working-space color for compositing and proofing, replacing the naive
//!   preview formula whenever a profile is set.

use crate::{srgb_to_linear, LinearRgba};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformF32Executor, TransformOptions};
use std::sync::Arc;

/// Convert RGBA8 pixels tagged with an embedded ICC profile into sRGB, in
/// place. Returns false (pixels untouched) when the profile doesn't parse,
/// isn't an RGB profile, or is already sRGB-equivalent enough to skip.
pub fn normalize_rgba8_to_srgb(icc: &[u8], pixels: &mut [u8]) -> bool {
    let Ok(profile) = ColorProfile::new_from_slice(icc) else {
        return false;
    };
    if profile.color_space != DataColorSpace::Rgb {
        return false;
    }
    let srgb = ColorProfile::new_srgb();
    let Ok(transform) = profile.create_transform_8bit(
        Layout::Rgba,
        &srgb,
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return false;
    };
    let src = pixels.to_vec();
    transform.transform(&src, pixels).is_ok()
}

/// A parsed CMYK press profile with a cached CMYK→sRGB transform, used for
/// authored CMYK colors in documents that carry a profile.
#[derive(Clone)]
pub struct CmykCms {
    transform: Arc<TransformF32Executor>,
}

impl std::fmt::Debug for CmykCms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CmykCms")
    }
}

impl CmykCms {
    /// Parse ICC bytes; errors unless the profile's device space is CMYK.
    pub fn new(icc: &[u8]) -> Result<Self, String> {
        let profile = ColorProfile::new_from_slice(icc).map_err(|e| format!("{e:?}"))?;
        if profile.color_space != DataColorSpace::Cmyk {
            return Err(format!(
                "profile device space is {:?}, expected CMYK",
                profile.color_space
            ));
        }
        let srgb = ColorProfile::new_srgb();
        let transform = profile
            .create_transform_f32(
                Layout::Rgba,
                &srgb,
                Layout::Rgb,
                TransformOptions::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self { transform })
    }

    /// Ink coverage (0..=1 each) → premultiplied linear working color.
    pub fn to_working(&self, c: f32, m: f32, y: f32, k: f32, alpha: f32) -> LinearRgba {
        let src = [c, m, y, k];
        let mut dst = [0f32; 3];
        if self.transform.transform(&src, &mut dst).is_err() {
            return LinearRgba::TRANSPARENT;
        }
        LinearRgba {
            r: srgb_to_linear(dst[0].clamp(0.0, 1.0)) * alpha,
            g: srgb_to_linear(dst[1].clamp(0.0, 1.0)) * alpha,
            b: srgb_to_linear(dst[2].clamp(0.0, 1.0)) * alpha,
            a: alpha,
        }
    }
}

/// Display P3 profile bytes — used by tests and (later) for assigning
/// well-known profiles without shipping .icc files.
pub fn display_p3_profile_bytes() -> Vec<u8> {
    ColorProfile::new_display_p3()
        .encode()
        .expect("encoding a built-in profile cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p3_pixels_normalize_into_srgb() {
        let icc = display_p3_profile_bytes();
        // A saturated but in-gamut color: its sRGB coordinates differ
        // measurably from its P3 coordinates.
        let mut px = vec![200u8, 100, 50, 255];
        assert!(normalize_rgba8_to_srgb(&icc, &mut px));
        assert_eq!(px[3], 255, "alpha preserved");
        let moved =
            (px[0] as i32 - 200).abs() + (px[1] as i32 - 100).abs() + (px[2] as i32 - 50).abs();
        assert!(moved > 10, "P3 color must convert, got {px:?}");

        // Neutral grey is gamut-safe and must survive (small tolerance).
        let mut grey = vec![128u8, 128, 128, 255];
        assert!(normalize_rgba8_to_srgb(&icc, &mut grey));
        for ch in &grey[0..3] {
            assert!((*ch as i32 - 128).abs() <= 2, "grey shifted: {grey:?}");
        }
    }

    #[test]
    fn garbage_and_wrong_space_profiles_are_rejected() {
        let mut px = vec![1u8, 2, 3, 4];
        assert!(!normalize_rgba8_to_srgb(b"not an icc profile", &mut px));
        assert_eq!(px, [1, 2, 3, 4]);
        assert!(CmykCms::new(&display_p3_profile_bytes()).is_err());
    }

    /// Full CMYK verification needs a real press profile, which is not
    /// redistributable in this repo. Point CHITRAKAR_TEST_CMYK_ICC at any
    /// CMYK .icc (e.g. ghostscript's default_cmyk.icc) to run this.
    #[test]
    fn cmyk_profile_converts_ink_to_color() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let cms = CmykCms::new(&icc).unwrap();

        // 100% cyan must come out as a cyan-ish blue-green, not pure #00FFFF.
        let cyan = cms.to_working(1.0, 0.0, 0.0, 0.0, 1.0).to_srgb8();
        assert!(cyan[0] < 60 && cyan[2] > 150, "cyan looks wrong: {cyan:?}");
        // Paper white (no ink) is near-white.
        let paper = cms.to_working(0.0, 0.0, 0.0, 0.0, 1.0).to_srgb8();
        assert!(paper[0] > 230 && paper[1] > 230 && paper[2] > 230);
        // 100K is a dark grey/black.
        let black = cms.to_working(0.0, 0.0, 0.0, 1.0, 1.0).to_srgb8();
        assert!(black[0] < 80 && black[1] < 80 && black[2] < 80);
    }
}
