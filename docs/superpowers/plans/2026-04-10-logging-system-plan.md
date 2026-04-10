# Logging System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 hollow 建立统一的开发调试日志系统，打通 Rust core 和 Swift 前端，基于 os.Logger + tracing。

**Architecture:** Rust 侧用 tracing + 内存 ring buffer，通过 FFI 暴露 get_logs。Swift 侧用 os.Logger 作为唯一日志后端，RustLogRelay 定时拉取 Rust 日志转发到 os.Logger。Debug Log Viewer 通过 OSLogStore 读取所有日志展示。

**Tech Stack:** Rust tracing, Swift os.Logger, OSLogStore, UniFFI, SwiftUI

---

### Task 1: Rust logging infrastructure — ring buffer + tracing Layer

**Files:**
- Create: `hollow-core/src/logging.rs`
- Modify: `hollow-core/src/lib.rs:1-20` (add mod, init, FFI exports)
- Modify: `hollow-core/Cargo.toml` (add tracing dependency)

- [ ] **Step 1: Add tracing dependency to Cargo.toml**

Add `tracing` to `[dependencies]` in `hollow-core/Cargo.toml`:

```toml
tracing = "0.1"
```

- [ ] **Step 2: Create `hollow-core/src/logging.rs` with LogEntry, LogLevel, and ring buffer**

```rust
use std::collections::VecDeque;
use std::sync::{atomic::{AtomicU64, Ordering}, Mutex, OnceLock};

use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const MAX_LOG_ENTRIES: usize = 5000;

static LOG_BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(uniffi::Record, Clone)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

#[derive(uniffi::Enum, Clone, PartialEq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl From<&Level> for LogLevel {
    fn from(level: &Level) -> Self {
        match *level {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            Level::INFO => LogLevel::Info,
            _ => LogLevel::Debug,
        }
    }
}

fn buffer() -> &'static Mutex<VecDeque<LogEntry>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)))
}

fn push_entry(entry: LogEntry) {
    let buf = buffer();
    if let Ok(mut ring) = buf.lock() {
        if ring.len() >= MAX_LOG_ENTRIES {
            ring.pop_front();
        }
        ring.push_back(entry);
    }
}

pub fn get_logs_since(since_id: u64) -> Vec<LogEntry> {
    let buf = buffer();
    let Ok(ring) = buf.lock() else { return vec![] };
    ring.iter()
        .filter(|e| e.id > since_id)
        .cloned()
        .collect()
}

pub fn clear_log_buffer() {
    let buf = buffer();
    if let Ok(mut ring) = buf.lock() {
        ring.clear();
    }
}

// --- tracing Layer ---

struct HollowLogLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for HollowLogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);

        let meta = event.metadata();
        let entry = LogEntry {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            timestamp: crate::iso8601_now(),
            level: LogLevel::from(meta.level()),
            target: meta.target().to_string(),
            message: visitor.0,
        };

        push_entry(entry);
    }
}

struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value);
        } else if !self.0.is_empty() {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        } else {
            self.0 = format!("{}={:?}", field.name(), value);
        }
    }
}

/// Initialize tracing with the ring buffer layer. Safe to call multiple times — only first call takes effect.
pub fn init_logging() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::registry()
            .with(HollowLogLayer)
            .init();
    });
}
```

- [ ] **Step 3: Wire logging into lib.rs — add mod, init in constructor, FFI exports**

In `hollow-core/src/lib.rs`, add:

1. Add `mod logging;` at the top with other mods.
2. Add `pub use logging::{LogEntry, LogLevel};` with other pub uses.
3. In `HollowCore::new()`, call `logging::init_logging();` before opening the DB.
4. Add two new FFI methods to the `HollowCore` impl block:

```rust
pub fn get_logs(&self, since_id: u64) -> Vec<LogEntry> {
    logging::get_logs_since(since_id)
}

pub fn clear_logs(&self) {
    logging::clear_log_buffer();
}
```

- [ ] **Step 4: Add `tracing-subscriber` dependency**

Also need `tracing-subscriber` in `Cargo.toml`:

```toml
tracing-subscriber = "0.3"
```

- [ ] **Step 5: Add tracing instrumentation to existing Rust functions**

In `hollow-core/src/lib.rs`, add `use tracing::{info, warn, error, debug};` and instrument key operations:

- `ingest_file`: `info!("Ingested file: {} ({} bytes)", file_name, fs_metadata.len())` on success, `debug!("Duplicate skipped: {}", file_path)` on DuplicateFile
- `compute_hash`: `debug!("Computing full hash for {}", file_id)` at start, `info!("Hash computed for {}: {}", file_id, &hash[..8])` on completion
- `mark_missing`: `info!("Marked missing: {}", path)`
- `new`: `info!("HollowCore initialized, db: {}", db_path)`

- [ ] **Step 6: Run Rust tests**

Run: `cargo test -p hollow-core`
Expected: All existing tests PASS. Logging init runs silently in test context.

- [ ] **Step 7: Commit**

```bash
git add hollow-core/src/logging.rs hollow-core/src/lib.rs hollow-core/Cargo.toml
git commit -m "feat(core): add tracing-based logging with ring buffer and FFI export"
```

---

### Task 2: Regenerate UniFFI Swift bindings

**Files:**
- Modify: `hollow/Generated/hollow_core.swift` (regenerated)
- Modify: `hollow/Generated/hollow_coreFFI.h` (regenerated)

- [ ] **Step 1: Temporarily add cdylib to crate-type for binding generation**

In `hollow-core/Cargo.toml`, temporarily change:
```toml
crate-type = ["staticlib", "lib", "cdylib"]
```

- [ ] **Step 2: Build and generate bindings**

```bash
cd /Users/dnf/Documents/hollow
cargo build -p hollow-core
cargo run -p hollow-core --bin uniffi-bindgen generate --library target/debug/libhollow_core.dylib --language swift --out-dir hollow/Generated
```

- [ ] **Step 3: Revert cdylib from crate-type**

Change back to:
```toml
crate-type = ["staticlib", "lib"]
```

- [ ] **Step 4: Rebuild static library for arm64**

```bash
cargo build -p hollow-core --target aarch64-apple-darwin
```

- [ ] **Step 5: Verify new methods appear in generated Swift**

Check `hollow/Generated/hollow_core.swift` contains `getLogs(sinceId:)` and `clearLogs()` methods, and `LogEntry` / `LogLevel` types.

- [ ] **Step 6: Commit**

```bash
git add hollow/Generated/ hollow-core/Cargo.toml
git commit -m "chore: regenerate UniFFI bindings with logging types"
```

---

### Task 3: Swift HollowLogger + HollowBridge updates

**Files:**
- Create: `hollow/Logging/HollowLogger.swift`
- Modify: `hollow/HollowBridge.swift` (add getLogs/clearLogs wrappers, replace print)

- [ ] **Step 1: Create `hollow/Logging/` group and `HollowLogger.swift`**

```swift
import os

enum HollowLogger {
    static let fileWatcher = Logger(subsystem: "com.syncpulse.hollow", category: "FileWatcher")
    static let ingestion   = Logger(subsystem: "com.syncpulse.hollow", category: "Ingestion")
    static let bridge      = Logger(subsystem: "com.syncpulse.hollow", category: "Bridge")
    static let app         = Logger(subsystem: "com.syncpulse.hollow", category: "App")
    static let rustCore    = Logger(subsystem: "com.syncpulse.hollow", category: "RustCore")
}
```

Note: After creating the file on disk, add it to the Xcode project via `hollow.xcodeproj`. If using Xcode's file navigator, drag the file in. If the project uses automatic file discovery, just ensure the file is within the `hollow/` target directory.

- [ ] **Step 2: Add getLogs and clearLogs to HollowBridge**

Add these methods to `HollowBridge`:

```swift
func getLogs(sinceId: UInt64) -> [LogEntry] {
    guard let core else { return [] }
    return core.getLogs(sinceId: sinceId)
}

func clearLogs() {
    guard let core else { return }
    core.clearLogs()
}
```

- [ ] **Step 3: Replace all print() in HollowBridge with HollowLogger**

Replace:
- `print("HollowBridge init failed: \(error)")` → `HollowLogger.bridge.error("HollowBridge init failed: \(error)")`
- `print("listFiles failed: \(error)")` → `HollowLogger.bridge.error("listFiles failed: \(error)")`

- [ ] **Step 4: Replace all print() in FileWatcher with HollowLogger**

Replace:
- `print("FileWatcher: inbox ready at \(watchedURL.path)")` → `HollowLogger.fileWatcher.info("Inbox ready at \(watchedURL.path)")`
- `print("FileWatcher: failed to create inbox directory: \(error)")` → `HollowLogger.fileWatcher.error("Failed to create inbox directory: \(error)")`
- `print("FileWatcher: failed to create FSEventStream")` → `HollowLogger.fileWatcher.error("Failed to create FSEventStream")`

- [ ] **Step 5: Add logging to IngestionService**

Add key log points in `IngestionService.swift`:
- `start()`: `HollowLogger.ingestion.info("Ingestion service started")`
- `stop()`: `HollowLogger.ingestion.info("Ingestion service stopped")`
- `intakeFiles` success: `HollowLogger.ingestion.info("Ingested: \(record.fileName)")`
- `intakeFiles` duplicate: `HollowLogger.ingestion.debug("Duplicate skipped: \(url.lastPathComponent)")`
- `intakeFiles` error: `HollowLogger.ingestion.error("Ingest error: \(message)")`

- [ ] **Step 6: Build and verify**

Build the Xcode project:
```bash
xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build
```

Expected: Build succeeds. No remaining `print()` calls in HollowBridge or FileWatcher.

- [ ] **Step 7: Commit**

```bash
git add hollow/Logging/HollowLogger.swift hollow/HollowBridge.swift hollow/FileWatcher.swift hollow/IngestionService.swift
git commit -m "feat(swift): add HollowLogger, replace print() with os.Logger"
```

---

### Task 4: RustLogRelay — pull Rust logs into os.Logger

**Files:**
- Create: `hollow/Logging/RustLogRelay.swift`
- Modify: `hollow/hollowApp.swift` (start relay on launch)

- [ ] **Step 1: Create `hollow/Logging/RustLogRelay.swift`**

```swift
import Foundation
import os

final class RustLogRelay {
    static let shared = RustLogRelay()

    private var timer: Timer?
    private var lastSeenId: UInt64 = 0
    private let logger = HollowLogger.rustCore

    private init() {}

    func start() {
        // Pull once immediately to get any logs from init
        pullLogs()

        // Then poll every 500ms
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            self?.pullLogs()
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    private func pullLogs() {
        let entries = HollowBridge.shared.getLogs(sinceId: lastSeenId)
        for entry in entries {
            lastSeenId = entry.id
            switch entry.level {
            case .debug:
                logger.debug("[\(entry.target)] \(entry.message)")
            case .info:
                logger.info("[\(entry.target)] \(entry.message)")
            case .warn:
                logger.warning("[\(entry.target)] \(entry.message)")
            case .error:
                logger.error("[\(entry.target)] \(entry.message)")
            }
        }
    }
}
```

- [ ] **Step 2: Start RustLogRelay in hollowApp.swift**

In the `.onAppear` block of `hollowApp`, after `ingestionService.start()`, add:

```swift
RustLogRelay.shared.start()
```

Also add a log at app startup:
```swift
HollowLogger.app.info("hollow app launched")
```

- [ ] **Step 3: Build and verify**

```bash
xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build
```

Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add hollow/Logging/RustLogRelay.swift hollow/hollowApp.swift
git commit -m "feat(swift): add RustLogRelay to forward Rust logs into os.Logger"
```

---

### Task 5: Debug Log Viewer UI

**Files:**
- Create: `hollow/Views/LogViewerView.swift`
- Modify: `hollow/hollowApp.swift` (add window + menu item)

- [ ] **Step 1: Create `hollow/Views/LogViewerView.swift`**

```swift
import SwiftUI
import OSLog

struct LogViewerView: View {
    @State private var entries: [OSLogEntryLog] = []
    @State private var selectedTab = 0       // 0=Swift, 1=Rust
    @State private var filterLevel: OSLogEntryLog.Level? = nil
    @State private var searchText = ""
    @State private var autoRefresh = true
    @State private var refreshTimer: Timer?

    private let subsystem = "com.syncpulse.hollow"
    private let rustCategory = "RustCore"

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Picker("", selection: $selectedTab) {
                    Text("Swift").tag(0)
                    Text("Rust").tag(1)
                }
                .pickerStyle(.segmented)
                .frame(width: 160)

                Picker("Level", selection: $filterLevel) {
                    Text("All").tag(nil as OSLogEntryLog.Level?)
                    Text("Debug").tag(OSLogEntryLog.Level.debug as OSLogEntryLog.Level?)
                    Text("Info").tag(OSLogEntryLog.Level.info as OSLogEntryLog.Level?)
                    Text("Warning").tag(OSLogEntryLog.Level.notice as OSLogEntryLog.Level?)
                    Text("Error").tag(OSLogEntryLog.Level.error as OSLogEntryLog.Level?)
                }
                .frame(width: 120)

                TextField("Search...", text: $searchText)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 200)

                Spacer()

                Toggle("Auto-refresh", isOn: $autoRefresh)
                    .toggleStyle(.switch)

                Button(action: refresh) {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh")

                Text("\(filteredEntries.count) entries")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }
            .padding(8)

            Divider()

            // Log list
            ScrollViewReader { proxy in
                List(filteredEntries, id: \.self) { entry in
                    HStack(alignment: .top, spacing: 8) {
                        Text(formatTime(entry.date))
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)

                        levelBadge(entry.level)
                            .frame(width: 50)

                        Text(entry.category)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)

                        Text(entry.composedMessage)
                            .font(.system(.caption, design: .monospaced))
                            .lineLimit(nil)
                            .textSelection(.enabled)
                    }
                    .id(entry)
                }
                .onChange(of: filteredEntries.count) {
                    if autoRefresh, let last = filteredEntries.last {
                        proxy.scrollTo(last, anchor: .bottom)
                    }
                }
            }
        }
        .frame(minWidth: 700, minHeight: 400)
        .onAppear {
            refresh()
            startAutoRefresh()
        }
        .onDisappear {
            stopAutoRefresh()
        }
        .onChange(of: autoRefresh) {
            if autoRefresh { startAutoRefresh() } else { stopAutoRefresh() }
        }
    }

    private var filteredEntries: [OSLogEntryLog] {
        var result = entries

        // Tab filter: Swift (non-RustCore) vs Rust (RustCore only)
        if selectedTab == 0 {
            result = result.filter { $0.category != rustCategory }
        } else {
            result = result.filter { $0.category == rustCategory }
        }

        // Level filter
        if let level = filterLevel {
            result = result.filter { $0.level == level }
        }

        // Search filter
        if !searchText.isEmpty {
            let query = searchText.lowercased()
            result = result.filter {
                $0.composedMessage.lowercased().contains(query) ||
                $0.category.lowercased().contains(query)
            }
        }

        return result
    }

    private func refresh() {
        do {
            let store = try OSLogStore(scope: .currentProcessIdentifier)
            let position = store.position(date: Date().addingTimeInterval(-3600)) // last hour
            let predicate = NSPredicate(format: "subsystem == %@", subsystem)
            let rawEntries = try store.getEntries(at: position, matching: predicate)
            entries = rawEntries.compactMap { $0 as? OSLogEntryLog }
        } catch {
            HollowLogger.app.error("Failed to read OSLogStore: \(error)")
        }
    }

    private func startAutoRefresh() {
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { _ in
            refresh()
        }
    }

    private func stopAutoRefresh() {
        refreshTimer?.invalidate()
        refreshTimer = nil
    }

    private func formatTime(_ date: Date) -> String {
        let fmt = DateFormatter()
        fmt.dateFormat = "HH:mm:ss"
        return fmt.string(from: date)
    }

    private func levelBadge(_ level: OSLogEntryLog.Level) -> some View {
        let (text, color): (String, Color) = switch level {
        case .debug: ("DEBUG", .gray)
        case .info: ("INFO", .blue)
        case .notice: ("WARN", .orange)
        case .error: ("ERROR", .red)
        case .fault: ("FAULT", .red)
        default: ("OTHER", .gray)
        }
        return Text(text)
            .font(.system(.caption2, design: .monospaced, weight: .bold))
            .foregroundStyle(color)
    }
}
```

- [ ] **Step 2: Add Log Viewer window and menu item in hollowApp.swift**

Add a new `Window` scene:
```swift
Window("Log Viewer", id: "log-viewer") {
    LogViewerView()
}
```

Add menu item inside the existing `if debugMode` block:
```swift
Button("Log Viewer") {
    openWindow(id: "log-viewer")
}
.keyboardShortcut("L", modifiers: [.command, .shift])
```

- [ ] **Step 3: Build and verify**

```bash
xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build
```

Expected: Build succeeds.

- [ ] **Step 4: Manual test**

1. Launch app
2. Enable Debug Mode in Settings
3. Cmd+Shift+L opens Log Viewer
4. Swift tab shows app/ingestion/filewatcher logs
5. Rust tab shows forwarded Rust core logs
6. Filters work (level, search)

- [ ] **Step 5: Commit**

```bash
git add hollow/Views/LogViewerView.swift hollow/hollowApp.swift
git commit -m "feat(ui): add Debug Log Viewer with os.Logger + OSLogStore"
```

---

### Task 6: Verify everything works end-to-end

- [ ] **Step 1: Run Rust tests**

```bash
cargo test -p hollow-core
```

Expected: All tests pass.

- [ ] **Step 2: Build full Xcode project**

```bash
xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build
```

Expected: Build succeeds with no warnings related to logging.

- [ ] **Step 3: Verify no remaining print() calls**

Search for stale `print(` in Swift files (excluding Generated/):
```bash
grep -r 'print(' hollow/ --include='*.swift' | grep -v 'Generated/' | grep -v '// print'
```

Expected: No results (all print() replaced with HollowLogger).

- [ ] **Step 4: Final commit if any cleanup needed**

```bash
git add -A
git commit -m "chore: logging system cleanup and verification"
```
