//! Tauri 2 shell. The entry point is a library so the same code drives the
//! desktop binary (main.rs) and the iOS/Android app hosts.
//!
//! For the MVP the engine runs as WASM inside the webview (see docs/PLAN.md
//! §1), so the shell stays thin: window, menus, file dialogs, and native
//! filesystem access. The `engine_version` command exists to prove the
//! shell↔core link end-to-end.

use chitrakar_engine::{ColorMode, Session};

mod files;
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

/// Ask, in the system's own panel, which file to open. `None` is the
/// panel cancelled. Async so the panel waits on a worker rather than on
/// the thread that draws the window, which is what a blocking panel on
/// the main thread would deadlock.
#[tauri::command]
async fn choose_to_open(
    app: tauri::AppHandle,
    title: String,
    filters: Vec<files::Filter>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let mut panel = app.dialog().file().set_title(title);
    for f in &filters {
        let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        panel = panel.add_filter(&f.name, &exts);
    }
    match panel.blocking_pick_file() {
        Some(chosen) => {
            files::path_to_string(chosen.into_path().map_err(|e| e.to_string())?).map(Some)
        }
        None => Ok(None),
    }
}

/// Ask where to write a file, offering `name` in `beside`'s folder when
/// there is one — Save As starts where the document already lives.
#[tauri::command]
async fn choose_to_save(
    app: tauri::AppHandle,
    title: String,
    name: String,
    beside: Option<String>,
    filters: Vec<files::Filter>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let mut panel = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name(files::offered_name(&name, &filters));
    if let Some(dir) = beside
        .as_deref()
        .and_then(|b| std::path::Path::new(b).parent())
    {
        if dir.is_dir() {
            panel = panel.set_directory(dir);
        }
    }
    for f in &filters {
        let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        panel = panel.add_filter(&f.name, &exts);
    }
    match panel.blocking_save_file() {
        Some(chosen) => {
            let path = chosen.into_path().map_err(|e| e.to_string())?;
            files::path_to_string(files::with_extension(path, &filters)).map(Some)
        }
        None => Ok(None),
    }
}

/// Ask for a folder, for an export that is more than one file: a panel
/// per file is not a thing anybody wants to answer twelve times.
#[tauri::command]
async fn choose_folder(app: tauri::AppHandle, title: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    match app.dialog().file().set_title(title).blocking_pick_folder() {
        Some(chosen) => {
            files::path_to_string(chosen.into_path().map_err(|e| e.to_string())?).map(Some)
        }
        None => Ok(None),
    }
}

/// A file's bytes, as bytes: a document is megabytes, and a JSON array
/// of numbers is several times that.
#[tauri::command]
async fn read_path(path: String) -> Result<tauri::ipc::Response, String> {
    std::fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|e| format!("{path}: {e}"))
}

/// Write the bytes in the body to the path in the `path` header, whole
/// or not at all.
#[tauri::command]
async fn write_path(request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("the file must be sent as bytes".into());
    };
    let header = request
        .headers()
        .get("path")
        .and_then(|h| h.to_str().ok())
        .ok_or("no path was given")?;
    let path = files::path_from_header(header)?;
    files::write_whole(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A path joined onto a folder, for the files of a many-file export.
/// Done here rather than in the webview, which does not know whether
/// the separator is a slash.
#[tauri::command]
fn join_path(folder: String, name: String) -> Result<String, String> {
    let name = std::path::Path::new(&name)
        .file_name()
        .ok_or("no file name was given")?
        .to_os_string();
    files::path_to_string(std::path::Path::new(&folder).join(name))
}

/// Which of these paths something is already at, so an export of
/// several files into a folder can ask before writing over them.
#[tauri::command]
async fn already_there(paths: Vec<String>) -> Vec<String> {
    files::already_there(&paths)
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
        .invoke_handler(tauri::generate_handler![
            engine_version,
            subject_matte,
            choose_to_open,
            choose_to_save,
            choose_folder,
            read_path,
            write_path,
            join_path,
            already_there
        ])
        .run(tauri::generate_context!())
        .expect("error while running Chitrakar");
}
