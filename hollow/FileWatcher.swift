import Foundation
import CoreServices

final class FileWatcher {
    private let watchedURL: URL
    private var stream: FSEventStreamRef?

    var onNewFiles: (([URL]) -> Void)?
    var onRemovedFiles: (([URL]) -> Void)?

    static let ignoredExtensions: Set<String> = [
        "tmp", "download", "crdownload", "partial"
    ]

    init(directory: URL) {
        self.watchedURL = directory
    }

    deinit {
        stop()
    }

    func start() {
        ensureDirectoryExists()
        startStream()
    }

    func stop() {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
        self.stream = nil
    }

    static var inboxURL: URL {
        let pw = getpwuid(getuid())!
        let realHome = String(cString: pw.pointee.pw_dir)
        return URL(fileURLWithPath: realHome)
            .appendingPathComponent("Hollow Inbox", isDirectory: true)
    }

    private func ensureDirectoryExists() {
        do {
            try FileManager.default.createDirectory(
                at: watchedURL,
                withIntermediateDirectories: true
            )
            print("FileWatcher: inbox ready at \(watchedURL.path)")
        } catch {
            print("FileWatcher: failed to create inbox directory: \(error)")
        }
    }

    private func startStream() {
        let pathToWatch = watchedURL.path as CFString
        let pathsToWatch = [pathToWatch] as CFArray

        // Bridge self into the C callback via UnsafeMutableRawPointer
        var context = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(),
            retain: nil,
            release: nil,
            copyDescription: nil
        )

        let flags: FSEventStreamCreateFlags =
            UInt32(kFSEventStreamCreateFlagFileEvents) |
            UInt32(kFSEventStreamCreateFlagUseCFTypes) |
            UInt32(kFSEventStreamCreateFlagNoDefer)

        guard let stream = FSEventStreamCreate(
            nil,                    // allocator
            fsEventCallback,        // callback
            &context,               // context
            pathsToWatch,           // paths
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            0.3,                    // latency (seconds) — coalesces rapid events
            flags
        ) else {
            print("FileWatcher: failed to create FSEventStream")
            return
        }

        self.stream = stream
        FSEventStreamSetDispatchQueue(stream, DispatchQueue.global(qos: .utility))
        FSEventStreamStart(stream)
    }

    // MARK: - FSEvents callback

    private let fsEventCallback: FSEventStreamCallback = {
        (streamRef, clientCallBackInfo, numEvents, eventPaths, eventFlags, eventIds) in

        guard let info = clientCallBackInfo else { return }
        let watcher = Unmanaged<FileWatcher>.fromOpaque(info).takeUnretainedValue()

        guard let paths = unsafeBitCast(eventPaths, to: NSArray.self) as? [String] else { return }
        let flags = UnsafeBufferPointer(start: eventFlags, count: numEvents)

        var newURLs: [URL] = []
        var removedURLs: [URL] = []

        for i in 0..<numEvents {
            let path = paths[i]
            let flag = flags[i]

            // Skip directories themselves — we only care about files
            if flag & UInt32(kFSEventStreamEventFlagItemIsDir) != 0 { continue }

            // Skip if not a file event
            if flag & UInt32(kFSEventStreamEventFlagItemIsFile) == 0 { continue }

            let url = URL(fileURLWithPath: path)

            // Filter out hidden and temp files
            if url.lastPathComponent.hasPrefix(".") { continue }
            let ext = url.pathExtension.lowercased()
            if FileWatcher.ignoredExtensions.contains(ext) { continue }

            if flag & UInt32(kFSEventStreamEventFlagItemRemoved) != 0 {
                removedURLs.append(url)
            } else if flag & UInt32(kFSEventStreamEventFlagItemCreated) != 0 ||
                      flag & UInt32(kFSEventStreamEventFlagItemRenamed) != 0 {
                // Renamed can mean "moved into this directory" — treat as new if file exists
                if FileManager.default.fileExists(atPath: path) {
                    newURLs.append(url)
                }
            }

            // MustScanSubDirs: kernel dropped events, fall back to full scan
            if flag & UInt32(kFSEventStreamEventFlagMustScanSubDirs) != 0 {
                watcher.fallbackFullScan()
                return
            }
        }

        if !removedURLs.isEmpty {
            watcher.onRemovedFiles?(removedURLs)
        }
        if !newURLs.isEmpty {
            watcher.onNewFiles?(newURLs)
        }
    }

    /// Fallback: when FSEvents says it dropped events, do a full diff scan.
    private func fallbackFullScan() {
        let currentFiles = scanAllFiles()
        let currentPaths = Set(currentFiles.map(\.path))

        // We don't have a previous snapshot in the new architecture,
        // so just report all current files as potentially new.
        // The ingestion layer's inode/path dedup will handle duplicates.
        if !currentFiles.isEmpty {
            onNewFiles?(currentFiles)
        }
    }

    /// Recursive scan of all files under watchedURL.
    func scanAllFiles() -> [URL] {
        guard let enumerator = FileManager.default.enumerator(
            at: watchedURL,
            includingPropertiesForKeys: [.isRegularFileKey],
            options: [.skipsHiddenFiles]
        ) else { return [] }

        var result: [URL] = []
        for case let url as URL in enumerator {
            let name = url.lastPathComponent
            if name.hasPrefix(".") {
                enumerator.skipDescendants()
                continue
            }
            let ext = url.pathExtension.lowercased()
            if Self.ignoredExtensions.contains(ext) { continue }
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir),
                  !isDir.boolValue else { continue }
            result.append(url)
        }
        return result
    }
}
