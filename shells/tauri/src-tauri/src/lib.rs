//! Tauri 2 shell. The entry point is a library so the same code drives the
//! desktop binary (main.rs) and the iOS/Android app hosts.
//!
//! For the MVP the engine runs as WASM inside the webview (see docs/PLAN.md
//! §1), so the shell stays thin: window, menus, file dialogs, and native
//! filesystem access. The `engine_version` command exists to prove the
//! shell↔core link end-to-end.

use chitrakar_engine::{ColorMode, Session};

mod onnx;
mod subject;

/// Ask the system for the subject of a photograph.
///
/// Apple platforms ship the model Photos uses to lift a subject off its
/// background, and it knows things no amount of reasoning about colour
/// can reach — that a white shirt in front of a white curtain is still a
/// shirt. Where there is nothing to ask, the answer is an empty reply
/// rather than an error, and the app falls back to the engine's own
/// pick, which is what every other platform does.
///
/// The picture goes over as bytes and the matte comes back as a PNG: a
/// silhouette is a few tens of kilobytes encoded and a couple of
/// megabytes raw, and it has to cross from the shell into the webview,
/// where that is the difference between instant and a visible pause.
#[tauri::command]
fn subject_matte(png: tauri::ipc::Request<'_>) -> Result<tauri::ipc::Response, String> {
    let tauri::ipc::InvokeBody::Raw(page) = png.body() else {
        return Err("the picture must be sent as bytes".into());
    };
    match subject::matte(page)? {
        // Encoded here rather than in the webview so the bytes never
        // cross as bytes.
        Some((bytes, width, height)) => {
            let rgba: Vec<u8> = bytes.iter().flat_map(|&v| [v, v, v, 255]).collect();
            let png = chitrakar_codecs::encode_png(width, height, &rgba)
                .map_err(|e| format!("the matte could not be encoded: {e}"))?;
            Ok(tauri::ipc::Response::new(png))
        }
        // Nothing here to ask. Not a failure: the caller falls back to
        // the engine's own pick, which is what every other platform
        // does.
        None => Ok(tauri::ipc::Response::new(Vec::new())),
    }
}

/// Smoke-test command: create an engine session natively and report on it.
/// Replaced by real native-engine plumbing if/when a platform needs the
/// native render path.
#[tauri::command]
fn engine_version() -> String {
    let session = Session::new(1, 1, ColorMode::Rgb);
    format!(
        "chitrakar-engine ok, empty document has {} node(s)",
        session.document().node_count()
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![engine_version, subject_matte])
        .run(tauri::generate_context!())
        .expect("error while running Chitrakar");
}
