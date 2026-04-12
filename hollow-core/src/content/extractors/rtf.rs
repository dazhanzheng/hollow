//! RtfExtractor: extracts plain text from Rich Text Format (.rtf) files.
//!
//! Uses the `rtf-parser` crate. RTF is relatively small so we read the whole
//! file to a `String` in one shot.

use std::fs;
use std::path::Path;

use rtf_parser::RtfDocument;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

pub struct RtfExtractor {
    max_size: u64,
}

impl RtfExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for RtfExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &["application/rtf", "text/rtf"];

impl Extractor for RtfExtractor {
    fn name(&self) -> &'static str {
        "Rtf"
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

        // RTF is 7-bit ASCII on the wire (non-ASCII characters are escaped),
        // so reading as UTF-8/Latin-1 is safe — `fs::read_to_string` accepts
        // any byte sequence that happens to be valid UTF-8, and rtf-parser's
        // internal lexer handles the escapes itself.
        let content = fs::read_to_string(path)?;

        let doc = RtfDocument::try_from(content.as_str())
            .map_err(|e| ExtractionError::Io(std::io::Error::other(format!("rtf parse: {:?}", e))))?;

        Ok(ExtractionResult {
            body_text: doc.get_text(),
            encoding: Some("UTF-8".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_rtf_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_simple_rtf() {
        let rtf = br"{\rtf1\ansi{\fonttbl\f0\fswiss Helvetica;}\f0\pard Hello \b world\b0 .\par }";
        let p = tmp("simple.rtf", rtf);
        let e = RtfExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("Hello"));
        assert!(r.body_text.contains("world"));
        // No control words in output
        assert!(!r.body_text.contains("\\b"));
        assert!(!r.body_text.contains("rtf1"));
    }

    #[test]
    fn test_extract_paragraphs() {
        // Two \par-separated paragraphs.
        let rtf = br"{\rtf1\ansi First paragraph.\par Second paragraph.\par }";
        let p = tmp("paragraphs.rtf", rtf);
        let e = RtfExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("First paragraph"));
        assert!(r.body_text.contains("Second paragraph"));
    }

    #[test]
    fn test_extract_file_too_large() {
        let p = tmp("big.rtf", &vec![b'a'; 200]);
        let e = RtfExtractor { max_size: 100 };
        let err = e.extract(&p).unwrap_err();
        assert!(matches!(err, ExtractionError::FileTooLarge { .. }));
    }
}
