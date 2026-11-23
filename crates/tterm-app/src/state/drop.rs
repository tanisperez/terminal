//! What the drop overlay (design D) says about the files being dragged over a terminal: the
//! name, the size and, for a PNG, its dimensions, read from its header (24 bytes, never the
//! whole file).

use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The image extensions the overlay shows a thumbnail of.
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// What is shown while files are dragged over the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPreview {
    /// The files.
    pub paths: Vec<PathBuf>,
    /// The size of the first file, in bytes.
    pub size: Option<u64>,
    /// The width and height of the first file, when it is a PNG.
    pub dimensions: Option<(u32, u32)>,
}

impl DropPreview {
    /// Reads what the overlay needs about `paths`.
    pub fn read(paths: &[PathBuf]) -> Self {
        let first = paths.first();
        Self {
            paths: paths.to_vec(),
            size: first
                .and_then(|path| std::fs::metadata(path).ok())
                .map(|m| m.len()),
            dimensions: first.and_then(|path| png_dimensions(path)),
        }
    }

    /// The image to show as a thumbnail: the only file, when it is an image.
    pub fn image(&self) -> Option<&Path> {
        match self.paths.as_slice() {
            [path] if is_image(path) => Some(path),
            _ => None,
        }
    }

    /// The name of the only file, `None` for several.
    pub fn name(&self) -> Option<String> {
        match self.paths.as_slice() {
            [path] => Some(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string()),
            ),
            _ => None,
        }
    }
}

/// Whether `path` has an image extension.
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The width and height in a PNG's IHDR chunk, `None` for anything else.
pub fn png_dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut header = [0u8; 24];
    File::open(path).ok()?.read_exact(&mut header).ok()?;
    parse_png_header(&header)
}

fn parse_png_header(header: &[u8; 24]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if header[..8] != SIGNATURE || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]);
    let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);
    Some((width, height))
}

/// A file size for people: `950 B`, `184 KB`, `3.4 MB`.
pub fn size_label(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn png_header(width: u32, height: u32) -> [u8; 24] {
        let mut header = [0u8; 24];
        header[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        header[8..12].copy_from_slice(&13u32.to_be_bytes());
        header[12..16].copy_from_slice(b"IHDR");
        header[16..20].copy_from_slice(&width.to_be_bytes());
        header[20..24].copy_from_slice(&height.to_be_bytes());
        header
    }

    #[test]
    fn a_png_header_gives_its_size() {
        assert_eq!(
            parse_png_header(&png_header(2880, 1800)),
            Some((2880, 1800))
        );
        assert_eq!(parse_png_header(&[0u8; 24]), None);
    }

    #[test]
    fn a_dropped_png_is_read_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Captura de pantalla.png");
        let mut bytes = png_header(640, 480).to_vec();
        bytes.extend([0u8; 1000]);
        std::fs::write(&path, &bytes).unwrap();
        let preview = DropPreview::read(std::slice::from_ref(&path));
        assert_eq!(preview.dimensions, Some((640, 480)));
        assert_eq!(preview.size, Some(1024));
        assert_eq!(preview.image(), Some(path.as_path()));
        assert_eq!(preview.name().as_deref(), Some("Captura de pantalla.png"));
    }

    #[test]
    fn several_files_have_no_name_nor_thumbnail() {
        let preview = DropPreview::read(&[PathBuf::from("/a.png"), PathBuf::from("/b.txt")]);
        assert_eq!(preview.name(), None);
        assert_eq!(preview.image(), None);
        assert!(is_image(Path::new("x.JPG")));
        assert!(!is_image(Path::new("x.rs")));
    }

    #[test]
    fn sizes_read_for_people() {
        assert_eq!(size_label(950), "950 B");
        assert_eq!(size_label(188_416), "184 KB");
        assert_eq!(size_label(3_565_158), "3.4 MB");
    }
}
