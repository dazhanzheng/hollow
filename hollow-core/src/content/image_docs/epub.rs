//! EPUB: chapter text + inline image placeholders.
//!
//! EPUB chapters are XHTML files scattered inside an archive (typically
//! under `OEBPS/` or `EPUB/`, but the layout varies). Unlike DOCX/PPTX
//! there's no centralised rels file — images are referenced directly
//! via `<img src="..." />` and the `src` is a path **relative to the
//! chapter file's directory**.
//!
//! Strategy:
//!   1. Enumerate chapter entries (sorted lexicographically — reading
//!      order is technically defined by the OPF spine, but for search
//!      indexing any deterministic ordering is fine).
//!   2. For each chapter, scan its bytes for `<img src="...">` tags.
//!      Each hit → resolve the src against the chapter's directory,
//!      remember the resolved archive path in `image_refs`, and replace
//!      the entire tag in the HTML with a plain-text marker string
//!      `{{HOLLOW_IMG_N}}`.
//!   3. Render the rewritten HTML through `html2text` to strip the rest
//!      of the tags. html2text preserves plain text (including our
//!      markers) as-is, so the markers survive into the final template
//!      at the correct position.
//!   4. Read each referenced image's bytes from the archive in
//!      placeholder order.
//!
//! The byte-level `<img>` scan avoids needing a full HTML parser (EPUB
//! is well-formed XHTML in practice, but permissive enough that regex
//! or DOM parsing would each bring their own failure modes). The
//! scanner handles whitespace-delimited, quoted src attributes, which
//! is the overwhelming majority of real-world EPUBs.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::content::extractor::ExtractionError;

use super::types::{guess_mime_from_name, marker, ImageDocResult, ImageEntry};

const DETECTED_MIME: &str = "application/epub+zip";
const EXTRACTOR_NAME: &str = "AppleVisionEpub";
const RENDER_WIDTH: usize = 10_000;

pub fn extract(path: &Path) -> Result<ImageDocResult, ExtractionError> {
    let file = fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

    // Gather chapter file names (.xhtml / .html / .htm), sorted.
    let mut html_names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| is_html_entry(n))
        .collect();
    html_names.sort();

    let mut template = String::new();
    let mut image_refs: Vec<String> = Vec::new();

    for name in &html_names {
        let bytes = match read_entry(&mut archive, name) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.is_empty() {
            continue;
        }

        let chapter_dir = parent_dir(name);
        // Rewrite <img> tags → plain-text markers, collect resolved paths.
        let rewritten = inject_img_markers(&bytes, &chapter_dir, &mut image_refs);

        // html2text preserves our markers as plain text.
        let chapter_text = html2text::from_read(&rewritten[..], RENDER_WIDTH)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(e.to_string())))?;

        if !chapter_text.trim().is_empty() {
            template.push_str(&chapter_text);
            template.push('\n');
        }
    }

    // Read image bytes in placeholder order.
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

fn is_html_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

/// Directory containing `name`, relative to archive root. Returns ""
/// for top-level files, otherwise the path without a trailing slash.
fn parent_dir(name: &str) -> String {
    match name.rfind('/') {
        Some(i) => name[..i].to_string(),
        None => String::new(),
    }
}

/// Scan an HTML byte buffer for `<img ... src="..." ...>` / `<img ... />`
/// tags, replacing each with a plain-text marker. Collects resolved
/// archive paths into `image_refs`. Unrecognised/malformed `<img>` tags
/// (no src attribute, unsupported scheme) are passed through unchanged
/// — html2text will strip them later.
fn inject_img_markers(
    html: &[u8],
    chapter_dir: &str,
    image_refs: &mut Vec<String>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        if html[i] == b'<'
            && i + 4 <= html.len()
            && html[i + 1..i + 4].eq_ignore_ascii_case(b"img")
            && (i + 4 == html.len()
                || matches!(html[i + 4], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'))
        {
            // Find end of tag `>`.
            let end = match find_byte(&html[i..], b'>') {
                Some(off) => i + off + 1,
                None => {
                    // Malformed, bail out — copy remainder and stop.
                    out.extend_from_slice(&html[i..]);
                    break;
                }
            };
            let tag = &html[i..end];
            if let Some(src) = extract_src_attr(tag) {
                let resolved = resolve_path(chapter_dir, &src);
                if !resolved.is_empty() && !src.starts_with("http://") && !src.starts_with("https://") && !src.starts_with("data:") {
                    let idx = image_refs.len();
                    image_refs.push(resolved);
                    // Inject marker as plain text, padded with whitespace
                    // so html2text doesn't collapse it into neighbours.
                    out.extend_from_slice(format!(" {{{{HOLLOW_IMG_{}}}}} ", idx).as_bytes());
                    i = end;
                    continue;
                }
            }
            // Unrecognised img — preserve original tag.
            out.extend_from_slice(tag);
            i = end;
        } else {
            out.push(html[i]);
            i += 1;
        }
    }
    out
}

fn find_byte(buf: &[u8], target: u8) -> Option<usize> {
    buf.iter().position(|&b| b == target)
}

/// Extract the `src` attribute value from an `<img ...>` tag byte slice.
/// Handles single- and double-quoted forms. Returns an owned String if
/// found.
fn extract_src_attr(tag: &[u8]) -> Option<String> {
    // Look for `src=` case-insensitively.
    let needles: &[&[u8]] = &[b"src=\"", b"src='", b"SRC=\"", b"SRC='"];
    for needle in needles {
        if let Some(start) = window_find(tag, needle) {
            let after = start + needle.len();
            let quote = needle[needle.len() - 1];
            if let Some(end_off) = find_byte(&tag[after..], quote) {
                let bytes = &tag[after..after + end_off];
                return String::from_utf8(bytes.to_vec()).ok();
            }
        }
    }
    None
}

fn window_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

/// Resolve a chapter-relative href against the chapter's directory,
/// returning an archive-absolute path with no leading slash. Handles
/// `.`, `..`, absolute (leading `/`), and plain relative segments.
fn resolve_path(base_dir: &str, href: &str) -> String {
    if let Some(stripped) = href.strip_prefix('/') {
        return stripped.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for segment in href.split('/') {
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

    fn make_epub(entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_epub_img_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("book-{}.epub", uuid_like()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts: SimpleFileOptions =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        for (name, bytes) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_chapter_text_no_images() {
        let p = make_epub(&[(
            "OEBPS/ch01.xhtml",
            b"<html><body><p>hello chapter</p></body></html>",
        )]);
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("hello chapter"));
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_chapter_with_image_inline() {
        let chapter = br#"<html><body><p>before</p><img src="images/cover.png" alt="cover"/><p>after</p></body></html>"#;
        let entries: &[(&str, &[u8])] = &[
            ("OEBPS/ch01.xhtml", chapter),
            ("OEBPS/images/cover.png", b"PNG-BYTES"),
        ];
        let p = make_epub(entries);
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("before"));
        assert!(r.text_template.contains("{{HOLLOW_IMG_0}}"));
        assert!(r.text_template.contains("after"));
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].bytes, b"PNG-BYTES".to_vec());
        assert_eq!(r.images[0].mime, "image/png");
    }

    #[test]
    fn test_parent_relative_href() {
        // Chapter references `../images/pic.png` — should resolve
        // relative to chapter dir.
        let chapter = br#"<html><body><img src="../images/pic.jpg"/></body></html>"#;
        let entries: &[(&str, &[u8])] = &[
            ("OEBPS/text/ch01.xhtml", chapter),
            ("OEBPS/images/pic.jpg", b"JPG"),
        ];
        let p = make_epub(entries);
        let r = extract(&p).unwrap();
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].bytes, b"JPG".to_vec());
    }

    #[test]
    fn test_external_image_skipped() {
        // http:// URL — no bytes in archive, should not produce a
        // placeholder.
        let chapter = br#"<html><body><img src="https://example.com/pic.png"/><p>text</p></body></html>"#;
        let p = make_epub(&[("OEBPS/ch01.xhtml", chapter)]);
        let r = extract(&p).unwrap();
        assert!(!r.text_template.contains("HOLLOW_IMG"));
        assert!(r.text_template.contains("text"));
        assert!(r.images.is_empty());
    }

    #[test]
    fn test_multiple_chapters_counter_shared() {
        let ch1: &[u8] = br#"<html><body><img src="images/a.png"/></body></html>"#;
        let ch2: &[u8] = br#"<html><body><img src="images/b.png"/></body></html>"#;
        let entries: &[(&str, &[u8])] = &[
            ("OEBPS/ch01.xhtml", ch1),
            ("OEBPS/ch02.xhtml", ch2),
            ("OEBPS/images/a.png", b"A"),
            ("OEBPS/images/b.png", b"B"),
        ];
        let p = make_epub(entries);
        let r = extract(&p).unwrap();
        assert!(r.text_template.contains("{{HOLLOW_IMG_0}}"));
        assert!(r.text_template.contains("{{HOLLOW_IMG_1}}"));
        assert_eq!(r.images.len(), 2);
        assert_eq!(r.images[0].bytes, b"A".to_vec());
        assert_eq!(r.images[1].bytes, b"B".to_vec());
    }

    #[test]
    fn test_resolve_path_forms() {
        assert_eq!(resolve_path("OEBPS/text", "../images/a.png"), "OEBPS/images/a.png");
        assert_eq!(resolve_path("OEBPS", "images/a.png"), "OEBPS/images/a.png");
        assert_eq!(resolve_path("OEBPS", "/Root/a.png"), "Root/a.png");
        assert_eq!(resolve_path("", "top.png"), "top.png");
    }

    #[test]
    fn test_extract_src_attr_variants() {
        assert_eq!(
            extract_src_attr(br#"<img src="pic.png" alt="x"/>"#),
            Some("pic.png".to_string())
        );
        assert_eq!(
            extract_src_attr(br#"<img alt="x" src='pic.png'/>"#),
            Some("pic.png".to_string())
        );
        assert_eq!(
            extract_src_attr(br#"<img SRC="PIC.PNG"/>"#),
            Some("PIC.PNG".to_string())
        );
        assert_eq!(extract_src_attr(br#"<img alt="no src"/>"#), None);
    }
}
