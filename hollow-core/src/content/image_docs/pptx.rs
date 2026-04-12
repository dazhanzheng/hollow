//! PPTX: slide text + inline image placeholders.
//!
//! PPTX structure is similar to DOCX, but the body is spread across
//! multiple slide XMLs instead of a single document. Each slide has its
//! own rels file.
//!
//!   - `ppt/slides/slide1.xml`, `ppt/slides/slide2.xml`, …
//!   - `ppt/slides/_rels/slide1.xml.rels`, …
//!   - `ppt/media/*`
//!
//! Text lives in `<a:t>` (drawingML text) rather than `<w:t>`. Image
//! references are the same: `<a:blip r:embed="rId*"/>`, but the rels
//! target is relative to the slide's directory (`../media/image1.png`),
//! which resolves to `ppt/media/image1.png`.
//!
//! Walk slides in numeric order; each slide contributes its text +
//! placeholders to a shared marker counter, so a single "HOLLOW_IMG_N"
//! sequence spans the whole deck.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::content::extractor::ExtractionError;

use super::types::{guess_mime_from_name, local_name, marker, ImageDocResult, ImageEntry};

const DETECTED_MIME: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const EXTRACTOR_NAME: &str = "AppleVisionPptx";

pub fn extract(path: &Path) -> Result<ImageDocResult, ExtractionError> {
    let file = fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

    // 1. Enumerate slide XML entries (`ppt/slides/slideN.xml`) sorted
    //    naturally so slide order is deterministic.
    let mut slide_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| is_slide_entry(n))
        .collect();
    slide_names.sort_by(|a, b| natural_slide_order(a).cmp(&natural_slide_order(b)));

    let mut template = String::new();
    let mut image_refs: Vec<String> = Vec::new();

    for slide_name in &slide_names {
        // Read slide XML
        let xml_bytes = match read_entry(&mut archive, slide_name) {
            Ok(b) => b,
            Err(_) => continue,
        };

        // Read the slide's rels file. Rels live in `<dir>/_rels/<basename>.rels`.
        let rels_name = slide_rels_path(slide_name);
        let rels = read_rels(&mut archive, &rels_name)?;

        // Walk slide XML, emit text + placeholders. Placeholders share the
        // same global `image_refs` so marker numbers are unique across the
        // whole deck.
        parse_slide(&xml_bytes, &rels, slide_name, &mut template, &mut image_refs)?;

        // Separator between slides for search tokenization.
        template.push('\n');
    }

    // 2. Pull image byte blobs. The references collected above are
    //    archive-absolute paths resolved from the slide's own rels.
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
        detected_mime: DETECTED_MIME.to_string(),
        extractor_name: EXTRACTOR_NAME.to_string(),
    })
}

fn is_slide_entry(name: &str) -> bool {
    name.starts_with("ppt/slides/") && name.ends_with(".xml") && !name.contains("_rels")
}

/// Key for sorting `ppt/slides/slide10.xml` after `slide2.xml` — parse the
/// trailing number out of the basename. Slides without a trailing number
/// (e.g. slideLayout*.xml if some mis-filter slipped through) sort last.
fn natural_slide_order(name: &str) -> (u32, String) {
    let base = name.rsplit('/').next().unwrap_or(name);
    let stem = base.trim_end_matches(".xml");
    let digit_start = stem.find(|c: char| c.is_ascii_digit()).unwrap_or(stem.len());
    let (_, num_part) = stem.split_at(digit_start);
    let num: u32 = num_part.parse().unwrap_or(u32::MAX);
    (num, name.to_string())
}

fn slide_rels_path(slide_name: &str) -> String {
    // "ppt/slides/slide1.xml" → "ppt/slides/_rels/slide1.xml.rels"
    let base = slide_name.rsplit('/').next().unwrap_or(slide_name);
    let dir = slide_name
        .strip_suffix(base)
        .unwrap_or("")
        .trim_end_matches('/');
    format!("{}/_rels/{}.rels", dir, base)
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

/// Read a slide's rels file and return a map of `rId → target path` where
/// the target has already been resolved to an archive-absolute path
/// (rels files store paths relative to the slide directory, e.g.
/// `../media/image1.png`).
fn read_rels(
    archive: &mut zip::ZipArchive<fs::File>,
    rels_path: &str,
) -> Result<HashMap<String, String>, ExtractionError> {
    let mut map = HashMap::new();

    let bytes = match read_entry(archive, rels_path) {
        Ok(b) => b,
        Err(_) => return Ok(map),
    };

    // Rels directories live at `ppt/slides/_rels/slide1.xml.rels`; targets
    // are resolved relative to the directory the referring file is in
    // (`ppt/slides/` for slide rels), NOT the _rels directory itself.
    let slide_dir = rels_path
        .rsplit_once("/_rels/")
        .map(|(dir, _)| dir)
        .unwrap_or("")
        .to_string();

    let mut reader = Reader::from_reader(bytes.as_slice());
    let mut buf = Vec::new();
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
        match ev {
            Event::Empty(e) => collect_rel(&e, &slide_dir, &mut map),
            Event::Start(e) => collect_rel(&e, &slide_dir, &mut map),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(map)
}

fn collect_rel(e: &BytesStart, slide_dir: &str, map: &mut HashMap<String, String>) {
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
        let resolved = resolve_relative(slide_dir, &target);
        map.insert(id, resolved);
    }
}

/// Resolve a rels Target (which may be absolute-with-leading-slash,
/// relative `../`, or plain) against the directory of the referring file.
/// Returns an archive-absolute path with no leading slash.
fn resolve_relative(base_dir: &str, target: &str) -> String {
    if let Some(stripped) = target.strip_prefix('/') {
        // Absolute reference — strip leading slash and use as-is.
        return stripped.to_string();
    }

    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn parse_slide(
    xml: &[u8],
    rels: &HashMap<String, String>,
    _slide_name: &str,
    out: &mut String,
    image_refs: &mut Vec<String>,
) -> Result<(), ExtractionError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;
        match ev {
            Event::Start(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if name == b"t" {
                    in_text = true;
                } else if name == b"blip" {
                    handle_blip(&e, rels, out, image_refs);
                }
            }
            Event::End(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if name == b"t" {
                    in_text = false;
                } else if name == b"p" {
                    // drawingML <a:p> — paragraph break. Newline for search.
                    out.push('\n');
                }
            }
            Event::Empty(e) => {
                let raw = e.name();
                let name = local_name(raw.as_ref());
                if name == b"br" {
                    out.push(' ');
                } else if name == b"blip" {
                    handle_blip(&e, rels, out, image_refs);
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
    Ok(())
}

fn handle_blip(
    e: &BytesStart,
    rels: &HashMap<String, String>,
    out: &mut String,
    image_refs: &mut Vec<String>,
) {
    for attr in e.attributes().flatten() {
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

// Make unused `PathBuf` import silent without actually removing it — I
// leave it here for the symmetry with docx.rs's style. (Delete in later
// cleanup if still unused.)
#[allow(dead_code)]
fn _pb_marker(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    // Strongly unique per-invocation id for test filenames. cargo runs
    // tests in parallel, and nanoseconds alone collide when two tests
    // create a path at the same instant. Combine an atomic counter with
    // the current thread id for full isolation.
    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{:?}-{}", std::thread::current().id(), n)
    }

    fn make_pptx(slides: &[(&str, &str, &str)], media: &[(&str, &[u8])]) -> std::path::PathBuf {
        // slides: (slideName, slideBodyXml, relsXml)
        let dir = std::env::temp_dir().join("hollow_pptx_img_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("deck-{}.pptx", uuid_like()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        for (name, body, rels) in slides {
            let slide_xml = format!(
                r#"<?xml version="1.0"?>
<p:sld
  xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:cSld><p:spTree>{}</p:spTree></p:cSld>
</p:sld>"#,
                body
            );
            zip.start_file(format!("ppt/slides/{}", name), opts).unwrap();
            zip.write_all(slide_xml.as_bytes()).unwrap();

            let rels_xml = format!(
                r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
{}
</Relationships>"#,
                rels
            );
            zip.start_file(
                format!("ppt/slides/_rels/{}.rels", name),
                opts,
            )
            .unwrap();
            zip.write_all(rels_xml.as_bytes()).unwrap();
        }

        for (name, bytes) in media {
            zip.start_file(format!("ppt/{}", name), opts).unwrap();
            zip.write_all(bytes).unwrap();
        }

        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_simple_slide_text() {
        let slides = &[(
            "slide1.xml",
            r#"<p:sp><p:txBody><a:p><a:r><a:t>Hello slide</a:t></a:r></a:p></p:txBody></p:sp>"#,
            "",
        )];
        let p = make_pptx(slides, &[]);
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("Hello slide"));
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_slide_with_image() {
        let body = r#"
<p:sp><p:txBody><a:p><a:r><a:t>Before</a:t></a:r></a:p></p:txBody></p:sp>
<p:pic>
  <p:blipFill><a:blip r:embed="rId7"/></p:blipFill>
</p:pic>
<p:sp><p:txBody><a:p><a:r><a:t>After</a:t></a:r></a:p></p:txBody></p:sp>"#;
        let rels = r#"<Relationship Id="rId7" Type="image" Target="../media/chart.png"/>"#;
        let slides = &[("slide1.xml", body, rels)];
        let media = &[("media/chart.png", &b"PNG"[..])];
        let p = make_pptx(slides, media);
        let r = extract(&p).unwrap();

        assert!(r.text_template.contains("Before"));
        assert!(r.text_template.contains("{{HOLLOW_IMG_0}}"));
        assert!(r.text_template.contains("After"));
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].bytes, b"PNG".to_vec());
        assert_eq!(r.images[0].mime, "image/png");
    }

    #[test]
    fn test_multiple_slides_shared_counter() {
        let slides = &[
            (
                "slide1.xml",
                r#"<p:pic><p:blipFill><a:blip r:embed="rId1"/></p:blipFill></p:pic>"#,
                r#"<Relationship Id="rId1" Type="image" Target="../media/a.png"/>"#,
            ),
            (
                "slide2.xml",
                r#"<p:pic><p:blipFill><a:blip r:embed="rId1"/></p:blipFill></p:pic>"#,
                r#"<Relationship Id="rId1" Type="image" Target="../media/b.png"/>"#,
            ),
        ];
        let media = &[
            ("media/a.png", &b"A"[..]),
            ("media/b.png", &b"B"[..]),
        ];
        let p = make_pptx(slides, media);
        let r = extract(&p).unwrap();
        // Two placeholders, numbered 0 and 1 across slides
        assert!(r.text_template.contains("{{HOLLOW_IMG_0}}"));
        assert!(r.text_template.contains("{{HOLLOW_IMG_1}}"));
        assert_eq!(r.images.len(), 2);
        assert_eq!(r.images[0].bytes, b"A".to_vec());
        assert_eq!(r.images[1].bytes, b"B".to_vec());
    }

    #[test]
    fn test_slide_ordering_natural() {
        // Put slide10 before slide2 alphabetically but expect numeric order.
        let slides = &[
            (
                "slide10.xml",
                r#"<p:sp><p:txBody><a:p><a:r><a:t>Ten</a:t></a:r></a:p></p:txBody></p:sp>"#,
                "",
            ),
            (
                "slide2.xml",
                r#"<p:sp><p:txBody><a:p><a:r><a:t>Two</a:t></a:r></a:p></p:txBody></p:sp>"#,
                "",
            ),
        ];
        let p = make_pptx(slides, &[]);
        let r = extract(&p).unwrap();
        // "Two" should appear before "Ten" in the template.
        let two_pos = r.text_template.find("Two").unwrap();
        let ten_pos = r.text_template.find("Ten").unwrap();
        assert!(two_pos < ten_pos);
    }

    #[test]
    fn test_resolve_relative_parent() {
        assert_eq!(
            resolve_relative("ppt/slides", "../media/image1.png"),
            "ppt/media/image1.png"
        );
        assert_eq!(
            resolve_relative("ppt/slides", "media/image1.png"),
            "ppt/slides/media/image1.png"
        );
        assert_eq!(
            resolve_relative("ppt/slides", "/ppt/media/image1.png"),
            "ppt/media/image1.png"
        );
    }
}
