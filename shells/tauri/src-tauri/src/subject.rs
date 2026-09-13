//! The subject of a photograph, asked of the system rather than reasoned
//! about.
//!
//! See `swift/subject.swift` for the part that does the asking, and
//! `build.rs` for when it is compiled in. Everything here is the join:
//! turning a returned buffer into a `Vec<u8>` that Rust owns, and saying
//! "not here" on a platform or a build where there is nothing to ask.

/// The subject's coverage for a PNG — one byte per pixel — with the size
/// of the matte, which need not be the size of the picture.
///
/// `Ok(None)` means there is nothing on this platform to ask, which is
/// not a failure: the caller falls back to the engine's own pick.
pub fn matte(png: &[u8]) -> Result<Option<(Vec<u8>, u32, u32)>, String> {
    // The system's own first, where there is one. It is better than
    // anything that could be carried, and it costs nothing to carry
    // because it is already there.
    #[cfg(has_subject_matte)]
    if let Ok(found) = ask(png) {
        return Ok(Some(found));
    }
    // Then a model, if one has been put where `onnx` looks — which is
    // what every platform without a system model has, and what this
    // falls back to on Apple platforms when the system declines to find
    // anything. See `onnx.rs` for why it is looked for and not shipped.
    if crate::onnx::model_path().is_some() {
        let img = chitrakar_codecs::decode(png)
            .map_err(|e| format!("that picture could not be read: {e}"))?;
        return crate::onnx::matte(&img.rgba8, img.width, img.height).map(Some);
    }
    // Nothing to ask. Not a failure: the caller falls back to the
    // engine's own pick, which is what this all existed as before.
    Ok(None)
}

#[cfg(has_subject_matte)]
unsafe extern "C" {
    fn chitrakar_subject_matte(
        png: *const u8,
        png_len: usize,
        width: *mut i32,
        height: *mut i32,
        bytes: *mut *mut u8,
    ) -> i32;
    fn chitrakar_subject_free(p: *mut u8);
}

#[cfg(has_subject_matte)]
fn ask(png: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    if png.is_empty() {
        return Err("there is no picture to look at".into());
    }
    let (mut w, mut h) = (0i32, 0i32);
    let mut out: *mut u8 = std::ptr::null_mut();
    // SAFETY: the pointers are to live locals for the length of the call,
    // the picture is passed with its own length, and the buffer handed
    // back is copied and then given back to the allocator that made it —
    // never freed here and never held past this function.
    let code =
        unsafe { chitrakar_subject_matte(png.as_ptr(), png.len(), &mut w, &mut h, &mut out) };
    if code != 0 {
        return Err(match code {
            1 => "that picture could not be read".into(),
            3 => "nothing in this picture looks like a subject".into(),
            other => format!("the subject could not be worked out (code {other})"),
        });
    }
    if out.is_null() || w <= 0 || h <= 0 {
        return Err("the subject came back empty".into());
    }
    let (w, h) = (w as u32, h as u32);
    // SAFETY: `out` is non-null and holds exactly w * h bytes, as the
    // Swift side allocated it; copied out before it is handed back.
    let bytes = unsafe { std::slice::from_raw_parts(out, (w * h) as usize).to_vec() };
    unsafe { chitrakar_subject_free(out) };
    Ok((bytes, w, h))
}
