//! ContentPipeline: runs detection → routing → extraction for one file.

use std::path::Path;

use crate::content::detector::FormatDetector;
use crate::content::registry::ExtractorRegistry;

#[derive(Debug, Clone)]
pub struct ExtractionOutcome {
    pub status: String,  // "indexed" or "extract_failed"
    pub extractor_name: Option<String>,
    pub body_text: Option<String>,
    pub encoding: Option<String>,
    pub detected_mime: String,
    pub extension_mismatch: bool,
    pub error: Option<String>,
}

pub struct ContentPipeline {
    registry: ExtractorRegistry,
}

impl ContentPipeline {
    pub fn new(registry: ExtractorRegistry) -> Self {
        Self { registry }
    }

    /// Run detection + extraction for one file. Never panics; errors are captured.
    pub fn process(&self, path: &Path, original_extension: Option<&str>) -> ExtractionOutcome {
        // Step 1: Detect format
        let detected = match FormatDetector::detect(path) {
            Ok(d) => d,
            Err(e) => {
                return ExtractionOutcome {
                    status: "extract_failed".to_string(),
                    extractor_name: None,
                    body_text: None,
                    encoding: None,
                    detected_mime: "application/octet-stream".to_string(),
                    extension_mismatch: false,
                    error: Some(format!("detection failed: {}", e)),
                };
            }
        };

        // Step 2: Check extension mismatch
        let extension_mismatch = match (original_extension, &detected.extension_hint) {
            (Some(orig), Some(hint)) => !orig.eq_ignore_ascii_case(hint),
            _ => false,
        };

        // Step 3: Find extractor — first by MIME, then by extension fallback,
        // then by exact basename (for no-extension files like Dockerfile).
        //
        // When the detected MIME is the generic text/plain heuristic fallback
        // (no magic-bytes match, so extension_hint is None), we prefer the
        // more specific routes (basename → extension) over the generic
        // PlainText MIME route, since the MIME route is just a catch-all.
        let mime_extractor = self.registry.find_by_mime(&detected.mime);
        let ext_extractor = original_extension
            .and_then(|ext| self.registry.find_by_extension(ext));
        let basename_extractor = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| self.registry.find_by_basename(n));

        let extractor = if detected.mime == "text/plain" && detected.extension_hint.is_none() {
            // Heuristic text fallback — prefer specific routes first.
            basename_extractor
                .or(ext_extractor)
                .or(mime_extractor)
        } else if extension_mismatch {
            // Magic bytes say one format, filename says another. Trust the
            // magic bytes — the extension is already proven to be lying, so
            // falling back to extension-based routing would read binary
            // content as text and index garbage. If there's no extractor for
            // the real format, report unsupported and let the mismatch flag
            // surface it to the user.
            mime_extractor
        } else {
            mime_extractor
                .or(ext_extractor)
                .or(basename_extractor)
        };

        let extractor = match extractor {
            Some(e) => e,
            None => {
                return ExtractionOutcome {
                    status: "unsupported".to_string(),
                    extractor_name: None,
                    body_text: None,
                    encoding: None,
                    detected_mime: detected.mime.clone(),
                    extension_mismatch,
                    error: Some(format!("no extractor for mime: {}", detected.mime)),
                };
            }
        };

        let extractor_name = extractor.name().to_string();

        // Respect user-disabled extractors from the settings UI.
        if crate::content::registry::is_extractor_disabled(&extractor_name) {
            return ExtractionOutcome {
                status: "unsupported".to_string(),
                extractor_name: Some(extractor_name),
                body_text: None,
                encoding: None,
                detected_mime: detected.mime,
                extension_mismatch,
                error: Some("extractor disabled in settings".to_string()),
            };
        }

        // Step 4: Run extraction, catching any panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extractor.extract(path)
        }));

        match result {
            Ok(Ok(res)) => ExtractionOutcome {
                status: "indexed".to_string(),
                extractor_name: Some(extractor_name),
                body_text: Some(res.body_text),
                encoding: res.encoding,
                detected_mime: detected.mime,
                extension_mismatch,
                error: None,
            },
            Ok(Err(e)) => ExtractionOutcome {
                status: "extract_failed".to_string(),
                extractor_name: Some(extractor_name),
                body_text: None,
                encoding: None,
                detected_mime: detected.mime,
                extension_mismatch,
                error: Some(e.to_string()),
            },
            Err(_) => ExtractionOutcome {
                status: "extract_failed".to_string(),
                extractor_name: Some(extractor_name),
                body_text: None,
                encoding: None,
                detected_mime: detected.mime,
                extension_mismatch,
                error: Some("extractor panicked".to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::registry::default_registry;
    use std::fs;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_pipeline_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_process_plain_text_success() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("note.txt", b"hello world");
        let outcome = pipeline.process(&p, Some("txt"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.body_text.as_deref(), Some("hello world"));
        assert_eq!(outcome.extractor_name.as_deref(), Some("PlainText"));
        assert!(!outcome.extension_mismatch);
    }

    #[test]
    fn test_process_rust_source_by_extension() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("main.rs", b"fn main() {}");
        let outcome = pipeline.process(&p, Some("rs"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.extractor_name.as_deref(), Some("SourceCode"));
    }

    #[test]
    fn test_process_extension_mismatch() {
        let pipeline = ContentPipeline::new(default_registry());
        // PNG magic bytes in a .txt file
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        let p = tmp("fake.txt", &png);
        let outcome = pipeline.process(&p, Some("txt"));
        assert!(outcome.extension_mismatch);
        assert_eq!(outcome.status, "unsupported"); // no image extractor
    }

    #[test]
    fn test_process_unknown_format() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("blob.bin", &[0xFF, 0xFE, 0x00, 0x01]);
        let outcome = pipeline.process(&p, Some("bin"));
        assert_eq!(outcome.status, "unsupported");
        assert!(outcome.error.is_some());
    }

    #[test]
    fn test_process_dockerfile_by_basename() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp(
            "Dockerfile",
            b"FROM alpine:3.20\nRUN apk add --no-cache curl\nCMD [\"sh\"]",
        );
        // No extension — routing must come via basename lookup.
        let outcome = pipeline.process(&p, None);
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.extractor_name.as_deref(), Some("SourceCode"));
        assert!(outcome.body_text.as_deref().unwrap().contains("alpine"));
    }

    #[test]
    fn test_process_ini_file_by_extension() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("settings.ini", b"[core]\nname=hollow\nversion=0.1");
        let outcome = pipeline.process(&p, Some("ini"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.extractor_name.as_deref(), Some("PlainText"));
        assert!(outcome.body_text.as_deref().unwrap().contains("hollow"));
    }

    #[test]
    fn test_process_ics_calendar_by_extension() {
        let pipeline = ContentPipeline::new(default_registry());
        let ics = b"BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nSUMMARY:team sync\nEND:VEVENT\nEND:VCALENDAR\n";
        let p = tmp("meeting.ics", ics);
        let outcome = pipeline.process(&p, Some("ics"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.extractor_name.as_deref(), Some("PlainText"));
        assert!(outcome.body_text.as_deref().unwrap().contains("team sync"));
    }

    #[test]
    fn test_process_terraform_by_extension() {
        let pipeline = ContentPipeline::new(default_registry());
        let tf = b"resource \"aws_s3_bucket\" \"main\" {\n  bucket = \"hollow-bucket\"\n}\n";
        let p = tmp("infra.tf", tf);
        let outcome = pipeline.process(&p, Some("tf"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.extractor_name.as_deref(), Some("SourceCode"));
    }
}
