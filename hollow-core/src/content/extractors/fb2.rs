//! Fb2Extractor: extracts text from FictionBook 2 (.fb2) ebooks.
//!
//! FB2 is a plain XML format used primarily for Russian/Eastern European
//! ebooks. The relevant text lives in `<description>` (metadata) and
//! `<body>` (the actual chapters). Embedded images, if any, are in
//! `<binary>` elements at the end of the document — we skip those entirely.
//!
//! The extraction is a streaming pass that collects all character data
//! *unless* we're currently inside a `<binary>` element.

use std::fs;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

pub struct Fb2Extractor {
    max_size: u64,
}

impl Fb2Extractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for Fb2Extractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &["application/x-fictionbook+xml"];

/// Tag local-names whose entire subtree should be dropped during extraction.
/// `binary` is where embedded raster images live (base64-encoded). Dropping
/// the subtree also guards against future FB2 extensions sneaking other
/// non-text payloads in.
const SKIP_ELEMENTS: &[&[u8]] = &[b"binary", b"stylesheet"];

/// Tag local-names whose end we treat as a line break in the output. This
/// isn't semantically perfect (FB2 has more structure: `<section>`,
/// `<subtitle>`, etc.) but a newline after each paragraph / title is enough
/// for search indexing.
const PARAGRAPH_ELEMENTS: &[&[u8]] =
    &[b"p", b"v", b"title", b"subtitle", b"epigraph", b"text-author"];

impl Extractor for Fb2Extractor {
    fn name(&self) -> &'static str {
        "Fb2"
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
        let body_text = extract_text(&bytes)?;

        Ok(ExtractionResult {
            body_text,
            encoding: Some("UTF-8".to_string()),
        })
    }
}

fn extract_text(xml: &[u8]) -> Result<String, ExtractionError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    // Depth of currently-open skip elements. We refuse to collect any
    // character data while this is > 0.
    let mut skip_depth: usize = 0;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?
        {
            Event::Start(e) => {
                if is_skip_element(e.name().as_ref()) {
                    skip_depth += 1;
                }
            }
            Event::End(e) => {
                let name = e.name();
                let raw = name.as_ref();
                if is_skip_element(raw) {
                    skip_depth = skip_depth.saturating_sub(1);
                } else if skip_depth == 0 && is_paragraph_element(raw) {
                    out.push('\n');
                }
            }
            Event::Empty(_) => {
                // <empty-line/> and friends — emit a newline only when not
                // inside a skip subtree.
                if skip_depth == 0 {
                    out.push('\n');
                }
            }
            Event::Text(t) if skip_depth == 0 => {
                let txt = t
                    .decode()
                    .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
                out.push_str(&txt);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

fn is_skip_element(qname: &[u8]) -> bool {
    let local = local_name(qname);
    SKIP_ELEMENTS.iter().any(|&tag| tag == local)
}

fn is_paragraph_element(qname: &[u8]) -> bool {
    let local = local_name(qname);
    PARAGRAPH_ELEMENTS.iter().any(|&tag| tag == local)
}

fn local_name(qname: &[u8]) -> &[u8] {
    match qname.iter().position(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_fb2_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_book_body() {
        let fb2 = br#"<?xml version="1.0" encoding="UTF-8"?>
<FictionBook>
  <description>
    <title-info>
      <book-title>The Example Book</book-title>
    </title-info>
  </description>
  <body>
    <section>
      <title><p>Chapter One</p></title>
      <p>It was a dark and stormy night.</p>
      <p>The rain fell in sheets.</p>
    </section>
  </body>
</FictionBook>"#;
        let p = tmp("book1.fb2", fb2);
        let e = Fb2Extractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("The Example Book"));
        assert!(r.body_text.contains("Chapter One"));
        assert!(r.body_text.contains("dark and stormy"));
        assert!(r.body_text.contains("rain fell"));
    }

    #[test]
    fn test_binary_sections_skipped() {
        // FB2 allows base64 images in <binary>. We must not leak that
        // garbage into the indexed text.
        let fb2 = br#"<?xml version="1.0"?>
<FictionBook>
  <body>
    <section>
      <p>real content here</p>
    </section>
  </body>
  <binary id="cover.jpg" content-type="image/jpeg">AAAABBBBCCCCDDDDEEEEFFFF</binary>
</FictionBook>"#;
        let p = tmp("book2.fb2", fb2);
        let e = Fb2Extractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("real content here"));
        assert!(!r.body_text.contains("AAAABBBB"));
        assert!(!r.body_text.contains("EEEEFFFF"));
    }

    #[test]
    fn test_multiple_paragraphs_separated() {
        let fb2 = br#"<?xml version="1.0"?>
<FictionBook>
  <body>
    <section>
      <p>first</p>
      <p>second</p>
      <p>third</p>
    </section>
  </body>
</FictionBook>"#;
        let p = tmp("book3.fb2", fb2);
        let e = Fb2Extractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("first\n"));
        assert!(r.body_text.contains("second\n"));
        assert!(r.body_text.contains("third"));
    }

    #[test]
    fn test_handles_utf8() {
        let fb2 = r#"<?xml version="1.0" encoding="UTF-8"?>
<FictionBook><body><section><p>你好世界</p></section></body></FictionBook>"#
            .as_bytes();
        let p = tmp("zh.fb2", fb2);
        let e = Fb2Extractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("你好世界"));
    }
}
