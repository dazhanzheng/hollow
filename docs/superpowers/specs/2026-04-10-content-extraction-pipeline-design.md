# Content Extraction Pipeline Design Spec

## Goal

为 hollow 建立一个**插件化、并行、异步**的文件内容摄取系统（Content Extraction Pipeline，简称 CEP）。在现有元数据摄取（metadata intake）之上，添加真正的内容理解层：读取文件正文、统一为 UTF-8 文本、存入数据库、为后续全文检索和语义理解做准备。

同时将现有的串行元数据摄取改造为并行异步队列。

## Non-Goals

- **不涉及 LLM 语义理解**（tags/summary/category）—— 那是 Batch 3 的工作，本 spec 只负责"把文本拿出来"这一步
- **不涉及 FTS5 索引本身的实现** —— 本 spec 只保证 `body_text` 被正确填充，FTS5 虚拟表的建立留给 Batch 3
- **不涉及 OCR** —— 图片、扫描 PDF 的文字识别留给后续批次
- **不实现复杂格式**（PDF/docx/rtf/html）—— 第一批只做纯文本和源代码类，其他格式留给后续批次
- **不修改文件名** —— 即使检测到后缀名错误也不自动 rename，只在 UI 上提示

---

## Architecture

### 整体流程

```
FSEvents
    │
    ▼
FileWatcher ──→ IngestionService
                    │
                    ├──→ MetadataQueue (并行 N 个 worker)
                    │       │
                    │       ▼
                    │   HollowBridge.ingestFile()
                    │       │
                    │       ▼
                    │   [Rust] 元数据摄取 → 写 files 表 (status: pending)
                    │       │
                    │       └──→ 入队 ContentQueue
                    │
                    └──→ ContentQueue (并行 N 个 worker)
                            │
                            ▼
                        HollowBridge.extractContent()
                            │
                            ▼
                        [Rust] ContentPipeline
                            │
                            ├──→ FormatDetector (infer crate, magic bytes)
                            │      │
                            │      ├──→ detected != extension → 标记 mismatch
                            │      └──→ 按 detected 分发
                            │
                            ├──→ ExtractorRegistry
                            │      │
                            │      ├──→ PlainTextExtractor (txt/md/log/json/yaml/...)
                            │      ├──→ SourceCodeExtractor (py/js/rs/swift/go/...)
                            │      └──→ (未来: DocxExtractor, PdfExtractor, ...)
                            │
                            └──→ 写入 file_content.body_text (压缩)
                                  更新 files.status = "indexed"
```

### 核心决策

1. **内容提取在 Rust 侧完成** —— Swift 只负责调度和 UI，提取逻辑留在 hollow-core，为未来 server 端复用做准备
2. **元数据摄取和内容摄取是两个独立的并行队列** —— 元数据快（<10ms），内容慢（秒级），不应互相阻塞
3. **并发度基于 CPU 核数** —— `max(2, cpu_cores / 2)`，后台任务不抢占前台应用资源
4. **插件化 Extractor** —— 通过注册表（`HashMap<FormatKey, Box<dyn Extractor>>`）分发，添加新格式只需新增一个 extractor 模块
5. **magic bytes 格式检测** —— 使用 `infer` crate 读取文件头识别真实格式，不信任后缀名
6. **统一存 body_text（压缩）** —— 所有提取出的文本都用 zstd 压缩后存入 `file_content.body_text`，FTS5 索引通过 external content 模式引用
7. **失败不重试** —— 提取失败标记 `extract_failed`，记录原因，用户可手动触发重新提取
8. **变更检测用 quick_hash** —— 文件变更时重算 quick_hash，变了才重新提取

---

## Status State Machine

当前只有 `pending / indexed / missing`，本 spec 扩展为：

```
        ingest_file()                   extract_content()
    ┌──────────────────┐           ┌────────────────────┐
    │                  │           │                    │
    ▼                  │           ▼                    │
 (new)              pending ────────→ extracting ──────→ indexed
                       │                  │
                       │                  └──→ extract_failed
                       │
                       └──(file removed)──→ missing
                                              │
                    (file changed)            │
  indexed ───────────────────────────→ pending (re-extract)
```

| Status | 含义 |
|--------|------|
| `pending` | 元数据已摄取，等待内容提取 |
| `extracting` | 内容提取进行中（worker 占用中） |
| `indexed` | 内容已提取并存入 `file_content.body_text` |
| `extract_failed` | 提取失败，`file_content.extract_error` 有错误原因 |
| `missing` | 文件已从磁盘删除 |

**兼容性**: 老的 `enableFullHash` 流程（computeHash → markIndexed）保留，但 `indexed` 的语义现在意味着"内容已提取"，而非"full hash 已算"。full hash 的触发从 "pending → indexed" 的转换中脱离，单独作为一个 opt-in 操作。

---

## Schema Changes

### `file_content` 表扩展

当前字段: `file_id, body_text, ocr_text, source`

新增字段:
```sql
ALTER TABLE file_content ADD COLUMN body_text_compressed BLOB;      -- zstd 压缩后的正文
ALTER TABLE file_content ADD COLUMN body_text_bytes INTEGER;        -- 解压后字节数（用于 UI 显示）
ALTER TABLE file_content ADD COLUMN encoding TEXT;                  -- 检测到的原始编码（如 "GBK", "UTF-8"）
ALTER TABLE file_content ADD COLUMN extracted_at TEXT;              -- 提取时间戳 RFC3339
ALTER TABLE file_content ADD COLUMN extractor_name TEXT;            -- 使用的 extractor 标识（如 "PlainText", "SourceCode"）
ALTER TABLE file_content ADD COLUMN extract_error TEXT;             -- 失败时的错误信息，成功时 NULL
```

原有的 `body_text` 字段废弃（保留列以向后兼容，但不再写入）。所有新数据写入 `body_text_compressed`。

> **注**：当前 `file_content` 表尚未被任何代码写入（ready for content extraction feature，见 exploration 报告），所以改动没有数据迁移风险，但仍需正确的 schema 迁移流程（bump `SCHEMA_VERSION` 4）。

### `files` 表扩展

```sql
ALTER TABLE files ADD COLUMN detected_mime TEXT;        -- magic bytes 检测结果
ALTER TABLE files ADD COLUMN extension_mismatch INTEGER DEFAULT 0;  -- 0/1: 后缀名是否与真实格式不符
```

原有的 `mime_type` 字段保留（来自 mime_guess，基于后缀），新字段 `detected_mime` 来自 `infer`。UI 展示时若 `extension_mismatch=1`，显示警告。

### `SCHEMA_VERSION` = 4

在 `hollow-core/src/db/schema.rs` 中新增 migration `v3_to_v4`。

---

## Rust Components

### 1. `hollow-core/src/content/mod.rs` — 模块入口

```rust
pub mod detector;
pub mod extractor;
pub mod pipeline;
pub mod registry;

pub use pipeline::ContentPipeline;
pub use extractor::{Extractor, ExtractionResult, ExtractionError};
```

### 2. `hollow-core/src/content/detector.rs` — 格式检测

```rust
pub struct FormatDetector;

pub struct DetectedFormat {
    pub mime: String,                 // e.g. "text/plain", "application/pdf"
    pub extension_hint: Option<String>, // infer crate 给出的建议后缀
    pub is_text: bool,                // 是否为文本类
}

impl FormatDetector {
    pub fn detect(path: &Path) -> Result<DetectedFormat, DetectionError>;
}
```

实现使用 `infer` crate 读取文件头前 8KB 判断。对于 `infer` 识别不出的文件，fallback 用后缀名推断 + heuristic（尝试 UTF-8 decode 前 4KB，成功则视为文本）。

### 3. `hollow-core/src/content/extractor.rs` — Extractor trait

```rust
pub trait Extractor: Send + Sync {
    /// 该 extractor 的唯一标识（用于日志和 DB 记录）
    fn name(&self) -> &'static str;

    /// 该 extractor 声明能处理的 MIME 类型列表
    fn supported_mimes(&self) -> &[&'static str];

    /// 提取文件内容
    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError>;
}

pub struct ExtractionResult {
    pub body_text: String,        // 提取出的 UTF-8 文本
    pub encoding: Option<String>, // 原始编码（如果做了转换）
}

#[derive(thiserror::Error, Debug)]
pub enum ExtractionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("encoding detection failed")]
    EncodingDetectionFailed,
    #[error("file too large: {size} bytes (limit: {limit})")]
    FileTooLarge { size: u64, limit: u64 },
    #[error("extraction failed: {0}")]
    Other(String),
}
```

### 4. `hollow-core/src/content/registry.rs` — 注册表

```rust
pub struct ExtractorRegistry {
    by_mime: HashMap<String, Arc<dyn Extractor>>,
}

impl ExtractorRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, extractor: Arc<dyn Extractor>);
    pub fn find(&self, mime: &str) -> Option<Arc<dyn Extractor>>;
}

/// 默认注册表：注册第一批 extractors
pub fn default_registry() -> ExtractorRegistry {
    let mut r = ExtractorRegistry::new();
    r.register(Arc::new(extractors::PlainTextExtractor::new()));
    r.register(Arc::new(extractors::SourceCodeExtractor::new()));
    r
}
```

### 5. `hollow-core/src/content/extractors/` — 具体 extractor 实现

#### `plain_text.rs` — PlainTextExtractor

处理的 MIME 类型：
- `text/plain`, `text/markdown`, `text/csv`, `text/tab-separated-values`
- `application/json`, `application/xml`, `application/yaml`, `application/toml`
- `text/x-log`（日志文件）

实现：
1. 读取文件全部字节（上限 50 MB，超过返回 `FileTooLarge`）
2. 用 `chardetng::EncodingDetector` 检测编码
3. 转换为 UTF-8 字符串
4. 返回 `ExtractionResult { body_text, encoding }`

#### `source_code.rs` — SourceCodeExtractor

处理的 MIME 类型（以及通过后缀兜底）：
- `text/x-python`, `text/x-rust`, `text/x-go`, `text/x-swift`, `text/x-java`
- `application/javascript`, `application/typescript`
- `text/x-c`, `text/x-c++`, `text/x-shellscript`, `text/x-ruby`
- 以及后缀兜底的：`.py .js .ts .rs .swift .go .java .c .cpp .h .hpp .rb .sh .bash .zsh .fish .m .mm .kt .scala .sql .html .css .scss .less .vue .jsx .tsx`

实现：与 `PlainTextExtractor` 基本相同（都是 UTF-8 文本直读），区别在于 `name()` 返回 `"SourceCode"`，方便后续分类统计。未来可以在这里做语言检测并记录到 metadata。

> **注**：PlainText 和 SourceCode 共享底层文本读取逻辑，实现上应抽取一个内部 helper `read_text_file(path, size_limit)`，两个 extractor 都调用它。

### 6. `hollow-core/src/content/pipeline.rs` — ContentPipeline

```rust
pub struct ContentPipeline {
    registry: ExtractorRegistry,
    max_file_size: u64,  // 默认 50 MB
}

pub struct ExtractionOutcome {
    pub status: String,              // "indexed" or "extract_failed"
    pub extractor_name: Option<String>,
    pub body_text: Option<String>,
    pub encoding: Option<String>,
    pub detected_mime: String,
    pub extension_mismatch: bool,
    pub error: Option<String>,
}

impl ContentPipeline {
    pub fn new(registry: ExtractorRegistry) -> Self;

    /// 对单个文件运行完整流程：检测 → 路由 → 提取
    pub fn process(&self, path: &Path, original_extension: Option<&str>) -> ExtractionOutcome;
}
```

**`process` 的逻辑**：

```
1. FormatDetector::detect(path) → DetectedFormat
2. 比较 detected.extension_hint 与 original_extension → 设置 extension_mismatch
3. registry.find(&detected.mime) → extractor
   ├─ 找到 → extractor.extract(path)
   │    ├─ Ok  → ExtractionOutcome { status: "indexed", body_text, ... }
   │    └─ Err → ExtractionOutcome { status: "extract_failed", error, ... }
   └─ 没找到 → ExtractionOutcome { status: "extract_failed", error: "no extractor for mime: ..." }
```

### 7. `hollow-core/src/lib.rs` — 新增 FFI 方法

```rust
impl HollowCore {
    /// 对指定文件执行内容提取。独立于 ingest_file。
    /// 返回提取结果供 Swift 日志展示。
    pub fn extract_content(&self, file_id: String) -> Result<ExtractContentResult, HollowError>;

    /// 检测文件是否已变更（重算 quick_hash 对比）
    pub fn has_changed(&self, file_id: String) -> Result<bool, HollowError>;

    /// 将已 indexed 的文件标记回 pending，用于触发重新提取
    pub fn mark_for_reextraction(&self, file_id: String) -> Result<(), HollowError>;

    /// 获取所有待提取的 file_id（status = "pending"）
    /// 注：此方法取代原有的 get_pending_ids()。语义一致，但命名更明确。
    /// 原有的 get_pending_ids() 保留以向后兼容（指向同一实现）。
    pub fn get_pending_extraction_ids(&self) -> Result<Vec<String>, HollowError>;
}

#[derive(uniffi::Record)]
pub struct ExtractContentResult {
    pub file_id: String,
    pub status: String,                  // "indexed" | "extract_failed"
    pub extractor_name: Option<String>,
    pub detected_mime: String,
    pub extension_mismatch: bool,
    pub body_text_bytes: u64,            // 解压后大小，UI 展示用
    pub error: Option<String>,
}
```

**`extract_content` 内部行为**：

```
1. FileStore::get_file(id) → record
2. ContentPipeline::process(&record.current_path, record.extension.as_deref()) → outcome
3. 如果 outcome.status == "indexed":
     a. zstd 压缩 body_text → body_text_compressed
     b. FileContentStore::upsert(file_id, body_text_compressed, bytes, encoding, extractor_name, extracted_at)
     c. FileStore::update_detected_mime(file_id, outcome.detected_mime, extension_mismatch)
     d. FileStore::update_status(file_id, "indexed")
4. 如果 outcome.status == "extract_failed":
     a. FileContentStore::upsert_error(file_id, extract_error, extractor_name, extracted_at)
     b. FileStore::update_status(file_id, "extract_failed")
5. 返回 ExtractContentResult
```

### 8. `hollow-core/src/store/file_content_store.rs` — 新增

```rust
pub struct FileContentStore;

impl FileContentStore {
    pub fn upsert(
        conn: &Connection,
        file_id: &str,
        body_text_compressed: &[u8],
        body_text_bytes: i64,
        encoding: Option<&str>,
        extractor_name: &str,
        extracted_at: &str,
    ) -> Result<(), HollowError>;

    pub fn upsert_error(
        conn: &Connection,
        file_id: &str,
        error: &str,
        extractor_name: Option<&str>,
        extracted_at: &str,
    ) -> Result<(), HollowError>;

    pub fn get_body_text(conn: &Connection, file_id: &str) -> Result<Option<String>, HollowError>;
}
```

`get_body_text` 内部解压 zstd → 返回 UTF-8 字符串。

### 9. 新增依赖（Cargo.toml）

```toml
infer = "0.19"                  # magic bytes 格式检测
chardetng = "0.1"               # 文本编码检测
zstd = { version = "0.13", default-features = false }  # 文本压缩
```

---

## Swift Components

### 1. `IngestionService.swift` 改造

**当前**: 一个 serial `DispatchQueue` 做元数据摄取。

**改造后**: 两个并行队列。

```swift
class IngestionService {
    private let metadataQueue: OperationQueue
    private let contentQueue: OperationQueue

    init() {
        let cores = ProcessInfo.processInfo.activeProcessorCount
        let concurrency = max(2, cores / 2)

        metadataQueue = OperationQueue()
        metadataQueue.maxConcurrentOperationCount = concurrency
        metadataQueue.qualityOfService = .utility
        metadataQueue.name = "com.syncpulse.hollow.metadata"

        contentQueue = OperationQueue()
        contentQueue.maxConcurrentOperationCount = concurrency
        contentQueue.qualityOfService = .utility
        contentQueue.name = "com.syncpulse.hollow.content"
    }
}
```

**关键点**:
- 元数据摄取和内容摄取各自一个 `OperationQueue`，并发度都是 `max(2, cores / 2)`
- `HollowBridge` 的 Rust 侧已经用 Mutex 串行化 DB 访问，所以并行提交 FFI 调用是安全的。注意：尽管 FFI 是并行的，Rust 侧 DB 锁会把写操作串行化，这是性能上的瓶颈 —— 后续可以考虑更细粒度的锁，但第一版接受这个权衡
- 元数据摄取成功后，**自动把该 file_id 提交到 contentQueue**，不需要等批量 pending 扫描
- **启动时的 pending 扫描**: `IngestionService.start()` 调用 `bridge.getPendingExtractionIds()`，将结果批量入队 contentQueue。处理应用上次未完成的提取任务

### 2. `ContentExtractionOperation` — 新增

```swift
class ContentExtractionOperation: Operation {
    let fileId: String
    weak var service: IngestionService?

    override func main() {
        guard !isCancelled else { return }
        let result = HollowBridge.shared.extractContent(fileId: fileId)
        DispatchQueue.main.async {
            self.service?.handleExtractionResult(result)
        }
    }
}
```

类似的 `MetadataIntakeOperation` 封装现有的 `bridge.ingestFile()` 逻辑。

### 3. `HollowBridge.swift` 新增方法

```swift
func extractContent(fileId: String) -> ExtractContentResult?
func hasChanged(fileId: String) -> Bool
func getPendingExtractionIds() -> [String]
```

### 4. `FileWatcher.swift` 改造

**现状**: 只处理 create / remove。

**新增**: 处理 modify 事件（FSEvents 的 `ItemModified` flag）。

```swift
var onModifiedFiles: (([URL]) -> Void)?
```

**IngestionService 对 modify 的处理**:
1. 提交到 metadataQueue 的 `MetadataIntakeOperation`（内部会调用新的 `bridge.hasChanged(fileId)`）
2. 如果 `hasChanged == true`，调用 Rust 侧一个新方法 `mark_for_reextraction(file_id)`，将 status 改回 `pending`，然后入队 contentQueue
3. 如果 `hasChanged == false`，忽略

### 5. `ContentView.swift` / `SettingsView.swift` UI 改造

- 在状态 HUD 显示两个队列的任务数（"元数据: 0 / 内容: 3"）
- `SettingsView` 展示并发度（"Content extraction: 4 workers"）
- `DatabaseBrowserView` 显示 `extension_mismatch` 警告图标，点击后提示用户手动重命名后缀
- 新增"重新提取内容"按钮（针对 `extract_failed` 的文件）

### 6. 初次启动 / 迁移场景

现有数据库中已有大量 `status = "indexed"` 的文件，但它们的 `file_content` 是空的（旧版 indexed 意味着 hash 计算完，不代表内容已提取）。

**处理策略**：
- schema 迁移 v3 → v4 时，将所有 `status = "indexed"` 的文件改回 `status = "pending"`，强制重新走内容提取流程
- 启动后 IngestionService 会扫描所有 `pending` 文件并入队 contentQueue

---

## File Change Detection Flow

用户修改 `~/Hollow Inbox/notes.txt` 后：

```
FSEvents (ItemModified)
    ↓
FileWatcher.onModifiedFiles callback
    ↓
IngestionService → metadataQueue 提交 MetadataIntakeOperation
    ↓
bridge.hasChanged(fileId)
    ├─ true → bridge.markForReextraction(fileId)    ← status: indexed → pending
    │         contentQueue 提交 ContentExtractionOperation
    │             ↓
    │         bridge.extractContent(fileId)
    │             ↓
    │         重新提取 → 覆写 file_content.body_text_compressed
    │             ↓
    │         status: pending → indexed
    └─ false → 什么都不做
```

---

## Error Handling & Edge Cases

| 场景 | 处理 |
|------|------|
| 文件在 extract 期间被删除 | `ExtractionError::Io` → `extract_failed`, error: "file removed during extraction" |
| 文件 > 50 MB | `ExtractionError::FileTooLarge` → `extract_failed`, error 记录大小 |
| 编码检测失败 | `ExtractionError::EncodingDetectionFailed` → `extract_failed` |
| MIME 类型无对应 extractor | `extract_failed`, error: "no extractor for mime: image/jpeg" |
| Extractor panic | Rust 侧用 `std::panic::catch_unwind` 包裹 extract 调用，转为 `ExtractionError::Other` |
| 二进制文件被误判为文本 | `chardetng` 检测置信度低时返回 `EncodingDetectionFailed`，避免存入乱码 |
| 空文件（0 字节） | `body_text = ""`, status = "indexed"（合法情况） |
| 并发修改同一文件 | OperationQueue 不保证顺序，但 Rust 侧 Mutex 保证 DB 一致性；最后一次写入为准 |

---

## Performance Targets

| 指标 | 目标 |
|------|------|
| 元数据摄取延迟（单文件） | < 10 ms（不变，原目标） |
| 内容摄取延迟（1 MB 纯文本） | < 50 ms |
| 内容摄取延迟（10 MB 纯文本） | < 500 ms |
| 并发 4 worker 吞吐 | > 20 files/sec（小文本）|
| zstd 压缩率（纯文本） | 70-85% 压缩率 |
| 启动时 pending 扫描（10k 文件） | < 1s |

---

## Testing Strategy

### Rust 侧单元测试

1. **FormatDetector**
   - 真实 PNG 后缀改为 `.txt` → 检测为 `image/png`，extension_mismatch = true
   - 纯 UTF-8 文本 → `text/plain`
   - 空文件 → fallback 为 `text/plain`（或 `application/octet-stream`，看 infer 行为）
   - GBK 编码的 .txt → `text/plain`（mime 正确）

2. **PlainTextExtractor**
   - ASCII 文本 → body_text 正确、encoding = "UTF-8"
   - GBK 中文 → 转换为 UTF-8、encoding = "GBK"
   - Shift-JIS 日文 → 转换为 UTF-8、encoding = "Shift_JIS"
   - 50 MB + 1 byte → `FileTooLarge`
   - 损坏的字节流 → `EncodingDetectionFailed`

3. **SourceCodeExtractor**
   - Python / Rust / Swift 文件 → 正确读取

4. **ExtractorRegistry**
   - 注册 + 查找 → 命中
   - 未知 MIME → None

5. **ContentPipeline**
   - 完整流程 end-to-end：输入路径 → 输出 `ExtractionOutcome`
   - 未知格式 → `extract_failed`
   - Extractor 抛错 → `extract_failed`，error 字段非空

6. **FileContentStore**
   - upsert + get_body_text 往返（压缩→存储→读取→解压）
   - 压缩后字节数 < 原始字节数（对于有冗余的文本）
   - upsert_error 正确存储

7. **Integration test**: ingest_file + extract_content 全流程，检查 DB 状态

### Swift 侧测试

1. **IngestionService**
   - 并发队列正确创建，maxConcurrentOperationCount 符合预期
   - FileWatcher 回调正确入队
   - Modify 事件触发 hasChanged → markForReextraction 链路

2. **UI smoke test**
   - DatabaseBrowserView 显示 extension_mismatch 警告
   - Extract failed 文件可以重新触发提取

---

## File Structure

```
hollow-core/src/
    content/
        mod.rs                    — 模块入口
        detector.rs               — FormatDetector
        extractor.rs              — Extractor trait + ExtractionError
        registry.rs               — ExtractorRegistry + default_registry()
        pipeline.rs               — ContentPipeline
        extractors/
            mod.rs
            plain_text.rs         — PlainTextExtractor
            source_code.rs        — SourceCodeExtractor
            common.rs             — 共享的 read_text_file helper
    store/
        file_content_store.rs     — FileContentStore (新增)
    db/
        schema.rs                 — SCHEMA_VERSION = 4, 新增 v3_to_v4 migration
    lib.rs                        — 新增 extract_content / has_changed / get_pending_extraction_ids FFI

hollow/
    IngestionService.swift        — 改造为双 OperationQueue
    Operations/                   — 新目录
        MetadataIntakeOperation.swift
        ContentExtractionOperation.swift
    HollowBridge.swift            — 新增 extractContent / hasChanged / getPendingExtractionIds
    FileWatcher.swift             — 新增 onModifiedFiles 回调
```

---

## Open Questions

1. **chardetng 的置信度阈值** — 检测失败时如何定义"失败"？需要在实现时根据真实数据调优
2. **zstd 压缩级别** — 默认级别（3）还是更高（6/9）？压缩时间 vs 压缩率的权衡，建议先用默认，实测后调整
3. **Source code 语言检测** — 第一版是否需要记录 language 字段？建议第一版不做，后续 Batch 3 语义阶段再处理
4. **FileWatcher 的 modify 事件去抖** — 用户保存文件时可能连续触发多次 modify，是否需要 500ms 防抖？建议实现时加上

---

## Future Extensions (Out of Scope)

- **Batch 2**: DocxExtractor, RtfExtractor, HtmlExtractor
- **Batch 3**: PdfExtractor, PdfOcrExtractor (扫描件)
- **Batch 4**: ImageOcrExtractor (图片中的文字)
- **Batch 5**: AudioTranscriptExtractor (音频转文字，复用 Whisper)
- **FTS5 集成**: **已知挑战** —— FTS5 不能直接索引压缩的 BLOB，三种可行方案留给 Batch 3 决定：
  1. 维护一个未压缩的 shadow table（存储翻倍但查询最快）
  2. 用 SQLite 虚拟表 hook，在插入 FTS5 时动态解压
  3. 把 `body_text_compressed` 改为 `body_text`（明文），放弃压缩换取 FTS5 简单性
  
  本 spec 的决策是**保留压缩**，FTS5 集成时再权衡。如果最终选方案 3，届时通过 schema v5 迁移即可
- **Re-extraction API**: UI 触发"全量重新提取"
- **Content diff**: 修改检测到变化后，保留历史版本（类似 git）
