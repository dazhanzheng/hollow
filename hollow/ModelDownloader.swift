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
    /// Also ensures the ONNX Runtime dylib is present.
    func downloadDefaultModel() async throws {
        let modelsBase = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
            .appendingPathComponent("com.syncpulse.hollow/models")

        let modelDir = modelsBase.appendingPathComponent("qwen3-embedding-0.6b-int8")
        try FileManager.default.createDirectory(at: modelDir, withIntermediateDirectories: true)

        await MainActor.run {
            isDownloading = true
            progress = 0
            error = nil
        }

        do {
            // Step 0: Ensure ONNX Runtime dylib is available (~30 MB)
            try await ensureOnnxRuntime(modelsBase: modelsBase)

            // Download model file (~585 MB)
            let modelURL = URL(string: "https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/main/onnx/model_int8.onnx")!
            let modelDest = modelDir.appendingPathComponent("model.onnx")
            try await downloadFile(from: modelURL, to: modelDest, progressWeight: 0.90)

            // Download tokenizer (~7 MB, fast)
            let tokenizerURL = URL(string: "https://huggingface.co/onnx-community/Qwen3-Embedding-0.6B-ONNX/resolve/main/tokenizer.json")!
            let tokenizerDest = modelDir.appendingPathComponent("tokenizer.json")
            try await downloadFile(from: tokenizerURL, to: tokenizerDest, progressWeight: 0.05)

            // Set ORT_DYLIB_PATH immediately so the user doesn't need to restart
            let dylibPath = modelsBase.appendingPathComponent("libonnxruntime.dylib").path
            if FileManager.default.fileExists(atPath: dylibPath) {
                setenv("ORT_DYLIB_PATH", dylibPath, 1)
            }

            await MainActor.run {
                isDownloading = false
                progress = 1.0
            }

            HollowLogger.embedding.info("Model download complete")
        } catch {
            // Clean up partial downloads on failure
            try? FileManager.default.removeItem(at: modelDir)

            await MainActor.run {
                self.isDownloading = false
                self.error = error.localizedDescription
            }

            HollowLogger.embedding.error("Model download failed: \(error)")
            throw error
        }
    }

    /// Download and extract the ONNX Runtime dylib if not already present.
    private func ensureOnnxRuntime(modelsBase: URL) async throws {
        let dylibDest = modelsBase.appendingPathComponent("libonnxruntime.dylib")
        guard !FileManager.default.fileExists(atPath: dylibDest.path) else { return }

        let tgzURL = URL(string: "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/onnxruntime-osx-arm64-1.22.0.tgz")!
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        defer { try? FileManager.default.removeItem(at: tempDir) }

        // Download tgz
        let tgzDest = tempDir.appendingPathComponent("ort.tgz")
        let (tgzTempURL, _) = try await URLSession.shared.download(from: tgzURL)
        try FileManager.default.moveItem(at: tgzTempURL, to: tgzDest)

        // Extract
        let extractDir = tempDir.appendingPathComponent("extracted")
        try FileManager.default.createDirectory(at: extractDir, withIntermediateDirectories: true)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/tar")
        process.arguments = ["xzf", tgzDest.path, "-C", extractDir.path]
        try process.run()
        process.waitUntilExit()

        // Known path: onnxruntime-osx-arm64-1.22.0/lib/libonnxruntime.1.22.0.dylib
        // This is the real dylib. The archive also contains a symlink and a
        // dSYM directory with a same-named debug companion — we must use the
        // exact path to avoid picking up the wrong file.
        let dylibSource = extractDir
            .appendingPathComponent("onnxruntime-osx-arm64-1.22.0/lib/libonnxruntime.1.22.0.dylib")
        guard FileManager.default.fileExists(atPath: dylibSource.path) else {
            throw URLError(.cannotCreateFile, userInfo: [
                NSLocalizedDescriptionKey: "libonnxruntime.1.22.0.dylib not found in archive"
            ])
        }
        try FileManager.default.copyItem(at: dylibSource, to: dylibDest)
        HollowLogger.embedding.info("ONNX Runtime dylib installed")
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

        // Use delegate for progress reporting.
        // DispatchQueue.main.async instead of Task{@MainActor} to ensure
        // each update is dispatched individually (Task can be coalesced).
        let delegate = DownloadDelegate { [weak self] fractionCompleted in
            let totalProgress = progressBase + fractionCompleted * progressWeight
            DispatchQueue.main.async {
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
/// Throttles to ~4 updates/sec to avoid flooding the main thread.
private final class DownloadDelegate: NSObject, URLSessionDownloadDelegate, Sendable {
    let onProgress: @Sendable (Double) -> Void
    private let lastUpdate = OSAllocatedUnfairLock(initialState: CFAbsoluteTimeGetCurrent())

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
        let now = CFAbsoluteTimeGetCurrent()
        let shouldUpdate = lastUpdate.withLock { last -> Bool in
            if now - last >= 0.25 {
                last = now
                return true
            }
            return false
        }
        // Always fire at 100%
        let fraction = Double(totalBytesWritten) / Double(totalBytesExpectedToWrite)
        guard shouldUpdate || fraction >= 1.0 else { return }
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
