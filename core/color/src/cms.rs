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
use moxcms::{
    curve_from_gamma, Chromaticity, ColorPrimaries, ColorProfile, DataColorSpace, Layout,
    TransformF32Executor, TransformOptions, XyY,
};
use std::sync::Arc;

/// Convert RGBA8 pixels tagged with an embedded ICC profile into sRGB, in
/// place. Returns false (pixels untouched) when the profile doesn't parse,
/// isn't an RGB profile, or is already sRGB-equivalent enough to skip.
pub fn normalize_rgba8_to_srgb(icc: &[u8], pixels: &mut [u8]) -> bool {
    let Ok(profile) = ColorProfile::new_from_slice(icc) else {
        return false;
    };
    normalize_rgba8_from_profile(&profile, pixels)
}

/// A colour space named by its primaries and a plain gamma rather than by
/// a profile: what a PNG says in its cHRM and gAMA chunks when it carries
/// no iCCP. Chromaticities are CIE xy; `gamma` is the encoding's exponent
/// as the file states it (a gAMA of 1/1.8 arrives as `1.0 / 1.8`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chromaticities {
    pub white: (f32, f32),
    pub red: (f32, f32),
    pub green: (f32, f32),
    pub blue: (f32, f32),
    pub gamma: f32,
}

impl Chromaticities {
    /// Whether these say sRGB, near enough that converting would only
    /// move pixels by the difference between a pure 2.2 gamma and sRGB's
    /// own curve — a change to a file that was never asking for one.
    /// The tolerances are a few units in the last place of what the
    /// chunks can store: xy comes in hundred-thousandths and gamma in
    /// hundred-thousandths too, and writers round differently.
    pub fn is_srgb(&self) -> bool {
        const SRGB: [(f32, f32); 4] = [(0.3127, 0.3290), (0.64, 0.33), (0.30, 0.60), (0.15, 0.06)];
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 0.001 && (a.1 - b.1).abs() < 0.001;
        near(self.white, SRGB[0])
            && near(self.red, SRGB[1])
            && near(self.green, SRGB[2])
            && near(self.blue, SRGB[3])
            && (self.gamma - 1.0 / 2.2).abs() < 0.001
    }

    /// The matrix/TRC profile these describe. `None` when the numbers
    /// cannot make one: a gamma that is not positive, or primaries that
    /// do not span a gamut (three points on a line) and so have no
    /// matrix to XYZ.
    fn profile(&self) -> Option<ColorProfile> {
        if !self.gamma.is_finite() || self.gamma <= 0.0 {
            return None;
        }
        let finite = |c: (f32, f32)| c.0.is_finite() && c.1.is_finite() && c.1 > 0.0;
        if !(finite(self.white) && finite(self.red) && finite(self.green) && finite(self.blue)) {
            return None;
        }
        // Three primaries on a line enclose nothing; the matrix that
        // would take them to XYZ has no inverse, and what comes back
        // from asking for one anyway is a number rather than an error.
        let (r, g, b) = (self.red, self.green, self.blue);
        let twice_area = (r.0 - b.0) * (g.1 - b.1) - (g.0 - b.0) * (r.1 - b.1);
        if twice_area.abs() < 1e-4 {
            return None;
        }
        let white = XyY {
            x: f64::from(self.white.0),
            y: f64::from(self.white.1),
            yb: 1.0,
        };
        let primaries = ColorPrimaries {
            red: Chromaticity {
                x: self.red.0,
                y: self.red.1,
            },
            green: Chromaticity {
                x: self.green.0,
                y: self.green.1,
            },
            blue: Chromaticity {
                x: self.blue.0,
                y: self.blue.1,
            },
        };
        // Start from the built-in sRGB profile for its header — an RGB
        // display profile with an XYZ connection space — and replace
        // everything that made it sRGB. Setting the colorimetry adapts
        // the primaries to the D50 connection space and drops the CICP
        // tag, which matters: with it left in, the engine would read
        // sRGB's transfer curve off it and ignore the gamma.
        let mut profile = ColorProfile::new_srgb();
        profile.description = None;
        profile.copyright = None;
        profile.update_rgb_colorimetry(white, primaries);
        profile.media_white_point = Some(white.to_xyzd());
        // A matrix profile's three colorants add up to its white, whose
        // Y is one by definition: anything else is a matrix that came out
        // of a degenerate inverse rather than a colour space.
        let colorants = [
            profile.red_colorant,
            profile.green_colorant,
            profile.blue_colorant,
        ];
        let white_y: f64 = colorants.iter().map(|c| c.y).sum();
        let sane = colorants
            .iter()
            .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite())
            && (white_y - 1.0).abs() < 0.01;
        if !sane {
            return None;
        }
        let curve = curve_from_gamma(1.0 / self.gamma);
        profile.red_trc = Some(curve.clone());
        profile.green_trc = Some(curve.clone());
        profile.blue_trc = Some(curve);
        Some(profile)
    }
}

/// Convert RGBA8 pixels whose colour space is given as primaries and a
/// gamma into sRGB, in place. Returns false (pixels untouched) when the
/// numbers already say sRGB or cannot describe a colour space at all.
///
/// This is the same conversion an embedded profile gets, built from the
/// chunks a PNG writer puts down when it tags a picture without one: a
/// ProPhoto PNG saying so through cHRM and gAMA used to arrive as its
/// raw numbers and show muted and dark, every colour read as sRGB.
pub fn normalize_rgba8_from_chromaticities(space: &Chromaticities, pixels: &mut [u8]) -> bool {
    if space.is_srgb() {
        return false;
    }
    let Some(profile) = space.profile() else {
        return false;
    };
    normalize_rgba8_from_profile(&profile, pixels)
}

fn normalize_rgba8_from_profile(profile: &ColorProfile, pixels: &mut [u8]) -> bool {
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

/// A monitor's own profile, as the transform the screen wants: sRGB in,
/// that display's numbers out.
///
/// Everything in the engine is sRGB by the time it is presented, and a
/// screen that is not sRGB will show those numbers as its own — a
/// wide-gamut display draws sRGB's red at its own red, which is a good
/// deal further out. Converting here is what makes a picture look the
/// same on that screen as on a plain one; a display already sRGB needs
/// none of it.
///
/// A view setting rather than a document one: it belongs to the machine
/// the document is being looked at on, so it is never saved with the
/// file and never applied to what is exported.
#[derive(Clone)]
pub struct DisplayCms {
    transform: Arc<moxcms::Transform8BitExecutor>,
}

impl std::fmt::Debug for DisplayCms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DisplayCms")
    }
}

impl DisplayCms {
    /// Parse ICC bytes; errors unless the profile's device space is RGB,
    /// since a monitor is an RGB device.
    pub fn new(icc: &[u8]) -> Result<Self, String> {
        let profile = ColorProfile::new_from_slice(icc).map_err(|e| format!("{e:?}"))?;
        if profile.color_space != DataColorSpace::Rgb {
            return Err(format!(
                "profile device space is {:?}, expected RGB",
                profile.color_space
            ));
        }
        let srgb = ColorProfile::new_srgb();
        let transform = srgb
            .create_transform_8bit(
                Layout::Rgba,
                &profile,
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self { transform })
    }

    /// Take presented sRGB pixels to the display's own numbers, in place.
    /// Alpha rides through untouched. Pixels are left alone rather than
    /// corrupted if the transform refuses them.
    pub fn to_display_rgba8(&self, pixels: &mut [u8]) {
        let src = pixels.to_vec();
        let _ = self.transform.transform(&src, pixels);
    }
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

/// Soft-proofing transform: round-trips display pixels through a CMYK press
/// profile (sRGB → press → sRGB) so the screen shows what the press can
/// actually reproduce, with optional out-of-gamut marking.
#[derive(Clone)]
pub struct ProofCms {
    to_press: Arc<TransformF32Executor>,
    from_press: Arc<TransformF32Executor>,
}

impl std::fmt::Debug for ProofCms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProofCms")
    }
}

/// Channel delta (0..=1) beyond which a round-tripped pixel counts as
/// out-of-gamut for the warning overlay.
const GAMUT_TOLERANCE: f32 = 0.04;
/// Neutral grey painted over out-of-gamut pixels (Photoshop convention).
const GAMUT_MARK: [u8; 3] = [128, 128, 128];

impl ProofCms {
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
        let to_press = srgb
            .create_transform_f32(
                Layout::Rgb,
                &profile,
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        let from_press = profile
            .create_transform_f32(
                Layout::Rgba,
                &srgb,
                Layout::Rgb,
                TransformOptions::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            to_press,
            from_press,
        })
    }

    /// Proof RGBA8 pixels in place. With `gamut_warn`, pixels whose
    /// round-trip moves more than the tolerance are painted neutral grey
    /// instead. Alpha is untouched.
    pub fn proof_rgba8(&self, pixels: &mut [u8], gamut_warn: bool) {
        // Chunked so temporaries stay small on big frames.
        const CHUNK: usize = 16 * 1024;
        let mut rgb = Vec::with_capacity(CHUNK * 3);
        let mut press = vec![0f32; CHUNK * 4];
        let mut back = vec![0f32; CHUNK * 3];
        for chunk in pixels.chunks_mut(CHUNK * 4) {
            let n = chunk.len() / 4;
            rgb.clear();
            for px in chunk.as_chunks::<4>().0 {
                rgb.extend_from_slice(&[
                    px[0] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[2] as f32 / 255.0,
                ]);
            }
            if self
                .to_press
                .transform(&rgb, &mut press[..n * 4])
                .and_then(|_| {
                    self.from_press
                        .transform(&press[..n * 4], &mut back[..n * 3])
                })
                .is_err()
            {
                return; // leave pixels unproofed rather than corrupt them
            }
            for (i, px) in chunk.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let proofed = &back[i * 3..i * 3 + 3];
                let out_of_gamut =
                    (0..3).any(|k| (proofed[k] - rgb[i * 3 + k]).abs() > GAMUT_TOLERANCE);
                if gamut_warn && out_of_gamut {
                    px[0..3].copy_from_slice(&GAMUT_MARK);
                } else {
                    for k in 0..3 {
                        px[k] = (proofed[k].clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                }
            }
        }
    }
}

/// sRGB → CMYK separation through a press profile: the transform print
/// export needs, turning composite color into the ink values a press lays
/// down.
#[derive(Clone)]
pub struct RgbToCmyk {
    transform: Arc<TransformF32Executor>,
}

impl std::fmt::Debug for RgbToCmyk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RgbToCmyk")
    }
}

impl RgbToCmyk {
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
        let transform = srgb
            .create_transform_f32(
                Layout::Rgb,
                &profile,
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self { transform })
    }

    /// Separate non-linear sRGB triples (0..=1) into 8-bit CMYK quads.
    pub fn separate(&self, srgb: &[f32]) -> Result<Vec<u8>, String> {
        let pixels = srgb.len() / 3;
        let mut ink = vec![0f32; pixels * 4];
        self.transform
            .transform(srgb, &mut ink)
            .map_err(|e| format!("{e:?}"))?;
        Ok(ink
            .into_iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect())
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

    /// ProPhoto RGB (200,100,50) is (255,80,47) in sRGB — the number
    /// littlecms gives for it through seven ProPhoto profiles from the
    /// wild (Kodak-style v2 curves, ISO parametric v4, colord's, the
    /// LUT-based ISO 22028-2 one), and what the two primary matrices
    /// give by hand. ProPhoto's primaries are so far outside sRGB's
    /// that the red clips and the green and blue drop by more than
    /// half; mid grey rises from 128 to 146 because its gamma is 1.8
    /// against sRGB's roughly 2.2. Shown untouched, the same picture
    /// is muted and dark: that is what an ignored profile looks like.
    const PRO_PHOTO: Chromaticities = Chromaticities {
        white: (0.3457, 0.3585),
        red: (0.7347, 0.2653),
        green: (0.1596, 0.8404),
        blue: (0.0366, 0.0001),
        gamma: 1.0 / 1.8,
    };
    const PRO_PHOTO_PIXELS: [u8; 12] = [200, 100, 50, 255, 128, 128, 128, 255, 30, 30, 30, 255];
    const PRO_PHOTO_IN_SRGB: [u8; 12] = [255, 80, 47, 255, 146, 146, 146, 255, 40, 40, 40, 255];

    fn assert_pro_photo_converted(px: &[u8]) {
        for (i, (got, want)) in px.iter().zip(PRO_PHOTO_IN_SRGB).enumerate() {
            assert!(
                (*got as i32 - want as i32).abs() <= 1,
                "channel {i}: want {want}, got {got} (whole run {px:?})"
            );
        }
    }

    #[test]
    fn pro_photo_pixels_normalize_into_srgb_through_a_profile() {
        let icc = ColorProfile::new_pro_photo_rgb().encode().unwrap();
        let mut px = PRO_PHOTO_PIXELS.to_vec();
        assert!(normalize_rgba8_to_srgb(&icc, &mut px));
        assert_pro_photo_converted(&px);
    }

    #[test]
    fn pro_photo_pixels_normalize_into_srgb_through_their_chromaticities() {
        let mut px = PRO_PHOTO_PIXELS.to_vec();
        assert!(normalize_rgba8_from_chromaticities(&PRO_PHOTO, &mut px));
        assert_pro_photo_converted(&px);
    }

    /// Chunks that say sRGB — as libpng writes beside every sRGB
    /// picture — are not a conversion, and a pure 2.2 gamma stood in
    /// for sRGB's curve would move the shadows of a file that asked
    /// for nothing.
    #[test]
    fn chromaticities_that_say_srgb_leave_pixels_alone() {
        let srgb = Chromaticities {
            white: (0.3127, 0.329),
            red: (0.64, 0.33),
            green: (0.3, 0.6),
            blue: (0.15, 0.06),
            gamma: 0.45455,
        };
        assert!(srgb.is_srgb());
        let mut px = vec![200u8, 100, 50, 255, 3, 2, 1, 255];
        assert!(!normalize_rgba8_from_chromaticities(&srgb, &mut px));
        assert_eq!(px, [200, 100, 50, 255, 3, 2, 1, 255]);
        assert!(!PRO_PHOTO.is_srgb());
    }

    /// A space named by its chunks lands where a profile saying the same
    /// thing lands it. Display P3's primaries with a plain 2.2 gamma,
    /// built both ways — the white is D65 here, so this is also the
    /// adaptation to the profile connection space agreeing with itself.
    /// (Against the real P3 profile the blue would sit eight levels
    /// off, which is what its sRGB-shaped curve costs against a pure
    /// gamma in the shadows: the chunks say a gamma, so a gamma is what
    /// they get.)
    #[test]
    fn chromaticities_agree_with_the_profile_they_describe() {
        let p3 = Chromaticities {
            white: (0.3127, 0.329),
            red: (0.68, 0.32),
            green: (0.265, 0.69),
            blue: (0.15, 0.06),
            gamma: 1.0 / 2.2,
        };
        let mut by_chunks = vec![200u8, 100, 50, 255, 20, 10, 5, 255];
        assert!(normalize_rgba8_from_chromaticities(&p3, &mut by_chunks));

        let mut profile = ColorProfile::new_display_p3();
        profile.cicp = None;
        let curve = curve_from_gamma(2.2);
        profile.red_trc = Some(curve.clone());
        profile.green_trc = Some(curve.clone());
        profile.blue_trc = Some(curve);
        let mut by_profile = vec![200u8, 100, 50, 255, 20, 10, 5, 255];
        assert!(normalize_rgba8_to_srgb(
            &profile.encode().unwrap(),
            &mut by_profile
        ));
        for c in 0..8 {
            assert!(
                (by_chunks[c] as i32 - by_profile[c] as i32).abs() <= 1,
                "{by_chunks:?} vs {by_profile:?}"
            );
        }
        // And it did convert: P3's red is further out than sRGB's.
        assert!(by_chunks[0] > 205, "{by_chunks:?}");
    }

    /// Numbers that describe no colour space are refused rather than
    /// turned into a transform full of NaNs.
    #[test]
    fn degenerate_chromaticities_are_refused() {
        let mut px = vec![200u8, 100, 50, 255];
        let flat = Chromaticities {
            red: (0.5, 0.5),
            green: (0.5, 0.5),
            blue: (0.5, 0.5),
            ..PRO_PHOTO
        };
        assert!(!normalize_rgba8_from_chromaticities(&flat, &mut px));
        let no_gamma = Chromaticities {
            gamma: 0.0,
            ..PRO_PHOTO
        };
        assert!(!normalize_rgba8_from_chromaticities(&no_gamma, &mut px));
        let nan = Chromaticities {
            white: (f32::NAN, 0.3),
            ..PRO_PHOTO
        };
        assert!(!normalize_rgba8_from_chromaticities(&nan, &mut px));
        assert_eq!(px, [200, 100, 50, 255]);
    }

    #[test]
    fn garbage_and_wrong_space_profiles_are_rejected() {
        let mut px = vec![1u8, 2, 3, 4];
        assert!(!normalize_rgba8_to_srgb(b"not an icc profile", &mut px));
        assert_eq!(px, [1, 2, 3, 4]);
        assert!(CmykCms::new(&display_p3_profile_bytes()).is_err());
    }

    /// Needs a real press profile (CHITRAKAR_TEST_CMYK_ICC), like the other
    /// CMYK tests.
    #[test]
    fn soft_proof_clamps_saturated_colors_and_marks_gamut() {
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped: set CHITRAKAR_TEST_CMYK_ICC to run");
            return;
        };
        let icc = std::fs::read(path).unwrap();
        let proof = ProofCms::new(&icc).unwrap();

        // Saturated blue is far outside a press gamut; near-white paper
        // tone is reproducible.
        let mut px = vec![0u8, 0, 255, 255, /**/ 240, 240, 238, 255];
        proof.proof_rgba8(&mut px, false);
        assert_ne!(&px[0..3], &[0, 0, 255], "saturated blue must shift");
        assert_eq!(px[3], 255, "alpha untouched");
        let paper_delta: i32 = (px[4] as i32 - 240).abs() + (px[5] as i32 - 240).abs();
        assert!(paper_delta < 30, "printable tone survives: {:?}", &px[4..7]);

        // Gamut warning paints the unprintable pixel grey.
        let mut px = vec![0u8, 0, 255, 255];
        proof.proof_rgba8(&mut px, true);
        assert_eq!(&px[0..3], &[128, 128, 128], "gamut mark applied");

        assert!(ProofCms::new(&display_p3_profile_bytes()).is_err());
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
