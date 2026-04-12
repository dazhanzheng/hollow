//! EpubExtractor: extracts body text from EPUB ebooks.
//!
//! An .epub is a ZIP archive containing:
//!   - META-INF/container.xml (points to the OPF package file)
//!   - <package>.opf  (manifest + spine — the reading order)
//!   - XHTML chapter files (the actual content)
//!
//! A "proper" implementation would follow container.xml → OPF → spine to
//! assemble chapters in reading order. For full-text search indexing that's
//! overkill: every spine item is an XHTML file inside the archive, so we
//! simply iterate every `.xhtml` / `.html` / `.htm` entry in the zip and
//! pipe its bytes through `html2text`, concatenating the results.
//!
//! This yields text suitable for search. Reading order is lost, but the
//! words are all there — which is what matters for indexing.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

/// Same wide width used by HtmlExtractor — effectively no wrapping.
const RENDER_WIDTH: usize = 10_000;

pub struct EpubExtractor {
    max_size: u64,
}

impl EpubExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for EpubExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &["application/epub+zip"];

impl Extractor for EpubExtractor {
    fn name(&self) -> &'static str {
        "Epub"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED
    }

    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();
        if size > self.max_size {
            return Err(ExtractionError::FileTooLarge {
                size,
                limit: self.max_size,
            });
        }

        let file = fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

        // Sort entry names so output order is deterministic. Real spine order
        // is lost either way, but a stable order makes tests and diffing sane.
        let mut html_entries: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|name| is_html_entry(name))
            .collect();
        html_entries.sort();

        let mut out = String::new();
        for name in html_entries {
            let mut entry = archive
                .by_name(&name)
                .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;

            // Skip empty / stub files quietly.
            if bytes.is_empty() {
                continue;
            }

            let text = html2text::from_read(&bytes[..], RENDER_WIDTH)
                .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

            if !text.trim().is_empty() {
                out.push_str(&text);
                out.push('\n');
            }
        }

        Ok(ExtractionResult {
            body_text: out,
            encoding: Some("UTF-8".to_string()),
        })
    }
}

fn is_html_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }

    /// Build a minimal .epub containing the given chapter HTML bodies.
    fn make_epub(chapters: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_epub_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("book-{}.epub", uuid_like()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // Mimetype (conventional — not strictly required for us, but real).
        zip.start_file("mimetype", opts.clone()).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        // Chapters
        for (name, body) in chapters {
            zip.start_file(format!("OEBPS/{}", name), opts.clone()).unwrap();
            let html = format!(
                "<?xml version=\"1.0\"?><html><body>{}</body></html>",
                body
            );
            zip.write_all(html.as_bytes()).unwrap();
        }

        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_extract_single_chapter() {
        let p = make_epub(&[("ch01.xhtml", "<p>Hello from chapter one.</p>")]);
        let e = EpubExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("Hello from chapter one"));
    }

    #[test]
    fn test_extract_multiple_chapters_concatenated() {
        let p = make_epub(&[
            ("ch01.xhtml", "<p>First chapter text.</p>"),
            ("ch02.xhtml", "<p>Second chapter text.</p>"),
            ("ch03.xhtml", "<p>Third chapter text.</p>"),
        ]);
        let e = EpubExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("First chapter text"));
        assert!(r.body_text.contains("Second chapter text"));
        assert!(r.body_text.contains("Third chapter text"));
    }

    #[test]
    fn test_extract_ignores_non_html_entries() {
        // Build an epub with one HTML chapter + a non-HTML file.
        let dir = std::env::temp_dir().join("hollow_epub_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("mixed-{}.epub", uuid_like()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file("mimetype", opts.clone()).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("OEBPS/ch01.xhtml", opts.clone()).unwrap();
        zip.write_all(b"<html><body><p>included</p></body></html>").unwrap();
        zip.start_file("OEBPS/images/cover.jpg", opts.clone()).unwrap();
        zip.write_all(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]).unwrap();
        zip.start_file("OEBPS/styles.css", opts.clone()).unwrap();
        zip.write_all(b"body { color: red; }").unwrap();
        zip.finish().unwrap();

        let e = EpubExtractor::new();
        let r = e.extract(&path).unwrap();
        assert!(r.body_text.contains("included"));
        assert!(!r.body_text.contains("color: red"));
    }

    #[test]
    fn test_extract_handles_utf8_content() {
        let p = make_epub(&[("ch01.xhtml", "<p>你好世界</p>")]);
        let e = EpubExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("你好世界"));
    }
}
