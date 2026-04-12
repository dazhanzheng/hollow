import Foundation
import CoreServices
import os

/// SwiftExtractor for Apple's iWork suite: Pages (.pages), Numbers
/// (.numbers), and Keynote (.key / .keynote).
///
/// iWork files are **bundles** (opaque directories on disk). Their
/// content is stored in Apple's proprietary IWA format — Snappy-
/// compressed protobuf with no public documentation, maintained via
/// third-party reverse engineering that constantly breaks with new
/// iWork versions. Rather than go down that road, we use two stable
/// Apple-provided entry points:
///
///   1. **Text layer** via `MDItemCopyAttribute` with
///      `kMDItemTextContent`. This is the same mechanism Spotlight
///      uses to index iWork files — Apple ships the Spotlight
///      importer as part of macOS, and it returns the user-visible
///      plain text content. Stable across iWork updates because
///      Apple themselves depend on it.
///
///   2. **Embedded images** via direct filesystem enumeration of
///      `<bundle>/Data/*.{png,jpg,...}`. iWork's internal convention
///      (stable for many years) puts every embedded media file in
///      the bundle's `Data/` subdirectory as a regular image file —
///      we can OCR them without touching any IWA internals.
///
/// Because `MDItemCopyAttribute` returns plain text without position
/// markers, image OCR results are **appended at the end** of the body
/// text rather than inlined, wrapped in `[Image: …]` markers so the
/// text can still be distinguished from the document's native text
/// layer. This is the best we can do without a structured parser.
struct AppleVisionIWorkExtractor: SwiftExtractor {
    let name = "AppleVisionIWork"
    let displayName = "Apple Vision (iWork)"
    let description = "Extracts text from Pages, Numbers, and Keynote files using Apple's Spotlight importer. Embedded images are OCR'd through Apple Vision and appended at the end of the document."
    let supportedExtensions = ["pages", "numbers", "key", "keynote"]

    func extract(fileURL: URL) throws -> SwiftExtractionResult {
        // Step 1: Spotlight text content
        let mainText = Self.copySpotlightText(from: fileURL) ?? ""

        // Step 2: OCR any images inside the bundle's Data/ directory.
        // For files that aren't bundles (older .key single-file form)
        // or bundles without Data/ this yields an empty list silently.
        let imageOcr = Self.ocrBundleImages(fileURL: fileURL)

        // Merge: text layer first, then per-image OCR results at end.
        var body = mainText
        for ocr in imageOcr {
            let trimmed = ocr.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.isEmpty { continue }
            if !body.isEmpty && !body.hasSuffix("\n") {
                body += "\n"
            }
            body += "[Image: \(trimmed)]\n"
        }

        let mime = Self.mimeType(for: fileURL.pathExtension.lowercased())
        return SwiftExtractionResult(
            bodyText: body,
            encoding: "UTF-8",
            detectedMime: mime
        )
    }

    // MARK: - Spotlight

    /// Read `kMDItemTextContent` via MDItem. Returns nil if Spotlight
    /// hasn't indexed the file (uncommon but possible on brand-new
    /// files not yet touched by the importer) or if the importer
    /// doesn't produce any text for this document.
    private static func copySpotlightText(from url: URL) -> String? {
        guard let item = MDItemCreateWithURL(kCFAllocatorDefault, url as CFURL) else {
            HollowLogger.ocr.warning(
                "MDItemCreate failed for \(url.lastPathComponent, privacy: .public)"
            )
            return nil
        }
        let attr = MDItemCopyAttribute(item, kMDItemTextContent)
        return attr as? String
    }

    // MARK: - Bundle image OCR

    /// Enumerate image files inside the iWork bundle's `Data/`
    /// directory and OCR each one. Returns the OCR text for each
    /// successfully-processed image, in filesystem enumeration order.
    /// Failures and empty-OCR results are filtered out.
    private static func ocrBundleImages(fileURL: URL) -> [String] {
        let fm = FileManager.default

        // iWork bundles are directories; older single-file .key
        // documents (pre-iWork 09) are just zip files. Only descend
        // when it's actually a directory.
        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: fileURL.path, isDirectory: &isDir),
              isDir.boolValue
        else {
            return []
        }

        let dataDir = fileURL.appendingPathComponent("Data", isDirectory: true)
        guard fm.fileExists(atPath: dataDir.path, isDirectory: &isDir),
              isDir.boolValue
        else {
            return []
        }

        let imageExts: Set<String> = [
            "png", "jpg", "jpeg", "heic", "heif",
            "tiff", "tif", "gif", "bmp", "webp",
        ]

        let enumerator = fm.enumerator(
            at: dataDir,
            includingPropertiesForKeys: [.isRegularFileKey],
            options: [.skipsHiddenFiles]
        )
        guard let enumerator else { return [] }

        var results: [String] = []
        for case let url as URL in enumerator {
            guard imageExts.contains(url.pathExtension.lowercased()) else {
                continue
            }
            do {
                let data = try Data(contentsOf: url, options: [.mappedIfSafe])
                let text = try OCRHelper.ocrImageData(data)
                if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    results.append(text)
                }
            } catch {
                HollowLogger.ocr.error(
                    "OCR failed on bundle image \(url.lastPathComponent, privacy: .public): \(error.localizedDescription, privacy: .public)"
                )
            }
        }
        return results
    }

    private static func mimeType(for ext: String) -> String {
        switch ext {
        case "pages":
            return "application/vnd.apple.pages"
        case "numbers":
            return "application/vnd.apple.numbers"
        case "key", "keynote":
            return "application/vnd.apple.keynote"
        default:
            return "application/octet-stream"
        }
    }
}
