import Foundation
import PDFKit
import CoreGraphics
import os

/// SwiftExtractor for PDF files. Uses a two-pass strategy:
///
///   1. **Native text layer** — try `PDFPage.string` first. Most PDFs are
///      digital-native (exported from Word/Pages/LaTeX/Chrome) and already
///      have perfectly usable text. This path is ~1000× faster than OCR
///      and produces higher-quality output.
///
///   2. **OCR fallback** — if the text layer is empty or suspiciously
///      thin (scanned page PDFs from a printer or phone camera), rasterize
///      each page at 200 DPI and run Apple Vision on the image.
///
/// The decision between the two paths is per-document, not per-page, so a
/// 200-page scanned PDF won't be triggered by one stray text-extractable
/// page and vice versa.
struct AppleVisionPdfExtractor: SwiftExtractor {
    let name = "AppleVisionPdf"
    let displayName = "Apple Vision (PDF)"
    let description = "Extracts text from PDFs using the native text layer when available, falling back to on-device OCR via Apple Vision for scanned pages."
    let supportedExtensions = ["pdf"]

    /// Threshold for "is the text layer usable?". Averaged across all pages:
    /// if the text layer has fewer than this many characters per page, we
    /// treat it as absent and OCR the pages instead. 50 chars per page is
    /// about "1–2 short sentences" — below that is probably only metadata
    /// and random ligature noise from a scanned PDF.
    private let textLayerMinAvgCharsPerPage = 50

    /// DPI for rasterizing pages when OCR is needed. 200 DPI is the
    /// commonly-cited sweet spot for modern text recognition — higher
    /// doesn't help accuracy but scales memory and time linearly.
    private let ocrDpi: CGFloat = 200

    /// Hard cap on page count to OCR. Scanned books with 500+ pages would
    /// otherwise freeze a worker for minutes. Past this many pages we
    /// index only the first N, log a warning, and report success.
    /// Users can still full-text search the indexed portion.
    private let maxOcrPages = 200

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        guard let pdf = PDFDocument(url: fileURL) else {
            throw SwiftExtractionError(
                message: "failed to open PDF at \(fileURL.path)"
            )
        }

        let pageCount = pdf.pageCount
        guard pageCount > 0 else {
            return SwiftExtractionResult(
                bodyText: "",
                encoding: "UTF-8",
                detectedMime: "application/pdf"
            )
        }

        // Pass 1: native text layer.
        var textLayer = ""
        for i in 0..<pageCount {
            guard let page = pdf.page(at: i) else { continue }
            if let s = page.string, !s.isEmpty {
                textLayer += s
                if !textLayer.hasSuffix("\n") {
                    textLayer += "\n"
                }
            }
        }

        let avgCharsPerPage = textLayer.count / max(pageCount, 1)
        if avgCharsPerPage >= textLayerMinAvgCharsPerPage {
            return SwiftExtractionResult(
                bodyText: textLayer,
                encoding: "UTF-8",
                detectedMime: "application/pdf"
            )
        }

        // Pass 2: OCR fallback.
        HollowLogger.ocr.info(
            "PDF text layer thin (\(avgCharsPerPage) chars/page) — falling back to OCR for \(fileURL.lastPathComponent, privacy: .public)"
        )

        let pagesToProcess = min(pageCount, maxOcrPages)
        var ocrOutput = ""
        var ocrFailures = 0

        for i in 0..<pagesToProcess {
            guard let page = pdf.page(at: i),
                  let cgImage = Self.renderPage(page, dpi: ocrDpi)
            else {
                ocrFailures += 1
                continue
            }

            do {
                let text = try AppleVisionOCR.recognizeText(in: cgImage)
                ocrOutput += text
                if !ocrOutput.hasSuffix("\n") {
                    ocrOutput += "\n"
                }
            } catch {
                ocrFailures += 1
                HollowLogger.ocr.error(
                    "OCR failed on page \(i + 1) of \(fileURL.lastPathComponent, privacy: .public): \(error.localizedDescription, privacy: .public)"
                )
            }
        }

        if pageCount > maxOcrPages {
            HollowLogger.ocr.warning(
                "PDF \(fileURL.lastPathComponent, privacy: .public) has \(pageCount) pages; only the first \(self.maxOcrPages) were OCR'd"
            )
        }

        // If every single page failed, report it up as an error so the file
        // isn't silently marked indexed with empty body.
        if ocrFailures == pagesToProcess && pagesToProcess > 0 {
            throw SwiftExtractionError(
                message: "OCR failed on all \(pagesToProcess) pages"
            )
        }

        return SwiftExtractionResult(
            bodyText: ocrOutput,
            encoding: "UTF-8",
            detectedMime: "application/pdf"
        )
    }

    /// Rasterize a single PDF page to a grayscale CGImage at the requested
    /// DPI. Grayscale is enough for OCR and costs 1/4 the memory of RGBA.
    private static func renderPage(_ page: PDFPage, dpi: CGFloat) -> CGImage? {
        let bounds = page.bounds(for: .cropBox)
        let scale = dpi / 72.0
        let width = Int(ceil(bounds.width * scale))
        let height = Int(ceil(bounds.height * scale))

        guard width > 0, height > 0 else { return nil }

        let colorSpace = CGColorSpaceCreateDeviceGray()
        guard let context = CGContext(
            data: nil,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.none.rawValue
        ) else {
            return nil
        }

        // White page background.
        context.setFillColor(CGColor(gray: 1.0, alpha: 1.0))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))

        context.scaleBy(x: scale, y: scale)
        context.translateBy(x: -bounds.origin.x, y: -bounds.origin.y)
        page.draw(with: .cropBox, to: context)

        return context.makeImage()
    }
}
