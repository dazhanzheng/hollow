//! Rich document extraction with inline image handoff to Swift OCR.
//!
//! The regular `extractors` module treats documents as pure text — it pulls
//! the XML/text layer out and ignores any embedded imagery. For formats
//! that commonly contain screenshots, diagrams, charts, and photographs
//! (.docx, .pptx, .odt/.ods/.odp, .epub) this leaves real content on the
//! table.
//!
//! This module provides a *second* extraction path for those formats:
//!
//!  1. Walk the document structure and produce a `text_template`, which is
//!     the plain text body **with `{{HOLLOW_IMG_N}}` placeholders inserted
//!     exactly where images appear** in reading order.
//!  2. Collect the raw bytes of every referenced image alongside, keyed by
//!     the same marker.
//!  3. Return both to Swift via the `extract_with_images` FFI.
//!
//! Swift then runs Apple Vision OCR on each image's bytes and substitutes
//! the OCR text back into the template at the placeholder positions. The
//! final merged body text is committed to the database via the existing
//! `extract_content_external` FFI — same state machine as any other
//! extraction path.
//!
//! The existing pure-text Rust extractors (DocxExtractor, EpubExtractor)
//! stay registered and are used as a fallback when the user turns off the
//! Apple Vision plugins in Settings.

pub mod docx;
pub mod epub;
pub mod odf;
pub mod pptx;
pub mod types;

use std::path::Path;

use crate::content::extractor::ExtractionError;
use types::ImageDocResult;

/// Dispatch on file extension. Returns `None` for file types that don't
/// have an image-aware extractor — caller should fall back to the regular
/// text-only pipeline.
pub fn extract(path: &Path) -> Result<Option<ImageDocResult>, ExtractionError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let result = match ext.as_str() {
        "docx" => docx::extract(path)?,
        "pptx" => pptx::extract(path)?,
        "epub" => epub::extract(path)?,
        "odt" | "ods" | "odp" => odf::extract(path)?,
        _ => return Ok(None),
    };
    Ok(Some(result))
}
