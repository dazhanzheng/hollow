//! DOCX: text layer + embedded image byte handoff.
//!
//! Structure of a .docx:
//!   - `word/document.xml`              — the main body; `<w:t>` carries
//!     the text, `<w:drawing>` wraps images referenced by relationship id
//!   - `word/_rels/document.xml.rels`   — maps `rId*` → media target paths
//!     (e.g. `media/image1.png`, relative to `word/`)
//!   - `word/media/*`                   — the actual image files
//!
//! We walk `document.xml` in order, emitting text for `<w:t>` content and
//! `{{HOLLOW_IMG_N}}` placeholders for `<a:blip>` references. After the
//! walk completes, we read each referenced image file out of the archive
//! in the order the placeholders were emitted, so Swift's marker → bytes
//! mapping lines up naturally.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::content::extractor::ExtractionError;

use super::types::{guess_mime_from_name, local_name, marker, ImageDocResult, ImageEntry};

const DETECTED_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const EXTRACTOR_NAME: &str = "AppleVisionDocx";

pub fn extract(path: &Path) -> Result<ImageDocResult, ExtractionError> {
    let file = fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

    // 1. rels — rId → "media/image1.png"
    let rels = read_rels(&mut archive, "word/_rels/document.xml.rels")?;

    // 2. document.xml into memory (typically <1MB even for large docs)
    let mut xml_bytes = Vec::new();
    {
        let mut entry = archive
            .by_name("word/document.xml")
            .map_err(|e| ExtractionError::Io(std::io::Error::other(format!(
                "word/document.xml missing: {}",
                e
            ))))?;
        entry.read_to_end(&mut xml_bytes)?;
    }

    // 3. Stream-parse text + collect image rels in document order
    let (template, image_refs) = parse_document(&xml_bytes, &rels)?;

    // 4. Read image byte blobs in placeholder order. Broken/missing
    //    references are silently skipped — the placeholder in the template
    //    will substitute to empty on the Swift side.
    let mut images = Vec::with_capacity(image_refs.len());
    for (idx, rel_target) in image_refs.iter().enumerate() {
        let archive_path = format!("word/{}", rel_target);
        let bytes = match read_archive_entry(&mut archive, &archive_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        images.push(ImageEntry {
            marker: marker(idx),
            bytes,
            mime: guess_mime_from_name(rel_target),
        });
    }

    Ok(ImageDocResult {
        text_template: template,
        images,
        detected_mime: DETECTED_MIME.to_string(),
        extractor_name: EXTRACTOR_NAME.to_string(),
    })
}

fn read_archive_entry(
    archive: &mut zip::ZipArchive<fs::File>,
    name: &str,
) -> Result<Vec<u8>, ExtractionError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Read `word/_rels/document.xml.rels` into a `rId → Target` map.
/// Missing rels is treated as an empty map (legal — just means the document
/// has no embedded resources).
fn read_rels(
    archive: &mut zip::ZipArchive<fs::File>,
    rels_path: &str,
) -> Result<HashMap<String, String>, ExtractionError> {
    let mut map = HashMap::new();

    let bytes = match read_archive_entry(archive, rels_path) {
        Ok(b) => b,
        Err(_) => return Ok(map),
    };

    let mut reader = Reader::from_reader(bytes.as_slice());
    let mut buf = Vec::new();
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
        match ev {
            Event::Empty(e) => collect_relationship(&e, &mut map),
            Event::Start(e) => collect_relationship(&e, &mut map),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(map)
}

fn collect_relationship(e: &BytesStart, map: &mut HashMap<String, String>) {
    if local_name(e.name().as_ref()) != b"Relationship" {
        return;
    }
    let mut id = String::new();
    let mut target = String::new();
    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"Id" => {
                id = attr.unescape_value().unwrap_or_default().into_owned();
            }
            b"Target" => {
                target = attr.unescape_value().unwrap_or_default().into_owned();
            }
            _ => {}
        }
    }
    if !id.is_empty() && !target.is_empty() {
        map.insert(id, target);
    }
}

/// Stream-parse `word/document.xml`, producing a text template with
/// placeholders and a list of `rId` → `target` references in document
/// order. The returned `Vec<String>` holds target paths (relative to
/// `word/`) in placeholder order.
fn parse_document(
    xml: &[u8],
    rels: &HashMap<String, String>,
) -> Result<(String, Vec<String>), ExtractionError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut in_text = false;
    let mut image_refs: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
        match ev {
            Event::Start(e) => {
                if local_name(e.name().as_ref()) == b"t" {
                    in_text = true;
                } else if local_name(e.name().as_ref()) == b"blip" {
                    // `<a:blip>` can technically appear as a Start tag with
                    // no inner content — handle it the same way as Empty.
                    handle_blip(&e, rels, &mut out, &mut image_refs);
                }
            }
            Event::End(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if name == b"t" {
                    in_text = false;
                } else if name == b"p" {
                    // End of a paragraph — newline for readability.
                    out.push('\n');
                }
            }
            Event::Empty(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if name == b"tab" || name == b"br" {
                    out.push(' ');
                } else if name == b"blip" {
                    handle_blip(&e, rels, &mut out, &mut image_refs);
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

    Ok((out, image_refs))
}

fn handle_blip(
    e: &BytesStart,
    rels: &HashMap<String, String>,
    out: &mut String,
    image_refs: &mut Vec<String>,
) {
    for attr in e.attributes().flatten() {
        // `r:embed="rId5"` is the primary reference. `r:link` is a less
        // common external link — we skip those since there's no bytes in
        // the archive to OCR.
        if attr.key.as_ref() == b"r:embed" {
            let rid = match attr.unescape_value() {
                Ok(v) => v.into_owned(),
                Err(_) => return,
            };
            if let Some(target) = rels.get(&rid) {
                let idx = image_refs.len();
                out.push_str(&format!("{{{{HOLLOW_IMG_{}}}}}", idx));
                image_refs.push(target.clone());
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    // Strongly unique per-invocation id — see pptx.rs for rationale.
    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{:?}-{}", std::thread::current().id(), n)
    }

    /// Helper to assemble a minimal .docx with the given body XML and
    /// optional image entries.
    fn make_docx(body_xml: &str, rels_extra: &str, media: &[(&str, &[u8])]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_docx_img_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("doc-{}.docx", uuid_like()));

        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        // document.xml
        let document = format!(
            r#"<?xml version="1.0"?>
<w:document
  xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<w:body>{}</w:body>
</w:document>"#,
            body_xml
        );
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(document.as_bytes()).unwrap();

        // rels
        let rels = format!(
            r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
{}
</Relationships>"#,
            rels_extra
        );
        zip.start_file("word/_rels/document.xml.rels", opts).unwrap();
        zip.write_all(rels.as_bytes()).unwrap();

        // media entries
        for (name, bytes) in media {
            zip.start_file(format!("word/{}", name), opts).unwrap();
            zip.write_all(bytes).unwrap();
        }

        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_text_only_no_images() {
        let p = make_docx(
            r#"<w:p><w:r><w:t>hello world</w:t></w:r></w:p>"#,
            "",
            &[],
        );
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("hello world"));
        assert!(r.images.is_empty());
        assert!(!r.text_template.contains("HOLLOW_IMG"));
    }

    #[test]
    fn test_single_image_inline_placeholder() {
        let body = r#"
<w:p>
  <w:r><w:t>before </w:t></w:r>
  <w:r>
    <w:drawing>
      <wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing">
        <a:graphic>
          <a:graphicData>
            <pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
              <pic:blipFill>
                <a:blip r:embed="rId5"/>
              </pic:blipFill>
            </pic:pic>
          </a:graphicData>
        </a:graphic>
      </wp:inline>
    </w:drawing>
  </w:r>
  <w:r><w:t> after</w:t></w:r>
</w:p>"#;
        let rels =
            r#"<Relationship Id="rId5" Type="image" Target="media/image1.png"/>"#;
        let image_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG magic
        let p = make_docx(body, rels, &[("media/image1.png", &image_bytes)]);
        let r = extract(&p).unwrap();

        // Template has placeholder between before/after
        assert!(r.text_template.contains("before {{HOLLOW_IMG_0}} after"));
        // Image was collected
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].marker, "HOLLOW_IMG_0");
        assert_eq!(r.images[0].bytes, image_bytes);
        assert_eq!(r.images[0].mime, "image/png");
    }

    #[test]
    fn test_multiple_images_ordered() {
        let body = r#"
<w:p>
  <w:r><w:t>A</w:t></w:r>
  <w:r><w:drawing><a:blip r:embed="rId10"/></w:drawing></w:r>
  <w:r><w:t>B</w:t></w:r>
  <w:r><w:drawing><a:blip r:embed="rId11"/></w:drawing></w:r>
  <w:r><w:t>C</w:t></w:r>
</w:p>"#;
        let rels = r#"
<Relationship Id="rId10" Type="image" Target="media/first.jpg"/>
<Relationship Id="rId11" Type="image" Target="media/second.png"/>"#;
        let p = make_docx(
            body,
            rels,
            &[
                ("media/first.jpg", b"JPEG-BYTES"),
                ("media/second.png", b"PNG-BYTES"),
            ],
        );

        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("A{{HOLLOW_IMG_0}}B{{HOLLOW_IMG_1}}C"));
        assert_eq!(r.images.len(), 2);
        assert_eq!(r.images[0].mime, "image/jpeg");
        assert_eq!(r.images[1].mime, "image/png");
        assert_eq!(r.images[0].bytes, b"JPEG-BYTES".to_vec());
        assert_eq!(r.images[1].bytes, b"PNG-BYTES".to_vec());
    }

    #[test]
    fn test_broken_rel_reference_skipped() {
        // Document references rId99 which doesn't exist in rels — should
        // NOT produce a placeholder, just silently drop the image.
        let body = r#"<w:p><w:r><w:t>hi</w:t><w:drawing><a:blip r:embed="rId99"/></w:drawing></w:r></w:p>"#;
        let p = make_docx(body, "", &[]);
        let r = extract(&p).unwrap();
        assert_eq!(r.text_template.trim(), "hi");
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_missing_image_file_drops_entry() {
        // Rels points at media/ghost.png but archive doesn't contain it.
        // Placeholder is still in the template, but images vec will be
        // missing that entry. Swift will replace the dangling placeholder
        // with empty string — acceptable.
        let body = r#"<w:p><w:r><w:drawing><a:blip r:embed="rId1"/></w:drawing></w:r></w:p>"#;
        let rels = r#"<Relationship Id="rId1" Type="image" Target="media/ghost.png"/>"#;
        let p = make_docx(body, rels, &[]);
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("{{HOLLOW_IMG_0}}"));
        assert!(r.images.is_empty());
    }
}
