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

        // On startup, check for files that were indexed but never embedded
        // (e.g. model was downloaded after extraction, or prior embed failed).
        // Delay 5s to let the ingestion service finish its startup scan first.
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(5))
            processAllPending()
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
