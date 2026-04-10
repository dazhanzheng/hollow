import Foundation
import Observation
import os

// MARK: - Operations

/// Runs metadata intake (fast: filesystem metadata + quick_hash).
/// Dispatched on IngestionService.metadataQueue.
///
/// IMPORTANT: main() is synchronous. As soon as it returns, the OperationQueue
/// releases the operation, so any async callback that captures `self` weakly
/// will see nil and bail out. Capture the needed values (service, path, result)
/// into local bindings and let the Task hold them directly.
final class MetadataIntakeOperation: Operation, @unchecked Sendable {
    let path: String
    private weak var service: IngestionService?

    init(path: String, service: IngestionService) {
        self.path = path
        self.service = service
        super.init()
    }

    override func main() {
        guard !isCancelled else { return }
        let result = HollowBridge.shared.ingestFile(path: path)
        // Promote weak → strong local. The Task closure captures these locals,
        // not `self`, so it survives the operation being released after main().
        let capturedPath = path
        let capturedService = service
        Task { @MainActor in
            capturedService?.handleMetadataIntakeResult(result, path: capturedPath)
        }
    }
}

/// Runs content extraction for a file (slow: read + decode + zstd compress + DB write).
/// Dispatched on IngestionService.contentQueue.
/// See MetadataIntakeOperation above for the capture-locals rationale.
final class ContentExtractionOperation: Operation, @unchecked Sendable {
    let fileId: String
    private weak var service: IngestionService?

    init(fileId: String, service: IngestionService) {
        self.fileId = fileId
        self.service = service
        super.init()
    }

    override func main() {
        guard !isCancelled else { return }
        let result = HollowBridge.shared.extractContent(fileId: fileId)
        let capturedFileId = fileId
        let capturedService = service
        Task { @MainActor in
            capturedService?.handleContentExtractionResult(result, fileId: capturedFileId)
        }
    }
}

// MARK: - IngestionService

@MainActor @Observable
final class IngestionService {
    private(set) var isWatching = false
    private(set) var totalIngested: Int = 0
    private(set) var recentFiles: [String] = []
    private(set) var lastError: String?

    // Content extraction queue stats
    private(set) var extractionsInFlight: Int = 0
    private(set) var extractionsCompleted: Int = 0
    private(set) var extractionsFailed: Int = 0

    private let watcher: FileWatcher
    nonisolated private let bridge: HollowBridge

    private let metadataQueue: OperationQueue
    private let contentQueue: OperationQueue

    /// Concurrency: half the CPU cores, minimum 2. Background work must not
    /// starve the UI or foreground apps.
    nonisolated static var workerConcurrency: Int {
        let cores = ProcessInfo.processInfo.activeProcessorCount
        return max(2, cores / 2)
    }

    init(bridge: HollowBridge = .shared) {
        self.bridge = bridge
        self.watcher = FileWatcher(directory: FileWatcher.inboxURL)

        let concurrency = Self.workerConcurrency

        let mq = OperationQueue()
        mq.maxConcurrentOperationCount = concurrency
        mq.qualityOfService = .utility
        mq.name = "com.syncpulse.hollow.metadata"
        self.metadataQueue = mq

        let cq = OperationQueue()
        cq.maxConcurrentOperationCount = concurrency
        cq.qualityOfService = .utility
        cq.name = "com.syncpulse.hollow.content"
        self.contentQueue = cq

        watcher.onNewFiles = { [weak self] urls in
            Task { @MainActor in
                self?.enqueueMetadataIntake(paths: urls.map { $0.path })
            }
        }
        watcher.onRemovedFiles = { [weak self] urls in
            Task { @MainActor in
                self?.handleRemovedFiles(urls)
            }
        }
        watcher.onModifiedFiles = { [weak self] urls in
            Task { @MainActor in
                self?.handleModifiedFiles(urls)
            }
        }
    }

    func start() {
        watcher.start()
        isWatching = true
        HollowLogger.ingestion.info("Ingestion service started (workers: \(Self.workerConcurrency))")

        let bridge = self.bridge
        // Scan for missed files on the main actor (watcher is main-actor-isolated),
        // then hand off to a detached task for all the heavy bridge work.
        let inboxFiles = watcher.scanAllFiles()

        Task.detached { [weak self] in
            guard let self else { return }

            let count = bridge.listFiles(limit: UInt32.max, offset: 0).count

            // Ingest any files added while app was closed
            let newPaths = inboxFiles
                .filter { !bridge.pathExists($0.path) }
                .map { $0.path }

            // Reclaim any files stuck in "extracting" state from a previous crash.
            let reclaimed = bridge.reclaimExtracting()
            if reclaimed > 0 {
                HollowLogger.ingestion.info("Startup: reclaimed \(reclaimed) files stuck in extracting state")
            }

            // Resume any pending extractions from previous sessions
            let pendingIds = bridge.getPendingExtractionIds()
            if !pendingIds.isEmpty {
                HollowLogger.ingestion.info("Startup: resuming \(pendingIds.count) pending extractions")
            }

            await MainActor.run { [weak self] in
                guard let self else { return }
                self.totalIngested = count
                if !newPaths.isEmpty {
                    self.enqueueMetadataIntake(paths: newPaths)
                }
                if !pendingIds.isEmpty {
                    self.enqueueContentExtraction(fileIds: pendingIds)
                }
            }
        }
    }

    func stop() {
        watcher.stop()
        isWatching = false
        metadataQueue.cancelAllOperations()
        contentQueue.cancelAllOperations()
        HollowLogger.ingestion.info("Ingestion service stopped")
    }

    // MARK: - Enqueue

    func enqueueMetadataIntake(paths: [String]) {
        guard !paths.isEmpty else { return }
        let ops = paths.map { MetadataIntakeOperation(path: $0, service: self) }
        metadataQueue.addOperations(ops, waitUntilFinished: false)
    }

    func enqueueContentExtraction(fileIds: [String]) {
        guard !fileIds.isEmpty else { return }
        extractionsInFlight += fileIds.count
        let ops = fileIds.map { ContentExtractionOperation(fileId: $0, service: self) }
        contentQueue.addOperations(ops, waitUntilFinished: false)
    }

    // MARK: - Handlers (called on main queue from Operations)

    func handleMetadataIntakeResult(_ result: HollowBridge.IngestResult, path: String) {
        switch result {
        case .success(let record):
            totalIngested += 1
            recentFiles.insert(record.fileName, at: 0)
            if recentFiles.count > 10 {
                recentFiles.removeLast()
            }
            lastError = nil
            HollowLogger.ingestion.info("Metadata intake: \(record.fileName)")
            // Auto-enqueue for content extraction
            enqueueContentExtraction(fileIds: [record.id])
        case .duplicate:
            HollowLogger.ingestion.debug("Duplicate skipped: \(path)")
        case .error(let message):
            lastError = message
            HollowLogger.ingestion.error("Metadata intake error: \(message)")
        }
    }

    func handleContentExtractionResult(_ result: ExtractContentResult?, fileId: String) {
        extractionsInFlight = max(0, extractionsInFlight - 1)
        guard let result else {
            extractionsFailed += 1
            lastExtractionError = "bridge error"
            HollowLogger.ingestion.error("Extraction bridge error: \(fileId)")
            return
        }
        switch result.status {
        case "indexed":
            extractionsCompleted += 1
            lastExtractionError = nil
            HollowLogger.ingestion.info("Extracted: \(fileId) (\(result.bodyTextBytes) bytes, \(result.extractorName ?? "?"))")
        case "missing":
            HollowLogger.ingestion.info("Skipped missing file: \(fileId)")
        case "unsupported":
            HollowLogger.ingestion.debug("Unsupported format for \(fileId): \(result.detectedMime)")
        default: // "extract_failed"
            extractionsFailed += 1
            lastExtractionError = result.error
            HollowLogger.ingestion.warning("Extract failed: \(fileId) — \(result.error ?? "?")")
        }
    }

    // MARK: - Modification

    private func handleModifiedFiles(_ urls: [URL]) {
        let bridge = self.bridge
        Task.detached { [weak self] in
            var reextractIds: [String] = []
            var newIngestPaths: [String] = []

            for url in urls {
                let path = url.path
                guard let fileId = bridge.fileIdForPath(path) else {
                    // Unknown path — treat as new file
                    newIngestPaths.append(path)
                    continue
                }
                if bridge.hasChanged(fileId: fileId) {
                    bridge.markForReextraction(fileId: fileId)
                    reextractIds.append(fileId)
                    HollowLogger.ingestion.info("Re-extraction queued: \(path)")
                }
            }

            // Capture as immutable lets before crossing into MainActor to avoid
            // "reference to captured var" warnings in Swift 6 strict concurrency.
            let finalNewIngestPaths = newIngestPaths
            let finalReextractIds = reextractIds

            await MainActor.run { [weak self] in
                guard let self else { return }
                if !finalNewIngestPaths.isEmpty {
                    self.enqueueMetadataIntake(paths: finalNewIngestPaths)
                }
                if !finalReextractIds.isEmpty {
                    self.enqueueContentExtraction(fileIds: finalReextractIds)
                }
            }
        }
    }

    // MARK: - Removal

    private func handleRemovedFiles(_ urls: [URL]) {
        let bridge = self.bridge
        Task.detached { [weak self] in
            for url in urls {
                bridge.markMissing(path: url.path)
            }
            let count = bridge.listFiles(limit: UInt32.max, offset: 0).count
            await MainActor.run { [weak self] in
                self?.totalIngested = count
            }
        }
    }

    // MARK: - Extraction error surfacing

    private(set) var lastExtractionError: String?
}
