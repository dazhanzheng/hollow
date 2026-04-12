import Foundation
import Observation
import os

@MainActor @Observable
final class EmbeddingService {
    var isProcessing = false
    var processedCount = 0
    var totalPending = 0

    private let embeddingQueue: OperationQueue = {
        let q = OperationQueue()
        q.name = "com.syncpulse.hollow.embedding"
        q.maxConcurrentOperationCount = 1
        q.qualityOfService = .utility
        return q
    }()

    private var debounceTask: Task<Void, Never>?
    private var notificationObserver: NSObjectProtocol?

    /// Begin listening for `.fileIndexed` notifications and auto-process
    /// pending embeddings with a 3-second debounce to batch rapid extractions.
    /// Also processes any pending embeddings from previous sessions on startup.
    func startListening() {
        notificationObserver = NotificationCenter.default.addObserver(
            forName: .fileIndexed,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            Task { @MainActor in
                self?.scheduleProcessing()
            }
        }

        // On startup: preload the model in the background, then process pending files.
        Task.detached {
            let loaded = HollowBridge.shared.preloadEmbeddingModel()
            if loaded {
                HollowLogger.embedding.info("Embedding model preloaded")
            }
            // After model is ready, process any pending files
            try? await Task.sleep(for: .seconds(2))
            await MainActor.run { [weak self] in
                self?.processAllPending()
            }
        }
    }

    private func scheduleProcessing() {
        debounceTask?.cancel()
        debounceTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(3)) // Wait for batch to settle
            guard !Task.isCancelled else { return }
            processAllPending()
        }
    }

    func processAllPending() {
        guard !isProcessing else { return }
        guard HollowBridge.shared.isEmbeddingReady() else {
            HollowLogger.embedding.info("Embedding model not downloaded, skipping")
            return
        }

        isProcessing = true
        processedCount = 0

        DispatchQueue.global(qos: .utility).async { [weak self] in
            let ids = HollowBridge.shared.getPendingEmbeddingIds()
            let total = ids.count

            Task { @MainActor in
                self?.totalPending = total
            }

            var succeeded = 0
            for (index, fileId) in ids.enumerated() {
                if HollowBridge.shared.embedFile(fileId: fileId) {
                    succeeded += 1
                }
                Task { @MainActor in
                    self?.processedCount = index + 1
                }
            }

            Task { @MainActor in
                self?.isProcessing = false
                if succeeded > 0 {
                    HollowLogger.embedding.info("Embedded \(succeeded)/\(total) files")
                } else if total > 0 {
                    HollowLogger.embedding.warning("Embedding failed for all \(total) files (model not downloaded?)")
                }
            }
        }
    }
}
