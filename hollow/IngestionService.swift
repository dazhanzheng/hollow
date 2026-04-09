import Foundation
import Observation

@Observable
final class IngestionService {
    private(set) var isWatching = false
    private(set) var totalIngested: Int = 0
    private(set) var recentFiles: [String] = []
    private(set) var lastError: String?

    private let watcher: FileWatcher
    private let bridge: HollowBridge

    // Serial queue for intake (metadata + quick_hash, fast)
    private let intakeQueue = DispatchQueue(label: "com.syncpulse.hollow.intake")

    init(bridge: HollowBridge = .shared) {
        self.bridge = bridge
        self.watcher = FileWatcher(directory: FileWatcher.inboxURL)

        watcher.onNewFiles = { [weak self] urls in
            self?.intakeFiles(urls)
        }
        watcher.onRemovedFiles = { [weak self] urls in
            self?.handleRemovedFiles(urls)
        }
    }

    func start() {
        watcher.start()
        isWatching = true

        let bridge = self.bridge
        intakeQueue.async { [weak self] in
            let count = bridge.listFiles(limit: UInt32.max, offset: 0).count
            DispatchQueue.main.async { self?.totalIngested = count }

            // Ingest any files added while app was closed
            let inboxFiles = Self.scanInbox(FileWatcher.inboxURL)
            let newFiles = inboxFiles.filter { !bridge.pathExists($0.path) }
            if !newFiles.isEmpty {
                self?.intakeFiles(newFiles)
            }

            // Mark any leftover pending files as indexed
            self?.processAllPending()
        }
    }

    func stop() {
        watcher.stop()
        isWatching = false
    }

    /// Fast intake — metadata + quick_hash, instant DB insert, UI updates immediately.
    private func intakeFiles(_ urls: [URL]) {
        let bridge = self.bridge
        intakeQueue.async { [weak self] in
            for url in urls {
                let result = bridge.ingestFile(path: url.path)
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    switch result {
                    case .success(let record):
                        self.totalIngested += 1
                        self.recentFiles.insert(record.fileName, at: 0)
                        if self.recentFiles.count > 10 {
                            self.recentFiles.removeLast()
                        }
                        self.lastError = nil
                    case .duplicate:
                        break
                    case .error(let message):
                        self.lastError = message
                    }
                }
            }

            self?.processAllPending()
        }
    }

    /// Handle files removed from inbox — mark as "missing" in DB.
    private func handleRemovedFiles(_ urls: [URL]) {
        let bridge = self.bridge
        intakeQueue.async { [weak self] in
            for url in urls {
                bridge.markMissing(path: url.path)
            }
            let count = bridge.listFiles(limit: UInt32.max, offset: 0).count
            DispatchQueue.main.async { [weak self] in
                self?.totalIngested = count
            }
        }
    }

    /// Mark pending files as indexed, optionally compute full hash if enabled.
    private func processAllPending() {
        let bridge = self.bridge
        let fullHashEnabled = UserDefaults.standard.bool(forKey: "enableFullHash")
        let pendingIds = bridge.getPendingIds()
        for fileId in pendingIds {
            if fullHashEnabled {
                _ = bridge.computeHash(fileId: fileId)
            }
            bridge.markIndexed(fileId: fileId)
        }
    }

    private static func scanInbox(_ inboxURL: URL) -> [URL] {
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: inboxURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ) else { return [] }

        return contents.filter { url in
            let name = url.lastPathComponent
            if name.hasPrefix(".") { return false }
            let ext = url.pathExtension.lowercased()
            if ["tmp", "download", "crdownload", "partial"].contains(ext) { return false }
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else { return false }
            return !isDir.boolValue
        }
    }
}
