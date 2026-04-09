# Phase 1 Batch 2：文件监听 + 元数据摄取

> 监听入口文件夹、检测新文件、提取元数据入库。不做内容提取——后续用插件式架构处理。

---

## 背景

Batch 1 完成了基础设施：SQLite 数据层、UniFFI 桥接、hollow-server 骨架。`HollowCore.ingestFile` 已经能读文件、算哈希、写入数据库。

Batch 2 的目标是让文件**自动**进入 hollow——用户把文件丢进入口文件夹，系统自动检测并摄取。

### 决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| 入口文件夹路径 | `~/Hollow Inbox/` | 与 Documents/Downloads 平级，Finder 中醒目 |
| 入口文件夹创建 | 首次启动自动创建 | 零配置，降低使用门槛 |
| 监听方案 | Swift `DispatchSource` (FSEvents) | macOS 原生 API，Swift 调用自然，不需要跨 FFI 回调 |
| 文件解析范围 | 仅元数据（文件名/大小/时间戳/哈希/MIME） | 内容提取后续用注册式插件架构，本批次不涉及 |
| MIME 类型检测 | 扩展名推断（`mime_guess` crate） | 阶段一够用，不做 magic bytes |

---

## 1. Swift 侧：文件夹监听

### 1.1 FileWatcher

**职责**：监听 `~/Hollow Inbox/`，检测新增文件，通知外部。

**实现**：
- 启动时通过 `FileManager` 创建 `~/Hollow Inbox/`（如不存在）
- 用 `DispatchSource.makeFileSystemObjectSource(.write)` 监听目录变更
- 事件触发后，`FileManager.contentsOfDirectory` 列出当前文件，与 `knownFiles: Set<String>` 做 diff
- 新文件通过回调 `onNewFiles: ([URL]) -> Void` 通知出去
- 启动时也做一次全量扫描，处理 app 未运行期间新增的文件

**过滤规则**：
- 忽略隐藏文件（文件名以 `.` 开头）
- 忽略临时文件后缀：`.tmp`, `.download`, `.crdownload`, `.partial`
- 只处理文件，不递归子目录

**防抖**：检测到变更后延迟 500ms 再扫描，避免读取正在写入的文件。多次变更事件在 500ms 内合并为一次扫描。

**文件**：`hollow/FileWatcher.swift`

### 1.2 IngestionService

**职责**：协调 FileWatcher 和 HollowBridge，管理摄取状态。

**实现**：
- 持有 `FileWatcher` 和 `HollowBridge` 引用
- 收到新文件列表后，在后台 `Task` 中逐个调用 `HollowBridge.ingestFile(path:)`
- 使用 `@Observable` 暴露状态供 UI 绑定：
  - `isWatching: Bool` — 监听是否运行中
  - `totalIngested: Int` — 已摄取文件总数
  - `recentFiles: [String]` — 最近摄取的文件名（最多 10 个）
  - `lastError: String?` — 最近一次错误

**错误处理**：
- `DuplicateFile` 错误：跳过，不算异常（文件已在库中）
- 其他错误：记录到 `lastError`，继续处理下一个文件

**文件**：`hollow/IngestionService.swift`

### 1.3 HollowBridge 扩展

在现有 `HollowBridge.swift` 中添加：

```swift
func ingestFile(path: String) throws -> FileRecord
```

包装 `core.ingestFile(filePath:)`。

**文件**：修改 `hollow/HollowBridge.swift`

### 1.4 App 入口

`hollowApp.swift` 中创建 `IngestionService` 作为 `@State`，注入 environment。App 启动即开始监听。

**文件**：修改 `hollow/hollowApp.swift`

### 1.5 UI 更新

`ContentView` 改为显示：
- 监听状态指示
- 已摄取文件数量
- 最近摄取的文件名列表

不做复杂 UI，只展示系统在工作。

**文件**：修改 `hollow/ContentView.swift`

---

## 2. Rust 侧：ingest_file 增强

### 2.1 MIME 类型检测

当前 `ingest_file` 的 `mime_type` 固定为 `None`。改为根据文件扩展名推断 MIME 类型。

**依赖**：添加 `mime_guess` crate。

**逻辑**：
```
extension → mime_guess::from_ext() → 取第一个匹配 → 存为 String
无法推断时保持 None
```

### 2.2 文件时间戳修正

当前 `created_at` 和 `modified_at` 使用摄取时间（`iso8601_now()`）。改为读取文件系统的实际时间：
- `created_at` ← `fs::metadata().created()`
- `modified_at` ← `fs::metadata().modified()`
- `ingested_at` 保持为当前时间

时间格式统一为 ISO 8601 / RFC 3339。

### 2.3 去重拦截

当前 `ingest_file` 不检查重复直接插入。改为：
1. 计算文件哈希后，调用 `check_duplicate(hash)`
2. 若已存在，返回 `HollowError::DuplicateFile(hash)` 错误
3. Swift 侧 `IngestionService` 捕获此错误并跳过

### 2.4 不在本批次范围内

- 不做内容提取（正文、OCR、转录等）
- 不做 MIME magic bytes 检测
- 不做文件移动/重命名
- 不做 `file_content` 表写入
- 不做 `file_metadata` 表写入

---

## 3. 文件结构变更

### 新增

| 文件 | 职责 |
|------|------|
| `hollow/FileWatcher.swift` | 文件夹监听，检测新文件 |
| `hollow/IngestionService.swift` | 摄取协调，状态管理 |

### 修改

| 文件 | 变更 |
|------|------|
| `hollow/HollowBridge.swift` | 添加 `ingestFile(path:)` 方法 |
| `hollow/hollowApp.swift` | 创建 IngestionService，注入 environment |
| `hollow/ContentView.swift` | 显示监听状态和摄取统计 |
| `hollow-core/Cargo.toml` | 添加 `mime_guess` 依赖 |
| `hollow-core/src/lib.rs` | ingest_file 增强：MIME、时间戳、去重 |

---

## 4. 测试策略

### Rust 侧（cargo test）
- MIME 推断测试：`.pdf` → `application/pdf`，`.txt` → `text/plain`，未知扩展名 → `None`
- 时间戳测试：`created_at` 和 `modified_at` 不等于 `ingested_at`（使用临时文件验证）
- 去重拦截测试：同一文件摄取两次，第二次返回 `DuplicateFile` 错误

### Swift 侧（手动测试）
- 启动 app → 确认 `~/Hollow Inbox/` 已创建
- 往文件夹拖入文件 → 确认 app UI 显示文件已摄取
- 拖入同名文件 → 确认不报错，跳过重复
- 拖入隐藏文件（`.DS_Store`）→ 确认被忽略
