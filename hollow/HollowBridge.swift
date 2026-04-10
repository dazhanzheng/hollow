import Foundation
import os

/// Swift-side wrapper that manages the HollowCore lifecycle.
/// Constructs the database path in Application Support and holds
/// a reference to the Rust-backed HollowCore instance.
final class HollowBridge: @unchecked Sendable {
    static let shared = HollowBridge()

    private var core: HollowCore?

    var isReady: Bool { core != nil }

    private init() {
        do {
            let dbPath = try Self.databasePath()
            core = try HollowCore(dbPath: dbPath)
        } catch {
            HollowLogger.bridge.error("HollowBridge init failed: \(error)")
            core = nil
        }
    }

    private static func databasePath() throws -> String {
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

    func listFiles(limit: UInt32 = 20, offset: UInt32 = 0) -> [FileRecord] {
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
    func ingestFile(path: String) -> IngestResult {
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
    func computeHash(fileId: String) -> String? {
        guard let core else { return nil }
        return try? core.computeHash(fileId: fileId)
    }

    /// Mark file as fully processed.
    func markIndexed(fileId: String) {
        guard let core else { return }
        try? core.markIndexed(fileId: fileId)
    }

    func getPendingIds() -> [String] {
        guard let core else { return [] }
        return (try? core.getPendingIds()) ?? []
    }

    func getLogs(sinceId: UInt64) -> [LogEntry] {
        guard let core else { return [] }
        return core.getLogs(sinceId: sinceId)
    }

    func clearLogs() {
        guard let core else { return }
        core.clearLogs()
    }

    func pathExists(_ path: String) -> Bool {
        guard let core else { return false }
        return (try? core.pathExists(path: path)) ?? false
    }

    func markMissing(path: String) {
        guard let core else { return }
        try? core.markMissing(path: path)
    }

    /// Run content extraction for a file. Returns nil on bridge error.
    func extractContent(fileId: String) -> ExtractContentResult? {
        guard let core else { return nil }
        do {
            return try core.extractContent(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("extractContent failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Check whether a file's content has changed since last ingestion.
    func hasChanged(fileId: String) -> Bool {
        guard let core else { return false }
        do {
            return try core.hasChanged(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("hasChanged failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return false
        }
    }

    /// Flip a file back to pending for re-extraction.
    func markForReextraction(fileId: String) {
        guard let core else { return }
        do {
            try core.markForReextraction(fileId: fileId)
        } catch {
            HollowLogger.bridge.error("markForReextraction failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Get all file IDs waiting for content extraction.
    func getPendingExtractionIds() -> [String] {
        guard let core else { return [] }
        do {
            return try core.getPendingExtractionIds()
        } catch {
            HollowLogger.bridge.error("getPendingExtractionIds failed: \(error.localizedDescription, privacy: .public)")
            return []
        }
    }
}
