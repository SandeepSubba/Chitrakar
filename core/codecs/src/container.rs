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

/// Serialize a document to `.chitra` bytes.
pub fn save_chitra(doc: &Document) -> Result<Vec<u8>, ContainerError> {
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        document: doc.clone(),
    };
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(MANIFEST_PATH, SimpleFileOptions::default())?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
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
    Ok(manifest.document)
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
