import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// SwiftExtractor that runs Apple Vision OCR on a single image file.
///
/// Covers the common photo and screenshot formats. The image is loaded via
/// `CGImageSource` (which handles everything macOS knows how to decode,
/// including HEIC, WebP, RAW, and multi-frame formats like GIF — we take
/// the first frame in those cases).
struct AppleVisionImageExtractor: SwiftExtractor {
    let name = "AppleVisionImage"
    let displayName = "Apple Vision (Images)"
    let description = "On-device OCR for image files (PNG, JPEG, HEIC, TIFF, GIF, BMP, WebP). Runs through Apple Vision — nothing leaves your Mac."
    let supportedExtensions = [
        "png", "jpg", "jpeg",
        "heic", "heif",
        "tiff", "tif",
        "gif", "bmp", "webp",
    ]

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        // Read the whole file in — the OCR helper handles decoding via
        // CGImageSource so we get HEIC, WebP, multi-frame formats for
        // free through a single code path shared with the document
        // extractors.
        let data: Data
        do {
            data = try Data(contentsOf: fileURL, options: [.mappedIfSafe])
        } catch {
            throw SwiftExtractionError(
                message: "failed to read image at \(fileURL.path): \(error.localizedDescription)"
            )
        }

        let text = try OCRHelper.ocrImageData(data)
        let mime = Self.mimeType(for: fileURL.pathExtension.lowercased())

        return SwiftExtractionResult(
            bodyText: text,
            encoding: "UTF-8",
            detectedMime: mime
        )
    }

    /// Map an extension to a plausible IANA MIME type. Prefer to derive
    /// from `UTType` so we agree with macOS on edge cases. Falls back to
    /// `application/octet-stream` for unknown extensions (shouldn't happen
    /// given our fixed supportedExtensions list, but stay defensive).
    private static func mimeType(for ext: String) -> String {
        if let type = UTType(filenameExtension: ext),
           let mime = type.preferredMIMEType
        {
            return mime
        }
        return "application/octet-stream"
    }
}
