import os

// HollowLogger intentionally has no actor isolation — Logger is Sendable and safe to use
// from any context (background threads, Task.detached, nonisolated code, etc.)
nonisolated enum HollowLogger {
    nonisolated static let fileWatcher = Logger(subsystem: "com.syncpulse.hollow", category: "FileWatcher")
    nonisolated static let ingestion   = Logger(subsystem: "com.syncpulse.hollow", category: "Ingestion")
    nonisolated static let bridge      = Logger(subsystem: "com.syncpulse.hollow", category: "Bridge")
    nonisolated static let app         = Logger(subsystem: "com.syncpulse.hollow", category: "App")
    nonisolated static let rustCore    = Logger(subsystem: "com.syncpulse.hollow", category: "RustCore")
    nonisolated static let ocr         = Logger(subsystem: "com.syncpulse.hollow", category: "OCR")
    nonisolated static let search      = Logger(subsystem: "com.syncpulse.hollow", category: "Search")
    nonisolated static let embedding   = Logger(subsystem: "com.syncpulse.hollow", category: "Embedding")
}
