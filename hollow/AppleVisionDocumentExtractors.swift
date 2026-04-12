import Foundation
import os

/// Shared implementation for all Swift extractors that delegate to the
/// Rust `extract_with_images` FFI and then run Apple Vision OCR on the
/// embedded images.
///
/// The Rust side hands back a `text_template` with `{{HOLLOW_IMG_N}}`
/// placeholders where images appear, plus the raw bytes of each image.
/// This helper:
///   1. calls Rust via `HollowBridge.extractWithImages(fileId:)`
///   2. runs OCR on each image blob
///   3. substitutes the OCR result back into the template at the
///      placeholder position, wrapped in `[Image: <text>]`
///   4. returns a `SwiftExtractionResult` the caller can return directly
///
/// Throws when Rust's `extract_with_images` returns nil (unexpected —
/// the caller has already claimed this file type) so the caller can
/// surface the failure via the normal SwiftExtractor error path.
///
/// OCR failures on individual images are absorbed silently as empty
/// substitutions — a single bad image shouldn't fail a 500-page book.
enum OCREnhancedDocument {

    /// Core flow shared by all four zip-based extractors.
    /// The `fileId` is needed because the Rust FFI looks up the path
    /// from the database (same state machine handshake as
    /// `extract_content`).
    static func extract(fileId: String, fileURL: URL) throws -> SwiftExtractionResult {
        guard let result = HollowBridge.shared.extractWithImages(fileId: fileId) else {
            throw SwiftExtractionError(
                message: "Rust image_docs returned nil for \(fileURL.lastPathComponent)"
            )
        }

        HollowLogger.ocr.info(
            "\(result.extractorName, privacy: .public): \(result.images.count) image(s) to OCR in \(fileURL.lastPathComponent, privacy: .public)"
        )

        var body = result.textTemplate
        for image in result.images {
            let replacement = substituteForImage(image)
            let placeholder = "{{\(image.marker)}}"
            body = body.replacingOccurrences(of: placeholder, with: replacement)
        }

        return SwiftExtractionResult(
            bodyText: body,
            encoding: "UTF-8",
            detectedMime: result.detectedMime
        )
    }

    /// Run OCR on one image blob and produce the text that replaces
    /// its `{{HOLLOW_IMG_N}}` placeholder. Failures and empty results
    /// collapse to an empty string (so the placeholder disappears
    /// cleanly). Non-empty OCR is wrapped in `[Image: …]` so users
    /// browsing the extracted text can tell which words came from
    /// images vs. the document's text layer.
    private static func substituteForImage(_ image: ExtractedImage) -> String {
        let bytes = Data(image.bytes)
        let ocr: String
        do {
            ocr = try OCRHelper.ocrImageData(bytes)
        } catch {
            HollowLogger.ocr.error(
                "OCR failed on image \(image.marker, privacy: .public): \(error.localizedDescription, privacy: .public)"
            )
            return ""
        }
        let trimmed = ocr.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "" : "[Image: \(trimmed)]"
    }
}

// MARK: - Concrete SwiftExtractor wrappers

/// Word documents. Claims `.docx`, OCRs embedded images from `word/media/`.
struct AppleVisionDocxExtractor: SwiftExtractor {
    let name = "AppleVisionDocx"
    let displayName = "Apple Vision (Word .docx)"
    let description = "Extracts text from Word documents and runs on-device OCR on embedded images — diagrams, screenshots, chart exports — inline at their position in the document."
    let supportedExtensions = ["docx"]

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        // Non-obvious: this extractor's run needs a fileId, not just a
        // URL, because the Rust `extract_with_images` FFI looks up the
        // path from the database. The IngestionService operation
        // dispatches using the fileId — we recover it from the context.
        throw SwiftExtractionError(
            message: "AppleVisionDocxExtractor must be invoked via the OCR-aware path; direct URL calls are not supported"
        )
    }
}

/// PowerPoint presentations. Claims `.pptx`, OCRs images from `ppt/media/`.
struct AppleVisionPptxExtractor: SwiftExtractor {
    let name = "AppleVisionPptx"
    let displayName = "Apple Vision (PowerPoint .pptx)"
    let description = "Extracts text from PowerPoint slides and runs on-device OCR on every embedded image — photos, screenshots, chart bitmaps — inline with each slide's text."
    let supportedExtensions = ["pptx"]

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        throw SwiftExtractionError(
            message: "AppleVisionPptxExtractor must be invoked via the OCR-aware path; direct URL calls are not supported"
        )
    }
}

/// OpenDocument text / spreadsheet / presentation (.odt/.ods/.odp).
struct AppleVisionOdfExtractor: SwiftExtractor {
    let name = "AppleVisionOdf"
    let displayName = "Apple Vision (OpenDocument)"
    let description = "Extracts text from ODT/ODS/ODP files and runs on-device OCR on embedded Pictures/ images, inline at their position in the document."
    let supportedExtensions = ["odt", "ods", "odp"]

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        throw SwiftExtractionError(
            message: "AppleVisionOdfExtractor must be invoked via the OCR-aware path; direct URL calls are not supported"
        )
    }
}

/// EPUB ebooks with inline image OCR.
struct AppleVisionEpubExtractor: SwiftExtractor {
    let name = "AppleVisionEpub"
    let displayName = "Apple Vision (EPUB)"
    let description = "Extracts text from EPUB ebooks and runs on-device OCR on every embedded image, inline with the chapter text where the image was placed."
    let supportedExtensions = ["epub"]

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        throw SwiftExtractionError(
            message: "AppleVisionEpubExtractor must be invoked via the OCR-aware path; direct URL calls are not supported"
        )
    }
}

/// Tag type used by the routing layer to decide whether to call
/// `OCREnhancedDocument.extract(fileId:fileURL:)` instead of the direct
/// `extract(fileURL:)` path. Lets us keep the `SwiftExtractor` protocol
/// simple (URL-only) while still supporting the fileId-aware OCR
/// pipeline for this subset.
protocol OCREnhancedExtractor: SwiftExtractor {}

extension AppleVisionDocxExtractor: OCREnhancedExtractor {}
extension AppleVisionPptxExtractor: OCREnhancedExtractor {}
extension AppleVisionOdfExtractor: OCREnhancedExtractor {}
extension AppleVisionEpubExtractor: OCREnhancedExtractor {}
