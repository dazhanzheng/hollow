//! SourceCodeExtractor: handles source code files via MIME or extension.

use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::{read_text_file, DEFAULT_MAX_FILE_SIZE};

pub struct SourceCodeExtractor {
    max_size: u64,
}

impl SourceCodeExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }

    /// Extensions this extractor can handle as a fallback when MIME is unclear.
    pub fn known_extensions() -> &'static [&'static str] {
        &[
            "py", "js", "ts", "jsx", "tsx", "rs", "swift", "go", "java", "kt",
            "scala", "c", "cc", "cpp", "cxx", "h", "hh", "hpp", "m", "mm",
            "rb", "sh", "bash", "zsh", "fish", "sql", "html", "htm", "css",
            "scss", "sass", "less", "vue", "svelte", "lua", "pl", "pm", "php",
            "r", "dart", "ex", "exs", "erl", "hs", "clj", "cljs", "edn",
        ]
    }
}

impl Default for SourceCodeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED_MIMES: &[&str] = &[
    "text/x-python",
    "text/x-rust",
    "text/x-go",
    "text/x-swift",
    "text/x-java",
    "text/x-c",
    "text/x-c++",
    "text/x-shellscript",
    "text/x-ruby",
    "application/javascript",
    "application/typescript",
    "text/javascript",
    "text/typescript",
    "text/html",
    "text/css",
];

impl Extractor for SourceCodeExtractor {
    fn name(&self) -> &'static str {
        "SourceCode"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED_MIMES
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
        let dir = std::env::temp_dir().join("hollow_src_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_rust_file() {
        let p = tmp("main.rs", b"fn main() { println!(\"hi\"); }");
        let e = SourceCodeExtractor::new();
        let result = e.extract(&p).unwrap();
        assert!(result.body_text.contains("fn main"));
    }

    #[test]
    fn test_known_extensions_includes_common_langs() {
        let exts = SourceCodeExtractor::known_extensions();
        for e in ["py", "rs", "swift", "ts", "go"] {
            assert!(exts.contains(&e), "missing ext {}", e);
        }
    }
}
