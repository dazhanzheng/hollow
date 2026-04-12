import Foundation
import Vision
import CoreGraphics

/// Thin synchronous wrapper around `VNRecognizeTextRequest`. Used by
/// `AppleVisionImageExtractor` and `AppleVisionPdfExtractor` to OCR a
/// single rasterized page or image.
///
/// Runs on the calling thread (typically a background Operation worker),
/// so callers should already be off the main actor.
enum AppleVisionOCR {

    /// Languages Vision will try to recognize. English first because it's
    /// the dominant North American market, plus CJK for users with mixed
    /// content. Vision also auto-detects beyond this set when
    /// `automaticallyDetectsLanguage` is on.
    static let defaultLanguages: [String] = [
        "en-US",
        "zh-Hans",
        "zh-Hant",
        "ja-JP",
        "ko-KR",
    ]

    /// Recognize text in a single CGImage. Returns the concatenation of
    /// all detected text lines, joined with newlines. Empty string if the
    /// image contained no detectable text (not an error — blank pages,
    /// pure graphics, etc. are legitimate outcomes).
    static func recognizeText(
        in cgImage: CGImage,
        languages: [String] = defaultLanguages
    ) throws -> String {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = languages
        request.usesLanguageCorrection = true
        request.automaticallyDetectsLanguage = true

        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        try handler.perform([request])

        let observations = request.results ?? []
        let lines = observations.compactMap { observation in
            observation.topCandidates(1).first?.string
        }
        return lines.joined(separator: "\n")
    }
}
