//! The page drawn by the graphics card, for the webview to show.
//!
//! The editor's engine runs inside the webview and draws its own frames
//! on the processor. Here, beside it, is a process with a graphics card
//! to draw with: `chitrakar_present` keeps a mirror of the document the
//! webview sends and draws its frames with `chitrakar_gpu`, and these are
//! the commands the webview reaches it through. Where the card cannot
//! draw a frame the way the processor would, it answers nothing and the
//! webview draws that frame itself — so turning this on can make the
//! page faster and never makes it different.

use std::sync::Mutex;

use chitrakar_present::{Presenter, Wants};

/// The presenter, made the first time anything asks: bringing up a
/// graphics device is not free, and someone who never turns this on
/// should not pay for it. `Some(None)` is a machine that was asked and
/// has no adapter, so it is not asked again.
#[derive(Default)]
pub struct Gpu(Mutex<Option<Option<Presenter>>>);

impl Gpu {
    fn with<R>(&self, f: impl FnOnce(&mut Presenter) -> R) -> Option<R> {
        let mut held = self.0.lock().ok()?;
        held.get_or_insert_with(Presenter::new).as_mut().map(f)
    }
}

/// Bring the graphics card up, if there is one: what the adapter calls
/// itself, or nothing.
#[tauri::command]
pub async fn gpu_start(gpu: tauri::State<'_, Gpu>) -> Result<Option<String>, String> {
    Ok(gpu.with(|p| p.adapter().to_string()))
}

/// The document as it now stands, and what is still wanted to draw it.
#[tauri::command]
pub async fn gpu_sync(gpu: tauri::State<'_, Gpu>, manifest: String) -> Result<Wants, String> {
    gpu.with(|p| p.sync(&manifest))
        .unwrap_or_else(|| Err("there is no graphics card to draw with".into()))
}

/// A picture's bytes, in the body, with its id and size in headers.
#[tauri::command]
pub async fn gpu_picture(
    gpu: tauri::State<'_, Gpu>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("the picture must be sent as bytes".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| format!("no {name} was given"))
    };
    let id = header("id")?.to_string();
    let width: u32 = header("width")?
        .parse()
        .map_err(|_| "the width is not a number")?;
    let height: u32 = header("height")?
        .parse()
        .map_err(|_| "the height is not a number")?;
    let bytes = bytes.clone();
    gpu.with(|p| p.give_picture(&id, width, height, bytes))
        .unwrap_or_else(|| Err("there is no graphics card to draw with".into()))
}

/// A face's file, in the body, with its name percent-encoded in a header.
#[tauri::command]
pub async fn gpu_font(
    gpu: tauri::State<'_, Gpu>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("the face must be sent as bytes".into());
    };
    let name = request
        .headers()
        .get("name")
        .and_then(|h| h.to_str().ok())
        .ok_or("no name was given")?;
    // The same encoding a path crosses in.
    let name = crate::files::path_from_header(name)?
        .to_string_lossy()
        .into_owned();
    let bytes = bytes.clone();
    gpu.with(|p| p.give_font(&name, bytes))
        .unwrap_or_else(|| Err("there is no graphics card to draw with".into()))
}

/// The viewport drawn by the card, as straight sRGB bytes row by row —
/// or no bytes at all when the card will not draw it the way the
/// processor would, and the webview should draw it itself.
#[tauri::command]
pub async fn gpu_frame(
    gpu: tauri::State<'_, Gpu>,
    scale: f32,
    x: f32,
    y: f32,
    width: u32,
    height: u32,
) -> Result<tauri::ipc::Response, String> {
    let bytes = gpu
        .with(|p| p.frame(scale, x, y, width, height))
        .flatten()
        .unwrap_or_default();
    Ok(tauri::ipc::Response::new(bytes))
}
