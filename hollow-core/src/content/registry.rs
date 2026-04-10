//! ExtractorRegistry: maps MIME types (and extensions) to Extractor implementations.

use std::collections::HashMap;
use std::sync::Arc;

use crate::content::extractor::Extractor;
use crate::content::extractors::plain_text::PlainTextExtractor;
use crate::content::extractors::source_code::SourceCodeExtractor;

pub struct ExtractorRegistry {
    by_mime: HashMap<String, Arc<dyn Extractor>>,
    by_extension: HashMap<String, Arc<dyn Extractor>>,
}

impl ExtractorRegistry {
    pub fn new() -> Self {
        Self {
            by_mime: HashMap::new(),
            by_extension: HashMap::new(),
        }
    }

    pub fn register(&mut self, extractor: Arc<dyn Extractor>) {
        for mime in extractor.supported_mimes() {
            self.by_mime
                .insert(mime.to_string(), Arc::clone(&extractor));
        }
    }

    pub fn register_with_extensions(
        &mut self,
        extractor: Arc<dyn Extractor>,
        extensions: &[&str],
    ) {
        self.register(Arc::clone(&extractor));
        for ext in extensions {
            self.by_extension
                .insert(ext.to_lowercase(), Arc::clone(&extractor));
        }
    }

    pub fn find_by_mime(&self, mime: &str) -> Option<Arc<dyn Extractor>> {
        self.by_mime.get(mime).cloned()
    }

    pub fn find_by_extension(&self, ext: &str) -> Option<Arc<dyn Extractor>> {
        self.by_extension.get(&ext.to_lowercase()).cloned()
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Default registry with first-batch extractors registered.
pub fn default_registry() -> ExtractorRegistry {
    let mut r = ExtractorRegistry::new();
    r.register(Arc::new(PlainTextExtractor::new()));
    r.register_with_extensions(
        Arc::new(SourceCodeExtractor::new()),
        SourceCodeExtractor::known_extensions(),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_finds_plain_text() {
        let r = default_registry();
        let e = r.find_by_mime("text/plain").unwrap();
        assert_eq!(e.name(), "PlainText");
    }

    #[test]
    fn test_default_registry_finds_source_code_by_mime() {
        let r = default_registry();
        let e = r.find_by_mime("text/x-rust").unwrap();
        assert_eq!(e.name(), "SourceCode");
    }

    #[test]
    fn test_default_registry_finds_source_by_extension() {
        let r = default_registry();
        let e = r.find_by_extension("py").unwrap();
        assert_eq!(e.name(), "SourceCode");
        let e = r.find_by_extension("RS").unwrap(); // case insensitive
        assert_eq!(e.name(), "SourceCode");
    }

    #[test]
    fn test_unknown_mime_returns_none() {
        let r = default_registry();
        assert!(r.find_by_mime("image/png").is_none());
    }
}
