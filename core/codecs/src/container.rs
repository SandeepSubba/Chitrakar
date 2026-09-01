//! The `.chitra` document container: a ZIP with a versioned JSON manifest
//! and (as they arrive) embedded source resources, ICC profiles, and
//! thumbnails (docs/PLAN.md §2).

use chitrakar_doc::Document;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Bumped on breaking manifest-schema changes; readers refuse newer majors.
pub const FORMAT_VERSION: u32 = 1;

const MANIFEST_PATH: &str = "manifest.json";

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest error: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("unsupported format version {found} (this build reads up to {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    document: Document,
}

/// Serialize a document to `.chitra` bytes. Resource pixels are stored as
/// PNG entries under `resources/`; the manifest carries only their metadata.
pub fn save_chitra(doc: &Document) -> Result<Vec<u8>, ContainerError> {
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        document: doc.clone(),
    };
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(MANIFEST_PATH, SimpleFileOptions::default())?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    for (id, res) in doc.resources() {
        if res.rgba8.is_empty() {
            continue; // metadata-only entry (bytes were never restored)
        }
        let png = crate::encode_png(res.width, res.height, &res.rgba8)
            .map_err(|e| ContainerError::Io(std::io::Error::other(e.to_string())))?;
        // PNG is already compressed; recompressing wastes time.
        zip.start_file(
            format!("resources/{id}.png"),
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )?;
        zip.write_all(&png)?;
    }
    Ok(zip.finish()?.into_inner())
}

/// Load a document from `.chitra` bytes.
pub fn load_chitra(bytes: &[u8]) -> Result<Document, ContainerError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let mut manifest_json = String::new();
    zip.by_name(MANIFEST_PATH)?
        .read_to_string(&mut manifest_json)?;
    let manifest: Manifest = serde_json::from_str(&manifest_json)?;
    if manifest.format_version > FORMAT_VERSION {
        return Err(ContainerError::UnsupportedVersion {
            found: manifest.format_version,
            supported: FORMAT_VERSION,
        });
    }
    let mut doc = manifest.document;
    let entries: Vec<String> = zip.file_names().map(String::from).collect();
    for name in entries {
        let Some(id) = name
            .strip_prefix("resources/")
            .and_then(|n| n.strip_suffix(".png"))
            .map(String::from)
        else {
            continue;
        };
        let mut png = Vec::new();
        zip.by_name(&name)?.read_to_end(&mut png)?;
        let img = crate::decode(&png)
            .map_err(|e| ContainerError::Io(std::io::Error::other(e.to_string())))?;
        // Silently ignores entries the manifest doesn't reference or whose
        // size disagrees — the manifest is the source of truth.
        doc.restore_resource_bytes(&id, img.rgba8);
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::ColorMode;
    use chitrakar_doc::{Command, Node, VectorShape};

    #[test]
    fn chitra_roundtrip_preserves_document() {
        let mut doc = Document::new(320, 240, ColorMode::Cmyk);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::vector(
                "shape",
                VectorShape::Ellipse { rx: 20.0, ry: 10.0 },
            )),
        })
        .unwrap();

        let bytes = save_chitra(&doc).unwrap();
        assert_eq!(&bytes[0..2], b"PK", "must be a zip container");

        let restored = load_chitra(&bytes).unwrap();
        assert_eq!(restored.node_count(), 2);
        assert_eq!(restored.meta.color_mode, ColorMode::Cmyk);
        assert_eq!(restored.meta.width, 320);
    }

    #[test]
    fn resources_roundtrip_through_container() {
        let mut doc = Document::new(64, 64, ColorMode::Rgb);
        let rgba8 = vec![
            10, 20, 30, 255, /**/ 40, 50, 60, 255, //
            70, 80, 90, 200, /**/ 0, 0, 0, 0,
        ];
        let id = doc.add_resource(2, 2, rgba8.clone());

        let bytes = save_chitra(&doc).unwrap();
        let restored = load_chitra(&bytes).unwrap();
        let res = restored.resource(&id).unwrap();
        assert_eq!((res.width, res.height), (2, 2));
        assert_eq!(res.rgba8, rgba8, "pixel bytes survive the PNG roundtrip");
    }

    #[test]
    fn newer_major_version_is_refused() {
        let doc = Document::new(8, 8, ColorMode::Rgb);
        let bytes = save_chitra(&doc).unwrap();
        let tampered = String::from_utf8({
            let mut zip = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
            let mut s = Vec::new();
            zip.by_name(MANIFEST_PATH)
                .unwrap()
                .read_to_end(&mut s)
                .unwrap();
            s
        })
        .unwrap()
        .replace("\"format_version\": 1", "\"format_version\": 99");

        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(MANIFEST_PATH, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(tampered.as_bytes()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();

        assert!(matches!(
            load_chitra(&bytes),
            Err(ContainerError::UnsupportedVersion { found: 99, .. })
        ));
    }
}
