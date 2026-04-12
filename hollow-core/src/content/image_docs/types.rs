//! Shared types and helpers for image-aware document extraction.

/// The output of `image_docs::extract` — a text body with placeholder
/// markers where images should go, plus the raw bytes of each referenced
/// image in placeholder order.
#[derive(Debug, Clone)]
pub struct ImageDocResult {
    /// Body text with `{{HOLLOW_IMG_N}}` placeholders embedded at image
    /// positions. Swift OCRs each image and substitutes the result back
    /// in; a failed OCR becomes an empty substitution, not an error.
    pub text_template: String,

    /// Image byte blobs. Index in this vector corresponds to the `N` in
    /// the `{{HOLLOW_IMG_N}}` marker.
    pub images: Vec<ImageEntry>,

    /// Canonical MIME type for this document (e.g.
    /// "application/vnd.openxmlformats-officedocument.wordprocessingml.document").
    pub detected_mime: String,

    /// Name stored on `file_content.extractor_name`. By convention these
    /// are the same names used by the Swift-side `SwiftExtractor` so the
    /// Settings UI can correlate them.
    pub extractor_name: String,
}

/// A single embedded image referenced by a `{{HOLLOW_IMG_N}}` placeholder.
#[derive(Debug, Clone)]
pub struct ImageEntry {
    /// Marker this image corresponds to, e.g. `HOLLOW_IMG_0`.
    /// Does not include the wrapping `{{ }}`.
    pub marker: String,

    /// Raw image bytes as stored in the document archive.
    pub bytes: Vec<u8>,

    /// Best-guess MIME type from the filename (e.g. "image/png").
    /// Used by Swift to pick the right decoder path.
    pub mime: String,
}

/// Generate a marker string for the N-th image in a document.
/// Callers wrap the returned name in `{{ }}` before inserting into the
/// text template; the bare name stays on `ImageEntry.marker` so Swift
/// can build either form.
pub fn marker(index: usize) -> String {
    format!("HOLLOW_IMG_{}", index)
}

/// Guess a MIME type from a filename by extension. Defaults to
/// `application/octet-stream` for anything we don't recognise — Vision
/// on the Swift side will happily try to decode it anyway via CGImageSource.
pub fn guess_mime_from_name(name: &str) -> String {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heic",
        "svg" => "image/svg+xml",
        "wmf" => "image/wmf",
        "emf" => "image/emf",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Return the local name of an XML element, stripping any `prefix:`.
/// Used by all the XML streaming extractors in this module.
pub fn local_name(qname: &[u8]) -> &[u8] {
    match qname.iter().position(|&b| b == b':') {
        Some(i) => &qname[i + 1..],
        None => qname,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_format() {
        assert_eq!(marker(0), "HOLLOW_IMG_0");
        assert_eq!(marker(42), "HOLLOW_IMG_42");
    }

    #[test]
    fn test_mime_guessing() {
        assert_eq!(guess_mime_from_name("image1.png"), "image/png");
        assert_eq!(guess_mime_from_name("cover.JPEG"), "image/jpeg");
        assert_eq!(guess_mime_from_name("diagram.svg"), "image/svg+xml");
        assert_eq!(guess_mime_from_name("blob"), "application/octet-stream");
    }

    #[test]
    fn test_local_name_strips_prefix() {
        assert_eq!(local_name(b"w:t"), b"t");
        assert_eq!(local_name(b"a:blip"), b"blip");
        assert_eq!(local_name(b"p"), b"p");
    }
}
