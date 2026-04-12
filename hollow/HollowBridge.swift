import Foundation
import os

/// Swift-side wrapper that manages the HollowCore lifecycle.
/// Constructs the database path in Application Support and holds
/// a reference to the Rust-backed HollowCore instance.
final class HollowBridge: @unchecked Sendable {
    nonisolated static let shared = HollowBridge()

    private nonisolated(unsafe) var core: (any HollowCoreProtocol)?

    nonisolated var isReady: Bool { core != nil }

    /// UserDefaults key prefix for per-plugin enable flags. The UI reads/writes
    /// the same key via @AppStorage; the bridge is the source of truth that
    /// pushes the state down into the Rust side at startup and on toggle.
    static let pluginEnabledKeyPrefix = "plugin.enabled."

    static func pluginEnabledKey(_ name: String) -> String {
        pluginEnabledKeyPrefix + name
    }

    private init() {
        // Set ONNX Runtime dylib path for the ort crate's load-dynamic feature
        // before any embedding calls happen.
        if let modelsDir = try? Self.modelsDirectory() {
            let dylibPath = modelsDir.appendingPathComponent("libonnxruntime.dylib").path
            if FileManager.default.fileExists(atPath: dylibPath) {
                setenv("ORT_DYLIB_PATH", dylibPath, 1)
            }
        }

        do {
            let dbPath = try Self.databasePath()
            core = try HollowCore(dbPath: dbPath) as any HollowCoreProtocol
            syncExtractorPreferencesToCore()
        } catch {
            HollowLogger.bridge.error("HollowBridge init failed: \(error)")
            core = nil
        }
    }

    /// Walk the built-in extractors and push UserDefaults prefs down into Rust.
    /// Default (no stored value) is enabled. Must run after `core` is set.
    private func syncExtractorPreferencesToCore() {
        guard let core else { return }
        let defaults = UserDefaults.standard
        for info in core.listExtractors() {
            let key = Self.pluginEnabledKey(info.name)
            // Default (missing key) = enabled. Only treat an explicit `false`
            // stored bool as disabled.
            let enabled = defaults.object(forKey: key) as? Bool ?? true
            core.setExtractorEnabled(name: info.name, enabled: enabled)
        }
    }

    /// Return all built-in extractor plugins for the settings UI.
    nonisolated func listExtractors() -> [ExtractorPluginInfo] {
        guard let core else { return [] }
        return core.listExtractors()
    }

    /// Enable or disable an extractor plugin by name. Persists the choice in
    /// UserDefaults and pushes it down into the Rust pipeline immediately.
    nonisolated func setExtractorEnabled(name: String, enabled: Bool) {
        UserDefaults.standard.set(enabled, forKey: Self.pluginEnabledKey(name))
        guard let core else { return }
        core.setExtractorEnabled(name: name, enabled: enabled)
    }

    static func modelsDirectory() throws -> URL {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
        return appSupport.appendingPathComponent("com.syncpulse.hollow/models")
    }

    static func databasePath() throws -> String {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!

        let hollowDir = appSupport.appendingPathComponent(
            "com.syncpulse.hollow",
            isDirectory: true
        )

        try FileManager.default.createDirectory(
            at: hollowDir,
            withIntermediateDirectories: true
        )

        return hollowDir.appendingPathComponent("hollow.db").path
    }

    nonisolated func listFiles(limit: UInt32 = 20, offset: UInt32 = 0) -> [FileRecord] {
        guard let core else { return [] }
        do {
            return try core.listFiles(limit: limit, offset: offset)
        } catch {
            HollowLogger.bridge.error("listFiles failed: \(error)")
            return []
        }
    }

    enum IngestResult {
        case success(FileRecord)
        case duplicate
        case error(String)
    }

    /// Fast intake: only reads fs metadata, no file content. Returns instantly.
    nonisolated func ingestFile(path: String) -> IngestResult {
        guard let core else { return .error("HollowCore not initialized") }
        do {
            let record = try core.ingestFile(filePath: path)
            return .success(record)
        } catch HollowError.DuplicateFile(_) {
            return .duplicate
        } catch {
            return .error(error.localizedDescription)
        }
    }

    /// Heavy: reads file, computes SHA-256. Call from background thread.
    nonisolated func computeHash(fileId: String) -> String? {
        guard let core else { return nil }
        return try? core.computeHash(fileId: fileId)
    }

    /// Mark file as fully processed.
    nonisolated func markIndexed(fileId: String) {
        guard let core else { return }
        try? core.markIndexed(fileId: fileId)
    }

    nonisolated func getPendingIds() -> [String] {
        guard let core else { return [] }
        return (try? core.getPendingIds()) ?? []
    }

    nonisolated func getLogs(sinceId: UInt64) -> [LogEntry] {
        guard let core else { return [] }
        return core.getLogs(sinceId: sinceId)
    }

    nonisolated func clearLogs() {
        guard let core else { return }
        core.clearLogs()
    }

    nonisolated func pathExists(_ path: String) -> Bool {
        guard let core else { return false }
        return (try? core.pathExists(path: path)) ?? false
    }

    nonisolated func markMissing(path: String) {
        guard let core else { return }
        try? core.markMissing(path: path)
    }

    /// Run content extraction for a file. Returns nil on bridge error.
    nonisolated func extractContent(fileId: String) -> ExtractContentResult? {
        guard let core else { return nil }
        do {
            return try core.extractContent(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("extractContent failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Commit an extraction outcome produced by a Swift-side extractor
    /// (e.g. Apple Vision OCR). Runs the same state machine as
    /// `extractContent` on the Rust side but uses the supplied body text
    /// instead of the Rust ContentPipeline. Returns nil on bridge error.
    nonisolated func extractContentExternal(
        fileId: String,
        status: String,
        bodyText: String?,
        extractorName: String,
        detectedMime: String,
        encoding: String?,
        error: String?
    ) -> ExtractContentResult? {
        guard let core else { return nil }
        do {
            return try core.extractContentExternal(
                fileId: fileId,
                status: status,
                bodyText: bodyText,
                extractorName: extractorName,
                detectedMime: detectedMime,
                encoding: encoding,
                error: error
            )
        } catch {
            HollowLogger.bridge.error("extractContentExternal failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Ask Rust to pull the text layer and embedded image bytes out of
    /// a zip-based document (docx/pptx/odt/ods/odp/epub). Returns `nil`
    /// if the file type isn't image-aware or if the file has vanished.
    ///
    /// Used by the Swift OCR-enhanced extractors: they get back a
    /// `text_template` with `{{HOLLOW_IMG_N}}` placeholders plus the
    /// raw bytes of every referenced image, run Vision on each image,
    /// and substitute the results into the template.
    nonisolated func extractWithImages(fileId: String) -> ExtractWithImagesResult? {
        guard let core else { return nil }
        do {
            return try core.extractWithImages(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("extractWithImages failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Read back the extracted body text for a file, decompressing the
    /// zstd blob stored in `file_content`. Returns nil if no content has
    /// been extracted yet (pending / unsupported / missing).
    nonisolated func getBodyText(fileId: String) -> String? {
        guard let core else { return nil }
        do {
            return try core.getBodyText(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("getBodyText failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Fetch the stored record for a file by id. Used by
    /// `ContentExtractionOperation` to look up the current path before
    /// dispatching to either Swift or Rust extraction.
    nonisolated func getFile(fileId: String) -> FileRecord? {
        guard let core else { return nil }
        do {
            return try core.getFile(id: fileId)
        } catch {
            HollowLogger.bridge.error("getFile failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Check whether a file's content has changed since last ingestion.
    nonisolated func hasChanged(fileId: String) -> Bool {
        guard let core else { return false }
        do {
            return try core.hasChanged(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("hasChanged failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return false
        }
    }

    /// Flip a file back to pending for re-extraction.
    nonisolated func markForReextraction(fileId: String) {
        guard let core else { return }
        do {
            try core.markForReextraction(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("markForReextraction failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Look up a file's UUID by its current path.
    nonisolated func fileIdForPath(_ path: String) -> String? {
        guard let core else { return nil }
        do {
            return try core.fileIdForPath(path: path)
        } catch {
            HollowLogger.bridge.error("fileIdForPath failed: \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Get all file IDs waiting for content extraction.
    nonisolated func getPendingExtractionIds() -> [String] {
        guard let core else { return [] }
        do {
            return try core.getPendingExtractionIds()
        } catch {
            HollowLogger.bridge.error("getPendingExtractionIds failed: \(error.localizedDescription, privacy: .public)")
            return []
        }
    }

    /// Reclaim files stuck in the `extracting` state from a previous crash.
    nonisolated func reclaimExtracting() -> UInt32 {
        guard let core else { return 0 }
        return (try? core.reclaimExtracting()) ?? 0
    }

    nonisolated func listEmbeddingModels() -> [EmbeddingModelInfo] {
        guard let core else { return [] }
        return core.listEmbeddingModels()
    }

    nonisolated func isEmbeddingReady() -> Bool {
        guard let core else { return false }
        return core.isEmbeddingReady()
    }

    nonisolated func getEmbeddingStatus() -> EmbeddingStatus? {
        guard let core else { return nil }
        return try? core.getEmbeddingStatus()
    }

    nonisolated func getPendingEmbeddingIds() -> [String] {
        guard let core else { return [] }
        return (try? core.getPendingEmbeddingIds()) ?? []
    }

    nonisolated func embedFile(fileId: String) -> Bool {
        guard let core else { return false }
        do {
            return try core.embedFile(fileId: fileId)
        } catch {
            HollowLogger.embedding.error("embedFile failed for \(fileId): \(error)")
            return false
        }
    }

    /// Preload embedding model into memory. Call from background thread on startup.
    nonisolated func preloadEmbeddingModel() -> Bool {
        guard let core else { return false }
        return core.preloadEmbeddingModel()
    }

    nonisolated func hybridSearch(query: String, limit: UInt32 = 50) -> [SearchResult] {
        guard let core else { return [] }
        do {
            return try core.hybridSearch(query: query, limit: limit)
        } catch {
            HollowLogger.search.error("Hybrid search failed: \(error)")
            return []
        }
    }

    /// Full-text search across all indexed content.
    nonisolated func search(query: String, limit: UInt32 = 50) -> [SearchResult] {
        guard let core else { return [] }
        do {
            return try core.search(query: query, limit: limit)
        } catch {
            HollowLogger.search.error("Search failed: \(error)")
            return []
        }
    }
}
