//! Tauri 2 shell. The entry point is a library so the same code drives the
//! desktop binary (main.rs) and the iOS/Android app hosts.
//!
//! For the MVP the engine runs as WASM inside the webview (see docs/PLAN.md
//! §1), so the shell stays thin: window, menus, file dialogs, and native
//! filesystem access. The `engine_version` command exists to prove the
//! shell↔core link end-to-end.

use chitrakar_engine::{ColorMode, Session};

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
        .invoke_handler(tauri::generate_handler![engine_version])
        .run(tauri::generate_context!())
        .expect("error while running Chitrakar");
}
