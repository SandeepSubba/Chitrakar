//! A subject's matte from a model on disk.
//!
//! The fallback for platforms with nothing of their own to ask — Windows,
//! Linux, Android — and the reason the engine grew a way in for a matte
//! that it did not work out itself.
//!
//! The model is *found*, never carried. Two reasons, and the second is
//! the real one. It is 170-odd megabytes, which is not a thing to put in
//! a git repository. And the small version of it that would fit, at four
//! and a half, was measured against the same photographs and is not good
//! enough to ship: it makes a soft, roughly-right shape of a child
//! against a plain curtain and finds nothing at all in a wedding
//! photograph of two people against a wall of flowers. Shipping that
//! would be shipping the appearance of the feature. So: if a model has
//! been put where this looks, it is used; if not, the engine's own
//! colour-based pick answers, which is what happened before any of this
//! existed.
//!
//! U^2-Net's shape: 320x320 of normalised RGB in, a saliency map of the
//! same size out, stretched back over the picture.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tract_onnx::prelude::*;

type Model = Arc<TypedRunnableModel>;

const SIDE: usize = 320;

/// Where a model would be if there is one.
///
/// `CHITRAKAR_SUBJECT_MODEL` first, so a build can be pointed at one
/// without installing anything; then the place the app would keep it.
pub fn model_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CHITRAKAR_SUBJECT_MODEL") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home)
        .join(".cache")
        .join("chitrakar")
        .join("u2net.onnx");
    p.is_file().then_some(p)
}

/// Loaded once and kept: reading and optimising the graph costs about as
/// long as running it, and this is a button somebody presses repeatedly.
fn loaded(path: &PathBuf) -> Result<&'static Model, String> {
    static MODEL: OnceLock<Result<Model, String>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            tract_onnx::onnx()
                .model_for_path(path)
                .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into()))
                .and_then(|m| m.into_optimized())
                .and_then(|m| m.into_runnable())
                .map_err(|e| format!("that model could not be read: {e}"))
        })
        .as_ref()
        .map_err(|e: &String| e.clone())
}

/// The subject's coverage for a decoded picture, one byte per pixel.
pub fn matte(rgba8: &[u8], w: u32, h: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let path = model_path().ok_or("there is no subject model on this machine")?;
    let model = loaded(&path)?;
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || rgba8.len() < w * h * 4 {
        return Err("that picture is not the size it says it is".into());
    }

    // Averaged down rather than sampled. Going from a couple of thousand
    // pixels to three hundred by taking one in seven throws most of the
    // picture away and aliases what is left, which is not a fair thing
    // to hand a model.
    let mut small = vec![[0f32; 3]; SIDE * SIDE];
    let mut count = vec![0f32; SIDE * SIDE];
    for y in 0..h {
        let ty = (y * SIDE / h).min(SIDE - 1);
        for x in 0..w {
            let tx = (x * SIDE / w).min(SIDE - 1);
            let k = ty * SIDE + tx;
            for c in 0..3 {
                small[k][c] += rgba8[(y * w + x) * 4 + c] as f32;
            }
            count[k] += 1.0;
        }
    }
    // What the model was trained against.
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const SD: [f32; 3] = [0.229, 0.224, 0.225];
    for k in 0..SIDE * SIDE {
        let n = count[k].max(1.0);
        for c in 0..3 {
            small[k][c] = (small[k][c] / (n * 255.0) - MEAN[c]) / SD[c];
        }
    }
    let input = tract_ndarray::Array4::from_shape_fn((1, 3, SIDE, SIDE), |(_, c, y, x)| {
        small[y * SIDE + x][c]
    });

    let out = model
        .run(tvec!(Tensor::from(input).into()))
        .map_err(|e| format!("the model would not run: {e}"))?;
    let map = out[0]
        .to_plain_array_view::<f32>()
        .map_err(|e| format!("the model gave back something unreadable: {e}"))?;

    // The map is a saliency, not a probability: its own range is
    // stretched to fill nothing-to-everything, which is how every use of
    // these reads them.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for &v in map.iter() {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(1e-6);
    let mut grey = vec![0u8; SIDE * SIDE];
    for y in 0..SIDE {
        for x in 0..SIDE {
            grey[y * SIDE + x] = (((map[[0, 0, y, x]] - lo) / span) * 255.0) as u8;
        }
    }
    // Handed back at the model's own size; the engine stretches a matte
    // over the page, so there is nothing to gain by doing it twice.
    Ok((grey, SIDE as u32, SIDE as u32))
}
