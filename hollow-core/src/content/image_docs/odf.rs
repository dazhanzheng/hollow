//! OpenDocument (ODF): .odt / .ods / .odp text + inline image placeholders.
//!
//! ODF's structure is simpler than OOXML because it doesn't have a
//! separate rels map — image references in the body XML point directly
//! to their archive-absolute path.
//!
//!   - `content.xml`                  — main body (all text + draw:image refs)
//!   - `Pictures/*`                   — embedded image files
//!
//! Text nodes are flat `<text:p>` paragraphs containing runs with
//! `<text:span>` / plain text children. `<draw:image>` elements carry an
//! `xlink:href="Pictures/10000000000001E0..."` attribute pointing at the
//! archive path. We walk the XML in order, emitting placeholders in
//! place. Text in headers/footers/metadata is technically in different
//! XML files (`styles.xml`, `meta.xml`), but `content.xml` is where all
//! user-authored body text lives and that's enough for search.

use std::fs;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::content::extractor::ExtractionError;

use super::types::{guess_mime_from_name, local_name, marker, ImageDocResult, ImageEntry};

const EXTRACTOR_NAME: &str = "AppleVisionOdf";

pub fn extract(path: &Path) -> Result<ImageDocResult, ExtractionError> {
    let file = fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

    // ODF variants are distinguished by the `mimetype` file at the root,
    // not by the content structure. Read it so the returned MIME is right.
    let detected_mime = read_mimetype(&mut archive).unwrap_or_else(|| {
        // Fall back to the extension if mimetype entry is missing.
        match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "odt" => "application/vnd.oasis.opendocument.text".to_string(),
            "ods" => "application/vnd.oasis.opendocument.spreadsheet".to_string(),
            "odp" => "application/vnd.oasis.opendocument.presentation".to_string(),
            _ => "application/zip".to_string(),
        }
    });

    // content.xml into memory
    let mut xml_bytes = Vec::new();
    {
        let mut entry = archive.by_name("content.xml").map_err(|e| {
            ExtractionError::Io(std::io::Error::other(format!(
                "content.xml missing: {}",
                e
            )))
        })?;
        entry.read_to_end(&mut xml_bytes)?;
    }

    let (template, image_refs) = parse_content(&xml_bytes)?;

    // Pull image byte blobs. Image refs are archive-absolute paths
    // (typically "Pictures/..."). Broken refs are silently dropped.
    let mut images = Vec::with_capacity(image_refs.len());
    for (idx, archive_path) in image_refs.iter().enumerate() {
        let bytes = match read_entry(&mut archive, archive_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        images.push(ImageEntry {
            marker: marker(idx),
            bytes,
            mime: guess_mime_from_name(archive_path),
        });
    }

    Ok(ImageDocResult {
        text_template: template,
        images,
        detected_mime,
        extractor_name: EXTRACTOR_NAME.to_string(),
    })
}

fn read_entry(
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

fn read_mimetype(archive: &mut zip::ZipArchive<fs::File>) -> Option<String> {
    let bytes = read_entry(archive, "mimetype").ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_content(xml: &[u8]) -> Result<(String, Vec<String>), ExtractionError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::new();
    let mut image_refs: Vec<String> = Vec::new();
    let mut text_depth: usize = 0;
    let mut buf = Vec::new();

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
        match ev {
            Event::Start(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                // ODF wraps user-visible text inside a variety of elements
                // (p, h, span, a). We use depth counting so text inside
                // any of them is collected — and nesting is fine.
                if is_text_container(name) {
                    text_depth += 1;
                } else if name == b"image" {
                    // `<draw:image xlink:href="Pictures/..."/>` is most
                    // commonly an Empty element but can be Start too.
                    handle_image(&e, &mut out, &mut image_refs);
                }
            }
            Event::End(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if is_text_container(name) {
                    text_depth = text_depth.saturating_sub(1);
                    if text_depth == 0 && (name == b"p" || name == b"h") {
                        // End of a paragraph / heading — newline separator
                        // for search tokenisation.
                        out.push('\n');
                    }
                }
            }
            Event::Empty(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if name == b"image" {
                    handle_image(&e, &mut out, &mut image_refs);
                } else if name == b"tab" || name == b"line-break" {
                    out.push(' ');
                }
            }
            Event::Text(t) => {
                if text_depth > 0 {
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

fn is_text_container(local: &[u8]) -> bool {
    matches!(
        local,
        b"p" | b"h" | b"span" | b"a" | b"list-item" | b"table-cell"
    )
}

fn handle_image(e: &BytesStart, out: &mut String, image_refs: &mut Vec<String>) {
    for attr in e.attributes().flatten() {
        // `xlink:href="Pictures/image.png"` is the reference. We look
        // for the exact attribute name without worrying about namespace
        // prefix variations — ODF always uses `xlink:`.
        if attr.key.as_ref() == b"xlink:href" {
            let href = match attr.unescape_value() {
                Ok(v) => v.into_owned(),
                Err(_) => return,
            };
            if href.is_empty() || href.starts_with("http") {
                // Skip external links — no bytes in the archive.
                return;
            }
            let idx = image_refs.len();
            out.push_str(&format!("{{{{HOLLOW_IMG_{}}}}}", idx));
            image_refs.push(href);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{:?}-{}", std::thread::current().id(), n)
    }

    fn make_odf(
        variant_ext: &str,
        mimetype: &str,
        content_xml: &str,
        pictures: &[(&str, &[u8])],
    ) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_odf_img_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("doc-{}.{}", uuid_like(), variant_ext));

        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(mimetype.as_bytes()).unwrap();

        zip.start_file("content.xml", opts).unwrap();
        let document = format!(
            r#"<?xml version="1.0"?>
<office:document-content
  xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
  xmlns:xlink="http://www.w3.org/1999/xlink">
<office:body><office:text>{}</office:text></office:body>
</office:document-content>"#,
            content_xml
        );
        zip.write_all(document.as_bytes()).unwrap();

        for (name, bytes) in pictures {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }

        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_odt_text_only() {
        let p = make_odf(
            "odt",
            "application/vnd.oasis.opendocument.text",
            r#"<text:p>hello world</text:p>"#,
            &[],
        );
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("hello world"));
        assert_eq!(r.detected_mime, "application/vnd.oasis.opendocument.text");
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_odt_with_image_inline() {
        let content = r#"
<text:p>before</text:p>
<text:p><draw:frame><draw:image xlink:href="Pictures/img1.png"/></draw:frame></text:p>
<text:p>after</text:p>"#;
        let picture = ("Pictures/img1.png", &b"PNG-BYTES"[..]);
        let p = make_odf(
            "odt",
            "application/vnd.oasis.opendocument.text",
            content,
            &[picture],
        );
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("before"));
        assert!(r.text_template.contains("{{HOLLOW_IMG_0}}"));
        assert!(r.text_template.contains("after"));
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].bytes, b"PNG-BYTES".to_vec());
        assert_eq!(r.images[0].mime, "image/png");
    }

    #[test]
    fn test_ods_variant_mime() {
        let p = make_odf(
            "ods",
            "application/vnd.oasis.opendocument.spreadsheet",
            r#"<text:p>cell text</text:p>"#,
            &[],
        );
        let r = extract(&p).unwrap();
        assert_eq!(
            r.detected_mime,
            "application/vnd.oasis.opendocument.spreadsheet"
        );
    }

    #[test]
    fn test_external_image_skipped() {
        // External links should not produce a placeholder (we have no
        // bytes to OCR).
        let content = r#"<text:p><draw:image xlink:href="http://example.com/pic.png"/></text:p>"#;
        let p = make_odf(
            "odt",
            "application/vnd.oasis.opendocument.text",
            content,
            &[],
        );
        let r = extract(&p).unwrap();
        assert!(!r.text_template.contains("HOLLOW_IMG"));
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_multiple_images_counter() {
        let content = r#"
<text:p>A<draw:image xlink:href="Pictures/a.png"/>B<draw:image xlink:href="Pictures/b.jpg"/>C</text:p>"#;
        let p = make_odf(
            "odt",
            "application/vnd.oasis.opendocument.text",
            content,
            &[
                ("Pictures/a.png", &b"AA"[..]),
                ("Pictures/b.jpg", &b"BB"[..]),
            ],
        );
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("A{{HOLLOW_IMG_0}}B{{HOLLOW_IMG_1}}C"));
        assert_eq!(r.images.len(), 2);
        assert_eq!(r.images[0].mime, "image/png");
        assert_eq!(r.images[1].mime, "image/jpeg");
    }
}
