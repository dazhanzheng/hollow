import Foundation
import Observation

@Observable
final class IngestionService {
    private(set) var isWatching = false
    private(set) var totalIngested: Int = 0
    private(set) var recentFiles: [String] = []
    private(set) var lastError: String?
    private(set) var processingProgress: String?

    private let watcher: FileWatcher
    private let bridge: HollowBridge

    init(bridge: HollowBridge = .shared) {
        self.bridge = bridge
        self.watcher = FileWatcher(directory: FileWatcher.inboxURL)

        watcher.onNewFiles = { [weak self] urls in
            self?.enqueueFiles(urls)
        }
    }

    func start() {
        watcher.start()
        isWatching = true

        // Everything heavy runs off main thread
        let bridge = self.bridge
        let inboxURL = FileWatcher.inboxURL
        Task.detached(priority: .utility) { [weak self] in
            // Count already-ingested files
            let count = bridge.listFiles(limit: UInt32.max, offset: 0).count
            await MainActor.run { [weak self] in
                self?.totalIngested = count
            }

            // Scan inbox for files not yet ingested
            let files = Self.scanInbox(inboxURL)
            if !files.isEmpty {
                self?.enqueueFiles(files)
            }
        }
    }

    func stop() {
        watcher.stop()
        isWatching = false
    }

    // Serial queue ensures only one ingestion batch runs at a time
    private let ingestionQueue = DispatchQueue(label: "com.syncpulse.hollow.ingestion")

    private func enqueueFiles(_ urls: [URL]) {
        let bridge = self.bridge
        // Filter out files already in DB (fast path check, avoids expensive hash)
        let newURLs = urls.filter { !bridge.pathExists($0.path) }
        guard !newURLs.isEmpty else { return }
        let total = newURLs.count
        ingestionQueue.async { [weak self] in
            for (index, url) in newURLs.enumerated() {
                DispatchQueue.main.async { [weak self] in
                    self?.processingProgress = "Processing \(index + 1)/\(total)..."
                }

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
            DispatchQueue.main.async { [weak self] in
                self?.processingProgress = nil
            }
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
