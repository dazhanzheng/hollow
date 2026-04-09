# Phase 1 Batch 1：基础设施设计

> 第一阶段第一批工作的技术设计：SQLite 数据层、UniFFI 桥接、hollow-server 骨架。

---

## 背景

hollow 处于初始脚手架阶段。第一阶段目标是"可用的语义入口"，第一批工作是搭建三个基础设施模块，为后续的文件监听、解析、理解、检索提供地基。

### 决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| FFI 方案 | UniFFI proc-macro | Mozilla 出品，Firefox 在用，生态最成熟，自动生成 Swift 绑定，可测试性好 |
| Web 框架 | Axum | Tokio 团队维护，Rust Web 生态最主流 |
| 数据存储路径 | `~/Library/Application Support/com.syncpulse.hollow/` | macOS 标准做法，Time Machine 自动备份 |
| 整体顺序 | 自底向上（数据库先行） | 每一步可 `cargo test` 验证，FFI 暴露有真实数据操作 |

---

## 1. hollow-core 数据层

### 1.1 SQLite Schema

数据库文件：`~/Library/Application Support/com.syncpulse.hollow/hollow.db`

#### `files` 表 — 文件对象核心记录

```sql
CREATE TABLE files (
    id           TEXT PRIMARY KEY,              -- UUID v7
    hash         TEXT NOT NULL,                  -- SHA-256，去重用（不 UNIQUE，允许相同内容多路径）
    current_path TEXT NOT NULL UNIQUE,          -- 当前文件路径
    original_path TEXT NOT NULL,                -- 摄取时原始路径
    file_name    TEXT NOT NULL,                 -- 文件名
    extension    TEXT,                          -- 扩展名
    mime_type    TEXT,                          -- MIME 类型
    size_bytes   INTEGER NOT NULL,              -- 文件大小
    created_at   TEXT NOT NULL,                 -- 文件创建时间 (ISO 8601)
    modified_at  TEXT NOT NULL,                 -- 文件修改时间
    ingested_at  TEXT NOT NULL,                 -- 摄取时间
    status       TEXT NOT NULL DEFAULT 'pending' -- pending/indexed/archived/error
);

CREATE INDEX idx_files_hash ON files(hash);
CREATE INDEX idx_files_status ON files(status);
CREATE INDEX idx_files_ingested_at ON files(ingested_at);
```

#### `file_metadata` 表 — AI 生成的语义信息（异步填充）

分离理由：metadata 由 AI 异步填充，生命周期与 files 不同。分表允许 metadata schema 独立演进，不影响核心文件记录的稳定性。

```sql
CREATE TABLE file_metadata (
    file_id        TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    summary        TEXT,                        -- AI 生成摘要
    tags           TEXT,                        -- JSON 数组，如 ["报税","2025Q4"]
    category       TEXT,                        -- 文件分类
    sensitivity    TEXT DEFAULT 'normal',       -- normal/privacy/financial/legal/sensitive
    suggested_name TEXT,                        -- AI 建议文件名
    suggested_path TEXT                         -- AI 建议归档路径
);
```

> **注意**：`tags` 字段阶段一用 JSON TEXT 存储。后续如果需要按标签高效查询，可迁移到 junction table。

#### `file_content` 表 — 提取的文本内容（全文检索基础）

```sql
CREATE TABLE file_content (
    file_id   TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    body_text TEXT,                             -- 解析提取的正文
    ocr_text  TEXT,                             -- OCR 识别文本
    source    TEXT                              -- 提取方式：parser/ocr/transcription
);
```

#### `operations_log` 表 — 操作日志（支持回滚）

```sql
CREATE TABLE operations_log (
    id           TEXT PRIMARY KEY,              -- UUID
    file_id      TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    op_type      TEXT NOT NULL,                 -- ingest/move/rename/tag/delete
    before_state TEXT,                          -- JSON，操作前状态
    after_state  TEXT,                          -- JSON，操作后状态
    performed_at TEXT NOT NULL                  -- 操作时间 (ISO 8601)
);

CREATE INDEX idx_operations_log_file_time ON operations_log(file_id, performed_at);
```

#### 迁移策略

- `schema.rs` 维护 `SCHEMA_VERSION` 常量（阶段一为 `1`）
- 启动时检查 `PRAGMA user_version`
- 若 `user_version < SCHEMA_VERSION`，逐版本执行迁移 SQL
- 阶段一只有 v0→v1（建表），框架到位即可

### 1.2 Rust 模块结构

```
hollow-core/src/
├── lib.rs              -- 公开 API（HollowCore struct）
├── db/
│   ├── mod.rs          -- Database struct，持有 rusqlite::Connection
│   ├── schema.rs       -- 建表 SQL、SCHEMA_VERSION、迁移逻辑
│   └── models.rs       -- FileRecord, FileMetadata, FileContent, OperationLog structs
├── store/
│   ├── mod.rs
│   └── file_store.rs   -- CRUD：insert_file, get_file, list_files, update_status, delete_file, check_duplicate
└── error.rs            -- HollowError（thiserror 派生）
```

### 1.3 关键依赖

```toml
[dependencies]
rusqlite = { version = "0.35", features = ["bundled"] }
uuid = { version = "1", features = ["v7"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uniffi = { version = "0.29", features = ["cli"] }

[build-dependencies]
uniffi = { version = "0.29", features = ["build"] }
```

### 1.4 核心类型

```rust
// db/models.rs
pub struct FileRecord {
    pub id: String,
    pub hash: String,
    pub current_path: String,
    pub original_path: String,
    pub file_name: String,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub created_at: String,
    pub modified_at: String,
    pub ingested_at: String,
    pub status: String,
}

// error.rs
pub enum HollowError {
    Database(String),
    FileNotFound(String),
    DuplicateFile(String),
    InvalidInput(String),
}
```

### 1.5 测试策略

- 所有 CRUD 操作使用内存数据库 (`:memory:`) 测试
- 去重测试：`check_duplicate` 对已存在的 hash 返回 true；相同 hash 不同路径的文件可正常插入
- 迁移测试：验证 `user_version` 正确递增
- 不依赖文件系统，不依赖网络

---

## 2. UniFFI 桥接

### 2.1 方案：proc-macro 模式

不使用 `.udl` 文件，直接在 Rust 代码中用宏标注：

```rust
// lib.rs
#[derive(uniffi::Record)]
pub struct FileRecord { /* ... */ }

#[derive(uniffi::Enum)]
pub enum HollowError { /* ... */ }

#[derive(uniffi::Object)]
pub struct HollowCore { /* ... */ }

#[uniffi::export]
impl HollowCore {
    #[uniffi::constructor]
    pub fn new(db_path: String) -> Result<Self, HollowError> { /* ... */ }

    pub fn ingest_file(&self, file_path: String) -> Result<FileRecord, HollowError> { /* ... */ }
    pub fn get_file(&self, id: String) -> Result<Option<FileRecord>, HollowError> { /* ... */ }
    pub fn list_files(&self, limit: u32, offset: u32) -> Result<Vec<FileRecord>, HollowError> { /* ... */ }
    pub fn check_duplicate(&self, hash: String) -> Result<bool, HollowError> { /* ... */ }
}
```

### 2.2 构建产物

- `libhollow_core.a` — 静态库（`Cargo.toml` 已配置 `crate-type = ["staticlib", "lib"]`）
- `hollow_coreFFI.h` — C 头文件（UniFFI 自动生成）
- `hollow_core.swift` — Swift 绑定（UniFFI 自动生成）

### 2.3 Xcode 集成

1. Xcode 项目添加 Build Phase 脚本，调用 `cargo build` 构建静态库
2. 将 `libhollow_core.a` 添加到 Link Binary With Libraries
3. 将生成的 `hollow_core.swift` 添加到 Swift 源码
4. 将 `hollow_coreFFI.h` 通过 bridging header 或 modulemap 引入

### 2.4 职责分工

| 职责 | 归属 |
|------|------|
| 构造 `db_path`（Application Support 路径拼接） | Swift 侧 |
| 数据库创建、迁移、CRUD | Rust 侧 (`HollowCore`) |
| `HollowCore` 生命周期管理 | UniFFI 自动 Drop（Swift 释放引用时关闭数据库连接） |

### 2.5 异步说明

阶段一所有 FFI 接口均为同步。`ingest_file` 涉及文件 I/O 和哈希计算，但阶段一文件量小，同步调用可接受。Swift 侧如需避免阻塞主线程，可在 `Task {}` 中调用。后续如性能瓶颈明显，可迁移到 UniFFI async export。

### 2.6 测试策略

- **Rust 侧**：`cargo test` 直接测试 `HollowCore` 方法（不经过 FFI，测业务逻辑）
- **Swift 侧**：Swift Testing 框架调用生成的绑定，验证端到端（`ingest_file` → `get_file` 数据一致）

---

## 3. hollow-server 骨架

### 3.1 模块结构

```
hollow-server/src/
├── main.rs         -- 启动入口：读配置、初始化 tracing、注册路由、启动服务
├── routes/
│   ├── mod.rs      -- 路由注册函数
│   └── health.rs   -- GET /health
├── config.rs       -- Config struct，从环境变量读取
└── error.rs        -- AppError 统一错误响应（实现 IntoResponse）
```

### 3.2 依赖

```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tower-http = { version = "0.6", features = ["trace"] }
```

### 3.3 API 端点

| 方法 | 路径 | 响应 | 说明 |
|------|------|------|------|
| GET | `/health` | `{"status": "ok", "version": "0.1.0"}` | 健康检查 |

阶段一仅此一个端点。LLM 代理、Embedding 代理等在后续批次添加。

### 3.4 配置

```rust
// config.rs
pub struct Config {
    pub port: u16,        // env: HOLLOW_PORT, 默认 3000
    pub log_level: String, // env: RUST_LOG, 默认 "info"
}
```

从环境变量读取，不引入额外配置库。后续配置项增多时再考虑 `dotenvy` 或配置文件。

### 3.5 启动流程

1. `Config::from_env()` 读取配置
2. `tracing_subscriber::init()` 初始化日志
3. 构建 `Router`：挂载 `/health` + `TraceLayer`
4. `axum::serve(listener, router)` 启动

### 3.6 测试策略

- 使用 `tower::ServiceExt::oneshot` 直接测试路由，不启动真实 HTTP 服务器
- 验证 `/health` 返回 200 + 正确 JSON body
- 不引入额外测试依赖

---

## 不在本批次范围内

以下内容明确排除，留给后续批次：

- 文件监听（FSEvents）— 第二批
- 文件内容解析（PDF、图片等）— 第二批
- LLM/Embedding API 代理 — 第三批
- 全文索引（FTS5）— 第三批
- 向量索引 — 第三批
- 任何 UI 工作 — 第四批
