# Logging System Design Spec

## Goal

为 hollow 客户端建立统一的开发调试日志系统，打通 Swift 前端和 Rust hollow-core 的日志输出。基于 Apple 官方 os.Logger 体系，同时提供 app 内 Debug 日志查看窗口。

## Non-Goals

- **不涉及用户操作日志**（operations_log）—— 那是未来独立的追踪系统（C 系统），与本系统无关
- **不涉及磁盘持久化** —— os.Logger 自身有持久化机制，不额外写文件
- **不涉及 hollow-server 的日志** —— server 已有 tracing + TraceLayer，独立体系

---

## Architecture

两条独立的日志管线，通过 os.Logger 统一汇聚：

```
Swift 代码 ──→ os.Logger (subsystem: com.syncpulse.hollow, category: 各模块)
                  │
                  ├──→ Console.app (系统级查看)
                  └──→ OSLogStore ──→ App 内 Debug Log Viewer

Rust 代码 ──→ tracing ──→ 内存 ring buffer (VecDeque<LogEntry>)
                              │
                              └──→ FFI get_logs(since_id) ──→ Swift LogManager
                                                                  │
                                                                  └──→ os.Logger (category: "RustCore")
                                                                          │
                                                                          ├──→ Console.app
                                                                          └──→ OSLogStore ──→ Debug Log Viewer
```

### 核心决策

1. **os.Logger 作为 Swift 侧唯一日志后端** —— 标准、官方、免费获得 Console.app 支持
2. **Rust 日志通过 FFI 拉取后转发到 os.Logger** —— 最终所有日志汇入统一体系
3. **拉取而非回调** —— 避免 UniFFI callback interface 的复杂度和线程安全问题
4. **Swift 和 Rust 日志分开存储、分开展示** —— 模块化，Debug Viewer 中按 tab/category 区分

---

## Components

### 1. Rust: Ring Buffer + FFI 导出

**位置**: `hollow-core/src/logging.rs`

**数据结构**:
```rust
pub struct LogEntry {
    pub id: u64,           // 自增 ID，用于增量拉取
    pub timestamp: String, // RFC 3339
    pub level: LogLevel,   // Debug, Info, Warn, Error
    pub target: String,    // 模块路径 (e.g. "hollow_core::store")
    pub message: String,
}

pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
```

**Ring Buffer**:
- 全局 `Mutex<VecDeque<LogEntry>>`，容量 5000 条
- 满时丢弃最旧的条目
- 自增 ID 单调递增，不随丢弃重置

**tracing Subscriber**:
- 自定义 `Layer` 实现，将 tracing 事件写入 ring buffer
- 在 `HollowCore::new()` 时初始化（只初始化一次）

**FFI 导出** (通过 uniffi):
```rust
fn get_logs(since_id: u64) -> Vec<LogEntry>;
fn clear_logs();
```

### 2. Swift: Logger 封装

**位置**: `hollow/Logging/HollowLogger.swift`

**设计**: 轻量封装，为每个模块提供预配置的 Logger 实例。

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

**替换所有 print()**: 现有 `FileWatcher.swift`、`HollowBridge.swift` 中的 `print()` 调用全部替换为对应 Logger 调用。

### 3. Swift: RustLogRelay

**位置**: `hollow/Logging/RustLogRelay.swift`

**职责**: 定时从 Rust ring buffer 拉取日志，转发到 `HollowLogger.rustCore`。

**行为**:
- 应用启动时开始，500ms 间隔 Timer
- 记录上次拉取的 `lastSeenId`，每次调用 `get_logs(since_id: lastSeenId)`
- 将每条 Rust LogEntry 转发为对应级别的 os.Logger 调用
- 应用退出时停止

### 4. Debug Log Viewer

**位置**: `hollow/Views/LogViewerView.swift`

**入口**: Debug 菜单下，与 Database Browser 平级，快捷键 `Cmd+Shift+L`

**功能**:
- 通过 `OSLogStore(scope: .currentProcessIdentifier)` 读取日志
- 按 subsystem `com.syncpulse.hollow` 过滤
- 两个 Tab: **Swift** (所有非 RustCore category) / **Rust** (RustCore category)
- 每个 Tab 内支持:
  - 按日志级别过滤 (debug/info/warning/error)
  - 按 category 过滤 (下拉选择)
  - 文本搜索
- 自动滚动到底部（最新日志），可手动暂停滚动
- 刷新按钮手动刷新，外加 2 秒自动刷新
- 注意: `OSLogStore(scope: .currentProcessIdentifier)` 只读当前进程日志，应用重启后历史清空。这是期望行为 —— 调试日志不需要跨重启持久化

**列表每行显示**: `[时间] [级别] [Category] 消息`

---

## Log Levels

| os.Logger 级别 | 用途 | 示例 |
|---|---|---|
| `.debug` | 开发细节，正常运行时不需要看 | "FSEvent callback: 3 events received" |
| `.info` | 关键流程节点 | "Ingested file: photo.jpg (245 KB)" |
| `.warning` | 异常但可恢复 | "Quick hash collision detected" |
| `.error` | 错误，需要关注 | "Failed to create inbox directory: permission denied" |

Rust 侧 `tracing` 级别映射: debug→debug, info→info, warn→warning, error→error。

---

## Integration Points

### HollowBridge 变更
- `ingestFile` 成功/失败记录 info/error 日志
- `computeHash` 开始/完成记录 debug 日志
- 初始化成功/失败记录 info/error 日志

### FileWatcher 变更
- 目录创建成功/失败 → info/error
- FSEventStream 创建/启动/停止 → info
- 文件事件处理 → debug
- Fallback full scan 触发 → warning

### IngestionService 变更
- 启动/停止 → info
- 文件摄取成功 → info
- 重复文件跳过 → debug
- 错误 → error
- processAllPending 开始/完成 → debug

### hollowApp 变更
- 注册 Debug 菜单新增 Log Viewer 窗口
- 启动时初始化 RustLogRelay

---

## File Structure

```
hollow/
  Logging/
    HollowLogger.swift       — Logger 实例枚举
    RustLogRelay.swift        — Rust 日志拉取转发
  Views/
    LogViewerView.swift       — Debug 日志查看窗口

hollow-core/src/
    logging.rs                — Ring buffer + tracing Layer
    lib.rs                    — 新增 get_logs / clear_logs FFI 导出
```
