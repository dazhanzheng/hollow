//! SvgExtractor: extracts human-readable text from SVG vector graphics.
//!
//! SVG is XML. Text content lives in a handful of elements — `<text>`,
//! `<tspan>`, `<textPath>`, `<title>`, and `<desc>`. Everything else
//! (paths, rects, gradients, transforms) describes geometry and is
//! irrelevant for search indexing.
//!
//! Note on `<image>`: the SVG spec allows an `<image>` element with either
//! an external URL or a `data:` URL carrying a raster bitmap. We **don't**
//! extract anything from those — they'd need OCR. In practice the vast
//! majority of SVGs (icons, charts, diagrams) contain no raster imagery,
//! so this is a safe omission for now.

use std::fs;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

pub struct SvgExtractor {
    max_size: u64,
}

impl SvgExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for SvgExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &["image/svg+xml"];

/// Local names of elements whose character data we want to collect.
const TEXT_ELEMENTS: &[&[u8]] =
    &[b"text", b"tspan", b"textPath", b"title", b"desc"];

impl Extractor for SvgExtractor {
    fn name(&self) -> &'static str {
        "Svg"
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
    // Depth of text-bearing elements currently open. We collect Text events
    // whenever this is > 0, so nesting (e.g. <text><tspan>...</tspan></text>)
    // is handled naturally.
    let mut text_depth: usize = 0;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?
        {
            Event::Start(e) => {
                if is_text_element(e.name().as_ref()) {
                    text_depth += 1;
                }
            }
            Event::End(e) => {
                if is_text_element(e.name().as_ref()) {
                    text_depth = text_depth.saturating_sub(1);
                    if text_depth == 0 {
                        // Separate consecutive text blocks with a newline so
                        // downstream search tokenization doesn't glue them.
                        out.push('\n');
                    }
                }
            }
            Event::Text(t) if text_depth > 0 => {
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

fn is_text_element(qname: &[u8]) -> bool {
    // Strip any XML namespace prefix ("svg:text" → "text").
    let local = match qname.iter().position(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    };
    TEXT_ELEMENTS.iter().any(|&tag| tag == local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_svg_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_text_labels() {
        let svg = br#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg">
  <text x="10" y="20">Chart Title</text>
  <text x="10" y="40">Axis Label</text>
</svg>"#;
        let p = tmp("chart.svg", svg);
        let e = SvgExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("Chart Title"));
        assert!(r.body_text.contains("Axis Label"));
    }

    #[test]
    fn test_extract_title_and_desc() {
        let svg = br#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg">
  <title>The Big Diagram</title>
  <desc>A flowchart showing data pipeline.</desc>
  <rect x="0" y="0" width="100" height="100"/>
</svg>"#;
        let p = tmp("diagram.svg", svg);
        let e = SvgExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("The Big Diagram"));
        assert!(r.body_text.contains("flowchart"));
    }

    #[test]
    fn test_extract_nested_tspans() {
        let svg = br#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg">
  <text><tspan>Hello </tspan><tspan>World</tspan></text>
</svg>"#;
        let p = tmp("nested.svg", svg);
        let e = SvgExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("Hello "));
        assert!(r.body_text.contains("World"));
    }

    #[test]
    fn test_ignores_geometry_and_style() {
        // Geometry content (d, style rules) should not appear in output.
        let svg = br#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg">
  <style>.cls-1 { fill: red; stroke: blue; }</style>
  <path d="M10 10 L 20 20 Z" class="cls-1"/>
  <text>caption</text>
</svg>"#;
        let p = tmp("geom.svg", svg);
        let e = SvgExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("caption"));
        assert!(!r.body_text.contains("fill: red"));
        assert!(!r.body_text.contains("M10 10"));
    }

    #[test]
    fn test_empty_svg_returns_empty() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let p = tmp("empty.svg", svg);
        let e = SvgExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.trim().is_empty());
    }

    #[test]
    fn test_handles_utf8() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>你好世界</text></svg>".as_bytes();
        let p = tmp("zh.svg", svg);
        let e = SvgExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("你好世界"));
    }
}
