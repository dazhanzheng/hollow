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

    // Serial queue for fast intake (metadata only, instant)
    private let intakeQueue = DispatchQueue(label: "com.syncpulse.hollow.intake")

    // Concurrent queue for heavy background work (hash, future: content extraction)
    // Semaphore limits to 3 concurrent I/O operations to avoid saturating disk bandwidth
    private let processingQueue = DispatchQueue(
        label: "com.syncpulse.hollow.processing",
        attributes: .concurrent
    )
    private let ioSemaphore = DispatchSemaphore(value: 3)

    init(bridge: HollowBridge = .shared) {
        self.bridge = bridge
        self.watcher = FileWatcher(directory: FileWatcher.inboxURL)

        watcher.onNewFiles = { [weak self] urls in
            self?.intakeFiles(urls)
        }
    }

    func start() {
        watcher.start()
        isWatching = true

        let bridge = self.bridge
        // Load count and process startup scan entirely off main thread
        intakeQueue.async { [weak self] in
            let count = bridge.listFiles(limit: UInt32.max, offset: 0).count
            DispatchQueue.main.async { self?.totalIngested = count }

            // Ingest any files added while app was closed
            let inboxFiles = Self.scanInbox(FileWatcher.inboxURL)
            let newFiles = inboxFiles.filter { !bridge.pathExists($0.path) }
            if !newFiles.isEmpty {
                self?.intakeFiles(newFiles)
            }

            // Process any pending files from previous runs (hash not computed)
            self?.processAllPending()
        }
    }

    func stop() {
        watcher.stop()
        isWatching = false
    }

    /// Phase 1: Fast intake — metadata only, instant DB insert, UI updates immediately.
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

            // After intake batch, kick off background processing
            self?.processAllPending()
        }
    }

    /// Phase 2: Background processing — hash computation, parallel across CPU cores.
    private func processAllPending() {
        let bridge = self.bridge
        let pendingIds = bridge.getPendingIds()
        guard !pendingIds.isEmpty else { return }

        let total = pendingIds.count
        let completed = AtomicCounter()

        DispatchQueue.main.async { [weak self] in
            self?.processingProgress = "Hashing 0/\(total)..."
        }

        let group = DispatchGroup()
        for fileId in pendingIds {
            group.enter()
            processingQueue.async { [weak self] in
                defer {
                    self?.ioSemaphore.signal()
                    group.leave()
                }
                self?.ioSemaphore.wait()

                // Compute hash (heavy I/O, max 3 concurrent)
                _ = bridge.computeHash(fileId: fileId)
                bridge.markIndexed(fileId: fileId)

                let done = completed.increment()
                DispatchQueue.main.async { [weak self] in
                    self?.processingProgress = "Hashing \(done)/\(total)..."
                }
            }
        }

        group.notify(queue: .main) { [weak self] in
            self?.processingProgress = nil
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

/// Simple thread-safe counter for tracking parallel progress.
private final class AtomicCounter: @unchecked Sendable {
    private var value = 0
    private let lock = NSLock()

    func increment() -> Int {
        lock.lock()
        defer { lock.unlock() }
        value += 1
        return value
    }
}
