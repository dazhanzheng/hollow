import Foundation
import Observation
import os

@Observable
final class ModelDownloader: NSObject, @unchecked Sendable {
    var isDownloading = false
    var progress: Double = 0.0  // 0..1
    var error: String?

    private var session: URLSession?

    /// Download the 0.6B INT8 model from HuggingFace.
    /// Creates the destination directory if needed.
    func downloadDefaultModel() async throws {
        let modelsBase = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
            .appendingPathComponent("com.syncpulse.hollow/models/qwen3-embedding-0.6b-int8")

        try FileManager.default.createDirectory(at: modelsBase, withIntermediateDirectories: true)

        await MainActor.run {
            isDownloading = true
            progress = 0
            error = nil
        }

        do {
            // Download model file (~585 MB)
            let modelURL = URL(string: "https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/main/onnx/model_int8.onnx")!
            let modelDest = modelsBase.appendingPathComponent("model.onnx")
            try await downloadFile(from: modelURL, to: modelDest, progressWeight: 0.95)

            // Download tokenizer (~7 MB, fast)
            let tokenizerURL = URL(string: "https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/main/tokenizer.json")!
            let tokenizerDest = modelsBase.appendingPathComponent("tokenizer.json")
            try await downloadFile(from: tokenizerURL, to: tokenizerDest, progressWeight: 0.05)

            await MainActor.run {
                isDownloading = false
                progress = 1.0
            }

            HollowLogger.embedding.info("Model download complete")
        } catch {
            // Clean up partial downloads on failure
            try? FileManager.default.removeItem(at: modelsBase)

            await MainActor.run {
                self.isDownloading = false
                self.error = error.localizedDescription
            }

            HollowLogger.embedding.error("Model download failed: \(error)")
            throw error
        }
    }

    func cancel() {
        session?.invalidateAndCancel()
        session = nil
    }

    /// Download a single file with progress tracking using URLSessionDownloadDelegate.
    private func downloadFile(from url: URL, to destination: URL, progressWeight: Double) async throws {
        // Remove existing file if any
        try? FileManager.default.removeItem(at: destination)

        let config = URLSessionConfiguration.default
        let progressBase = await MainActor.run { self.progress }

        // Use delegate for progress reporting
        let delegate = DownloadDelegate { [weak self] fractionCompleted in
            let totalProgress = progressBase + fractionCompleted * progressWeight
            Task { @MainActor in
                self?.progress = totalProgress
            }
        }

        let session = URLSession(configuration: config, delegate: delegate, delegateQueue: nil)
        self.session = session

        let (tempURL, response) = try await session.download(from: url)

        guard let httpResponse = response as? HTTPURLResponse,
              httpResponse.statusCode == 200 else {
            let code = (response as? HTTPURLResponse)?.statusCode ?? 0
            throw URLError(.badServerResponse, userInfo: [
                NSLocalizedDescriptionKey: "Download failed with HTTP \(code)"
            ])
        }

        try FileManager.default.moveItem(at: tempURL, to: destination)
        session.invalidateAndCancel()
        self.session = nil
    }
}

/// Delegate that reports download progress via a callback.
private final class DownloadDelegate: NSObject, URLSessionDownloadDelegate, Sendable {
    let onProgress: @Sendable (Double) -> Void

    init(onProgress: @escaping @Sendable (Double) -> Void) {
        self.onProgress = onProgress
    }

    func urlSession(
        _ session: URLSession,
        downloadTask: URLSessionDownloadTask,
        didWriteData bytesWritten: Int64,
        totalBytesWritten: Int64,
        totalBytesExpectedToWrite: Int64
    ) {
        guard totalBytesExpectedToWrite > 0 else { return }
        let fraction = Double(totalBytesWritten) / Double(totalBytesExpectedToWrite)
        onProgress(fraction)
    }

    func urlSession(
        _ session: URLSession,
        downloadTask: URLSessionDownloadTask,
        didFinishDownloadingTo location: URL
    ) {
        // Handled by the async download(from:) call
    }
}
