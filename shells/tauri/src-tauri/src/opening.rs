//! Files the system asks the app to open: a document double-clicked, an
//! image sent with "Open with". Windows and Linux start the app with the
//! paths on its command line; macOS starts it bare and says which files
//! afterwards, and keeps saying so while it runs. Either way they wait
//! here until the page asks for them, since a file can arrive before
//! there is a page to open it in.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Files asked for and not yet opened.
#[derive(Default)]
pub struct Waiting(pub Mutex<Vec<String>>);

impl Waiting {
    pub fn add(&self, files: Vec<String>) {
        if let Ok(mut waiting) = self.0.lock() {
            waiting.extend(files);
        }
    }

    /// Everything waiting, once: a file is opened by whichever asks first.
    pub fn take(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|mut waiting| std::mem::take(&mut *waiting))
            .unwrap_or_default()
    }
}

/// The files on a command line, as whole paths. The first argument is
/// the program; anything that reads as a flag is somebody else's
/// business; a relative path is relative to where the app was started,
/// which is not where it will be looked for later; and what is not a
/// file is not something to open.
pub fn files_in_args<I: IntoIterator<Item = OsString>>(args: I, cwd: &Path) -> Vec<String> {
    args.into_iter()
        .skip(1)
        .filter(|a| !a.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { cwd.join(p) })
        .filter(|p| p.is_file())
        .filter_map(|p| p.into_os_string().into_string().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_files_on_a_command_line_are_found_whole() {
        let dir = std::env::temp_dir().join(format!("chitrakar-args-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("poster.chitra"), b"PK").unwrap();
        fs::write(dir.join("sub").join("photo é.jpg"), b"jpg").unwrap();
        let args = [
            "chitrakar",
            "--some-flag",
            "poster.chitra",
            "sub/photo é.jpg",
            "missing.chitra",
            "sub",
        ]
        .map(OsString::from);
        let found = files_in_args(args, &dir);
        assert_eq!(
            found,
            vec![
                dir.join("poster.chitra").to_string_lossy().into_owned(),
                dir.join("sub/photo é.jpg").to_string_lossy().into_owned(),
            ]
        );
        // The program itself is never one of them, even when it is a file.
        assert!(files_in_args([dir.join("poster.chitra").into_os_string()], &dir).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_waits_is_taken_once() {
        let waiting = Waiting::default();
        waiting.add(vec!["/a.chitra".into()]);
        waiting.add(vec!["/b.png".into()]);
        assert_eq!(waiting.take(), vec!["/a.chitra", "/b.png"]);
        assert!(waiting.take().is_empty());
    }
}
