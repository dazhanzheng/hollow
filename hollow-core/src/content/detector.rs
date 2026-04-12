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
            let mime = kind.mime_type();
            let ext = kind.extension();

            // `infer` detects the generic ZIP envelope but not zip-based
            // document formats (EPUB / DOCX / XLSX / PPTX / ODT / …) when
            // the archive's entry layout doesn't match infer's strict
            // byte-offset heuristics. Peek inside the archive to narrow
            // down — we already depend on the `zip` crate for extractors.
            if mime == "application/zip" {
                if let Some((sub_mime, sub_ext)) = detect_zip_variant(path) {
                    return Ok(DetectedFormat {
                        mime: sub_mime.to_string(),
                        extension_hint: Some(sub_ext.to_string()),
                    });
                }
            }

            return Ok(DetectedFormat {
                mime: mime.to_string(),
                extension_hint: Some(ext.to_string()),
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

/// When infer has classified a file as a generic ZIP, open it as a zip
/// archive and inspect the entries to identify zip-based document formats.
///
/// Returns `(mime, extension_hint)` on a match, or `None` if the zip is
/// either not a known variant or can't be opened.
fn detect_zip_variant(path: &Path) -> Option<(&'static str, &'static str)> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Snapshot entry names — central directory is already fully read at this
    // point, so `archive.len()` + `by_index` is cheap.
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    // OpenDocument / EPUB signal their format via a `mimetype` entry whose
    // contents are the IANA media type.
    if names.iter().any(|n| n == "mimetype") {
        if let Ok(mut entry) = archive.by_name("mimetype") {
            let mut s = String::new();
            if entry.read_to_string(&mut s).is_ok() {
                match s.trim() {
                    "application/epub+zip" => {
                        return Some(("application/epub+zip", "epub"));
                    }
                    "application/vnd.oasis.opendocument.text" => {
                        return Some((
                            "application/vnd.oasis.opendocument.text",
                            "odt",
                        ));
                    }
                    "application/vnd.oasis.opendocument.spreadsheet" => {
                        return Some((
                            "application/vnd.oasis.opendocument.spreadsheet",
                            "ods",
                        ));
                    }
                    "application/vnd.oasis.opendocument.presentation" => {
                        return Some((
                            "application/vnd.oasis.opendocument.presentation",
                            "odp",
                        ));
                    }
                    _ => {}
                }
            }
        }
    }

    // OOXML markers (Word / Excel / PowerPoint).
    if names.iter().any(|n| n == "word/document.xml") {
        return Some((
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        ));
    }
    if names.iter().any(|n| n == "xl/workbook.xml") {
        return Some((
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        ));
    }
    if names.iter().any(|n| n == "ppt/presentation.xml") {
        return Some((
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        ));
    }

    None
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
    fn test_detect_epub_via_zip_inspection() {
        // Build a minimal .epub-like zip with a `mimetype` entry whose
        // contents say it's an EPUB. The `infer` crate's strict byte-offset
        // epub check may or may not trigger on this synthetic file, but
        // either way our zip-inspection fallback should land on the
        // correct MIME.
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let dir = std::env::temp_dir().join("hollow_detector_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.epub");
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", opts.clone()).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("OEBPS/ch01.xhtml", opts.clone()).unwrap();
        zip.write_all(b"<html><body><p>hi</p></body></html>").unwrap();
        zip.finish().unwrap();

        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "application/epub+zip");
        assert_eq!(detected.extension_hint.as_deref(), Some("epub"));
    }

    #[test]
    fn test_detect_docx_via_zip_inspection() {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let dir = std::env::temp_dir().join("hollow_detector_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.docx");
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file("word/document.xml", opts.clone()).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?><w:document xmlns:w="x"><w:body><w:p><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(
            detected.mime,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(detected.extension_hint.as_deref(), Some("docx"));
    }

    #[test]
    fn test_detect_unknown_zip_stays_generic() {
        // A plain zip with nothing inside that signals a document format —
        // should remain classified as `application/zip`.
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let dir = std::env::temp_dir().join("hollow_detector_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("random.zip");
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file("payload.bin", opts.clone()).unwrap();
        zip.write_all(&[0x00, 0x01, 0x02, 0x03]).unwrap();
        zip.finish().unwrap();

        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "application/zip");
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
