import Foundation

/// A content extractor implemented in Swift, parallel to the Rust
/// `Extractor` trait in hollow-core. Used for formats that need macOS
/// system frameworks (Vision, PDFKit, Core Image, …) which the Rust crate
/// can't access.
///
/// Swift extractors are routed by file extension before the Rust
/// ContentPipeline runs. Results are committed to the database via the
/// shared `extract_content_external` FFI, which runs the same state
/// machine (pending → extracting → indexed/failed) as the Rust pipeline.
protocol SwiftExtractor: Sendable {
    /// Stable identifier used in DB records and in the Settings plugin list.
    /// Must be unique across both Rust and Swift extractors.
    var name: String { get }

    /// Human-readable name for the Settings UI.
    var displayName: String { get }

    /// One-line description shown in the Settings UI.
    var description: String { get }

    /// File extensions this extractor claims. Matched case-insensitively.
    var supportedExtensions: [String] { get }

    /// Extract body text from the file at the given URL.
    /// Runs synchronously on the calling thread (typically a background
    /// worker from the content extraction OperationQueue).
    func extract(fileURL: URL) throws -> SwiftExtractionResult
}

/// Result of a successful Swift-side extraction. Matches the shape of
/// the Rust `ExtractionResult` plus a MIME type, because Swift extractors
/// also own their own format identification.
struct SwiftExtractionResult {
    /// The extracted text, ready to be compressed and written to
    /// `file_content.body_text_compressed`.
    let bodyText: String

    /// Encoding label. Swift extractors produce `String` directly, so this
    /// is almost always "UTF-8".
    let encoding: String?

    /// MIME type identifying this file's format. Stored on the `files` row
    /// as `detected_mime`. Use the most specific type you can, e.g.
    /// "image/png" rather than "application/octet-stream".
    let detectedMime: String
}

/// Error raised by Swift extractors. Wraps a descriptive message that gets
/// stored in `file_content.extract_error` on failure.
struct SwiftExtractionError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
