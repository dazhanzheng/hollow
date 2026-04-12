import Foundation

/// Registry of Swift-side extractors. Parallels the Rust `ExtractorRegistry`
/// and lives alongside it — when the content pipeline picks up a file, it
/// checks this registry first, and only falls back to Rust if no Swift
/// extractor claims the extension.
///
/// This is intentionally a plain class (not `@MainActor`) because the
/// content pipeline runs on background threads. All state is immutable
/// after init, so lookups are thread-safe.
final class SwiftExtractorRegistry: @unchecked Sendable {
    static let shared = SwiftExtractorRegistry()

    private let extractors: [any SwiftExtractor]

    private init() {
        self.extractors = [
            // Image files — Vision OCR directly
            AppleVisionImageExtractor(),
            // PDFs — text layer first, OCR fallback for scans
            AppleVisionPdfExtractor(),
            // OOXML with inline image OCR
            AppleVisionDocxExtractor(),
            AppleVisionPptxExtractor(),
            // OpenDocument with inline image OCR
            AppleVisionOdfExtractor(),
            // EPUB ebooks with inline image OCR
            AppleVisionEpubExtractor(),
            // iWork via Spotlight importer + bundle Data/ OCR
            AppleVisionIWorkExtractor(),
        ]
    }

    /// All registered extractors, for the Settings plugin list.
    var all: [any SwiftExtractor] {
        extractors
    }

    /// Find an enabled Swift extractor for the file at the given URL.
    /// Returns nil if no extractor claims the file's extension, or if the
    /// matching extractor has been turned off by the user in Settings.
    func find(for fileURL: URL) -> (any SwiftExtractor)? {
        let ext = fileURL.pathExtension.lowercased()
        guard !ext.isEmpty else { return nil }

        for extractor in extractors where extractor.supportedExtensions.contains(ext) {
            if isEnabled(extractor) {
                return extractor
            }
        }
        return nil
    }

    /// UserDefaults-backed enabled state. Shares the `plugin.enabled.<name>`
    /// key format with Rust-side extractors, so the Settings UI can treat
    /// both kinds uniformly.
    func isEnabled(_ extractor: any SwiftExtractor) -> Bool {
        let key = "plugin.enabled.\(extractor.name)"
        return UserDefaults.standard.object(forKey: key) as? Bool ?? true
    }

    func setEnabled(_ extractor: any SwiftExtractor, enabled: Bool) {
        let key = "plugin.enabled.\(extractor.name)"
        UserDefaults.standard.set(enabled, forKey: key)
    }
}
