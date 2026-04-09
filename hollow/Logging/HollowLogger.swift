import os

enum HollowLogger {
    static let fileWatcher = Logger(subsystem: "com.syncpulse.hollow", category: "FileWatcher")
    static let ingestion   = Logger(subsystem: "com.syncpulse.hollow", category: "Ingestion")
    static let bridge      = Logger(subsystem: "com.syncpulse.hollow", category: "Bridge")
    static let app         = Logger(subsystem: "com.syncpulse.hollow", category: "App")
    static let rustCore    = Logger(subsystem: "com.syncpulse.hollow", category: "RustCore")
}
