//! DocxExtractor: extracts body text from Word .docx documents.
//!
//! A .docx file is a ZIP archive containing an Open Packaging Convention
//! structure. The primary document body lives in `word/document.xml`, and
//! the user-visible text is inside `<w:t>` elements (with `<w:tab/>` and
//! `<w:br/>` as whitespace markers and `<w:p>` ending paragraphs).
//!
//! This extractor deliberately avoids pulling in a full-featured docx crate
//! (e.g. `docx-rs`) which carries a large dependency tree. We only need text
//! for search indexing, so a streaming quick-xml pass over `word/document.xml`
//! is enough.

use std::fs;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

pub struct DocxExtractor {
    max_size: u64,
}

impl DocxExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for DocxExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
];

impl Extractor for DocxExtractor {
    fn name(&self) -> &'static str {
        "Docx"
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

        // Pull out word/document.xml. If it's not there, it's not a valid docx.
        let mut xml_bytes = Vec::new();
        {
            let mut entry = archive
                .by_name("word/document.xml")
                .map_err(|e| ExtractionError::Io(std::io::Error::other(
                    format!("word/document.xml missing: {}", e)
                )))?;
            entry.read_to_end(&mut xml_bytes)?;
        }

        let body_text = extract_text_from_document_xml(&xml_bytes)?;

        Ok(ExtractionResult {
            body_text,
            encoding: Some("UTF-8".to_string()),
        })
    }
}

/// Stream-parse a `word/document.xml` payload and concatenate the text nodes.
/// Emits a newline at the end of each `<w:p>` paragraph and a space for
/// `<w:tab/>` and `<w:br/>` so words don't run together.
fn extract_text_from_document_xml(xml: &[u8]) -> Result<String, ExtractionError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?
        {
            Event::Start(e) => {
                let name = e.name();
                if local_name_matches(name.as_ref(), b"t") {
                    in_text = true;
                }
            }
            Event::End(e) => {
                let name = e.name();
                if local_name_matches(name.as_ref(), b"t") {
                    in_text = false;
                } else if local_name_matches(name.as_ref(), b"p") {
                    // End of paragraph — break for readability / search tokenisation.
                    out.push('\n');
                }
            }
            Event::Empty(e) => {
                let name = e.name();
                if local_name_matches(name.as_ref(), b"tab")
                    || local_name_matches(name.as_ref(), b"br")
                {
                    out.push(' ');
                }
            }
            Event::Text(t) => {
                if in_text {
                    let txt = t
                        .decode()
                        .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
                    out.push_str(&txt);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

/// Match against the element local name, ignoring any XML namespace prefix
/// (docx uses `w:t`, `w:p`, etc).
fn local_name_matches(qname: &[u8], local: &[u8]) -> bool {
    let name = match qname.iter().position(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    };
    name == local
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    /// Build a minimal in-memory .docx file with the given body XML.
    fn make_docx(body_xml: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_docx_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("doc-{}.docx", uuid_like()));

        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>{}</w:body>
</w:document>"#,
            body_xml
        );

        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }

    #[test]
    fn test_extract_single_paragraph() {
        let p = make_docx(
            r#"<w:p><w:r><w:t>hello world</w:t></w:r></w:p>"#,
        );
        let e = DocxExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("hello world"));
    }

    #[test]
    fn test_extract_multiple_runs_join() {
        let p = make_docx(
            r#"<w:p><w:r><w:t>hello </w:t></w:r><w:r><w:t>world</w:t></w:r></w:p>"#,
        );
        let e = DocxExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("hello world"));
    }

    #[test]
    fn test_extract_multiple_paragraphs_separated_by_newline() {
        let p = make_docx(
            r#"<w:p><w:r><w:t>first</w:t></w:r></w:p><w:p><w:r><w:t>second</w:t></w:r></w:p>"#,
        );
        let e = DocxExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("first\n"));
        assert!(r.body_text.contains("second"));
    }

    #[test]
    fn test_extract_tab_and_break_become_spaces() {
        let p = make_docx(
            r#"<w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p>"#,
        );
        let e = DocxExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("a b c"));
    }

    #[test]
    fn test_extract_handles_utf8() {
        let p = make_docx(
            r#"<w:p><w:r><w:t>你好世界</w:t></w:r></w:p>"#,
        );
        let e = DocxExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("你好世界"));
    }

    #[test]
    fn test_missing_document_xml_errors() {
        // Build a zip without word/document.xml
        let dir = std::env::temp_dir().join("hollow_docx_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("bad-{}.docx", uuid_like()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions = SimpleFileOptions::default();
        zip.start_file("other.xml", opts).unwrap();
        zip.write_all(b"<root/>").unwrap();
        zip.finish().unwrap();

        let e = DocxExtractor::new();
        assert!(e.extract(&path).is_err());
    }
}
