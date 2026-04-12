//! HtmlExtractor: strips tags and renders HTML as plain text for indexing.
//!
//! Uses `html2text` which produces readable output (handles nested tags,
//! lists, tables, etc). Output is wrapped at a fixed column width — we pick
//! a wide width so wrapping effectively means "one line per paragraph".

use std::fs;
use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

/// Width passed to html2text. Wide enough that it almost never wraps
/// within a paragraph, so downstream search matches full sentences.
const RENDER_WIDTH: usize = 10_000;

pub struct HtmlExtractor {
    max_size: u64,
}

impl HtmlExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for HtmlExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &[
    "text/html",
    "application/xhtml+xml",
];

impl Extractor for HtmlExtractor {
    fn name(&self) -> &'static str {
        "Html"
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

        let bytes = fs::read(path)?;
        let text = html2text::from_read(&bytes[..], RENDER_WIDTH)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

        Ok(ExtractionResult {
            body_text: text,
            // html2text takes care of decoding via html5ever; we report the
            // container format rather than guessing the original byte encoding.
            encoding: Some("UTF-8".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_html_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_basic_html() {
        let html = b"<html><body><h1>Title</h1><p>Hello <b>world</b></p></body></html>";
        let p = tmp("basic.html", html);
        let e = HtmlExtractor::new();
        let result = e.extract(&p).unwrap();
        assert!(result.body_text.contains("Title"));
        assert!(result.body_text.contains("Hello"));
        assert!(result.body_text.contains("world"));
        // No raw tags
        assert!(!result.body_text.contains("<h1>"));
        assert!(!result.body_text.contains("<p>"));
    }

    #[test]
    fn test_extract_strips_script_and_style() {
        let html = br#"<html><head><style>body { color: red; }</style><script>alert(1)</script></head><body><p>content</p></body></html>"#;
        let p = tmp("scripted.html", html);
        let e = HtmlExtractor::new();
        let result = e.extract(&p).unwrap();
        assert!(result.body_text.contains("content"));
        // Script/style content should not be in the rendered text
        assert!(!result.body_text.contains("alert"));
        assert!(!result.body_text.contains("color: red"));
    }

    #[test]
    fn test_extract_preserves_utf8_text() {
        let html = "<html><body><p>你好世界</p></body></html>".as_bytes();
        let p = tmp("zh.html", html);
        let e = HtmlExtractor::new();
        let result = e.extract(&p).unwrap();
        assert!(result.body_text.contains("你好世界"));
    }

    #[test]
    fn test_extract_file_too_large() {
        let p = tmp("big.html", &vec![b'a'; 100]);
        let e = HtmlExtractor { max_size: 50 };
        let err = e.extract(&p).unwrap_err();
        assert!(matches!(err, ExtractionError::FileTooLarge { .. }));
    }
}
