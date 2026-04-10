//! Extractor trait and error types.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// Extracted UTF-8 text (already decoded from source encoding if needed).
    pub body_text: String,
    /// Original encoding if decoding occurred, e.g. "UTF-8", "GBK", "Shift_JIS".
    pub encoding: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum ExtractionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("encoding detection failed")]
    EncodingDetectionFailed,
    #[error("file too large: {size} bytes (limit: {limit})")]
    FileTooLarge { size: u64, limit: u64 },
    #[error("extraction failed: {0}")]
    Other(String),
}

pub trait Extractor: Send + Sync {
    /// Stable identifier used in DB records and logs (e.g. "PlainText").
    fn name(&self) -> &'static str;

    /// MIME types this extractor claims to handle.
    fn supported_mimes(&self) -> &[&'static str];

    /// Perform extraction.
    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyExtractor;
    impl Extractor for DummyExtractor {
        fn name(&self) -> &'static str {
            "Dummy"
        }
        fn supported_mimes(&self) -> &[&'static str] {
            &["text/plain"]
        }
        fn extract(&self, _path: &Path) -> Result<ExtractionResult, ExtractionError> {
            Ok(ExtractionResult {
                body_text: "hello".to_string(),
                encoding: Some("UTF-8".to_string()),
            })
        }
    }

    #[test]
    fn test_extractor_trait_object() {
        let e: Box<dyn Extractor> = Box::new(DummyExtractor);
        assert_eq!(e.name(), "Dummy");
        assert_eq!(e.supported_mimes(), &["text/plain"]);
    }
}
