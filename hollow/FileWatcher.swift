import Foundation
import CoreServices
import os

final class FileWatcher {
    private let watchedURL: URL
    private var stream: FSEventStreamRef?

    var onNewFiles: (([URL]) -> Void)?
    var onRemovedFiles: (([URL]) -> Void)?
    var onModifiedFiles: (([URL]) -> Void)?

    private var modifyDebounce: [String: DispatchWorkItem] = [:]
    private let modifyDebounceQueue = DispatchQueue(label: "com.syncpulse.hollow.modify-debounce")
    private let modifyDebounceDelay: TimeInterval = 0.5

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
            HollowLogger.fileWatcher.info("Inbox ready at \(self.watchedURL.path)")
        } catch {
            HollowLogger.fileWatcher.error("Failed to create inbox directory: \(error)")
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
            HollowLogger.fileWatcher.error("Failed to create FSEventStream")
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

            // Modified existing file — debounce (save events can fire multiple times)
            if flag & UInt32(kFSEventStreamEventFlagItemModified) != 0 {
                // Only treat as modify if the file still exists and wasn't already handled as new/removed
                let alreadyHandledAsNew = newURLs.contains(url)
                let alreadyHandledAsRemoved = removedURLs.contains(url)
                if !alreadyHandledAsNew && !alreadyHandledAsRemoved &&
                   FileManager.default.fileExists(atPath: path) {
                    watcher.scheduleModify(url)
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

    private func scheduleModify(_ url: URL) {
        let key = url.path
        modifyDebounceQueue.async { [weak self] in
            guard let self else { return }
            self.modifyDebounce[key]?.cancel()
            let work = DispatchWorkItem { [weak self] in
                guard let self else { return }
                self.modifyDebounceQueue.async {
                    self.modifyDebounce.removeValue(forKey: key)
                }
                self.onModifiedFiles?([url])
            }
            self.modifyDebounce[key] = work
            DispatchQueue.global(qos: .utility).asyncAfter(
                deadline: .now() + self.modifyDebounceDelay,
                execute: work
            )
        }
    }

    /// Fallback: when FSEvents says it dropped events, do a full diff scan.
    private func fallbackFullScan() {
        let currentFiles = scanAllFiles()
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
