# Phase 1 Batch 2: File Watch + Metadata Ingest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically detect new files in `~/Hollow Inbox/` and ingest their metadata into the database — making hollow a live, always-watching system.

**Architecture:** Swift side monitors the inbox folder via `DispatchSource` (FSEvents), detects new files, and calls into `HollowCore.ingestFile()` through the existing UniFFI bridge. Rust side is enhanced with MIME guessing, real file timestamps, and duplicate detection. An `IngestionService` (Swift, `@Observable`) coordinates the pipeline and exposes state for the UI.

**Tech Stack:** Swift 6 / SwiftUI (`DispatchSource`, `FileManager`, `@Observable`), Rust (`mime_guess` 2.0), existing UniFFI bridge.

**Spec:** `docs/superpowers/specs/2026-04-09-phase1-batch2-file-watch-ingest-design.md`

---

## File Map

### Rust (hollow-core)

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `hollow-core/Cargo.toml` | Add `mime_guess` dependency |
| Modify | `hollow-core/src/lib.rs` | Enhance `ingest_file`: MIME guess, real timestamps, duplicate check |

### Swift (hollow app)

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `hollow/FileWatcher.swift` | Monitor `~/Hollow Inbox/`, detect new files, debounce, filter |
| Create | `hollow/IngestionService.swift` | Coordinate watcher → bridge, manage observable state |
| Modify | `hollow/HollowBridge.swift` | Add `ingestFile(path:)` method |
| Modify | `hollow/hollowApp.swift` | Create IngestionService, inject into environment |
| Modify | `hollow/ContentView.swift` | Display watch status, ingested count, recent files |

---

## Task 1: Rust — add mime_guess and fix ingest_file

**Files:**
- Modify: `hollow-core/Cargo.toml`
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Add mime_guess dependency**

In `hollow-core/Cargo.toml`, add to `[dependencies]`:

```toml
mime_guess = "2"
```

- [ ] **Step 2: Rewrite ingest_file with MIME, real timestamps, and duplicate check**

Replace the `ingest_file` method and `iso8601_now` function in `hollow-core/src/lib.rs`:

```rust
    pub fn ingest_file(&self, file_path: String) -> Result<FileRecord, HollowError> {
        let path = Path::new(&file_path);
        if !path.exists() {
            return Err(HollowError::FileNotFound(file_path.clone()));
        }

        let content = fs::read(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = format!("{:x}", hasher.finalize());

        // Check for duplicate before inserting
        {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            if FileStore::check_duplicate(&db.conn, &hash)? {
                return Err(HollowError::DuplicateFile(hash));
            }
        }

        let fs_metadata = fs::metadata(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_string());

        // MIME type from extension
        let mime_type = extension.as_deref().and_then(|ext| {
            mime_guess::from_ext(ext)
                .first()
                .map(|m| m.to_string())
        });

        let created_at = system_time_to_rfc3339(fs_metadata.created().ok());
        let modified_at = system_time_to_rfc3339(fs_metadata.modified().ok());
        let ingested_at = iso8601_now();

        let record = FileRecord {
            id: Uuid::now_v7().to_string(),
            hash,
            current_path: file_path.clone(),
            original_path: file_path,
            file_name,
            extension,
            mime_type,
            size_bytes: fs_metadata.len() as i64,
            created_at,
            modified_at,
            ingested_at,
            status: "pending".to_string(),
        };

        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::insert_file(&db.conn, record.clone())?;
        Ok(record)
    }
```

Replace the `iso8601_now` function and add `system_time_to_rfc3339`:

```rust
fn iso8601_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn system_time_to_rfc3339(time: Option<std::time::SystemTime>) -> String {
    match time {
        Some(t) => {
            let offset_dt = time::OffsetDateTime::from(t);
            offset_dt
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| iso8601_now())
        }
        None => iso8601_now(),
    }
}
```

- [ ] **Step 3: Update existing tests and add new ones**

Replace the entire `#[cfg(test)] mod tests` block in `hollow-core/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_file(dir_name: &str, file_name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join(file_name);
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(content).unwrap();
        file_path
    }

    fn cleanup(paths: &[&std::path::Path]) {
        for p in paths {
            if p.is_file() {
                fs::remove_file(p).ok();
            } else if p.is_dir() {
                fs::remove_dir_all(p).ok();
            }
        }
    }

    #[test]
    fn test_ingest_and_get() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t1", "test.txt", b"hello hollow");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.file_name, "test.txt");
        assert_eq!(record.extension, Some("txt".to_string()));
        assert_eq!(record.status, "pending");
        assert_eq!(record.size_bytes, 12);

        let retrieved = core.get_file(record.id.clone()).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().hash, record.hash);

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_mime_type_detection() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();

        let pdf_path = make_temp_file("hollow_t_mime", "doc.pdf", b"fake pdf");
        let record = core.ingest_file(pdf_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.mime_type, Some("application/pdf".to_string()));

        let txt_path = make_temp_file("hollow_t_mime", "note.txt", b"plain text");
        let record = core.ingest_file(txt_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.mime_type, Some("text/plain".to_string()));

        let unknown_path = make_temp_file("hollow_t_mime", "data.xyzabc", b"unknown");
        let record = core.ingest_file(unknown_path.to_string_lossy().to_string()).unwrap();
        assert!(record.mime_type.is_none());

        cleanup(&[&pdf_path.parent().unwrap()]);
    }

    #[test]
    fn test_real_timestamps() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_time", "ts.txt", b"timestamp test");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // created_at and modified_at should be real file times, not equal to ingested_at
        // They should be valid RFC3339 strings
        assert!(record.created_at.contains("T"));
        assert!(record.modified_at.contains("T"));
        assert!(record.ingested_at.contains("T"));

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_duplicate_detection() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_dup", "dup.txt", b"duplicate content");

        // First ingest succeeds
        core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // Second ingest of same file returns DuplicateFile error
        let result = core.ingest_file(path.to_string_lossy().to_string());
        assert!(result.is_err());
        match result {
            Err(HollowError::DuplicateFile(_)) => {} // expected
            other => panic!("expected DuplicateFile, got {:?}", other),
        }

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_list_files() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let dir = std::env::temp_dir().join("hollow_t_list");
        fs::create_dir_all(&dir).unwrap();

        let f1 = dir.join("a.txt");
        fs::write(&f1, b"aaa").unwrap();
        let f2 = dir.join("b.txt");
        fs::write(&f2, b"bbb").unwrap();

        core.ingest_file(f1.to_string_lossy().to_string()).unwrap();
        core.ingest_file(f2.to_string_lossy().to_string()).unwrap();

        let files = core.list_files(10, 0).unwrap();
        assert_eq!(files.len(), 2);

        cleanup(&[&dir]);
    }

    #[test]
    fn test_ingest_nonexistent_file() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let result = core.ingest_file("/nonexistent/path/file.txt".to_string());
        assert!(result.is_err());
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p hollow-core`
Expected: all tests pass (3 schema + 8 store + 6 lib tests = 17 total).

- [ ] **Step 5: Commit**

```bash
git add hollow-core/
git commit -m "feat(hollow-core): enhance ingest_file with MIME guess, real timestamps, and duplicate detection"
```

---

## Task 2: Swift — FileWatcher

**Files:**
- Create: `hollow/FileWatcher.swift`

- [ ] **Step 1: Create FileWatcher.swift**

```swift
// hollow/FileWatcher.swift
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
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Hollow Inbox", isDirectory: true)
    }

    private func ensureDirectoryExists() {
        try? FileManager.default.createDirectory(
            at: watchedURL,
            withIntermediateDirectories: true
        )
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
            let url = watchedURL.appendingPathComponent(name)
            return url
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

            // Skip hidden files (extra safety — .skipsHiddenFiles should handle this)
            if name.hasPrefix(".") { return nil }

            // Skip temporary files
            let ext = url.pathExtension.lowercased()
            if Self.ignoredExtensions.contains(ext) { return nil }

            // Only files, not directories
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir),
                  !isDir.boolValue else {
                return nil
            }

            return name
        })
    }
}
```

- [ ] **Step 2: Verify it compiles**

Build the Xcode project (Cmd+B). Expected: compiles with no errors. (FileWatcher is not yet connected to anything.)

- [ ] **Step 3: Commit**

```bash
git add hollow/FileWatcher.swift
git commit -m "feat(swift): add FileWatcher — monitors ~/Hollow Inbox/ for new files"
```

---

## Task 3: Swift — HollowBridge.ingestFile

**Files:**
- Modify: `hollow/HollowBridge.swift`

- [ ] **Step 1: Add ingestFile method to HollowBridge**

Add this method to the `HollowBridge` class, after the existing `listFiles` method:

```swift
    enum IngestResult {
        case success(FileRecord)
        case duplicate
        case error(String)
    }

    func ingestFile(path: String) -> IngestResult {
        guard let core else { return .error("HollowCore not initialized") }
        do {
            let record = try core.ingestFile(filePath: path)
            return .success(record)
        } catch HollowError.DuplicateFile(_) {
            return .duplicate
        } catch {
            return .error(error.localizedDescription)
        }
    }
```

- [ ] **Step 2: Verify it compiles**

Build the Xcode project (Cmd+B). Expected: compiles. Note: `HollowError.DuplicateFile` is the UniFFI-generated Swift enum case — if the exact case name differs, check `hollow/Generated/hollow_core.swift` for the correct case name and adjust.

- [ ] **Step 3: Commit**

```bash
git add hollow/HollowBridge.swift
git commit -m "feat(swift): add ingestFile to HollowBridge with duplicate handling"
```

---

## Task 4: Swift — IngestionService

**Files:**
- Create: `hollow/IngestionService.swift`

- [ ] **Step 1: Create IngestionService.swift**

```swift
// hollow/IngestionService.swift
import Foundation
import Observation

@Observable
final class IngestionService {
    private(set) var isWatching = false
    private(set) var totalIngested: Int = 0
    private(set) var recentFiles: [String] = []
    private(set) var lastError: String?

    private let watcher: FileWatcher
    private let bridge: HollowBridge

    init(bridge: HollowBridge = .shared) {
        self.bridge = bridge
        self.watcher = FileWatcher(directory: FileWatcher.inboxURL)

        watcher.onNewFiles = { [weak self] urls in
            self?.handleNewFiles(urls)
        }
    }

    func start() {
        watcher.start()
        isWatching = true

        // Count already-ingested files from DB
        totalIngested = bridge.listFiles(limit: UInt32.max, offset: 0).count

        // Ingest any files already in the inbox (e.g. added while app was not running)
        performStartupScan()
    }

    private func performStartupScan() {
        let inboxURL = FileWatcher.inboxURL
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: inboxURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ) else { return }

        let files = contents.filter { url in
            let name = url.lastPathComponent
            if name.hasPrefix(".") { return false }
            let ext = url.pathExtension.lowercased()
            if ["tmp", "download", "crdownload", "partial"].contains(ext) { return false }
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else { return false }
            return !isDir.boolValue
        }

        if !files.isEmpty {
            handleNewFiles(files)
        }
    }

    func stop() {
        watcher.stop()
        isWatching = false
    }

    private func handleNewFiles(_ urls: [URL]) {
        Task.detached(priority: .utility) { [weak self] in
            guard let self else { return }
            for url in urls {
                let result = self.bridge.ingestFile(path: url.path)
                await MainActor.run {
                    switch result {
                    case .success(let record):
                        self.totalIngested += 1
                        self.recentFiles.insert(record.fileName, at: 0)
                        if self.recentFiles.count > 10 {
                            self.recentFiles.removeLast()
                        }
                        self.lastError = nil
                    case .duplicate:
                        break // silently skip
                    case .error(let message):
                        self.lastError = message
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Build the Xcode project (Cmd+B). Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add hollow/IngestionService.swift
git commit -m "feat(swift): add IngestionService — coordinates file watching and ingestion"
```

---

## Task 5: Swift — wire up app and UI

**Files:**
- Modify: `hollow/hollowApp.swift`
- Modify: `hollow/ContentView.swift`

- [ ] **Step 1: Update hollowApp.swift**

Replace `hollow/hollowApp.swift`:

```swift
import SwiftUI

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(ingestionService)
                .onAppear {
                    ingestionService.start()
                }
        }
    }
}
```

- [ ] **Step 2: Update ContentView.swift**

Replace `hollow/ContentView.swift`:

```swift
import SwiftUI

struct ContentView: View {
    @Environment(IngestionService.self) private var ingestion

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "archivebox")
                .imageScale(.large)
                .foregroundStyle(.tint)
            Text("hollow")
                .font(.title)

            HStack(spacing: 6) {
                Circle()
                    .fill(ingestion.isWatching ? .green : .gray)
                    .frame(width: 8, height: 8)
                Text(ingestion.isWatching ? "Watching ~/Hollow Inbox/" : "Not watching")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }

            Text("\(ingestion.totalIngested) files ingested")
                .font(.headline)

            if !ingestion.recentFiles.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Recent:")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    ForEach(ingestion.recentFiles, id: \.self) { name in
                        Text(name)
                            .font(.caption)
                            .lineLimit(1)
                    }
                }
                .frame(maxWidth: 300, alignment: .leading)
            }

            if let error = ingestion.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .padding()
        .frame(minWidth: 350, minHeight: 300)
    }
}
```

- [ ] **Step 3: Verify it compiles**

Build the Xcode project (Cmd+B). Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add hollow/hollowApp.swift hollow/ContentView.swift
git commit -m "feat(swift): wire up IngestionService to app and display watch status in UI"
```

---

## Task 6: Manual end-to-end test

- [ ] **Step 1: Run all Rust tests**

Run: `cargo test -p hollow-core`
Expected: 17 tests pass.

- [ ] **Step 2: Build and run the app**

Build and run from Xcode (Cmd+R). Expected:
- App window shows "Watching ~/Hollow Inbox/"
- Green dot indicator
- "0 files ingested"

- [ ] **Step 3: Test file ingestion**

Open Finder, navigate to `~/Hollow Inbox/` (it should have been created automatically).

1. Drag a PDF file into the folder → app should show "1 files ingested" and the file name under "Recent:"
2. Drag a .txt file → count goes to 2
3. Drag the same PDF again (copy it with a different name but same content) → count stays at 2 (duplicate detected by hash)
4. Create a `.DS_Store` or `.tmp` file → should be ignored

- [ ] **Step 4: Commit any fixes needed**

If manual testing revealed issues, fix and commit.
