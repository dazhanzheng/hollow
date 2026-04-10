//! Format detection via magic bytes.

use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DetectedFormat {
    /// MIME type from magic bytes detection, or fallback from extension/heuristic.
    pub mime: String,
    /// Suggested extension from magic bytes (e.g. "png", "pdf"), if identifiable.
    pub extension_hint: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum DetectionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct FormatDetector;

impl FormatDetector {
    /// Read up to 8 KiB from the file head and detect format.
    pub fn detect(path: &Path) -> Result<DetectedFormat, DetectionError> {
        let mut file = fs::File::open(path)?;
        let mut head = vec![0u8; 8192];
        let n = file.read(&mut head)?;
        head.truncate(n);

        // Try magic bytes first
        if let Some(kind) = infer::get(&head) {
            return Ok(DetectedFormat {
                mime: kind.mime_type().to_string(),
                extension_hint: Some(kind.extension().to_string()),
            });
        }

        // Fallback: heuristic text check
        let mime = if is_plausibly_text(&head) {
            "text/plain".to_string()
        } else {
            "application/octet-stream".to_string()
        };

        Ok(DetectedFormat {
            mime,
            extension_hint: None,
        })
    }
}

/// Heuristic: a buffer is plausibly text if it's either valid UTF-8 or
/// contains no NUL bytes and no more than 5% non-printable non-whitespace bytes.
fn is_plausibly_text(buf: &[u8]) -> bool {
    if buf.is_empty() {
        return true;
    }
    if std::str::from_utf8(buf).is_ok() {
        return true;
    }
    if buf.contains(&0) {
        return false;
    }
    let bad = buf
        .iter()
        .filter(|&&b| !(b.is_ascii_graphic() || b.is_ascii_whitespace()))
        .count();
    (bad * 100) / buf.len() <= 5
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_detector_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::File::create(&path).unwrap().write_all(content).unwrap();
        path
    }

    #[test]
    fn test_detect_png_by_magic() {
        // PNG magic: 89 50 4E 47 0D 0A 1A 0A
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        let path = tmp_file("fake.txt", &png); // wrong extension!
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "image/png");
        assert_eq!(detected.extension_hint.as_deref(), Some("png"));
    }

    #[test]
    fn test_detect_plain_text_fallback() {
        let path = tmp_file("note.txt", b"hello world\nhow are you?");
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "text/plain");
    }

    #[test]
    fn test_detect_empty_file() {
        let path = tmp_file("empty.txt", b"");
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "text/plain");
    }

    #[test]
    fn test_detect_binary_without_magic() {
        // Random binary with NUL bytes
        let path = tmp_file("blob.bin", &[0xFF, 0x00, 0x01, 0x02, 0xFE]);
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "application/octet-stream");
    }

    #[test]
    fn test_detect_gbk_text_bytes() {
        // GBK-encoded "你好" = C4 E3 BA C3 (no NUL bytes, but all high bytes).
        // The heuristic treats an all-high-byte buffer as binary (100% "bad" ratio),
        // which is the honest result for a 4-byte sample with no ASCII context.
        let path = tmp_file("gbk.txt", &[0xC4, 0xE3, 0xBA, 0xC3]);
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "application/octet-stream");
    }
}
