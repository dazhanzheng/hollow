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

    init(bridge: HollowBridge = .shared) {
        self.bridge = bridge
        self.watcher = FileWatcher(directory: FileWatcher.inboxURL)

        watcher.onNewFiles = { [weak self] urls in
            self?.handleNewFiles(urls)
        }
    }

    func start() {
        watcher.start()
        isWatching = true
        totalIngested = bridge.listFiles(limit: UInt32.max, offset: 0).count
        performStartupScan()
    }

    func stop() {
        watcher.stop()
        isWatching = false
    }

    private func performStartupScan() {
        let inboxURL = FileWatcher.inboxURL
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: inboxURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ) else { return }

        let files = contents.filter { url in
            let name = url.lastPathComponent
            if name.hasPrefix(".") { return false }
            let ext = url.pathExtension.lowercased()
            if ["tmp", "download", "crdownload", "partial"].contains(ext) { return false }
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else { return false }
            return !isDir.boolValue
        }

        if !files.isEmpty {
            handleNewFiles(files)
        }
    }

    private func handleNewFiles(_ urls: [URL]) {
        Task.detached(priority: .utility) { [weak self] in
            guard let self else { return }
            for url in urls {
                let result = self.bridge.ingestFile(path: url.path)
                await MainActor.run {
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
        }
    }
}
