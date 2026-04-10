//! Shared helpers for text-reading extractors.

use std::fs;
use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult};

pub const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

/// Read an entire file, detect encoding, and return UTF-8 text.
pub fn read_text_file(path: &Path, max_size: u64) -> Result<ExtractionResult, ExtractionError> {
    let metadata = fs::metadata(path)?;
    let size = metadata.len();
    if size > max_size {
        return Err(ExtractionError::FileTooLarge {
            size,
            limit: max_size,
        });
    }

    let bytes = fs::read(path)?;

    // Fast path: already valid UTF-8
    if let Ok(s) = std::str::from_utf8(&bytes) {
        return Ok(ExtractionResult {
            body_text: s.to_string(),
            encoding: Some("UTF-8".to_string()),
        });
    }

    // Slow path: detect encoding with chardetng
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(&bytes, true);
    let encoding = detector.guess(None, true);
    let (decoded, _, had_errors) = encoding.decode(&bytes);

    if had_errors {
        return Err(ExtractionError::EncodingDetectionFailed);
    }

    Ok(ExtractionResult {
        body_text: decoded.into_owned(),
        encoding: Some(encoding.name().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_common_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_read_utf8_ascii() {
        let p = tmp("ascii.txt", b"hello world");
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "hello world");
        assert_eq!(result.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn test_read_utf8_chinese() {
        let p = tmp("zh.txt", "你好世界".as_bytes());
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "你好世界");
        assert_eq!(result.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn test_read_gbk_chinese() {
        // "你好世界，这是一个测试文件，用于检测GBK编码。" in GBK (45 bytes).
        // A longer string is needed so chardetng has enough signal to distinguish
        // GBK from other CJK encodings like EUC-KR.
        let gbk_bytes: &[u8] = &[
            0xC4, 0xE3, 0xBA, 0xC3, 0xCA, 0xC0, 0xBD, 0xE7, 0xA3, 0xAC, 0xD5, 0xE2, 0xCA,
            0xC7, 0xD2, 0xBB, 0xB8, 0xF6, 0xB2, 0xE2, 0xCA, 0xD4, 0xCE, 0xC4, 0xBC, 0xFE,
            0xA3, 0xAC, 0xD3, 0xC3, 0xD3, 0xDA, 0xBC, 0xEC, 0xB2, 0xE2, 0x47, 0x42, 0x4B,
            0xB1, 0xE0, 0xC2, 0xEB, 0xA1, 0xA3,
        ];
        let p = tmp("gbk.txt", gbk_bytes);
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert!(
            result.body_text.starts_with("你好"),
            "expected decoded text to start with '你好', got: {:?}",
            result.body_text
        );
        // chardetng should pick GBK or GB18030 — label string may vary
        let enc = result.encoding.unwrap();
        assert!(
            enc == "GBK" || enc == "GB18030" || enc.to_lowercase().contains("gb"),
            "unexpected encoding: {enc}"
        );
    }

    #[test]
    fn test_file_too_large() {
        let p = tmp("big.txt", &vec![b'a'; 100]);
        let err = read_text_file(&p, 50).unwrap_err();
        assert!(matches!(err, ExtractionError::FileTooLarge { .. }));
    }

    #[test]
    fn test_empty_file() {
        let p = tmp("empty.txt", b"");
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "");
    }
}
