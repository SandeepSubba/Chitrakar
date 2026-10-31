//! Files by path: what a desktop application does with them and a
//! browser tab cannot.
//!
//! In a browser a document is saved by being downloaded, which writes a
//! new copy every time and never knows where the last one went. Here a
//! document remembers the file it came from, Save writes back over it,
//! and the panels are the system's own. Everything that is not a panel
//! is plain functions over paths, so it is tested without a window.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// One line of a panel's "files of type" list.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Filter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// Write a file so that it is either the old one or the new one, never
/// half of each. Saving over a document is the one write in the app that
/// destroys something if it stops halfway — a full disk, a pulled cable,
/// a crash — so the bytes go to a file beside it first and take its
/// place only once they are all down.
pub fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let mut beside = name.to_os_string();
    beside.push(format!(".{}.part", std::process::id()));
    let part = path.with_file_name(beside);
    let written = (|| {
        let mut file = fs::File::create(&part)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    let placed = written.and_then(|()| fs::rename(&part, path));
    if placed.is_err() {
        // Whatever went wrong, the half-written copy is not something
        // anybody should find later.
        let _ = fs::remove_file(&part);
    }
    placed
}

/// A path as the webview spells it. Paths cross in a header so the bytes
/// beside them can cross as bytes, and a header is ASCII, so the app
/// sends it through `encodeURIComponent` and this takes it back.
pub fn path_from_header(encoded: &str) -> Result<PathBuf, String> {
    let raw = encoded.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = raw
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("the path is not encoded properly: {encoded}"))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(out).map_err(|_| "the path is not UTF-8".to_string())?;
    if path.is_empty() {
        return Err("no path was given".into());
    }
    Ok(PathBuf::from(path))
}

/// A chosen path as a string the webview can keep. A path that is not
/// Unicode would come back as some other file if it were made into one
/// lossily, so it is refused instead.
pub fn path_to_string(path: PathBuf) -> Result<String, String> {
    path.into_os_string()
        .into_string()
        .map_err(|p| format!("{} cannot be named here", p.to_string_lossy()))
}

/// What the page calls a file a panel chose: its path where it has one,
/// and otherwise the address the system gave for it. Android's picker
/// hands over `content://` addresses — a file in Drive, in Downloads,
/// in another app's keeping — which are not paths and never become
/// one; only the system can open them, so they are kept as they came
/// and opened through it ([`is_address`]).
///
/// On an iPhone or an iPad a `file://` address is kept too. A file
/// picked there is the app's to read only while it holds the grant the
/// picker gave with it, and the fs plugin takes that grant when it opens
/// a `file://` address — read as a bare path, the same file is refused.
pub fn chosen(file: tauri_plugin_fs::FilePath) -> Result<String, String> {
    chosen_on(file, KEEP_FILE_ADDRESSES)
}

/// Whether `file://` addresses are kept as addresses here: on iOS.
const KEEP_FILE_ADDRESSES: bool = cfg!(target_os = "ios");

/// [`chosen`], with whether this platform keeps `file://` addresses
/// said outright, so both answers are tested on any machine.
pub fn chosen_on(file: tauri_plugin_fs::FilePath, keep_file: bool) -> Result<String, String> {
    use tauri_plugin_fs::FilePath;
    match file {
        FilePath::Path(p) => path_to_string(p),
        FilePath::Url(u) if u.scheme() == "file" && !keep_file => u
            .to_file_path()
            .map_err(|()| format!("{u} is not a file on this device"))
            .and_then(path_to_string),
        FilePath::Url(u) => Ok(u.to_string()),
    }
}

/// Whether what the page calls a file is an address only the system can
/// open (see [`chosen`]) rather than a path. A Windows drive letter reads
/// as a one-letter scheme and is a path; so is a `file:` address, except
/// on iOS.
pub fn is_address(name: &str) -> bool {
    is_address_on(name, KEEP_FILE_ADDRESSES)
}

/// [`is_address`], with the platform's answer about `file:` said outright.
pub fn is_address_on(name: &str, keep_file: bool) -> bool {
    use std::str::FromStr;
    matches!(
        tauri_plugin_fs::FilePath::from_str(name),
        Ok(tauri_plugin_fs::FilePath::Url(u)) if u.scheme() != "file" || keep_file
    )
}

/// The file name a save panel should offer, with the extension the
/// first filter wants if the name has none of its own. Some panels add
/// it and some do not; this way they all start from the same name.
pub fn offered_name(name: &str, filters: &[Filter]) -> String {
    let name = name.trim();
    let name = if name.is_empty() { "untitled" } else { name };
    match filters.first().and_then(|f| f.extensions.first()) {
        Some(ext) if !has_extension(name, ext) => format!("{name}.{ext}"),
        _ => name.to_string(),
    }
}

/// The path a save panel answered with, made to end the way the file
/// is: a panel that lets the extension be typed away would otherwise
/// write a PNG nothing will open as one.
pub fn with_extension(path: PathBuf, filters: &[Filter]) -> PathBuf {
    let Some(ext) = filters.first().and_then(|f| f.extensions.first()) else {
        return path;
    };
    let fits = filters
        .iter()
        .flat_map(|f| &f.extensions)
        .any(|e| path.extension().is_some_and(|x| x.eq_ignore_ascii_case(e)));
    if fits {
        return path;
    }
    let mut named = path.into_os_string();
    named.push(format!(".{ext}"));
    PathBuf::from(named)
}

/// Which of these paths something is already at. A save panel asks
/// about replacing a file itself; writing several into a folder has no
/// panel to ask, so the app asks, and this is what it asks about.
pub fn already_there(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| fs::symlink_metadata(p).is_ok())
        .cloned()
        .collect()
}

fn has_extension(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_content_address_is_kept_as_it_came_and_a_path_is_a_path() {
        use std::str::FromStr;
        use tauri_plugin_fs::FilePath;
        let content = "content://com.android.providers.downloads.documents/document/msf%3A1000";
        let chosen_content = chosen(FilePath::from_str(content).unwrap()).unwrap();
        assert_eq!(chosen_content, content);
        assert!(is_address(&chosen_content));

        let path = chosen(FilePath::Path(PathBuf::from("/tmp/a b.png"))).unwrap();
        assert_eq!(path, "/tmp/a b.png");
        assert!(!is_address(&path));

        // A file: address is a path by another name.
        let file = chosen(FilePath::from_str("file:///tmp/a%20b.png").unwrap()).unwrap();
        assert_eq!(file, "/tmp/a b.png");
        assert!(!is_address(&file));

        // On iOS a file: address is kept, to be opened under its grant.
        let kept = chosen_on(
            FilePath::from_str("file:///var/mobile/a%20b.png").unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(kept, "file:///var/mobile/a%20b.png");
        assert!(is_address_on(&kept, true));
        assert!(!is_address_on(&kept, false));
        assert!(!is_address_on("/var/mobile/a.png", true));

        // A drive letter is not a scheme.
        assert!(!is_address("C:\\Users\\me\\a.chitra"));
        assert!(!is_address("relative/a.png"));
    }

    use super::*;

    fn png() -> Vec<Filter> {
        vec![Filter {
            name: "PNG image".into(),
            extensions: vec!["png".into()],
        }]
    }

    #[test]
    fn a_whole_write_replaces_the_file_and_leaves_nothing_beside_it() {
        let dir = std::env::temp_dir().join(format!("chitrakar-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("poster.chitra");
        fs::write(&path, b"what was there").unwrap();
        write_whole(&path, b"what is there now").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"what is there now");
        let left: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1, "a part file was left behind: {left:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_write_that_cannot_land_leaves_the_old_file_alone() {
        let dir = std::env::temp_dir().join(format!("chitrakar-files-dir-{}", std::process::id()));
        fs::create_dir_all(dir.join("in-the-way")).unwrap();
        // A directory where the file would go: the rename cannot happen,
        // and the directory is still the directory afterwards.
        assert!(write_whole(&dir.join("in-the-way"), b"bytes").is_err());
        assert!(dir.join("in-the-way").is_dir());
        let left: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1, "a part file was left behind: {left:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_is_already_there_is_named_and_nothing_else() {
        let dir =
            std::env::temp_dir().join(format!("chitrakar-files-there-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
        fs::write(at("poster@1x.png"), b"png").unwrap();
        fs::create_dir_all(at("poster@3x.png")).unwrap();
        let asked = [
            at("poster@1x.png"),
            at("poster@2x.png"),
            at("poster@3x.png"),
        ];
        // A folder in the way is in the way as much as a file is.
        assert_eq!(
            already_there(&asked),
            vec![at("poster@1x.png"), at("poster@3x.png")]
        );
        assert!(already_there(&[]).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_path_survives_the_header() {
        // What `encodeURIComponent` makes of a path with spaces, accents
        // and a character outside the first plane in it.
        let encoded = "%2Fhome%2Fan%C3%A9%2FPosters%20%F0%9F%8E%A8%2Fsummer%20(2).chitra";
        assert_eq!(
            path_from_header(encoded).unwrap(),
            PathBuf::from("/home/ané/Posters 🎨/summer (2).chitra")
        );
        assert_eq!(
            path_from_header("C%3A%5CUsers%5Cme%5Ca.chitra").unwrap(),
            PathBuf::from("C:\\Users\\me\\a.chitra")
        );
        assert!(path_from_header("").is_err());
        assert!(path_from_header("%2").is_err());
        assert!(path_from_header("%zz").is_err());
        // Bytes that are not UTF-8 are not quietly some other name.
        assert!(path_from_header("%FF").is_err());
    }

    #[test]
    fn a_save_panel_offers_the_name_with_its_extension() {
        assert_eq!(offered_name("poster", &png()), "poster.png");
        assert_eq!(offered_name("poster.PNG", &png()), "poster.PNG");
        assert_eq!(offered_name("  ", &png()), "untitled.png");
        assert_eq!(offered_name("poster", &[]), "poster");
        // A dot in a name is not an extension of the right kind.
        assert_eq!(offered_name("v1.2", &png()), "v1.2.png");
    }

    #[test]
    fn a_name_typed_without_its_extension_gets_it_back() {
        assert_eq!(
            with_extension(PathBuf::from("/a/poster"), &png()),
            PathBuf::from("/a/poster.png")
        );
        assert_eq!(
            with_extension(PathBuf::from("/a/poster.Png"), &png()),
            PathBuf::from("/a/poster.Png")
        );
        let either = vec![Filter {
            name: "JPEG image".into(),
            extensions: vec!["jpg".into(), "jpeg".into()],
        }];
        assert_eq!(
            with_extension(PathBuf::from("/a/p.jpeg"), &either),
            PathBuf::from("/a/p.jpeg")
        );
        assert_eq!(
            with_extension(PathBuf::from("/a/p.png"), &either),
            PathBuf::from("/a/p.png.jpg")
        );
    }
}
