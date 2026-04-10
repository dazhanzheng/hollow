//! PlainTextExtractor: handles plain text, markdown, CSV, JSON, YAML, etc.

use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::{read_text_file, DEFAULT_MAX_FILE_SIZE};

pub struct PlainTextExtractor {
    max_size: u64,
}

impl PlainTextExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for PlainTextExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &[
    "text/plain",
    "text/markdown",
    "text/csv",
    "text/tab-separated-values",
    "text/x-log",
    "application/json",
    "application/xml",
    "text/xml",
    "application/yaml",
    "text/yaml",
    "application/toml",
    "text/toml",
];

impl Extractor for PlainTextExtractor {
    fn name(&self) -> &'static str {
        "PlainText"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED
    }

    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError> {
        read_text_file(path, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_plain_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_name_and_mimes() {
        let e = PlainTextExtractor::new();
        assert_eq!(e.name(), "PlainText");
        assert!(e.supported_mimes().contains(&"text/plain"));
        assert!(e.supported_mimes().contains(&"application/json"));
    }

    #[test]
    fn test_extract_utf8() {
        let p = tmp("greet.txt", "你好\nworld".as_bytes());
        let e = PlainTextExtractor::new();
        let result = e.extract(&p).unwrap();
        assert_eq!(result.body_text, "你好\nworld");
    }
}
