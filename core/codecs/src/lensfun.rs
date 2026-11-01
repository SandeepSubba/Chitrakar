//! Lens corrections from lensfun's database, for a photograph whose file
//! carries none of its own (a Canon's CR2 or CR3, a Nikon's NEF): the
//! lens and camera it names, looked up in XML files that whoever is
//! using the editor has, and made into the same [`Lens`] a Fuji's RAF
//! gives, so it is developed by the same steps.
//!
//! The database is CC BY-SA 3.0, which a GPL-3.0 program cannot carry;
//! nothing of it is shipped. It is read from where the person put it
//! (the desktop shell also looks where a system keeps it).
//!
//! What a profile means follows lensfun's own library (`modifier.cpp`,
//! `mod-coord.cpp`, `mod-subpix.cpp`, `mod-color.cpp`, `lens.cpp`):
//!
//! - Distortion (`ptlens`, `poly3`, `poly5`) takes an undistorted radius
//!   `ru` to where it is on the sensor, `rd = ru · f(ru)`, with r = 1 at
//!   half the shorter side of the calibration's frame. Divided through
//!   by `f(0)`, so the middle of the picture keeps its size.
//! - Colour fringes (`linear`, `poly3`) take green's radius on the sensor
//!   to red's and blue's, in the same units.
//! - Vignetting (`pa`) is the light reaching a radius as a fraction of
//!   the middle's, `1 + k1 r² + k2 r⁴ + k3 r⁶`, with r = 1 at the
//!   calibration frame's corner.
//! - Between calibrated focal lengths, distortion and fringes are a
//!   Hermite spline through the two nearest on each side; vignetting is
//!   weighted by inverse distance over focal length, aperture and focus
//!   distance.
//!
//! A frame's size in millimetres comes from its crop factor (a 36 × 24
//! frame's diagonal over it), the photograph's from its camera's entry
//! in the database, or the lens's own calibration where the camera is
//! not there.

use crate::develop::Lens;

/// What a photograph's file says of how it was taken, as far as finding
/// its lens goes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shot {
    pub camera_make: String,
    pub camera_model: String,
    pub lens_make: String,
    pub lens_model: String,
    /// Millimetres; nought where the file does not say.
    pub focal: f32,
    /// The f-number; nought where the file does not say.
    pub aperture: f32,
    /// Metres to what was focused on; nought where the file does not say.
    pub distance: f32,
}

/// One calibrated focal length of a distortion or fringe model.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Calibrated {
    focal: f32,
    model: Model,
    terms: [f32; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Model {
    PtLens,
    Poly3,
    Poly5,
    Linear,
    TcaPoly3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Vignetting {
    focal: f32,
    aperture: f32,
    distance: f32,
    k: [f32; 3],
}

/// A lens's entry, one calibration set of it.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    maker: String,
    /// The first model name, as shown; every name, normalised, to match.
    name: String,
    names: Vec<String>,
    crop: f32,
    aspect: f32,
    focal_range: (f32, f32),
    distortion: Vec<Calibrated>,
    tca: Vec<Calibrated>,
    vignetting: Vec<Vignetting>,
}

#[derive(Debug, Clone, PartialEq)]
struct Camera {
    names: Vec<String>,
    crop: f32,
}

/// Lens profiles read from lensfun's XML files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profiles {
    lenses: Vec<Entry>,
    cameras: Vec<Camera>,
}

/// A lens profile matched to a photograph: the lens's name as the
/// database has it, and its corrections at the photograph's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Matched {
    pub name: String,
    pub lens: Lens,
}

/// A name with only what tells names apart: letters and digits, lower
/// case, and the points of a focal ratio. Canon's EXIF says
/// "EF-S 18-55mm f/4-5.6 IS STM" and lensfun "EF-S18-55mm f/4-5.6 IS STM".
fn normal(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '.')
        .flat_map(char::to_lowercase)
        .collect()
}

/// A name without its maker in front, if it starts with it.
fn without_maker(name: &str, maker: &str) -> String {
    let (n, m) = (normal(name), normal(maker));
    match n.strip_prefix(&m) {
        Some(rest) if !m.is_empty() && !rest.is_empty() => rest.to_string(),
        _ => n,
    }
}

/// "3:2", "4:3", or a number.
fn aspect(s: &str) -> Option<f32> {
    match s.split_once(':') {
        Some((a, b)) => {
            let (a, b) = (a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?);
            (a > 0.0 && b > 0.0).then(|| a.max(b) / a.min(b))
        }
        None => s.trim().parse().ok().filter(|v: &f32| *v > 0.0),
    }
}

impl Profiles {
    /// Add the lenses and cameras of one of lensfun's XML files; how many
    /// lenses with any correction were in it. A file that is not one is
    /// an error, and adds nothing.
    pub fn add(&mut self, xml: &str) -> Result<usize, String> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| format!("lens profiles: {e}"))?;
        let root = doc.root_element();
        if root.tag_name().name() != "lensdatabase" {
            return Err("lens profiles: not a lensfun database".into());
        }
        let text = |n: roxmltree::Node, tag: &str| {
            n.children()
                .find(|c| c.has_tag_name(tag) && c.attribute("lang").is_none())
                .and_then(|c| c.text())
                .map(|t| t.trim().to_string())
        };
        let names = |n: roxmltree::Node, maker: &str| -> Vec<String> {
            let mut v: Vec<String> = n
                .children()
                .filter(|c| c.has_tag_name("model"))
                .filter_map(|c| c.text())
                .map(|t| without_maker(t, maker))
                .collect();
            v.dedup();
            v
        };
        let crop_of = |n: roxmltree::Node| {
            text(n, "cropfactor")
                .and_then(|t| t.parse::<f32>().ok())
                .filter(|c| *c > 0.0)
        };
        for cam in root.children().filter(|c| c.has_tag_name("camera")) {
            let maker = text(cam, "maker").unwrap_or_default();
            if let Some(crop) = crop_of(cam) {
                let mut names = names(cam, &maker);
                names.extend(
                    cam.children()
                        .filter(|c| c.has_tag_name("model"))
                        .filter_map(|c| c.text())
                        .map(normal),
                );
                self.cameras.push(Camera { names, crop });
            }
        }
        let mut added = 0;
        let f =
            |n: roxmltree::Node, a: &str| n.attribute(a).and_then(|v| v.trim().parse::<f32>().ok());
        for lens in root.children().filter(|c| c.has_tag_name("lens")) {
            // Fisheyes and panoramic lenses are not straightened to a
            // rectilinear picture by these models alone.
            if text(lens, "type").is_some_and(|t| t != "rectilinear") {
                continue;
            }
            let maker = text(lens, "maker").unwrap_or_default();
            let names = names(lens, &maker);
            let Some(name) = text(lens, "model") else {
                continue;
            };
            let crop = crop_of(lens).unwrap_or(1.0);
            let lens_aspect = text(lens, "aspect-ratio")
                .and_then(|t| aspect(&t))
                .unwrap_or(1.5);
            let range = lens
                .children()
                .find(|c| c.has_tag_name("focal"))
                .map(|c| (f(c, "min"), f(c, "max"), f(c, "value")));
            for calib in lens.children().filter(|c| c.has_tag_name("calibration")) {
                let mut e = Entry {
                    maker: maker.clone(),
                    name: name.clone(),
                    names: names.clone(),
                    crop: f(calib, "cropfactor").unwrap_or(crop),
                    aspect: calib
                        .attribute("aspect-ratio")
                        .and_then(aspect)
                        .unwrap_or(lens_aspect),
                    focal_range: (f32::MAX, 0.0),
                    distortion: Vec::new(),
                    tca: Vec::new(),
                    vignetting: Vec::new(),
                };
                for c in calib.children().filter(|c| c.is_element()) {
                    let Some(focal) = f(c, "focal").filter(|v| *v > 0.0) else {
                        continue;
                    };
                    let t = |names: &[&str]| -> Option<[f32; 6]> {
                        let mut out = [0f32; 6];
                        for (slot, n) in out.iter_mut().zip(names) {
                            *slot = match f(c, n) {
                                Some(v) if v.is_finite() => v,
                                None if !n.starts_with('v') && !n.starts_with('k') => 0.0,
                                _ => return None,
                            };
                        }
                        Some(out)
                    };
                    let model = c.attribute("model").unwrap_or("");
                    let got = match (c.tag_name().name(), model) {
                        ("distortion", "ptlens") => {
                            t(&["a", "b", "c"]).map(|t| (true, Model::PtLens, t))
                        }
                        ("distortion", "poly3") => t(&["k1"]).map(|t| (true, Model::Poly3, t)),
                        ("distortion", "poly5") => {
                            t(&["k1", "k2"]).map(|t| (true, Model::Poly5, t))
                        }
                        ("tca", "linear") => t(&["kr", "kb"]).map(|t| (false, Model::Linear, t)),
                        ("tca", "poly3") => t(&["vr", "vb", "cr", "cb", "br", "bb"])
                            .map(|t| (false, Model::TcaPoly3, t)),
                        ("vignetting", "pa") => {
                            if let (Some(aperture), Some(k1), Some(k2), Some(k3)) =
                                (f(c, "aperture"), f(c, "k1"), f(c, "k2"), f(c, "k3"))
                            {
                                e.vignetting.push(Vignetting {
                                    focal,
                                    aperture,
                                    distance: f(c, "distance").unwrap_or(1000.0),
                                    k: [k1, k2, k3],
                                });
                                e.focal_range.0 = e.focal_range.0.min(focal);
                                e.focal_range.1 = e.focal_range.1.max(focal);
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some((distortion, model, terms)) = got {
                        let into = if distortion {
                            &mut e.distortion
                        } else {
                            &mut e.tca
                        };
                        // lensfun takes the first model a lens gives and
                        // passes over any other.
                        if into.first().is_none_or(|first| first.model == model) {
                            into.push(Calibrated {
                                focal,
                                model,
                                terms,
                            });
                        }
                        e.focal_range.0 = e.focal_range.0.min(focal);
                        e.focal_range.1 = e.focal_range.1.max(focal);
                    }
                }
                if let Some((min, max, value)) = range {
                    if let Some(v) = min.or(value) {
                        e.focal_range.0 = e.focal_range.0.min(v);
                    }
                    if let Some(v) = max.or(value) {
                        e.focal_range.1 = e.focal_range.1.max(v);
                    }
                }
                if !(e.distortion.is_empty() && e.tca.is_empty() && e.vignetting.is_empty()) {
                    for list in [&mut e.distortion, &mut e.tca] {
                        list.sort_by(|a, b| a.focal.total_cmp(&b.focal));
                    }
                    self.lenses.push(e);
                    added += 1;
                }
            }
        }
        Ok(added)
    }

    /// How many lens calibrations are held.
    pub fn len(&self) -> usize {
        self.lenses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lenses.is_empty()
    }

    /// The camera's crop factor, if the database has the camera.
    fn crop_of(&self, shot: &Shot) -> Option<f32> {
        let full = normal(&format!("{} {}", shot.camera_make, shot.camera_model));
        let bare = without_maker(&shot.camera_model, &shot.camera_make);
        self.cameras
            .iter()
            .find(|c| c.names.iter().any(|n| *n == full || *n == bare))
            .map(|c| c.crop)
    }

    /// The corrections for the lens a photograph was taken with, at its
    /// focal length and aperture, if the lens is here and was calibrated
    /// on a frame no smaller than the photograph's (lensfun's own rule:
    /// a calibration holds for a frame up to 4% bigger than its own).
    pub fn lens_for(&self, shot: &Shot) -> Option<Matched> {
        if shot.lens_model.trim().is_empty() || !(shot.focal.is_finite() && shot.focal > 0.0) {
            return None;
        }
        let wanted = [
            without_maker(&shot.lens_model, &shot.lens_make),
            without_maker(&shot.lens_model, &shot.camera_make),
        ];
        let candidates: Vec<&Entry> = self
            .lenses
            .iter()
            .filter(|e| {
                e.names.iter().any(|n| {
                    wanted
                        .iter()
                        .any(|w| n == w || *n == without_maker(w, &e.maker))
                })
            })
            .collect();
        let first = candidates.first()?;
        let crop = self.crop_of(shot).unwrap_or(first.crop);
        let entry = candidates
            .iter()
            .filter(|e| crop / e.crop >= 0.96)
            .min_by(|a, b| (crop / a.crop).total_cmp(&(crop / b.crop)))?;
        let lens = entry.at(shot, crop)?;
        Some(Matched {
            name: entry.name.clone(),
            lens,
        })
    }
}

/// lensfun's Hermite step between `y2` and `y3`, its neighbours `y1`
/// and `y4` setting the slopes where there are any.
fn hermite(y1: Option<f32>, y2: f32, y3: f32, y4: Option<f32>, t: f32) -> f32 {
    let tg2 = y1.map_or(y3 - y2, |y1| (y3 - y1) * 0.5);
    let tg3 = y4.map_or(y3 - y2, |y4| (y4 - y2) * 0.5);
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y2
        + (t3 - 2.0 * t2 + t) * tg2
        + (-2.0 * t3 + 3.0 * t2) * y3
        + (t3 - t2) * tg3
}

/// A model's terms at `focal`, from its calibrations (sorted by focal).
fn interpolate(list: &[Calibrated], focal: f32) -> Option<Calibrated> {
    if let Some(exact) = list.iter().find(|c| c.focal == focal) {
        return Some(*exact);
    }
    let above = list.partition_point(|c| c.focal < focal);
    let (Some(lo), Some(hi)) = (
        above.checked_sub(1).map(|i| list[i]),
        list.get(above).copied(),
    ) else {
        // Outside what was calibrated: the nearest, as lensfun does.
        return list.get(above.min(list.len().saturating_sub(1))).copied();
    };
    let before = above.checked_sub(2).map(|i| list[i]);
    let after = list.get(above + 1).copied();
    let t = (focal - lo.focal) / (hi.focal - lo.focal);
    let terms = std::array::from_fn(|i| {
        hermite(
            before.map(|c| c.terms[i]),
            lo.terms[i],
            hi.terms[i],
            after.map(|c| c.terms[i]),
            t,
        )
    });
    Some(Calibrated {
        focal,
        model: lo.model,
        terms,
    })
}

/// How many radii a profile is tabled at.
const KNOTS: usize = 48;

impl Entry {
    /// lensfun's vignetting at the photograph's settings: inverse distance
    /// weighting, power 3.5, over focal length, aperture and distance
    /// each brought to about nought to one; none where the nearest
    /// calibration is more than one away.
    fn vignetting_at(&self, focal: f32, aperture: f32, distance: f32) -> Option<[f32; 3]> {
        if self.vignetting.is_empty() || !(aperture.is_finite() && aperture > 0.0) {
            return None;
        }
        let (lo, hi) = self.focal_range;
        let span = hi - lo;
        let along = |f: f32| if span > 0.0 { (f - lo) / span } else { f - lo };
        let (mut k, mut total, mut nearest) = ([0f32; 3], 0f32, f32::MAX);
        for v in &self.vignetting {
            let d = ((along(v.focal) - along(focal)).powi(2)
                + (4.0 / v.aperture - 4.0 / aperture).powi(2)
                + (0.1 / v.distance - 0.1 / distance).powi(2))
            .sqrt();
            if d < 1e-4 {
                return Some(v.k);
            }
            nearest = nearest.min(d);
            let w = 1.0 / d.powf(3.5);
            for (slot, kv) in k.iter_mut().zip(v.k) {
                *slot += w * kv;
            }
            total += w;
        }
        (nearest <= 1.0 && total > 0.0).then(|| k.map(|v| v / total))
    }

    /// The corrections at the photograph's settings, for a frame `crop`
    /// times smaller than 36 × 24, tabled as [`Lens`] wants them: by
    /// radius on the sensor in half-diagonals.
    fn at(&self, shot: &Shot, crop: f32) -> Option<Lens> {
        let distortion = interpolate(&self.distortion, shot.focal);
        let tca = interpolate(&self.tca, shot.focal);
        let distance = if shot.distance > 0.0 {
            shot.distance
        } else {
            1000.0
        };
        let vignetting = self.vignetting_at(shot.focal, shot.aperture, distance);
        if distortion.is_none() && tca.is_none() && vignetting.is_none() {
            return None;
        }
        // Half-diagonals of the photograph to the units distortion and
        // fringes are written in (half the calibration frame's shorter
        // side), and to vignetting's (its half-diagonal).
        let to_hugin = self.crop * (self.aspect * self.aspect + 1.0).sqrt() / crop;
        let to_corner = self.crop / crop;
        // Where an undistorted radius is on the sensor, over the size the
        // middle keeps.
        let f = |ru: f32| -> f32 {
            match distortion {
                None => 1.0,
                Some(Calibrated {
                    model, terms: t, ..
                }) => match model {
                    Model::PtLens => {
                        let d = 1.0 - t[0] - t[1] - t[2];
                        (t[0] * ru * ru * ru + t[1] * ru * ru + t[2] * ru + d) / d
                    }
                    Model::Poly3 => (1.0 - t[0] + t[0] * ru * ru) / (1.0 - t[0]),
                    Model::Poly5 => 1.0 + t[0] * ru * ru + t[1] * ru.powi(4),
                    _ => 1.0,
                },
            }
        };
        let fringe = |r: f32| -> (f32, f32) {
            match tca {
                Some(Calibrated {
                    model: Model::Linear,
                    terms: t,
                    ..
                }) => (t[0], t[1]),
                Some(Calibrated {
                    model: Model::TcaPoly3,
                    terms: t,
                    ..
                }) => (
                    t[4] * r * r + t[2] * r + t[0],
                    t[5] * r * r + t[3] * r + t[1],
                ),
                _ => (1.0, 1.0),
            }
        };
        // Tabled out past the corner, which a zoomed-in picture reads
        // from: undistorted radii far enough that the sensor's reach 1.25
        // half-diagonals.
        let mut reach = 1.3 * to_hugin;
        for _ in 0..8 {
            let rd = reach * f(reach);
            if !rd.is_finite() || rd >= 1.25 * to_hugin {
                break;
            }
            reach *= 1.25;
        }
        let mut lens = Lens {
            radii: Vec::with_capacity(KNOTS),
            distortion: Vec::with_capacity(KNOTS),
            red: Vec::with_capacity(KNOTS),
            blue: Vec::with_capacity(KNOTS),
            light: Vec::with_capacity(KNOTS),
        };
        for i in 0..KNOTS {
            let ru = reach * i as f32 / (KNOTS - 1) as f32;
            let m = f(ru);
            let rd = ru * m;
            let radius = rd / to_hugin;
            // Where the curve folds back on itself the profile is past
            // what it was fitted to.
            let usable = m.is_finite() && m > 0.0 && radius.is_finite();
            if !usable || lens.radii.last().is_some_and(|&r| r >= radius) {
                break;
            }
            let (kr, kb) = fringe(rd);
            let light = vignetting.map_or(1.0, |k| {
                let r2 = (radius * to_corner).powi(2);
                1.0 + r2 * (k[0] + r2 * (k[1] + r2 * k[2]))
            });
            lens.radii.push(radius);
            lens.distortion.push((m - 1.0) * 100.0);
            lens.red.push(kr - 1.0);
            lens.blue.push(kb - 1.0);
            lens.light.push(light * 100.0);
        }
        (lens.radii.len() >= 2).then_some(lens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small database in lensfun's own form: one zoom, one camera of a
    /// smaller frame than the zoom was calibrated on, one fisheye.
    const XML: &str = r#"<lensdatabase version="2">
    <camera>
        <maker>Canon</maker>
        <model>Canon EOS 7D</model>
        <model lang="en">EOS 7D</model>
        <mount>Canon EF-S</mount>
        <cropfactor>1.611</cropfactor>
    </camera>
    <lens>
        <maker>Canon</maker>
        <model>Canon EF-S 15-85mm f/3.5-5.6 IS USM</model>
        <mount>Canon EF-S</mount>
        <cropfactor>1.611</cropfactor>
        <calibration>
            <!-- a comment -->
            <distortion model="ptlens" focal="15" a="0.02" b="-0.05" c="0.01"/>
            <distortion model="ptlens" focal="24" a="0.01" b="-0.02" c="0.0"/>
            <distortion model="ptlens" focal="35" a="0.0" b="0.0" c="0.0"/>
            <distortion model="poly3" focal="50" k1="0.3"/>
            <tca model="poly3" focal="15" vr="1.0004" vb="0.9998" br="0.0001"/>
            <vignetting model="pa" focal="15" aperture="3.5" distance="1000" k1="-0.5" k2="0.1" k3="0"/>
            <vignetting model="pa" focal="15" aperture="8" distance="1000" k1="-0.2" k2="0" k3="0"/>
        </calibration>
    </lens>
    <lens>
        <maker>Canon</maker>
        <model>Canon EF 8-15mm f/4L Fisheye USM</model>
        <type>fisheye</type>
        <cropfactor>1</cropfactor>
        <calibration>
            <distortion model="poly3" focal="8" k1="0.1"/>
        </calibration>
    </lens>
</lensdatabase>"#;

    fn shot(focal: f32, aperture: f32) -> Shot {
        Shot {
            camera_make: "Canon".into(),
            camera_model: "EOS 7D".into(),
            lens_make: "Canon".into(),
            lens_model: "EF-S 15-85mm f/3.5-5.6 IS USM".into(),
            focal,
            aperture,
            distance: 0.0,
        }
    }

    #[test]
    fn a_database_is_read_and_its_fisheyes_left_out() {
        let mut p = Profiles::default();
        assert_eq!(p.add(XML), Ok(1));
        assert_eq!(p.crop_of(&shot(15.0, 3.5)), Some(1.611));
        assert!(p.add("<lensdatabase").is_err());
        assert!(p.add("<svg/>").is_err());
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn a_lens_is_found_by_its_name_however_it_is_spaced() {
        let mut p = Profiles::default();
        p.add(XML).unwrap();
        let mut s = shot(15.0, 3.5);
        assert_eq!(
            p.lens_for(&s).unwrap().name,
            "Canon EF-S 15-85mm f/3.5-5.6 IS USM"
        );
        s.lens_model = "EF-S15-85mm f/3.5-5.6 IS USM".into();
        assert!(p.lens_for(&s).is_some());
        s.lens_model = "Canon EF-S 15-85mm f/3.5-5.6 IS USM".into();
        s.lens_make = String::new();
        assert!(p.lens_for(&s).is_some());
        s.lens_model = "EF-S 17-85mm f/4-5.6 IS USM".into();
        assert!(p.lens_for(&s).is_none());
        // No focal length, nothing to look up at.
        assert!(p.lens_for(&shot(0.0, 3.5)).is_none());
    }

    #[test]
    fn a_calibration_holds_for_a_smaller_frame_and_not_a_bigger_one() {
        let mut p = Profiles::default();
        p.add(XML).unwrap();
        let mut s = shot(15.0, 3.5);
        s.camera_model = "EOS 5D Mark III".into();
        // Not in the database: the lens's own frame is assumed.
        assert!(p.lens_for(&s).is_some());
        p.add(
            r#"<lensdatabase><camera><maker>Canon</maker><model>Canon EOS 5D Mark III</model>
            <cropfactor>1</cropfactor></camera></lensdatabase>"#,
        )
        .unwrap();
        assert!(
            p.lens_for(&s).is_none(),
            "an APS-C calibration on full frame"
        );
    }

    #[test]
    fn between_calibrated_focal_lengths_the_terms_follow_lensfun_s_spline() {
        let list: Vec<Calibrated> = [(10.0, 0.0), (20.0, 1.0), (30.0, 4.0), (40.0, 9.0)]
            .iter()
            .map(|&(focal, a)| Calibrated {
                focal,
                model: Model::PtLens,
                terms: [a, 0.0, 0.0, 0.0, 0.0, 0.0],
            })
            .collect();
        // Through the knots, held past the ends.
        assert_eq!(interpolate(&list, 20.0).unwrap().terms[0], 1.0);
        assert_eq!(interpolate(&list, 5.0).unwrap().terms[0], 0.0);
        assert_eq!(interpolate(&list, 50.0).unwrap().terms[0], 9.0);
        // A square's values, which the spline with these slopes keeps
        // exactly between the inner knots: (2.5)² at 25.
        assert!((interpolate(&list, 25.0).unwrap().terms[0] - 2.25).abs() < 1e-5);
        // lensfun's slope at an end with no neighbour is the step itself.
        let v = interpolate(&list, 15.0).unwrap().terms[0];
        assert!(v > 0.0 && v < 1.0);
    }

    #[test]
    fn a_profile_is_tabled_as_a_lens_with_the_middle_keeping_its_size() {
        let mut p = Profiles::default();
        p.add(XML).unwrap();
        let lens = p.lens_for(&shot(15.0, 3.5)).unwrap().lens;
        assert_eq!(lens.radii[0], 0.0);
        assert!(lens.distortion[0].abs() < 1e-4);
        assert!(lens.radii.windows(2).all(|w| w[0] < w[1]));
        assert!(*lens.radii.last().unwrap() >= 1.2);
        // At the corner: ptlens's own formula, worked out by hand in the
        // calibration's units (r = 1 at half the shorter side, so the
        // corner of a frame the same size is √(1.5² + 1) out).
        let (a, b, c) = (0.02f32, -0.05f32, 0.01f32);
        let d = 1.0 - a - b - c;
        let rd_corner = (1.5f32 * 1.5 + 1.0).sqrt();
        // Find ru with ru·f(ru)/… landing at the corner by bisection.
        let f = |ru: f32| (a * ru.powi(3) + b * ru * ru + c * ru + d) / d;
        let (mut lo, mut hi) = (0.0f32, 3.0f32);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if mid * f(mid) < rd_corner {
                lo = mid
            } else {
                hi = mid
            }
        }
        let want = (f(lo) - 1.0) * 100.0;
        let i = lens.radii.partition_point(|&r| r < 1.0);
        let t = (1.0 - lens.radii[i - 1]) / (lens.radii[i] - lens.radii[i - 1]);
        let got = lens.distortion[i - 1] + (lens.distortion[i] - lens.distortion[i - 1]) * t;
        assert!((got - want).abs() < 0.05, "{got} against {want}");
        // Vignetting at the corner: 1 - 0.5 + 0.1 at f/3.5 exactly.
        let light = lens.light[i - 1] + (lens.light[i] - lens.light[i - 1]) * t;
        assert!((light - 60.0).abs() < 0.5, "{light}");
        // Red's fringe: 1.0004 + 0.0001 r², r in the calibration's units.
        let red = lens.red[i - 1] + (lens.red[i] - lens.red[i - 1]) * t;
        assert!(
            (red - (0.0004 + 0.0001 * rd_corner * rd_corner)).abs() < 1e-5,
            "{red}"
        );
        assert!(lens.blue.iter().all(|b| (b + 0.0002).abs() < 1e-6));
    }

    #[test]
    fn vignetting_is_weighted_between_apertures_and_left_off_far_from_any() {
        let mut p = Profiles::default();
        p.add(XML).unwrap();
        let e = &p.lenses[0];
        let near_wide = e.vignetting_at(15.0, 4.0, 1000.0).unwrap();
        assert!(near_wide[0] < -0.2 && near_wide[0] > -0.5);
        // f/3.5 is 4/3.5 = 1.14 on lensfun's scale; f/1.4 is 2.9, more
        // than one from any calibration.
        assert!(e.vignetting_at(15.0, 1.4, 1000.0).is_none());
        // An aperture not said: none.
        assert!(e.vignetting_at(15.0, 0.0, 1000.0).is_none());
    }
}
