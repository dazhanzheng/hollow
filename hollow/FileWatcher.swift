import Foundation

final class FileWatcher {
    private let watchedURL: URL
    private var source: DispatchSourceFileSystemObject?
    private var fileDescriptor: Int32 = -1
    private var knownFiles: Set<String> = []
    private var debounceWorkItem: DispatchWorkItem?

    var onNewFiles: (([URL]) -> Void)?

    private static let ignoredExtensions: Set<String> = [
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
        knownFiles = currentFileNames()
        startWatching()
    }

    func stop() {
        debounceWorkItem?.cancel()
        source?.cancel()
        source = nil
        if fileDescriptor >= 0 {
            close(fileDescriptor)
            fileDescriptor = -1
        }
    }

    static var inboxURL: URL {
        // Use real home directory, not sandboxed container path
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

    private func startWatching() {
        fileDescriptor = Darwin.open(watchedURL.path, O_EVTONLY)
        guard fileDescriptor >= 0 else {
            print("FileWatcher: failed to open \(watchedURL.path)")
            return
        }

        source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fileDescriptor,
            eventMask: .write,
            queue: .global(qos: .utility)
        )

        source?.setEventHandler { [weak self] in
            self?.scheduleDebounce()
        }

        source?.setCancelHandler { [weak self] in
            if let fd = self?.fileDescriptor, fd >= 0 {
                close(fd)
                self?.fileDescriptor = -1
            }
        }

        source?.resume()
    }

    private func scheduleDebounce() {
        debounceWorkItem?.cancel()
        let workItem = DispatchWorkItem { [weak self] in
            self?.scanForNewFiles()
        }
        debounceWorkItem = workItem
        DispatchQueue.global(qos: .utility).asyncAfter(
            deadline: .now() + .milliseconds(500),
            execute: workItem
        )
    }

    private func scanForNewFiles() {
        let currentFiles = currentFileNames()
        let newFileNames = currentFiles.subtracting(knownFiles)
        knownFiles = currentFiles

        guard !newFileNames.isEmpty else { return }

        let newURLs = newFileNames.compactMap { name -> URL? in
            watchedURL.appendingPathComponent(name)
        }

        if !newURLs.isEmpty {
            onNewFiles?(newURLs)
        }
    }

    private func currentFileNames() -> Set<String> {
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: watchedURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ) else {
            return []
        }

        return Set(contents.compactMap { url -> String? in
            let name = url.lastPathComponent
            if name.hasPrefix(".") { return nil }
            let ext = url.pathExtension.lowercased()
            if Self.ignoredExtensions.contains(ext) { return nil }
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir),
                  !isDir.boolValue else {
                return nil
            }
            return name
        })
    }
}
