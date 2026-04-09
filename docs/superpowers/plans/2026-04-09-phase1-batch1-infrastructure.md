# Phase 1 Batch 1: Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the three foundation modules — SQLite data layer, UniFFI bridge, and hollow-server skeleton — so that subsequent batches (file monitoring, parsing, search) have solid infrastructure to build on.

**Architecture:** hollow-core is a Rust static library linked into the Swift macOS app via UniFFI. It owns SQLite storage and all CRUD operations. hollow-server is an independent Rust binary using Axum, serving as a lightweight API proxy (skeleton only in this batch). The Swift app constructs the database path and calls into hollow-core through auto-generated bindings.

**Tech Stack:** Rust (edition 2024), rusqlite 0.39 (bundled), UniFFI 0.31 (proc-macro), Axum 0.8, Swift 6 / SwiftUI, Xcode 26.

**Spec:** `docs/superpowers/specs/2026-04-09-phase1-batch1-infrastructure-design.md`

---

## File Map

### hollow-core

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `hollow-core/src/error.rs` | `HollowError` enum with UniFFI + thiserror derive |
| Create | `hollow-core/src/db/mod.rs` | `Database` struct — owns `rusqlite::Connection`, init + migration |
| Create | `hollow-core/src/db/schema.rs` | `SCHEMA_VERSION`, migration SQL, `migrate()` |
| Create | `hollow-core/src/db/models.rs` | `FileRecord`, `FileMetadata`, `FileContent`, `OperationLog` structs |
| Create | `hollow-core/src/store/mod.rs` | Re-export `FileStore` |
| Create | `hollow-core/src/store/file_store.rs` | CRUD: `insert_file`, `get_file`, `list_files`, `update_status`, `delete_file`, `check_duplicate` |
| Rewrite | `hollow-core/src/lib.rs` | `HollowCore` UniFFI object — public API wrapping Database + FileStore |
| Modify | `hollow-core/Cargo.toml` | Add dependencies |
| Create | `hollow-core/build.rs` | UniFFI scaffolding generation |

### hollow-server

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `hollow-server/src/config.rs` | `Config` struct from env vars |
| Create | `hollow-server/src/error.rs` | `AppError` implementing `IntoResponse` |
| Create | `hollow-server/src/routes/mod.rs` | `create_router()` function |
| Create | `hollow-server/src/routes/health.rs` | `GET /health` handler |
| Rewrite | `hollow-server/src/main.rs` | Startup: config → tracing → router → serve |
| Modify | `hollow-server/Cargo.toml` | Add dependencies |

### Swift integration

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `hollow/HollowBridge.swift` | Swift wrapper: constructs `db_path`, holds `HollowCore` instance |
| Modify | `hollow/ContentView.swift` | Minimal smoke test: display DB status on launch |

---

## Task 1: hollow-core dependencies and error type

**Files:**
- Modify: `hollow-core/Cargo.toml`
- Create: `hollow-core/src/error.rs`

- [ ] **Step 1: Update Cargo.toml with all dependencies**

```toml
[package]
name = "hollow-core"
version = "0.1.0"
edition = "2024"
description = "Core engine for hollow — semantic file ingestion, understanding, and retrieval"

[lib]
crate-type = ["staticlib", "lib"]

[dependencies]
rusqlite = { version = "0.39", features = ["bundled"] }
uuid = { version = "1", features = ["v7"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uniffi = { version = "0.31", features = ["cli"] }
sha2 = "0.10"
time = { version = "0.3", features = ["formatting"] }

[build-dependencies]
uniffi = { version = "0.31", features = ["build"] }
```

- [ ] **Step 2: Create error.rs**

```rust
// hollow-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum HollowError {
    #[error("database error: {0}")]
    Database(String),
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("duplicate file: {0}")]
    DuplicateFile(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl From<rusqlite::Error> for HollowError {
    fn from(e: rusqlite::Error) -> Self {
        HollowError::Database(e.to_string())
    }
}
```

- [ ] **Step 3: Create a minimal lib.rs that compiles**

Replace `hollow-core/src/lib.rs` with:

```rust
// hollow-core/src/lib.rs
mod error;

pub use error::HollowError;

uniffi::setup_scaffolding!();
```

- [ ] **Step 4: Create build.rs**

```rust
// hollow-core/build.rs
fn main() {
    uniffi::generate_scaffolding("src/lib.rs").unwrap();
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p hollow-core`
Expected: compiles with no errors.

- [ ] **Step 6: Commit**

```bash
git add hollow-core/
git commit -m "feat(hollow-core): add dependencies, error type, and UniFFI scaffolding"
```

---

## Task 2: Database schema and migration

**Files:**
- Create: `hollow-core/src/db/mod.rs`
- Create: `hollow-core/src/db/schema.rs`

- [ ] **Step 1: Write the failing test for schema migration**

Create `hollow-core/src/db/schema.rs`:

```rust
// hollow-core/src/db/schema.rs

pub const SCHEMA_VERSION: u32 = 1;

const MIGRATION_V1: &str = "
CREATE TABLE files (
    id           TEXT PRIMARY KEY,
    hash         TEXT NOT NULL,
    current_path TEXT NOT NULL UNIQUE,
    original_path TEXT NOT NULL,
    file_name    TEXT NOT NULL,
    extension    TEXT,
    mime_type    TEXT,
    size_bytes   INTEGER NOT NULL,
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL,
    ingested_at  TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX idx_files_hash ON files(hash);
CREATE INDEX idx_files_status ON files(status);
CREATE INDEX idx_files_ingested_at ON files(ingested_at);

CREATE TABLE file_metadata (
    file_id        TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    summary        TEXT,
    tags           TEXT,
    category       TEXT,
    sensitivity    TEXT DEFAULT 'normal',
    suggested_name TEXT,
    suggested_path TEXT
);

CREATE TABLE file_content (
    file_id   TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    body_text TEXT,
    ocr_text  TEXT,
    source    TEXT
);

CREATE TABLE operations_log (
    id           TEXT PRIMARY KEY,
    file_id      TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    op_type      TEXT NOT NULL,
    before_state TEXT,
    after_state  TEXT,
    performed_at TEXT NOT NULL
);

CREATE INDEX idx_operations_log_file_time ON operations_log(file_id, performed_at);
";

pub fn migrate(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let current_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current_version < 1 {
        conn.execute_batch(MIGRATION_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_fresh_database() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        migrate(&conn).unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_migrate_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // second call should be a no-op

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_tables_exist_after_migration() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();

        // Verify all four tables exist by querying sqlite_master
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"file_metadata".to_string()));
        assert!(tables.contains(&"file_content".to_string()));
        assert!(tables.contains(&"operations_log".to_string()));
    }
}
```

- [ ] **Step 2: Create db/mod.rs**

```rust
// hollow-core/src/db/mod.rs
pub mod schema;
pub mod models;

use crate::HollowError;

pub struct Database {
    pub(crate) conn: rusqlite::Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self, HollowError> {
        let conn = if path == ":memory:" {
            rusqlite::Connection::open_in_memory()
        } else {
            rusqlite::Connection::open(path)
        }
        .map_err(HollowError::from)?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(HollowError::from)?;
        schema::migrate(&conn).map_err(HollowError::from)?;

        Ok(Database { conn })
    }
}
```

- [ ] **Step 3: Create a placeholder db/models.rs**

```rust
// hollow-core/src/db/models.rs
// Models will be added in Task 3.
```

- [ ] **Step 4: Wire up lib.rs**

Update `hollow-core/src/lib.rs`:

```rust
mod db;
mod error;

pub use error::HollowError;

uniffi::setup_scaffolding!();
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p hollow-core`
Expected: 3 tests pass (`test_migrate_fresh_database`, `test_migrate_idempotent`, `test_tables_exist_after_migration`).

- [ ] **Step 6: Commit**

```bash
git add hollow-core/src/db/
git commit -m "feat(hollow-core): add SQLite schema migration with 4 tables"
```

---

## Task 3: Data models

**Files:**
- Modify: `hollow-core/src/db/models.rs`

- [ ] **Step 1: Define all model structs**

```rust
// hollow-core/src/db/models.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: String,
    pub summary: Option<String>,
    pub tags: Option<Vec<String>>,
    pub category: Option<String>,
    pub sensitivity: String,
    pub suggested_name: Option<String>,
    pub suggested_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub file_id: String,
    pub body_text: Option<String>,
    pub ocr_text: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLog {
    pub id: String,
    pub file_id: String,
    pub op_type: String,
    pub before_state: Option<String>,
    pub after_state: Option<String>,
    pub performed_at: String,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p hollow-core`
Expected: compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/db/models.rs
git commit -m "feat(hollow-core): add data model structs (FileRecord, FileMetadata, FileContent, OperationLog)"
```

---

## Task 4: FileStore — insert and get

**Files:**
- Create: `hollow-core/src/store/mod.rs`
- Create: `hollow-core/src/store/file_store.rs`
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Create store/mod.rs**

```rust
// hollow-core/src/store/mod.rs
pub mod file_store;
pub use file_store::FileStore;
```

- [ ] **Step 2: Write failing tests for insert and get**

Create `hollow-core/src/store/file_store.rs`:

```rust
// hollow-core/src/store/file_store.rs
use rusqlite::Connection;
use crate::db::models::FileRecord;
use crate::HollowError;

pub struct FileStore;

impl FileStore {
    pub fn insert_file(conn: &Connection, record: &FileRecord) -> Result<(), HollowError> {
        conn.execute(
            "INSERT INTO files (id, hash, current_path, original_path, file_name, extension, mime_type, size_bytes, created_at, modified_at, ingested_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                record.id,
                record.hash,
                record.current_path,
                record.original_path,
                record.file_name,
                record.extension,
                record.mime_type,
                record.size_bytes,
                record.created_at,
                record.modified_at,
                record.ingested_at,
                record.status,
            ],
        )?;
        Ok(())
    }

    pub fn get_file(conn: &Connection, id: &str) -> Result<Option<FileRecord>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT id, hash, current_path, original_path, file_name, extension, mime_type, size_bytes, created_at, modified_at, ingested_at, status
             FROM files WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                hash: row.get(1)?,
                current_path: row.get(2)?,
                original_path: row.get(3)?,
                file_name: row.get(4)?,
                extension: row.get(5)?,
                mime_type: row.get(6)?,
                size_bytes: row.get(7)?,
                created_at: row.get(8)?,
                modified_at: row.get(9)?,
                ingested_at: row.get(10)?,
                status: row.get(11)?,
            })
        })?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn test_db() -> Database {
        Database::open(":memory:").unwrap()
    }

    fn sample_record() -> FileRecord {
        FileRecord {
            id: "01961234-5678-7abc-def0-123456789abc".to_string(),
            hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            current_path: "/Users/test/Documents/test.pdf".to_string(),
            original_path: "/Users/test/Downloads/test.pdf".to_string(),
            file_name: "test.pdf".to_string(),
            extension: Some("pdf".to_string()),
            mime_type: Some("application/pdf".to_string()),
            size_bytes: 1024,
            created_at: "2026-04-09T10:00:00Z".to_string(),
            modified_at: "2026-04-09T10:00:00Z".to_string(),
            ingested_at: "2026-04-09T12:00:00Z".to_string(),
            status: "pending".to_string(),
        }
    }

    #[test]
    fn test_insert_and_get_file() {
        let db = test_db();
        let record = sample_record();

        FileStore::insert_file(&db.conn, &record).unwrap();
        let retrieved = FileStore::get_file(&db.conn, &record.id).unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, record.id);
        assert_eq!(retrieved.hash, record.hash);
        assert_eq!(retrieved.current_path, record.current_path);
        assert_eq!(retrieved.file_name, record.file_name);
        assert_eq!(retrieved.size_bytes, record.size_bytes);
    }

    #[test]
    fn test_get_nonexistent_file() {
        let db = test_db();
        let result = FileStore::get_file(&db.conn, "nonexistent-id").unwrap();
        assert!(result.is_none());
    }
}
```

- [ ] **Step 3: Wire up lib.rs**

Update `hollow-core/src/lib.rs`:

```rust
mod db;
mod error;
mod store;

pub use error::HollowError;

uniffi::setup_scaffolding!();
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p hollow-core`
Expected: all tests pass (3 schema + 2 store tests).

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/store/ hollow-core/src/lib.rs
git commit -m "feat(hollow-core): add FileStore with insert and get operations"
```

---

## Task 5: FileStore — list, update_status, delete, check_duplicate

**Files:**
- Modify: `hollow-core/src/store/file_store.rs`

- [ ] **Step 1: Add remaining CRUD methods and tests**

Append to `FileStore` impl in `hollow-core/src/store/file_store.rs`:

```rust
    pub fn list_files(
        conn: &Connection,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<FileRecord>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT id, hash, current_path, original_path, file_name, extension, mime_type, size_bytes, created_at, modified_at, ingested_at, status
             FROM files ORDER BY ingested_at DESC LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                hash: row.get(1)?,
                current_path: row.get(2)?,
                original_path: row.get(3)?,
                file_name: row.get(4)?,
                extension: row.get(5)?,
                mime_type: row.get(6)?,
                size_bytes: row.get(7)?,
                created_at: row.get(8)?,
                modified_at: row.get(9)?,
                ingested_at: row.get(10)?,
                status: row.get(11)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn update_status(
        conn: &Connection,
        id: &str,
        status: &str,
    ) -> Result<(), HollowError> {
        let updated = conn.execute(
            "UPDATE files SET status = ?1 WHERE id = ?2",
            rusqlite::params![status, id],
        )?;
        if updated == 0 {
            return Err(HollowError::FileNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_file(conn: &Connection, id: &str) -> Result<(), HollowError> {
        let deleted = conn.execute(
            "DELETE FROM files WHERE id = ?1",
            rusqlite::params![id],
        )?;
        if deleted == 0 {
            return Err(HollowError::FileNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn check_duplicate(conn: &Connection, hash: &str) -> Result<bool, HollowError> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE hash = ?1",
            rusqlite::params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
```

Append to `mod tests`:

```rust
    #[test]
    fn test_list_files_with_pagination() {
        let db = test_db();
        let mut r1 = sample_record();
        r1.id = "id-001".to_string();
        r1.current_path = "/path/a.pdf".to_string();
        r1.ingested_at = "2026-04-09T12:00:00Z".to_string();

        let mut r2 = sample_record();
        r2.id = "id-002".to_string();
        r2.current_path = "/path/b.pdf".to_string();
        r2.ingested_at = "2026-04-09T13:00:00Z".to_string();

        FileStore::insert_file(&db.conn, &r1).unwrap();
        FileStore::insert_file(&db.conn, &r2).unwrap();

        let all = FileStore::list_files(&db.conn, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
        // Most recent first
        assert_eq!(all[0].id, "id-002");

        let page = FileStore::list_files(&db.conn, 1, 0).unwrap();
        assert_eq!(page.len(), 1);

        let page2 = FileStore::list_files(&db.conn, 1, 1).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].id, "id-001");
    }

    #[test]
    fn test_update_status() {
        let db = test_db();
        let record = sample_record();
        FileStore::insert_file(&db.conn, &record).unwrap();

        FileStore::update_status(&db.conn, &record.id, "indexed").unwrap();

        let updated = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
        assert_eq!(updated.status, "indexed");
    }

    #[test]
    fn test_update_status_nonexistent() {
        let db = test_db();
        let result = FileStore::update_status(&db.conn, "nonexistent", "indexed");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_file() {
        let db = test_db();
        let record = sample_record();
        FileStore::insert_file(&db.conn, &record).unwrap();

        FileStore::delete_file(&db.conn, &record.id).unwrap();

        let result = FileStore::get_file(&db.conn, &record.id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_check_duplicate() {
        let db = test_db();
        let record = sample_record();

        assert!(!FileStore::check_duplicate(&db.conn, &record.hash).unwrap());

        FileStore::insert_file(&db.conn, &record).unwrap();

        assert!(FileStore::check_duplicate(&db.conn, &record.hash).unwrap());
    }

    #[test]
    fn test_same_hash_different_paths() {
        let db = test_db();
        let mut r1 = sample_record();
        r1.id = "id-aaa".to_string();
        r1.current_path = "/path/copy1.pdf".to_string();

        let mut r2 = sample_record();
        r2.id = "id-bbb".to_string();
        r2.current_path = "/path/copy2.pdf".to_string();
        // same hash as r1

        FileStore::insert_file(&db.conn, &r1).unwrap();
        FileStore::insert_file(&db.conn, &r2).unwrap();

        let all = FileStore::list_files(&db.conn, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core`
Expected: all tests pass (3 schema + 8 store tests).

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/store/file_store.rs
git commit -m "feat(hollow-core): add list, update_status, delete, check_duplicate to FileStore"
```

---

## Task 6: HollowCore public API with UniFFI export

**Files:**
- Rewrite: `hollow-core/src/lib.rs`

- [ ] **Step 1: Implement HollowCore as UniFFI object**

```rust
// hollow-core/src/lib.rs
mod db;
mod error;
mod store;

pub use db::models::FileRecord;
pub use error::HollowError;

use db::Database;
use store::FileStore;

use sha2::{Sha256, Digest};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

uniffi::setup_scaffolding!();

#[derive(uniffi::Object)]
pub struct HollowCore {
    db: Mutex<Database>,
}

#[uniffi::export]
impl HollowCore {
    #[uniffi::constructor]
    pub fn new(db_path: String) -> Result<Self, HollowError> {
        let db = Database::open(&db_path)?;
        Ok(HollowCore { db: Mutex::new(db) })
    }

    pub fn ingest_file(&self, file_path: String) -> Result<FileRecord, HollowError> {
        let path = Path::new(&file_path);
        if !path.exists() {
            return Err(HollowError::FileNotFound(file_path.clone()));
        }

        let metadata = fs::metadata(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let content = fs::read(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = format!("{:x}", hasher.finalize());

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_string());

        let now = iso8601_now();

        let record = FileRecord {
            id: Uuid::now_v7().to_string(),
            hash,
            current_path: file_path.clone(),
            original_path: file_path,
            file_name,
            extension,
            mime_type: None,
            size_bytes: metadata.len() as i64,
            created_at: now.clone(),
            modified_at: now.clone(),
            ingested_at: now,
            status: "pending".to_string(),
        };

        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::insert_file(&db.conn, &record)?;
        Ok(record)
    }

    pub fn get_file(&self, id: String) -> Result<Option<FileRecord>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::get_file(&db.conn, &id)
    }

    pub fn list_files(&self, limit: u32, offset: u32) -> Result<Vec<FileRecord>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::list_files(&db.conn, limit, offset)
    }

    pub fn check_duplicate(&self, hash: String) -> Result<bool, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::check_duplicate(&db.conn, &hash)
    }
}

fn iso8601_now() -> String {
    // ISO 8601 UTC timestamp using the `time` crate
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_hollow_core_ingest_and_get() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();

        // Create a temp file
        let dir = std::env::temp_dir().join("hollow_test");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test_ingest.txt");
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(b"hello hollow").unwrap();

        let record = core
            .ingest_file(file_path.to_string_lossy().to_string())
            .unwrap();
        assert_eq!(record.file_name, "test_ingest.txt");
        assert_eq!(record.extension, Some("txt".to_string()));
        assert_eq!(record.status, "pending");
        assert_eq!(record.size_bytes, 12);

        let retrieved = core.get_file(record.id.clone()).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().hash, record.hash);

        // Clean up
        fs::remove_file(&file_path).ok();
        fs::remove_dir(&dir).ok();
    }

    #[test]
    fn test_hollow_core_list_files() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();

        let dir = std::env::temp_dir().join("hollow_test_list");
        fs::create_dir_all(&dir).unwrap();

        let f1_path = dir.join("a.txt");
        fs::write(&f1_path, b"aaa").unwrap();
        let f2_path = dir.join("b.txt");
        fs::write(&f2_path, b"bbb").unwrap();

        core.ingest_file(f1_path.to_string_lossy().to_string()).unwrap();
        core.ingest_file(f2_path.to_string_lossy().to_string()).unwrap();

        let files = core.list_files(10, 0).unwrap();
        assert_eq!(files.len(), 2);

        // Clean up
        fs::remove_file(&f1_path).ok();
        fs::remove_file(&f2_path).ok();
        fs::remove_dir(&dir).ok();
    }

    #[test]
    fn test_hollow_core_check_duplicate() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();

        let dir = std::env::temp_dir().join("hollow_test_dup");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("dup_test.txt");
        fs::write(&file_path, b"duplicate content").unwrap();

        let record = core
            .ingest_file(file_path.to_string_lossy().to_string())
            .unwrap();

        assert!(core.check_duplicate(record.hash.clone()).unwrap());
        assert!(!core.check_duplicate("nonexistent_hash".to_string()).unwrap());

        // Clean up
        fs::remove_file(&file_path).ok();
        fs::remove_dir(&dir).ok();
    }

    #[test]
    fn test_hollow_core_ingest_nonexistent_file() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let result = core.ingest_file("/nonexistent/path/file.txt".to_string());
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core`
Expected: all tests pass (3 schema + 8 store + 4 lib tests).

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/lib.rs
git commit -m "feat(hollow-core): add HollowCore public API with UniFFI export"
```

---

## Task 7: hollow-server skeleton

**Files:**
- Modify: `hollow-server/Cargo.toml`
- Create: `hollow-server/src/config.rs`
- Create: `hollow-server/src/error.rs`
- Create: `hollow-server/src/routes/mod.rs`
- Create: `hollow-server/src/routes/health.rs`
- Rewrite: `hollow-server/src/main.rs`

- [ ] **Step 1: Update Cargo.toml**

```toml
[package]
name = "hollow-server"
version = "0.1.0"
edition = "2024"
description = "Cloud server for hollow — lightweight API proxy for LLM/Embedding services"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tower-http = { version = "0.6", features = ["trace"] }
tower = "0.5"

[dev-dependencies]
http-body-util = "0.1"
```

- [ ] **Step 2: Create config.rs**

```rust
// hollow-server/src/config.rs

pub struct Config {
    pub port: u16,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            port: std::env::var("HOLLOW_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            log_level: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        // Clear env vars to test defaults
        std::env::remove_var("HOLLOW_PORT");
        let config = Config::from_env();
        assert_eq!(config.port, 3000);
        assert!(!config.log_level.is_empty());
    }
}
```

- [ ] **Step 3: Create error.rs**

```rust
// hollow-server/src/error.rs
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.message,
        });
        (self.status, axum::Json(body)).into_response()
    }
}
```

- [ ] **Step 4: Create routes/health.rs**

```rust
// hollow-server/src/routes/health.rs
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
```

- [ ] **Step 5: Create routes/mod.rs**

```rust
// hollow-server/src/routes/mod.rs
pub mod health;

use axum::{routing::get, Router};

pub fn create_router() -> Router {
    Router::new().route("/health", get(health::health))
}
```

- [ ] **Step 6: Rewrite main.rs**

```rust
// hollow-server/src/main.rs
mod config;
mod error;
mod routes;

use config::Config;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let config = Config::from_env();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_level))
        .init();

    let app = routes::create_router().layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port))
        .await
        .expect("failed to bind port");

    tracing::info!("hollow-server listening on port {}", config.port);

    axum::serve(listener, app).await.expect("server error");
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo build -p hollow-server`
Expected: compiles with no errors.

- [ ] **Step 8: Commit**

```bash
git add hollow-server/
git commit -m "feat(hollow-server): add Axum skeleton with /health endpoint, config, and tracing"
```

---

## Task 8: hollow-server integration test

**Files:**
- Modify: `hollow-server/src/routes/health.rs` (add test)

- [ ] **Step 1: Add integration test for /health**

Append to `hollow-server/src/routes/health.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::create_router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_router();

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "0.1.0");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-server`
Expected: 2 tests pass (`test_default_config`, `test_health_endpoint`).

- [ ] **Step 3: Commit**

```bash
git add hollow-server/src/routes/health.rs
git commit -m "test(hollow-server): add /health endpoint integration test"
```

---

## Task 9: UniFFI binding generation and Swift bridge

**Files:**
- Create: `hollow/HollowBridge.swift`
- Modify: `hollow/ContentView.swift`

**Prerequisites:** This task requires `cargo build -p hollow-core` to have run successfully, producing the static library and UniFFI bindings.

- [ ] **Step 1: Generate UniFFI bindings**

Run:
```bash
cargo build -p hollow-core
cargo run -p hollow-core --features uniffi/cli -- generate --library target/debug/libhollow_core.a --language swift --out-dir generated/
```

If the `uniffi/cli` approach doesn't work with proc-macro mode, use the uniffi-bindgen CLI:

```bash
cargo install uniffi-bindgen-cli --version 0.31.0
uniffi-bindgen generate --library target/debug/libhollow_core.dylib --language swift --out-dir generated/
```

Expected: `generated/` directory contains `hollow_core.swift` and `hollow_coreFFI.h`.

- [ ] **Step 2: Create the Swift bridge wrapper**

```swift
// hollow/HollowBridge.swift
import Foundation

/// Swift-side wrapper that manages the HollowCore lifecycle.
/// Constructs the database path in Application Support and holds
/// a reference to the Rust-backed HollowCore instance.
class HollowBridge {
    static let shared = HollowBridge()

    private var core: HollowCore?

    var isReady: Bool { core != nil }

    private init() {
        do {
            let dbPath = try Self.databasePath()
            core = try HollowCore(dbPath: dbPath)
        } catch {
            print("HollowBridge init failed: \(error)")
            core = nil
        }
    }

    private static func databasePath() throws -> String {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!

        let hollowDir = appSupport.appendingPathComponent(
            "com.syncpulse.hollow",
            isDirectory: true
        )

        try FileManager.default.createDirectory(
            at: hollowDir,
            withIntermediateDirectories: true
        )

        return hollowDir.appendingPathComponent("hollow.db").path
    }

    func listFiles(limit: UInt32 = 20, offset: UInt32 = 0) -> [FileRecord] {
        guard let core else { return [] }
        do {
            return try core.listFiles(limit: limit, offset: offset)
        } catch {
            print("listFiles failed: \(error)")
            return []
        }
    }
}
```

- [ ] **Step 3: Update ContentView for smoke test**

```swift
// hollow/ContentView.swift
import SwiftUI

struct ContentView: View {
    @State private var status: String = "Initializing..."

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "archivebox")
                .imageScale(.large)
                .foregroundStyle(.tint)
            Text("hollow")
                .font(.title)
            Text(status)
                .foregroundStyle(.secondary)
        }
        .padding()
        .task {
            if HollowBridge.shared.isReady {
                let count = HollowBridge.shared.listFiles().count
                status = "Database ready. \(count) files indexed."
            } else {
                status = "Failed to initialize database."
            }
        }
    }
}

#Preview {
    ContentView()
}
```

- [ ] **Step 4: Configure Xcode project**

This step requires manual Xcode configuration:

1. Copy `generated/hollow_core.swift` into the `hollow/` source group in Xcode
2. Copy `generated/hollow_coreFFI.h` into the project
3. Create or update the bridging header (`hollow/hollow-Bridging-Header.h`):
   ```c
   #import "hollow_coreFFI.h"
   ```
4. In Build Settings, set "Objective-C Bridging Header" to `hollow/hollow-Bridging-Header.h`
5. Add `target/debug/libhollow_core.a` to "Link Binary With Libraries" in Build Phases
6. Add `target/debug/` to "Library Search Paths" in Build Settings

- [ ] **Step 5: Build and run**

Build the Xcode project. Expected: app launches, displays "Database ready. 0 files indexed."

If linking fails, check:
- Library Search Paths includes the correct cargo target directory
- Bridging header path is correct
- The static library architecture matches (arm64 for Apple Silicon)

For Apple Silicon: `cargo build -p hollow-core --target aarch64-apple-darwin`

- [ ] **Step 6: Commit**

```bash
git add hollow/HollowBridge.swift hollow/ContentView.swift generated/
git commit -m "feat: add Swift bridge and Xcode integration for hollow-core"
```

---

## Task 10: Full verification

- [ ] **Step 1: Run all Rust tests**

Run: `cargo test`
Expected: all hollow-core tests (15) and hollow-server tests (2) pass.

- [ ] **Step 2: Run Xcode build**

Run:
```bash
xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build
```
Expected: build succeeds.

- [ ] **Step 3: Run hollow-server manually**

Run: `RUST_LOG=debug cargo run -p hollow-server`
Expected: logs show "hollow-server listening on port 3000".

In another terminal:
```bash
curl http://localhost:3000/health
```
Expected: `{"status":"ok","version":"0.1.0"}`

- [ ] **Step 4: Final commit (if any remaining changes)**

```bash
git status
# If there are unstaged changes, commit them
```
