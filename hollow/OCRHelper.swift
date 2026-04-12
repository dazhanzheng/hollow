import Foundation
import CoreGraphics
import ImageIO

/// Shared OCR utilities. Wraps the boilerplate of decoding an image
/// payload into a CGImage and running Apple Vision recognition on it.
///
/// Used by:
///   - `AppleVisionImageExtractor` (decodes a file on disk)
///   - The zip-based document extractors (DOCX/PPTX/ODF/EPUB) which
///     get image byte blobs from Rust and feed them through
///     `ocrImageData` one at a time
///   - `AppleVisionIWorkExtractor` for the images inside iWork bundles
enum OCRHelper {

    /// Decode raw image bytes into a CGImage (any format CGImageSource
    /// supports — PNG/JPEG/HEIC/TIFF/GIF/BMP/WebP/WMF/EMF where macOS
    /// has a decoder) and run Apple Vision text recognition.
    ///
    /// Returns an empty string when the image:
    ///   - cannot be decoded (e.g. WMF on older macOS, broken bytes)
    ///   - contains no detectable text
    ///
    /// Does not throw for "no text detected" — that's a legitimate
    /// outcome for graphics, photos of landscapes, decorative images.
    /// Throws only for I/O-style failures Vision reports.
    static func ocrImageData(_ data: Data) throws -> String {
        guard let cgImage = decodeFirstFrame(data) else {
            return ""
        }
        return try AppleVisionOCR.recognizeText(in: cgImage)
    }

    /// Decode the first frame of an image payload into a CGImage.
    /// Multi-frame formats (GIF, APNG, multi-page TIFF) return the
    /// first frame only — acceptable since Vision can only OCR one
    /// image at a time anyway.
    private static func decodeFirstFrame(_ data: Data) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(
            data as CFData,
            nil
        ) else {
            return nil
        }
        guard CGImageSourceGetCount(source) > 0 else {
            return nil
        }
        return CGImageSourceCreateImageAtIndex(source, 0, nil)
    }
}
