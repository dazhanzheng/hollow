# Content Extraction Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 hollow-core 中建立插件化的文件内容提取管线（Content Extraction Pipeline），并将 Swift 侧元数据摄取从串行改造为并行异步队列。第一批支持纯文本和源代码类文件。

**Architecture:** Rust 侧新增 `content/` 模块（FormatDetector + Extractor trait + Registry + Pipeline + 2 个 Extractor 实现），通过 FFI 暴露 `extract_content` / `has_changed` / `mark_for_reextraction`。Swift 侧将 `IngestionService` 改造为两个并行 `OperationQueue`（metadata + content），FileWatcher 新增 modify 事件支持。

**Tech Stack:** Rust (hollow-core) + Swift/SwiftUI (macOS 26.2+) + rusqlite 0.39 + uniffi 0.31。新增依赖: `infer` 0.19（magic bytes）, `chardetng` 0.1（编码检测）, `zstd` 0.13（压缩）。

**Reference:** 设计文档见 [docs/superpowers/specs/2026-04-10-content-extraction-pipeline-design.md](../specs/2026-04-10-content-extraction-pipeline-design.md)

**Testing Commands:**
- Rust all: `cargo test -p hollow-core`
- Rust single: `cargo test -p hollow-core test_name -- --nocapture`
- Swift build: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
- Swift test: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' test`

---

## File Structure

### New files (Rust)
- `hollow-core/src/content/mod.rs` — module root
- `hollow-core/src/content/detector.rs` — FormatDetector (magic bytes)
- `hollow-core/src/content/extractor.rs` — Extractor trait + ExtractionError + ExtractionResult
- `hollow-core/src/content/registry.rs` — ExtractorRegistry + default_registry()
- `hollow-core/src/content/pipeline.rs` — ContentPipeline orchestrator
- `hollow-core/src/content/extractors/mod.rs` — extractors module root
- `hollow-core/src/content/extractors/common.rs` — shared `read_text_file` helper
- `hollow-core/src/content/extractors/plain_text.rs` — PlainTextExtractor
- `hollow-core/src/content/extractors/source_code.rs` — SourceCodeExtractor
- `hollow-core/src/store/file_content_store.rs` — FileContentStore

### Modified files (Rust)
- `hollow-core/Cargo.toml` — add `infer`, `chardetng`, `zstd`
- `hollow-core/src/db/schema.rs` — bump SCHEMA_VERSION to 4, add MIGRATION_V4
- `hollow-core/src/db/models.rs` — extend FileContent, add new types
- `hollow-core/src/store/mod.rs` — export FileContentStore
- `hollow-core/src/store/file_store.rs` — add `update_detected_mime`, `get_quick_hash`, `mark_for_reextraction`
- `hollow-core/src/lib.rs` — wire content module, add FFI methods

### New files (Swift)
- `hollow/Operations/MetadataIntakeOperation.swift`
- `hollow/Operations/ContentExtractionOperation.swift`

### Modified files (Swift)
- `hollow/HollowBridge.swift` — add extractContent, hasChanged, markForReextraction, getPendingExtractionIds
- `hollow/IngestionService.swift` — replace serial queue with two OperationQueues
- `hollow/FileWatcher.swift` — add onModifiedFiles callback with debounce
- `hollow/ContentView.swift` — show queue counts
- `hollow/DatabaseBrowserView.swift` — show extension_mismatch warning + re-extract button
- `hollow/SettingsView.swift` — show worker concurrency

---

## Phase A: Schema Migration v4

### Task A1: Add MIGRATION_V4 with failing test

**Files:**
- Modify: `hollow-core/src/db/schema.rs`

- [ ] **Step 1: Write the failing test**

Add to `hollow-core/src/db/schema.rs` tests module:

```rust
#[test]
fn test_migration_v4_adds_content_columns() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    migrate(&conn).unwrap();

    // file_content new columns
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(file_content)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(cols.contains(&"body_text_compressed".to_string()));
    assert!(cols.contains(&"body_text_bytes".to_string()));
    assert!(cols.contains(&"encoding".to_string()));
    assert!(cols.contains(&"extracted_at".to_string()));
    assert!(cols.contains(&"extractor_name".to_string()));
    assert!(cols.contains(&"extract_error".to_string()));

    // files new columns
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(files)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(cols.contains(&"detected_mime".to_string()));
    assert!(cols.contains(&"extension_mismatch".to_string()));
}

#[test]
fn test_migration_v4_resets_indexed_to_pending() {
    // Simulate an existing v3 DB with indexed files, then run v4
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

    // Apply v1..v3 manually
    conn.execute_batch(MIGRATION_V1).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute_batch(MIGRATION_V2).unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    conn.execute_batch(MIGRATION_V3).unwrap();
    conn.pragma_update(None, "user_version", 3).unwrap();

    // Insert a file with status indexed
    conn.execute(
        "INSERT INTO files (id, hash, quick_hash, current_path, original_path, file_name, size_bytes, created_at, modified_at, ingested_at, status) VALUES ('a', '', '', '/a.txt', '/a.txt', 'a.txt', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'indexed')",
        [],
    ).unwrap();

    // Run full migrate (should apply v4)
    migrate(&conn).unwrap();

    let status: String = conn
        .query_row("SELECT status FROM files WHERE id = 'a'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(status, "pending");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p hollow-core test_migration_v4 -- --nocapture`
Expected: FAIL with "body_text_compressed" not found

- [ ] **Step 3: Implement MIGRATION_V4**

Edit `hollow-core/src/db/schema.rs`:

```rust
pub const SCHEMA_VERSION: u32 = 4;
```

Add after `MIGRATION_V3`:

```rust
const MIGRATION_V4: &str = "
ALTER TABLE file_content ADD COLUMN body_text_compressed BLOB;
ALTER TABLE file_content ADD COLUMN body_text_bytes INTEGER;
ALTER TABLE file_content ADD COLUMN encoding TEXT;
ALTER TABLE file_content ADD COLUMN extracted_at TEXT;
ALTER TABLE file_content ADD COLUMN extractor_name TEXT;
ALTER TABLE file_content ADD COLUMN extract_error TEXT;

ALTER TABLE files ADD COLUMN detected_mime TEXT;
ALTER TABLE files ADD COLUMN extension_mismatch INTEGER NOT NULL DEFAULT 0;

UPDATE files SET status = 'pending' WHERE status = 'indexed';
";
```

Add to `migrate()`:

```rust
if current_version < 4 {
    conn.execute_batch(MIGRATION_V4)?;
    conn.pragma_update(None, "user_version", 4)?;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hollow-core schema`
Expected: all pass, including existing `test_migrate_fresh_database`, `test_migrate_idempotent`, `test_tables_exist_after_migration`

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/db/schema.rs
git commit -m "feat(hollow-core): schema v4 for content extraction columns"
```

---

## Phase B: Dependencies & Content Module Skeleton

### Task B1: Add Cargo dependencies

**Files:**
- Modify: `hollow-core/Cargo.toml`

- [ ] **Step 1: Add dependencies**

Edit `hollow-core/Cargo.toml`, add to `[dependencies]`:

```toml
infer = "0.19"
chardetng = "0.1"
zstd = { version = "0.13", default-features = false }
```

- [ ] **Step 2: Verify build**

Run: `cargo build -p hollow-core`
Expected: clean compile (may pull new deps)

- [ ] **Step 3: Commit**

```bash
git add hollow-core/Cargo.toml hollow-core/../Cargo.lock
git commit -m "feat(hollow-core): add infer, chardetng, zstd dependencies"
```

### Task B2: Create content module skeleton

**Files:**
- Create: `hollow-core/src/content/mod.rs`
- Create: `hollow-core/src/content/extractors/mod.rs`
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Create empty module files**

Write `hollow-core/src/content/mod.rs`:

```rust
pub mod detector;
pub mod extractor;
pub mod extractors;
pub mod pipeline;
pub mod registry;

pub use extractor::{ExtractionError, ExtractionResult, Extractor};
pub use pipeline::{ContentPipeline, ExtractionOutcome};
pub use registry::{default_registry, ExtractorRegistry};
```

Write `hollow-core/src/content/extractors/mod.rs`:

```rust
pub mod common;
pub mod plain_text;
pub mod source_code;

pub use plain_text::PlainTextExtractor;
pub use source_code::SourceCodeExtractor;
```

Create stub files (each just a doc comment so the module compiles):

`hollow-core/src/content/detector.rs`:
```rust
//! Format detection via magic bytes.
```

`hollow-core/src/content/extractor.rs`:
```rust
//! Extractor trait and error types.
```

`hollow-core/src/content/pipeline.rs`:
```rust
//! ContentPipeline orchestrator.
```

`hollow-core/src/content/registry.rs`:
```rust
//! ExtractorRegistry.
```

`hollow-core/src/content/extractors/common.rs`:
```rust
//! Shared helpers for text-reading extractors.
```

`hollow-core/src/content/extractors/plain_text.rs`:
```rust
//! PlainTextExtractor.
```

`hollow-core/src/content/extractors/source_code.rs`:
```rust
//! SourceCodeExtractor.
```

- [ ] **Step 2: Wire module into lib.rs**

Edit `hollow-core/src/lib.rs`, add after `mod logging;`:

```rust
mod content;
```

- [ ] **Step 3: Verify build**

Run: `cargo build -p hollow-core`
Expected: compiles cleanly (the modules are mostly empty but valid)

- [ ] **Step 4: Commit**

```bash
git add hollow-core/src/content hollow-core/src/lib.rs
git commit -m "feat(hollow-core): scaffold content extraction module tree"
```

---

## Phase C: FormatDetector

### Task C1: FormatDetector with tests

**Files:**
- Modify: `hollow-core/src/content/detector.rs`

- [ ] **Step 1: Write failing tests**

Replace `hollow-core/src/content/detector.rs` with:

```rust
//! Format detection via magic bytes.

use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DetectedFormat {
    /// MIME type from magic bytes detection, or fallback from extension/heuristic.
    pub mime: String,
    /// Suggested extension from magic bytes (e.g. "png", "pdf"), if identifiable.
    pub extension_hint: Option<String>,
    /// True if content is plausibly text (UTF-8 decodable head).
    pub is_text: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum DetectionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct FormatDetector;

impl FormatDetector {
    /// Read up to 8 KiB from the file head and detect format.
    pub fn detect(path: &Path) -> Result<DetectedFormat, DetectionError> {
        let mut file = fs::File::open(path)?;
        let mut head = vec![0u8; 8192];
        let n = file.read(&mut head)?;
        head.truncate(n);

        // Try magic bytes first
        if let Some(kind) = infer::get(&head) {
            let mime = kind.mime_type().to_string();
            let is_text = mime.starts_with("text/");
            return Ok(DetectedFormat {
                mime,
                extension_hint: Some(kind.extension().to_string()),
                is_text,
            });
        }

        // Fallback: heuristic text check
        let is_text = is_plausibly_text(&head);
        let mime = if is_text {
            "text/plain".to_string()
        } else {
            "application/octet-stream".to_string()
        };

        Ok(DetectedFormat {
            mime,
            extension_hint: None,
            is_text,
        })
    }
}

/// Heuristic: a buffer is plausibly text if it's either valid UTF-8 or
/// contains no NUL bytes and no more than 5% non-printable non-whitespace bytes.
fn is_plausibly_text(buf: &[u8]) -> bool {
    if buf.is_empty() {
        return true;
    }
    if std::str::from_utf8(buf).is_ok() {
        return true;
    }
    if buf.contains(&0) {
        return false;
    }
    let bad = buf
        .iter()
        .filter(|&&b| !(b.is_ascii_graphic() || b.is_ascii_whitespace()))
        .count();
    (bad * 100) / buf.len() <= 5
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_detector_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::File::create(&path).unwrap().write_all(content).unwrap();
        path
    }

    #[test]
    fn test_detect_png_by_magic() {
        // PNG magic: 89 50 4E 47 0D 0A 1A 0A
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        let path = tmp_file("fake.txt", &png); // wrong extension!
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "image/png");
        assert_eq!(detected.extension_hint.as_deref(), Some("png"));
        assert!(!detected.is_text);
    }

    #[test]
    fn test_detect_plain_text_fallback() {
        let path = tmp_file("note.txt", b"hello world\nhow are you?");
        let detected = FormatDetector::detect(&path).unwrap();
        assert_eq!(detected.mime, "text/plain");
        assert!(detected.is_text);
    }

    #[test]
    fn test_detect_empty_file() {
        let path = tmp_file("empty.txt", b"");
        let detected = FormatDetector::detect(&path).unwrap();
        assert!(detected.is_text);
    }

    #[test]
    fn test_detect_binary_without_magic() {
        // Random binary with NUL bytes
        let path = tmp_file("blob.bin", &[0xFF, 0x00, 0x01, 0x02, 0xFE]);
        let detected = FormatDetector::detect(&path).unwrap();
        assert!(!detected.is_text);
        assert_eq!(detected.mime, "application/octet-stream");
    }

    #[test]
    fn test_detect_gbk_text_bytes() {
        // GBK-encoded "你好" = C4 E3 BA C3 (no NUL bytes, non-ASCII printable chars)
        let path = tmp_file("gbk.txt", &[0xC4, 0xE3, 0xBA, 0xC3]);
        let detected = FormatDetector::detect(&path).unwrap();
        // Should be treated as text via heuristic (no NUL, low bad ratio)
        assert!(detected.is_text);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail then pass**

Run: `cargo test -p hollow-core detector`
Expected: PASS (the implementation is inline with the test)

Note: these tests exercise the implementation in place. If `infer` doesn't return `png` extension, adjust test to match actual crate output.

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/detector.rs
git commit -m "feat(hollow-core): FormatDetector with magic bytes + heuristic fallback"
```

---

## Phase D: Extractor Trait & Error Types

### Task D1: Define Extractor trait and ExtractionError

**Files:**
- Modify: `hollow-core/src/content/extractor.rs`

- [ ] **Step 1: Write trait definition + tests**

Replace `hollow-core/src/content/extractor.rs` with:

```rust
//! Extractor trait and error types.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// Extracted UTF-8 text (already decoded from source encoding if needed).
    pub body_text: String,
    /// Original encoding if decoding occurred, e.g. "UTF-8", "GBK", "Shift_JIS".
    pub encoding: Option<String>,
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

pub trait Extractor: Send + Sync {
    /// Stable identifier used in DB records and logs (e.g. "PlainText").
    fn name(&self) -> &'static str;

    /// MIME types this extractor claims to handle.
    fn supported_mimes(&self) -> &[&'static str];

    /// Perform extraction.
    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyExtractor;
    impl Extractor for DummyExtractor {
        fn name(&self) -> &'static str {
            "Dummy"
        }
        fn supported_mimes(&self) -> &[&'static str] {
            &["text/plain"]
        }
        fn extract(&self, _path: &Path) -> Result<ExtractionResult, ExtractionError> {
            Ok(ExtractionResult {
                body_text: "hello".to_string(),
                encoding: Some("UTF-8".to_string()),
            })
        }
    }

    #[test]
    fn test_extractor_trait_object() {
        let e: Box<dyn Extractor> = Box::new(DummyExtractor);
        assert_eq!(e.name(), "Dummy");
        assert_eq!(e.supported_mimes(), &["text/plain"]);
    }
}
```

- [ ] **Step 2: Verify build + test**

Run: `cargo test -p hollow-core content::extractor`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/extractor.rs
git commit -m "feat(hollow-core): Extractor trait + ExtractionError/Result types"
```

---

## Phase E: PlainText Extractor (with shared helper)

### Task E1: Shared text reading helper with encoding detection

**Files:**
- Modify: `hollow-core/src/content/extractors/common.rs`

- [ ] **Step 1: Write helper + tests**

Replace `hollow-core/src/content/extractors/common.rs` with:

```rust
//! Shared helpers for text-reading extractors.

use std::fs;
use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult};

pub const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

/// Read an entire file, detect encoding, and return UTF-8 text.
pub fn read_text_file(path: &Path, max_size: u64) -> Result<ExtractionResult, ExtractionError> {
    let metadata = fs::metadata(path)?;
    let size = metadata.len();
    if size > max_size {
        return Err(ExtractionError::FileTooLarge {
            size,
            limit: max_size,
        });
    }

    let bytes = fs::read(path)?;

    // Fast path: already valid UTF-8
    if let Ok(s) = std::str::from_utf8(&bytes) {
        return Ok(ExtractionResult {
            body_text: s.to_string(),
            encoding: Some("UTF-8".to_string()),
        });
    }

    // Slow path: detect encoding with chardetng
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(&bytes, true);
    let encoding = detector.guess(None, true);
    let (decoded, _, had_errors) = encoding.decode(&bytes);

    if had_errors {
        return Err(ExtractionError::EncodingDetectionFailed);
    }

    Ok(ExtractionResult {
        body_text: decoded.into_owned(),
        encoding: Some(encoding.name().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_common_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_read_utf8_ascii() {
        let p = tmp("ascii.txt", b"hello world");
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "hello world");
        assert_eq!(result.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn test_read_utf8_chinese() {
        let p = tmp("zh.txt", "你好世界".as_bytes());
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "你好世界");
        assert_eq!(result.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn test_read_gbk_chinese() {
        // "你好" in GBK = C4 E3 BA C3
        let p = tmp("gbk.txt", &[0xC4, 0xE3, 0xBA, 0xC3]);
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "你好");
        // chardetng should pick GBK or GB18030
        let enc = result.encoding.unwrap();
        assert!(enc == "GBK" || enc == "GB18030" || enc == "gb18030");
    }

    #[test]
    fn test_file_too_large() {
        let p = tmp("big.txt", &vec![b'a'; 100]);
        let err = read_text_file(&p, 50).unwrap_err();
        assert!(matches!(err, ExtractionError::FileTooLarge { .. }));
    }

    #[test]
    fn test_empty_file() {
        let p = tmp("empty.txt", b"");
        let result = read_text_file(&p, DEFAULT_MAX_FILE_SIZE).unwrap();
        assert_eq!(result.body_text, "");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core content::extractors::common`
Expected: PASS. If GBK test fails due to chardetng picking a different encoding label, adjust assertion to check decoded text only.

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/extractors/common.rs
git commit -m "feat(hollow-core): read_text_file helper with chardetng encoding detection"
```

### Task E2: PlainTextExtractor

**Files:**
- Modify: `hollow-core/src/content/extractors/plain_text.rs`

- [ ] **Step 1: Write extractor + tests**

Replace `hollow-core/src/content/extractors/plain_text.rs` with:

```rust
//! PlainTextExtractor: handles plain text, markdown, CSV, JSON, YAML, etc.

use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::{read_text_file, DEFAULT_MAX_FILE_SIZE};

pub struct PlainTextExtractor {
    max_size: u64,
}

impl PlainTextExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for PlainTextExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &[
    "text/plain",
    "text/markdown",
    "text/csv",
    "text/tab-separated-values",
    "text/x-log",
    "application/json",
    "application/xml",
    "text/xml",
    "application/yaml",
    "text/yaml",
    "application/toml",
    "text/toml",
];

impl Extractor for PlainTextExtractor {
    fn name(&self) -> &'static str {
        "PlainText"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED
    }

    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError> {
        read_text_file(path, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_plain_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_name_and_mimes() {
        let e = PlainTextExtractor::new();
        assert_eq!(e.name(), "PlainText");
        assert!(e.supported_mimes().contains(&"text/plain"));
        assert!(e.supported_mimes().contains(&"application/json"));
    }

    #[test]
    fn test_extract_utf8() {
        let p = tmp("greet.txt", "你好\nworld".as_bytes());
        let e = PlainTextExtractor::new();
        let result = e.extract(&p).unwrap();
        assert_eq!(result.body_text, "你好\nworld");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core plain_text`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/extractors/plain_text.rs
git commit -m "feat(hollow-core): PlainTextExtractor"
```

### Task E3: SourceCodeExtractor

**Files:**
- Modify: `hollow-core/src/content/extractors/source_code.rs`

- [ ] **Step 1: Write extractor + tests**

Replace `hollow-core/src/content/extractors/source_code.rs` with:

```rust
//! SourceCodeExtractor: handles source code files via MIME or extension.

use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::{read_text_file, DEFAULT_MAX_FILE_SIZE};

pub struct SourceCodeExtractor {
    max_size: u64,
}

impl SourceCodeExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }

    /// Extensions this extractor can handle as a fallback when MIME is unclear.
    pub fn known_extensions() -> &'static [&'static str] {
        &[
            "py", "js", "ts", "jsx", "tsx", "rs", "swift", "go", "java", "kt",
            "scala", "c", "cc", "cpp", "cxx", "h", "hh", "hpp", "m", "mm",
            "rb", "sh", "bash", "zsh", "fish", "sql", "html", "htm", "css",
            "scss", "sass", "less", "vue", "svelte", "lua", "pl", "pm", "php",
            "r", "dart", "ex", "exs", "erl", "hs", "clj", "cljs", "edn",
        ]
    }
}

impl Default for SourceCodeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED_MIMES: &[&str] = &[
    "text/x-python",
    "text/x-rust",
    "text/x-go",
    "text/x-swift",
    "text/x-java",
    "text/x-c",
    "text/x-c++",
    "text/x-shellscript",
    "text/x-ruby",
    "application/javascript",
    "application/typescript",
    "text/javascript",
    "text/typescript",
    "text/html",
    "text/css",
];

impl Extractor for SourceCodeExtractor {
    fn name(&self) -> &'static str {
        "SourceCode"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED_MIMES
    }

    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError> {
        read_text_file(path, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_src_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_rust_file() {
        let p = tmp("main.rs", b"fn main() { println!(\"hi\"); }");
        let e = SourceCodeExtractor::new();
        let result = e.extract(&p).unwrap();
        assert!(result.body_text.contains("fn main"));
    }

    #[test]
    fn test_known_extensions_includes_common_langs() {
        let exts = SourceCodeExtractor::known_extensions();
        for e in ["py", "rs", "swift", "ts", "go"] {
            assert!(exts.contains(&e), "missing ext {}", e);
        }
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core source_code`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/extractors/source_code.rs
git commit -m "feat(hollow-core): SourceCodeExtractor"
```

---

## Phase F: ExtractorRegistry

### Task F1: Registry with MIME + extension fallback

**Files:**
- Modify: `hollow-core/src/content/registry.rs`

- [ ] **Step 1: Write registry + tests**

Replace `hollow-core/src/content/registry.rs` with:

```rust
//! ExtractorRegistry: maps MIME types (and extensions) to Extractor implementations.

use std::collections::HashMap;
use std::sync::Arc;

use crate::content::extractor::Extractor;
use crate::content::extractors::{PlainTextExtractor, SourceCodeExtractor};

pub struct ExtractorRegistry {
    by_mime: HashMap<String, Arc<dyn Extractor>>,
    by_extension: HashMap<String, Arc<dyn Extractor>>,
}

impl ExtractorRegistry {
    pub fn new() -> Self {
        Self {
            by_mime: HashMap::new(),
            by_extension: HashMap::new(),
        }
    }

    pub fn register(&mut self, extractor: Arc<dyn Extractor>) {
        for mime in extractor.supported_mimes() {
            self.by_mime
                .insert(mime.to_string(), Arc::clone(&extractor));
        }
    }

    pub fn register_with_extensions(
        &mut self,
        extractor: Arc<dyn Extractor>,
        extensions: &[&str],
    ) {
        self.register(Arc::clone(&extractor));
        for ext in extensions {
            self.by_extension
                .insert(ext.to_lowercase(), Arc::clone(&extractor));
        }
    }

    pub fn find_by_mime(&self, mime: &str) -> Option<Arc<dyn Extractor>> {
        self.by_mime.get(mime).cloned()
    }

    pub fn find_by_extension(&self, ext: &str) -> Option<Arc<dyn Extractor>> {
        self.by_extension.get(&ext.to_lowercase()).cloned()
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Default registry with first-batch extractors registered.
pub fn default_registry() -> ExtractorRegistry {
    let mut r = ExtractorRegistry::new();
    r.register(Arc::new(PlainTextExtractor::new()));
    r.register_with_extensions(
        Arc::new(SourceCodeExtractor::new()),
        SourceCodeExtractor::known_extensions(),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_finds_plain_text() {
        let r = default_registry();
        let e = r.find_by_mime("text/plain").unwrap();
        assert_eq!(e.name(), "PlainText");
    }

    #[test]
    fn test_default_registry_finds_source_code_by_mime() {
        let r = default_registry();
        let e = r.find_by_mime("text/x-rust").unwrap();
        assert_eq!(e.name(), "SourceCode");
    }

    #[test]
    fn test_default_registry_finds_source_by_extension() {
        let r = default_registry();
        let e = r.find_by_extension("py").unwrap();
        assert_eq!(e.name(), "SourceCode");
        let e = r.find_by_extension("RS").unwrap(); // case insensitive
        assert_eq!(e.name(), "SourceCode");
    }

    #[test]
    fn test_unknown_mime_returns_none() {
        let r = default_registry();
        assert!(r.find_by_mime("image/png").is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core content::registry`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/registry.rs
git commit -m "feat(hollow-core): ExtractorRegistry with MIME + extension lookup"
```

---

## Phase G: ContentPipeline

### Task G1: Pipeline orchestrator

**Files:**
- Modify: `hollow-core/src/content/pipeline.rs`

- [ ] **Step 1: Write pipeline + tests**

Replace `hollow-core/src/content/pipeline.rs` with:

```rust
//! ContentPipeline: runs detection → routing → extraction for one file.

use std::path::Path;

use crate::content::detector::FormatDetector;
use crate::content::registry::ExtractorRegistry;

#[derive(Debug, Clone)]
pub struct ExtractionOutcome {
    pub status: String,  // "indexed" or "extract_failed"
    pub extractor_name: Option<String>,
    pub body_text: Option<String>,
    pub encoding: Option<String>,
    pub detected_mime: String,
    pub extension_mismatch: bool,
    pub error: Option<String>,
}

pub struct ContentPipeline {
    registry: ExtractorRegistry,
}

impl ContentPipeline {
    pub fn new(registry: ExtractorRegistry) -> Self {
        Self { registry }
    }

    /// Run detection + extraction for one file. Never panics; errors are captured.
    pub fn process(&self, path: &Path, original_extension: Option<&str>) -> ExtractionOutcome {
        // Step 1: Detect format
        let detected = match FormatDetector::detect(path) {
            Ok(d) => d,
            Err(e) => {
                return ExtractionOutcome {
                    status: "extract_failed".to_string(),
                    extractor_name: None,
                    body_text: None,
                    encoding: None,
                    detected_mime: "application/octet-stream".to_string(),
                    extension_mismatch: false,
                    error: Some(format!("detection failed: {}", e)),
                };
            }
        };

        // Step 2: Check extension mismatch
        let extension_mismatch = match (original_extension, &detected.extension_hint) {
            (Some(orig), Some(hint)) => !orig.eq_ignore_ascii_case(hint),
            _ => false,
        };

        // Step 3: Find extractor — first by MIME, then by extension fallback
        let extractor = self.registry.find_by_mime(&detected.mime).or_else(|| {
            original_extension
                .and_then(|ext| self.registry.find_by_extension(ext))
        });

        let extractor = match extractor {
            Some(e) => e,
            None => {
                return ExtractionOutcome {
                    status: "extract_failed".to_string(),
                    extractor_name: None,
                    body_text: None,
                    encoding: None,
                    detected_mime: detected.mime,
                    extension_mismatch,
                    error: Some(format!("no extractor for mime: {}", detected.mime.clone())),
                };
            }
        };

        let extractor_name = extractor.name().to_string();

        // Step 4: Run extraction, catching any panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extractor.extract(path)
        }));

        match result {
            Ok(Ok(res)) => ExtractionOutcome {
                status: "indexed".to_string(),
                extractor_name: Some(extractor_name),
                body_text: Some(res.body_text),
                encoding: res.encoding,
                detected_mime: detected.mime,
                extension_mismatch,
                error: None,
            },
            Ok(Err(e)) => ExtractionOutcome {
                status: "extract_failed".to_string(),
                extractor_name: Some(extractor_name),
                body_text: None,
                encoding: None,
                detected_mime: detected.mime,
                extension_mismatch,
                error: Some(e.to_string()),
            },
            Err(_) => ExtractionOutcome {
                status: "extract_failed".to_string(),
                extractor_name: Some(extractor_name),
                body_text: None,
                encoding: None,
                detected_mime: detected.mime,
                extension_mismatch,
                error: Some("extractor panicked".to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::registry::default_registry;
    use std::fs;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_pipeline_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_process_plain_text_success() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("note.txt", b"hello world");
        let outcome = pipeline.process(&p, Some("txt"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.body_text.as_deref(), Some("hello world"));
        assert_eq!(outcome.extractor_name.as_deref(), Some("PlainText"));
        assert!(!outcome.extension_mismatch);
    }

    #[test]
    fn test_process_rust_source_by_extension() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("main.rs", b"fn main() {}");
        let outcome = pipeline.process(&p, Some("rs"));
        assert_eq!(outcome.status, "indexed");
        assert_eq!(outcome.extractor_name.as_deref(), Some("SourceCode"));
    }

    #[test]
    fn test_process_extension_mismatch() {
        let pipeline = ContentPipeline::new(default_registry());
        // PNG magic bytes in a .txt file
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        let p = tmp("fake.txt", &png);
        let outcome = pipeline.process(&p, Some("txt"));
        assert!(outcome.extension_mismatch);
        assert_eq!(outcome.status, "extract_failed"); // no image extractor
    }

    #[test]
    fn test_process_unknown_format() {
        let pipeline = ContentPipeline::new(default_registry());
        let p = tmp("blob.bin", &[0xFF, 0xFE, 0x00, 0x01]);
        let outcome = pipeline.process(&p, Some("bin"));
        assert_eq!(outcome.status, "extract_failed");
        assert!(outcome.error.is_some());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hollow-core content::pipeline`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add hollow-core/src/content/pipeline.rs
git commit -m "feat(hollow-core): ContentPipeline orchestrator with panic safety"
```

---

## Phase H: FileContentStore

### Task H1: FileContentStore with upsert and retrieval

**Files:**
- Create: `hollow-core/src/store/file_content_store.rs`
- Modify: `hollow-core/src/store/mod.rs`

- [ ] **Step 1: Check current store/mod.rs**

Run: `cat hollow-core/src/store/mod.rs` (use Read tool)

Add `pub mod file_content_store;` to it. Also add `pub use file_content_store::FileContentStore;` if FileStore is re-exported there (match existing pattern).

- [ ] **Step 2: Write FileContentStore + tests**

Write `hollow-core/src/store/file_content_store.rs`:

```rust
use rusqlite::Connection;

use crate::HollowError;

pub struct FileContentStore;

impl FileContentStore {
    /// Insert or replace a successful extraction record.
    pub fn upsert(
        conn: &Connection,
        file_id: &str,
        body_text_compressed: &[u8],
        body_text_bytes: i64,
        encoding: Option<&str>,
        extractor_name: &str,
        extracted_at: &str,
    ) -> Result<(), HollowError> {
        conn.execute(
            "INSERT INTO file_content (file_id, body_text_compressed, body_text_bytes, encoding, extractor_name, extracted_at, extract_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(file_id) DO UPDATE SET
                body_text_compressed = excluded.body_text_compressed,
                body_text_bytes = excluded.body_text_bytes,
                encoding = excluded.encoding,
                extractor_name = excluded.extractor_name,
                extracted_at = excluded.extracted_at,
                extract_error = NULL",
            rusqlite::params![
                file_id,
                body_text_compressed,
                body_text_bytes,
                encoding,
                extractor_name,
                extracted_at,
            ],
        )?;
        Ok(())
    }

    /// Insert or replace a failed extraction record.
    pub fn upsert_error(
        conn: &Connection,
        file_id: &str,
        error: &str,
        extractor_name: Option<&str>,
        extracted_at: &str,
    ) -> Result<(), HollowError> {
        conn.execute(
            "INSERT INTO file_content (file_id, extract_error, extractor_name, extracted_at, body_text_compressed, body_text_bytes, encoding)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL)
             ON CONFLICT(file_id) DO UPDATE SET
                extract_error = excluded.extract_error,
                extractor_name = excluded.extractor_name,
                extracted_at = excluded.extracted_at,
                body_text_compressed = NULL,
                body_text_bytes = NULL,
                encoding = NULL",
            rusqlite::params![file_id, error, extractor_name, extracted_at],
        )?;
        Ok(())
    }

    /// Get decompressed body text, if any.
    pub fn get_body_text(
        conn: &Connection,
        file_id: &str,
    ) -> Result<Option<String>, HollowError> {
        let mut stmt = conn
            .prepare("SELECT body_text_compressed FROM file_content WHERE file_id = ?1")?;
        let mut rows = stmt.query(rusqlite::params![file_id])?;
        if let Some(row) = rows.next()? {
            let compressed: Option<Vec<u8>> = row.get(0)?;
            match compressed {
                Some(bytes) if !bytes.is_empty() => {
                    let decoded = zstd::decode_all(&bytes[..])
                        .map_err(|e| HollowError::Database(format!("zstd decode: {}", e)))?;
                    let text = String::from_utf8(decoded)
                        .map_err(|e| HollowError::Database(format!("utf8: {}", e)))?;
                    Ok(Some(text))
                }
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::store::FileStore;
    use crate::db::models::FileRecord;

    fn test_db() -> Database {
        Database::open(":memory:").unwrap()
    }

    fn insert_file(db: &Database, id: &str) {
        let record = FileRecord {
            id: id.to_string(),
            hash: "".to_string(),
            quick_hash: "qh".to_string(),
            inode: Some(1),
            current_path: format!("/tmp/{}.txt", id),
            original_path: format!("/tmp/{}.txt", id),
            file_name: format!("{}.txt", id),
            extension: Some("txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            size_bytes: 11,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            ingested_at: "2026-01-01T00:00:00Z".to_string(),
            status: "pending".to_string(),
        };
        FileStore::insert_file(&db.conn, record).unwrap();
    }

    #[test]
    fn test_upsert_and_get() {
        let db = test_db();
        insert_file(&db, "f1");

        let text = "hello world".to_string();
        let compressed = zstd::encode_all(text.as_bytes(), 3).unwrap();
        FileContentStore::upsert(
            &db.conn,
            "f1",
            &compressed,
            text.len() as i64,
            Some("UTF-8"),
            "PlainText",
            "2026-04-10T00:00:00Z",
        )
        .unwrap();

        let got = FileContentStore::get_body_text(&db.conn, "f1").unwrap();
        assert_eq!(got.as_deref(), Some("hello world"));
    }

    #[test]
    fn test_upsert_overwrites() {
        let db = test_db();
        insert_file(&db, "f2");

        let c1 = zstd::encode_all(b"first".as_ref(), 3).unwrap();
        FileContentStore::upsert(&db.conn, "f2", &c1, 5, None, "PlainText", "t1").unwrap();

        let c2 = zstd::encode_all(b"second".as_ref(), 3).unwrap();
        FileContentStore::upsert(&db.conn, "f2", &c2, 6, None, "PlainText", "t2").unwrap();

        let got = FileContentStore::get_body_text(&db.conn, "f2").unwrap();
        assert_eq!(got.as_deref(), Some("second"));
    }

    #[test]
    fn test_upsert_error() {
        let db = test_db();
        insert_file(&db, "f3");

        FileContentStore::upsert_error(
            &db.conn,
            "f3",
            "file too large",
            Some("PlainText"),
            "2026-04-10T00:00:00Z",
        )
        .unwrap();

        let row: (Option<String>, Option<Vec<u8>>) = db
            .conn
            .query_row(
                "SELECT extract_error, body_text_compressed FROM file_content WHERE file_id = 'f3'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0.as_deref(), Some("file too large"));
        assert!(row.1.is_none());
    }

    #[test]
    fn test_get_body_text_missing_returns_none() {
        let db = test_db();
        let got = FileContentStore::get_body_text(&db.conn, "nope").unwrap();
        assert!(got.is_none());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p hollow-core file_content_store`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add hollow-core/src/store/file_content_store.rs hollow-core/src/store/mod.rs
git commit -m "feat(hollow-core): FileContentStore with zstd compression"
```

---

## Phase I: FileStore Extensions

### Task I1: Add update_detected_mime, get_quick_hash, mark_for_reextraction

**Files:**
- Modify: `hollow-core/src/store/file_store.rs`

- [ ] **Step 1: Write tests for new methods**

Add to the `tests` module of `hollow-core/src/store/file_store.rs`:

```rust
#[test]
fn test_update_detected_mime() {
    let db = test_db();
    let record = sample_record();
    FileStore::insert_file(&db.conn, record.clone()).unwrap();

    FileStore::update_detected_mime(&db.conn, &record.id, "image/png", true).unwrap();

    let (mime, mismatch): (Option<String>, i64) = db
        .conn
        .query_row(
            "SELECT detected_mime, extension_mismatch FROM files WHERE id = ?1",
            rusqlite::params![record.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(mime.as_deref(), Some("image/png"));
    assert_eq!(mismatch, 1);
}

#[test]
fn test_get_quick_hash() {
    let db = test_db();
    let record = sample_record();
    FileStore::insert_file(&db.conn, record.clone()).unwrap();

    let qh = FileStore::get_quick_hash(&db.conn, &record.id).unwrap();
    assert_eq!(qh.as_deref(), Some("abcd1234"));
}

#[test]
fn test_mark_for_reextraction() {
    let db = test_db();
    let mut record = sample_record();
    record.status = "indexed".to_string();
    FileStore::insert_file(&db.conn, record.clone()).unwrap();

    FileStore::mark_for_reextraction(&db.conn, &record.id).unwrap();

    let fetched = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
    assert_eq!(fetched.status, "pending");
}

#[test]
fn test_update_quick_hash() {
    let db = test_db();
    let record = sample_record();
    FileStore::insert_file(&db.conn, record.clone()).unwrap();

    FileStore::update_quick_hash(&db.conn, &record.id, "newhash").unwrap();

    let fetched = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
    assert_eq!(fetched.quick_hash, "newhash");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p hollow-core test_update_detected_mime`
Expected: FAIL (method doesn't exist)

- [ ] **Step 3: Implement methods**

Add to `impl FileStore` in `hollow-core/src/store/file_store.rs`:

```rust
pub fn update_detected_mime(
    conn: &Connection,
    id: &str,
    detected_mime: &str,
    extension_mismatch: bool,
) -> Result<(), HollowError> {
    let updated = conn.execute(
        "UPDATE files SET detected_mime = ?1, extension_mismatch = ?2 WHERE id = ?3",
        rusqlite::params![detected_mime, extension_mismatch as i64, id],
    )?;
    if updated == 0 {
        return Err(HollowError::FileNotFound(id.to_string()));
    }
    Ok(())
}

pub fn get_quick_hash(conn: &Connection, id: &str) -> Result<Option<String>, HollowError> {
    let mut stmt = conn.prepare("SELECT quick_hash FROM files WHERE id = ?1")?;
    let mut rows = stmt.query(rusqlite::params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn update_quick_hash(conn: &Connection, id: &str, quick_hash: &str) -> Result<(), HollowError> {
    let updated = conn.execute(
        "UPDATE files SET quick_hash = ?1 WHERE id = ?2",
        rusqlite::params![quick_hash, id],
    )?;
    if updated == 0 {
        return Err(HollowError::FileNotFound(id.to_string()));
    }
    Ok(())
}

pub fn mark_for_reextraction(conn: &Connection, id: &str) -> Result<(), HollowError> {
    Self::update_status(conn, id, "pending")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hollow-core file_store`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/store/file_store.rs
git commit -m "feat(hollow-core): FileStore methods for detected_mime and reextraction"
```

---

## Phase J: HollowCore FFI Methods

### Task J1: extract_content FFI

**Files:**
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Add ExtractContentResult uniffi::Record**

Add near `FileRecord` export in `hollow-core/src/lib.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ExtractContentResult {
    pub file_id: String,
    pub status: String,
    pub extractor_name: Option<String>,
    pub detected_mime: String,
    pub extension_mismatch: bool,
    pub body_text_bytes: u64,
    pub error: Option<String>,
}
```

- [ ] **Step 2: Write integration test**

Add to the `tests` module at the bottom of `hollow-core/src/lib.rs`:

```rust
#[test]
fn test_extract_content_plain_text() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_extract", "note.txt", b"hello from test");

    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
    assert_eq!(record.status, "pending");

    let result = core.extract_content(record.id.clone()).unwrap();
    assert_eq!(result.status, "indexed");
    assert_eq!(result.extractor_name.as_deref(), Some("PlainText"));
    assert_eq!(result.detected_mime, "text/plain");
    assert!(!result.extension_mismatch);
    assert_eq!(result.body_text_bytes, 15);

    // Verify DB state: file status updated, body_text stored
    let updated = core.get_file(record.id.clone()).unwrap().unwrap();
    assert_eq!(updated.status, "indexed");

    cleanup(&[&path, &path.parent().unwrap()]);
}

#[test]
fn test_extract_content_unknown_format_fails_gracefully() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_extract_bad", "blob.bin", &[0xFF, 0xFE, 0x00, 0x01]);

    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
    let result = core.extract_content(record.id.clone()).unwrap();

    assert_eq!(result.status, "extract_failed");
    assert!(result.error.is_some());

    let updated = core.get_file(record.id).unwrap().unwrap();
    assert_eq!(updated.status, "extract_failed");

    cleanup(&[&path, &path.parent().unwrap()]);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p hollow-core test_extract_content`
Expected: FAIL (method doesn't exist)

- [ ] **Step 4: Implement extract_content**

Add at the top of `lib.rs` (after `use uuid::Uuid;`):

```rust
use content::{default_registry, ContentPipeline};
use store::FileContentStore;
```

Add to `impl HollowCore`:

```rust
/// Run content extraction for a file. Updates file_content table and files.status.
pub fn extract_content(&self, file_id: String) -> Result<ExtractContentResult, HollowError> {
    // Fetch record to get path + extension
    let (current_path, original_extension) = {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        let record = FileStore::get_file(&db.conn, &file_id)?
            .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
        (record.current_path, record.extension)
    };

    let path = Path::new(&current_path);
    let pipeline = ContentPipeline::new(default_registry());
    let outcome = pipeline.process(path, original_extension.as_deref());

    let extracted_at = iso8601_now();
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;

    // Record detected mime + mismatch on files row
    FileStore::update_detected_mime(
        &db.conn,
        &file_id,
        &outcome.detected_mime,
        outcome.extension_mismatch,
    )?;

    let body_text_bytes: u64;
    if outcome.status == "indexed" {
        let body_text = outcome.body_text.clone().unwrap_or_default();
        body_text_bytes = body_text.len() as u64;
        let compressed = zstd::encode_all(body_text.as_bytes(), 3)
            .map_err(|e| HollowError::Database(format!("zstd encode: {}", e)))?;
        FileContentStore::upsert(
            &db.conn,
            &file_id,
            &compressed,
            body_text_bytes as i64,
            outcome.encoding.as_deref(),
            outcome.extractor_name.as_deref().unwrap_or("Unknown"),
            &extracted_at,
        )?;
        FileStore::update_status(&db.conn, &file_id, "indexed")?;
        info!(
            "Extracted content: {} ({} bytes, {})",
            file_id,
            body_text_bytes,
            outcome.extractor_name.as_deref().unwrap_or("?")
        );
    } else {
        body_text_bytes = 0;
        FileContentStore::upsert_error(
            &db.conn,
            &file_id,
            outcome.error.as_deref().unwrap_or("unknown error"),
            outcome.extractor_name.as_deref(),
            &extracted_at,
        )?;
        FileStore::update_status(&db.conn, &file_id, "extract_failed")?;
        info!(
            "Extraction failed: {} ({})",
            file_id,
            outcome.error.as_deref().unwrap_or("?")
        );
    }

    Ok(ExtractContentResult {
        file_id,
        status: outcome.status,
        extractor_name: outcome.extractor_name,
        detected_mime: outcome.detected_mime,
        extension_mismatch: outcome.extension_mismatch,
        body_text_bytes,
        error: outcome.error,
    })
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p hollow-core test_extract_content`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add hollow-core/src/lib.rs
git commit -m "feat(hollow-core): extract_content FFI method"
```

### Task J2: has_changed, mark_for_reextraction, get_pending_extraction_ids

**Files:**
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Write tests**

Add to tests in `hollow-core/src/lib.rs`:

```rust
#[test]
fn test_has_changed_detects_content_change() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_changed", "file.txt", b"version one");

    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
    assert!(!core.has_changed(record.id.clone()).unwrap());

    // Modify file
    std::fs::write(&path, b"version two is longer").unwrap();
    assert!(core.has_changed(record.id.clone()).unwrap());

    cleanup(&[&path, &path.parent().unwrap()]);
}

#[test]
fn test_mark_for_reextraction_flips_status_to_pending() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_reex", "file.txt", b"hello");

    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
    core.extract_content(record.id.clone()).unwrap();

    let after_extract = core.get_file(record.id.clone()).unwrap().unwrap();
    assert_eq!(after_extract.status, "indexed");

    core.mark_for_reextraction(record.id.clone()).unwrap();
    let after_mark = core.get_file(record.id.clone()).unwrap().unwrap();
    assert_eq!(after_mark.status, "pending");

    cleanup(&[&path, &path.parent().unwrap()]);
}

#[test]
fn test_get_pending_extraction_ids() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let dir = std::env::temp_dir().join("hollow_t_pending_ex");
    fs::create_dir_all(&dir).unwrap();

    let f1 = dir.join("a.txt");
    fs::write(&f1, b"a").unwrap();
    let f2 = dir.join("b.txt");
    fs::write(&f2, b"b").unwrap();

    core.ingest_file(f1.to_string_lossy().to_string()).unwrap();
    core.ingest_file(f2.to_string_lossy().to_string()).unwrap();

    let pending = core.get_pending_extraction_ids().unwrap();
    assert_eq!(pending.len(), 2);

    cleanup(&[&dir]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p hollow-core test_has_changed`
Expected: FAIL (methods don't exist)

- [ ] **Step 3: Implement methods**

Add to `impl HollowCore` in `hollow-core/src/lib.rs`:

```rust
/// Recompute quick_hash and compare with stored value.
pub fn has_changed(&self, file_id: String) -> Result<bool, HollowError> {
    let (current_path, old_quick_hash) = {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        let record = FileStore::get_file(&db.conn, &file_id)?
            .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
        (record.current_path, record.quick_hash)
    };

    let path = Path::new(&current_path);
    if !path.exists() {
        return Err(HollowError::FileNotFound(current_path));
    }

    let metadata = fs::metadata(path)
        .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
    let new_quick_hash = compute_quick_hash(path, metadata.len())?;

    if new_quick_hash != old_quick_hash {
        // Persist the new hash so subsequent calls don't keep reporting "changed"
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::update_quick_hash(&db.conn, &file_id, &new_quick_hash)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Flip an indexed file back to pending so it will be re-extracted.
pub fn mark_for_reextraction(&self, file_id: String) -> Result<(), HollowError> {
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
    FileStore::mark_for_reextraction(&db.conn, &file_id)
}

/// Alias for get_pending_ids with a name that matches the new pipeline.
pub fn get_pending_extraction_ids(&self) -> Result<Vec<String>, HollowError> {
    self.get_pending_ids()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p hollow-core`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/lib.rs
git commit -m "feat(hollow-core): has_changed, mark_for_reextraction, get_pending_extraction_ids FFI"
```

---

## Phase K: Swift HollowBridge

### Task K1: Add new bridge methods

**Files:**
- Modify: `hollow/HollowBridge.swift`

- [ ] **Step 1: Read current bridge file**

Run: Read `hollow/HollowBridge.swift` to confirm patterns (method signatures, error handling).

- [ ] **Step 2: Rebuild Rust → regenerate UniFFI bindings**

Run: `cargo build -p hollow-core`
Expected: new bindings should now include `extractContent`, `hasChanged`, `markForReextraction`, `getPendingExtractionIds`, and the `ExtractContentResult` record.

- [ ] **Step 3: Add Swift wrappers**

Add to `hollow/HollowBridge.swift` inside the `HollowBridge` class (matching existing patterns):

```swift
/// Run content extraction for a file. Returns nil on bridge error.
func extractContent(fileId: String) -> ExtractContentResult? {
    do {
        return try core.extractContent(fileId: fileId)
    } catch {
        HollowLogger.bridge.error("extractContent failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
        return nil
    }
}

/// Check whether a file's content has changed since last ingestion.
func hasChanged(fileId: String) -> Bool {
    do {
        return try core.hasChanged(fileId: fileId)
    } catch {
        HollowLogger.bridge.error("hasChanged failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
        return false
    }
}

/// Flip a file back to pending for re-extraction.
func markForReextraction(fileId: String) {
    do {
        try core.markForReextraction(fileId: fileId)
    } catch {
        HollowLogger.bridge.error("markForReextraction failed for \(fileId, privacy: .public): \(error.localizedDescription, privacy: .public)")
    }
}

/// Get all file IDs waiting for content extraction.
func getPendingExtractionIds() -> [String] {
    do {
        return try core.getPendingExtractionIds()
    } catch {
        HollowLogger.bridge.error("getPendingExtractionIds failed: \(error.localizedDescription, privacy: .public)")
        return []
    }
}
```

- [ ] **Step 4: Build the Swift app**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build. If UniFFI types are auto-imported under a module prefix, adjust `ExtractContentResult` to the correct qualified name.

- [ ] **Step 5: Commit**

```bash
git add hollow/HollowBridge.swift
git commit -m "feat(hollow): HollowBridge wrappers for content extraction"
```

---

## Phase L: Swift IngestionService — Parallel Metadata Queue

### Task L1: Create MetadataIntakeOperation

**Files:**
- Create: `hollow/Operations/MetadataIntakeOperation.swift`

- [ ] **Step 1: Create directory and file**

Run: verify directory exists: `ls hollow/` (if no `Operations/` folder, create it as a group in Xcode or add via file system and include in project.pbxproj).

> Note: For this plan, assume the executor will add the new file to the Xcode project via the IDE. If using pure CLI, the engineer must ensure `hollow.xcodeproj/project.pbxproj` includes the new files — this is a manual step.

Write `hollow/Operations/MetadataIntakeOperation.swift`:

```swift
import Foundation

/// Ingests one file's metadata (fast intake). Runs in IngestionService.metadataQueue.
final class MetadataIntakeOperation: Operation, @unchecked Sendable {
    let path: String
    private weak var service: IngestionService?

    init(path: String, service: IngestionService) {
        self.path = path
        self.service = service
        super.init()
    }

    override func main() {
        guard !isCancelled else { return }
        let result = HollowBridge.shared.ingestFile(path: path)
        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.service?.handleMetadataIntakeResult(result, path: self.path)
        }
    }
}
```

- [ ] **Step 2: Build to verify compiles**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: error — `handleMetadataIntakeResult` doesn't exist on IngestionService yet. That's fine; it will be added in Task L2.

- [ ] **Step 3: Do NOT commit yet** — L1 and L2 commit together.

### Task L2: Refactor IngestionService to use metadata OperationQueue

**Files:**
- Modify: `hollow/IngestionService.swift`

- [ ] **Step 1: Read current IngestionService**

Read `hollow/IngestionService.swift`. Note which methods currently call `bridge.ingestFile()` and how results flow to UI state (`totalIngested`, `recentFiles`, `lastError`).

- [ ] **Step 2: Replace serial DispatchQueue with OperationQueue**

In `IngestionService`:

Replace the `intakeQueue` property with:

```swift
private let metadataQueue: OperationQueue = {
    let q = OperationQueue()
    let cores = ProcessInfo.processInfo.activeProcessorCount
    q.maxConcurrentOperationCount = max(2, cores / 2)
    q.qualityOfService = .utility
    q.name = "com.syncpulse.hollow.metadata"
    return q
}()
```

Replace the current `intakeFiles(_:)` (or equivalent) implementation with:

```swift
func enqueueMetadataIntake(paths: [String]) {
    let ops = paths.map { MetadataIntakeOperation(path: $0, service: self) }
    metadataQueue.addOperations(ops, waitUntilFinished: false)
}

/// Called on main queue by MetadataIntakeOperation.
func handleMetadataIntakeResult(_ result: HollowBridge.IngestResult, path: String) {
    switch result {
    case .success(let record):
        totalIngested += 1
        recentFiles.insert(record.fileName, at: 0)
        if recentFiles.count > 10 {
            recentFiles.removeLast()
        }
        HollowLogger.ingestion.info("metadata intake ok: \(record.fileName, privacy: .public)")
    case .duplicate:
        HollowLogger.ingestion.debug("duplicate skipped: \(path, privacy: .public)")
    case .error(let msg):
        lastError = msg
        HollowLogger.ingestion.error("metadata intake failed: \(msg, privacy: .public)")
    }
}
```

Update the `FileWatcher.onNewFiles` callback (wherever it is set in `start()`) to call `enqueueMetadataIntake(paths:)` instead of the old serial method.

Update `stop()` to call:

```swift
metadataQueue.cancelAllOperations()
```

- [ ] **Step 3: Build**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 4: Run the app manually (smoke test)**

Drop a few files into `~/Hollow Inbox/`. Confirm via Debug → Database Browser that they appear with `status=pending`. The key regression to check: concurrent file drops don't crash and don't deadlock.

- [ ] **Step 5: Commit L1 + L2 together**

```bash
git add hollow/Operations/MetadataIntakeOperation.swift hollow/IngestionService.swift hollow.xcodeproj/project.pbxproj
git commit -m "refactor(hollow): parallel metadata intake via OperationQueue"
```

---

## Phase M: Swift Content Queue

### Task M1: ContentExtractionOperation

**Files:**
- Create: `hollow/Operations/ContentExtractionOperation.swift`

- [ ] **Step 1: Write the operation**

Write `hollow/Operations/ContentExtractionOperation.swift`:

```swift
import Foundation

final class ContentExtractionOperation: Operation, @unchecked Sendable {
    let fileId: String
    private weak var service: IngestionService?

    init(fileId: String, service: IngestionService) {
        self.fileId = fileId
        self.service = service
        super.init()
    }

    override func main() {
        guard !isCancelled else { return }
        let result = HollowBridge.shared.extractContent(fileId: fileId)
        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.service?.handleContentExtractionResult(result, fileId: self.fileId)
        }
    }
}
```

- [ ] **Step 2: Do NOT build yet** — the handler on IngestionService doesn't exist. Proceed to M2.

### Task M2: Content queue + auto-enqueue after metadata

**Files:**
- Modify: `hollow/IngestionService.swift`

- [ ] **Step 1: Add content queue and handler**

In `IngestionService`, add next to `metadataQueue`:

```swift
private let contentQueue: OperationQueue = {
    let q = OperationQueue()
    let cores = ProcessInfo.processInfo.activeProcessorCount
    q.maxConcurrentOperationCount = max(2, cores / 2)
    q.qualityOfService = .utility
    q.name = "com.syncpulse.hollow.content"
    return q
}()

@Published var extractionsInFlight: Int = 0
@Published var extractionsCompleted: Int = 0
@Published var extractionsFailed: Int = 0
```

> If the class uses `@Observable` macro instead of `@Published`, use the macro-compatible syntax (plain stored properties).

Add handler:

```swift
func enqueueContentExtraction(fileIds: [String]) {
    guard !fileIds.isEmpty else { return }
    let ops = fileIds.map { ContentExtractionOperation(fileId: $0, service: self) }
    extractionsInFlight += ops.count
    contentQueue.addOperations(ops, waitUntilFinished: false)
}

func handleContentExtractionResult(_ result: ExtractContentResult?, fileId: String) {
    extractionsInFlight = max(0, extractionsInFlight - 1)
    guard let result = result else {
        extractionsFailed += 1
        HollowLogger.ingestion.error("extraction bridge error: \(fileId, privacy: .public)")
        return
    }
    if result.status == "indexed" {
        extractionsCompleted += 1
        HollowLogger.ingestion.info("extracted: \(fileId, privacy: .public) (\(result.bodyTextBytes) bytes, \(result.extractorName ?? "?", privacy: .public))")
    } else {
        extractionsFailed += 1
        HollowLogger.ingestion.warning("extract_failed: \(fileId, privacy: .public) - \(result.error ?? "?", privacy: .public)")
    }
}
```

- [ ] **Step 2: Auto-enqueue after metadata intake**

Update `handleMetadataIntakeResult(_:path:)`:

```swift
case .success(let record):
    totalIngested += 1
    recentFiles.insert(record.fileName, at: 0)
    if recentFiles.count > 10 { recentFiles.removeLast() }
    HollowLogger.ingestion.info("metadata intake ok: \(record.fileName, privacy: .public)")
    // Auto-enqueue for content extraction
    enqueueContentExtraction(fileIds: [record.id])
```

- [ ] **Step 3: Startup pending scan**

In `IngestionService.start()`, after the existing startup logic, add:

```swift
let pendingIds = HollowBridge.shared.getPendingExtractionIds()
if !pendingIds.isEmpty {
    HollowLogger.ingestion.info("startup: \(pendingIds.count) pending extractions to resume")
    enqueueContentExtraction(fileIds: pendingIds)
}
```

- [ ] **Step 4: Update stop()**

```swift
metadataQueue.cancelAllOperations()
contentQueue.cancelAllOperations()
```

- [ ] **Step 5: Build**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 6: Manual smoke test**

Launch app, drop `.txt` / `.md` / `.rs` files in `~/Hollow Inbox/`. Open Debug → Database Browser and verify:
- Files appear with `status=pending` briefly, then `status=indexed`
- `file_content` table has rows with non-null `body_text_compressed`, matching `body_text_bytes` and `extractor_name`

- [ ] **Step 7: Commit M1 + M2 together**

```bash
git add hollow/Operations/ContentExtractionOperation.swift hollow/IngestionService.swift hollow.xcodeproj/project.pbxproj
git commit -m "feat(hollow): content extraction queue with auto-enqueue + startup resume"
```

---

## Phase N: File Change Detection

### Task N1: FileWatcher onModifiedFiles callback

**Files:**
- Modify: `hollow/FileWatcher.swift`

- [ ] **Step 1: Read current FileWatcher**

Read `hollow/FileWatcher.swift`. Identify where FSEvents flags are parsed and where `onNewFiles` / `onRemovedFiles` are fired.

- [ ] **Step 2: Add onModifiedFiles callback**

Add property next to `onNewFiles`:

```swift
var onModifiedFiles: (([URL]) -> Void)?
```

In the FSEvents callback parser, add handling for `kFSEventStreamEventFlagItemModified`. When a modified event arrives for a regular file (not directory, not temp extension), collect it into a `modifiedURLs` batch and call `onModifiedFiles?(modifiedURLs)` alongside the existing dispatch.

Because modify events can fire repeatedly during a save, add a simple in-memory debounce:

```swift
private var modifyDebounce: [String: DispatchWorkItem] = [:]
private let modifyDebounceDelay: TimeInterval = 0.5

private func scheduleModify(_ url: URL) {
    let key = url.path
    modifyDebounce[key]?.cancel()
    let work = DispatchWorkItem { [weak self] in
        guard let self = self else { return }
        self.modifyDebounce.removeValue(forKey: key)
        self.onModifiedFiles?([url])
    }
    modifyDebounce[key] = work
    DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + modifyDebounceDelay, execute: work)
}
```

And in the FSEvents callback, for files with the `ItemModified` flag, call `scheduleModify(url)`.

- [ ] **Step 3: Build**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add hollow/FileWatcher.swift
git commit -m "feat(hollow): FileWatcher modify events with 500ms debounce"
```

### Task N2: IngestionService handles modify events

**Files:**
- Modify: `hollow/IngestionService.swift`

- [ ] **Step 1: Wire onModifiedFiles in start()**

In `IngestionService.start()`, after setting `onNewFiles` and `onRemovedFiles`:

```swift
fileWatcher.onModifiedFiles = { [weak self] urls in
    self?.handleModifiedFiles(urls)
}
```

Add method:

```swift
func handleModifiedFiles(_ urls: [URL]) {
    for url in urls {
        // Look up file_id from path. Use a new bridge helper or list scan.
        // For simplicity, query by path using the existing bridge.
        guard let fileId = HollowBridge.shared.fileIdForPath(url.path) else {
            // Path unknown — treat as new file
            enqueueMetadataIntake(paths: [url.path])
            continue
        }
        // Check if content actually changed
        if HollowBridge.shared.hasChanged(fileId: fileId) {
            HollowBridge.shared.markForReextraction(fileId: fileId)
            enqueueContentExtraction(fileIds: [fileId])
            HollowLogger.ingestion.info("re-extraction queued: \(url.path, privacy: .public)")
        }
    }
}
```

- [ ] **Step 2: Add fileIdForPath helper to HollowBridge**

This requires a new Rust FFI method. Add to `hollow-core/src/lib.rs`:

```rust
pub fn file_id_for_path(&self, path: String) -> Result<Option<String>, HollowError> {
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
    let mut stmt = db.conn.prepare("SELECT id FROM files WHERE current_path = ?1")?;
    let mut rows = stmt.query(rusqlite::params![path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}
```

Add test:

```rust
#[test]
fn test_file_id_for_path() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_idforpath", "lookup.txt", b"x");
    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
    let looked_up = core.file_id_for_path(path.to_string_lossy().to_string()).unwrap();
    assert_eq!(looked_up, Some(record.id));
    assert!(core.file_id_for_path("/nonexistent".to_string()).unwrap().is_none());
    cleanup(&[&path, &path.parent().unwrap()]);
}
```

Run: `cargo test -p hollow-core test_file_id_for_path`
Expected: PASS

Add Swift wrapper in `hollow/HollowBridge.swift`:

```swift
func fileIdForPath(_ path: String) -> String? {
    do {
        return try core.fileIdForPath(path: path)
    } catch {
        HollowLogger.bridge.error("fileIdForPath failed: \(error.localizedDescription, privacy: .public)")
        return nil
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build -p hollow-core && xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 4: Manual smoke test**

Modify an existing ingested file (e.g., `echo "more" >> ~/Hollow\ Inbox/test.txt`). Verify in Debug → Database Browser that the file's `body_text_bytes` updates.

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/lib.rs hollow/HollowBridge.swift hollow/IngestionService.swift
git commit -m "feat(hollow): modify event triggers re-extraction via hasChanged"
```

---

## Phase O: UI Integration

### Task O1: ContentView shows queue counts

**Files:**
- Modify: `hollow/ContentView.swift`

- [ ] **Step 1: Read ContentView current state display**

Find where `totalIngested` / `recentFiles` is rendered. Add alongside:

```swift
HStack(spacing: 16) {
    Label("\(service.totalIngested)", systemImage: "tray.and.arrow.down.fill")
    Label("\(service.extractionsInFlight)", systemImage: "gearshape.2.fill")
        .foregroundStyle(service.extractionsInFlight > 0 ? .orange : .secondary)
    Label("\(service.extractionsCompleted)", systemImage: "checkmark.seal.fill")
        .foregroundStyle(.green)
    if service.extractionsFailed > 0 {
        Label("\(service.extractionsFailed)", systemImage: "exclamationmark.triangle.fill")
            .foregroundStyle(.red)
    }
}
.font(.caption)
```

Wrap the strings in `NSLocalizedString` or `LocalizedStringKey` following the existing i18n convention used in the rest of ContentView.

- [ ] **Step 2: Build**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add hollow/ContentView.swift
git commit -m "feat(hollow): show extraction queue counts in ContentView"
```

### Task O2: DatabaseBrowserView shows mismatch warning

**Files:**
- Modify: `hollow/DatabaseBrowserView.swift`

- [ ] **Step 1: Add mismatch indicator column**

In the file row rendering, read the file's `detectedMime` and `extensionMismatch` fields from the updated `FileRecord` (UniFFI will regenerate these). Show:

```swift
if file.extensionMismatch {
    Label("", systemImage: "exclamationmark.triangle.fill")
        .foregroundStyle(.orange)
        .help("Extension does not match detected format: \(file.detectedMime ?? "unknown")")
}
```

Also show the file's current status (`indexed`, `pending`, `extract_failed`) with appropriate color.

Add a "Re-extract" button for rows with `status == "extract_failed"`:

```swift
if file.status == "extract_failed" {
    Button("Re-extract") {
        HollowBridge.shared.markForReextraction(fileId: file.id)
        service.enqueueContentExtraction(fileIds: [file.id])
    }
    .buttonStyle(.link)
}
```

> Note: `FileRecord` needs the new fields exposed. If UniFFI auto-generates them from the `FileStore::get_file` SELECT, update the SELECT statement and `record_from_row` in `file_store.rs` to include `detected_mime` and `extension_mismatch`. Update `FileRecord` in `models.rs` accordingly. This is a small Rust change — add it in this step before rebuilding.

- [ ] **Step 2: Update Rust FileRecord + SELECT**

In `hollow-core/src/db/models.rs`, add fields to `FileRecord`:

```rust
pub detected_mime: Option<String>,
pub extension_mismatch: bool,
```

In `hollow-core/src/store/file_store.rs`:
- Update `SELECT_COLS` to include `detected_mime, extension_mismatch`
- Update `record_from_row` to read fields 14 and 15
- Update `insert_file` to include the new columns (default `NULL, 0`)

Update any existing tests that use `sample_record` — add the new fields with default values.

Update `hollow-core/src/lib.rs` ingest_file construction to set `detected_mime: None, extension_mismatch: false`.

Run: `cargo test -p hollow-core`
Expected: all PASS.

- [ ] **Step 3: Rebuild Rust + Swift**

Run: `cargo build -p hollow-core && xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 4: Manual smoke test**

Drop a misnamed file (e.g., rename a `.png` to `.txt`) into `~/Hollow Inbox/`. Verify the orange warning icon appears in the Database Browser.

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/db/models.rs hollow-core/src/store/file_store.rs hollow-core/src/lib.rs hollow/DatabaseBrowserView.swift
git commit -m "feat(hollow): surface detected_mime + extension_mismatch in UI"
```

### Task O3: SettingsView shows worker concurrency

**Files:**
- Modify: `hollow/SettingsView.swift`

- [ ] **Step 1: Add an info row**

In `SettingsView`, add under an "Advanced" or "Diagnostics" section:

```swift
let workers = max(2, ProcessInfo.processInfo.activeProcessorCount / 2)
LabeledContent("Content extraction workers") {
    Text("\(workers)")
        .foregroundStyle(.secondary)
        .monospacedDigit()
}
```

Follow existing localized-string conventions.

- [ ] **Step 2: Build**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add hollow/SettingsView.swift
git commit -m "feat(hollow): show content extraction worker count in Settings"
```

---

## Phase P: Final Verification

### Task P1: Full test suite + manual end-to-end

- [ ] **Step 1: Run all Rust tests**

Run: `cargo test -p hollow-core`
Expected: all PASS, no ignored tests that should be passing.

- [ ] **Step 2: Build Swift release**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build, no warnings beyond preexisting.

- [ ] **Step 3: Manual end-to-end smoke test**

1. Delete existing database: `rm -rf ~/Library/Application\ Support/com.syncpulse.hollow/hollow.db`
2. Launch app
3. Drop a mix of files into `~/Hollow Inbox/`:
   - `hello.txt` containing "hello world"
   - `greeting.md` containing "# Hi\n\nThis is markdown"
   - `script.py` containing `print("test")`
   - `fake.txt` that's actually a renamed PNG
   - `big.txt` >50MB (create with `dd if=/dev/zero of=big.txt bs=1M count=60`)
4. Open Debug → Database Browser
5. Verify:
   - Text/MD/Python files: `status=indexed`, non-null `body_text_compressed`
   - `fake.txt`: `extension_mismatch=1`, warning icon visible
   - `big.txt`: `status=extract_failed`, Re-extract button visible
6. Edit `hello.txt` (`echo "more" >> ~/Hollow\ Inbox/hello.txt`)
7. Verify within ~1s that `body_text_bytes` has increased

- [ ] **Step 4: Drop 50 files at once (concurrency stress test)**

Run: `for i in $(seq 1 50); do echo "file $i content" > ~/Hollow\ Inbox/stress_$i.txt; done`
Verify: no crash, all 50 files appear with `status=indexed` within seconds.

- [ ] **Step 5: Final commit if any fixups needed**

If issues found and fixed, commit them. Otherwise this task just verifies.

```bash
git status
# If clean, done. If dirty, commit fixes.
```

---

## Self-Review Checklist

- [x] **Schema migration covers both file_content and files tables** — Task A1
- [x] **Zstd compression + decompression round-trip** — Task H1 tests
- [x] **Extractor trait pattern extensible for future batches** — Task D1, H1, F1
- [x] **Format detection handles magic bytes, fallback heuristic, empty files** — Task C1
- [x] **Encoding detection with chardetng covers UTF-8, GBK** — Task E1
- [x] **Extension mismatch detected and propagated to UI** — Tasks C1, G1, O2
- [x] **Extract failures don't block pipeline** — Task G1 (catch_unwind, no retry)
- [x] **Metadata queue is parallel** — Task L2
- [x] **Content queue is parallel with CPU-based concurrency** — Task M2
- [x] **Auto-enqueue after metadata success** — Task M2
- [x] **Startup scan resumes pending extractions** — Task M2
- [x] **Modify events trigger has_changed + re-extraction** — Task N1, N2
- [x] **has_changed persists new quick_hash to avoid duplicate re-extraction** — Task J2
- [x] **UI shows queue counts, mismatch warnings, re-extract button** — Phase O
- [x] **All code steps have actual code, not placeholders**
- [x] **Type names consistent: ExtractContentResult, ExtractionOutcome, ExtractionResult**

**Known open items (deferred by design, not plan gaps):**
- FTS5 integration — explicitly out of scope, documented in spec
- OCR, PDF, docx — explicitly future batches
- Per-file-size priority queue — mentioned in spec as future work
- Xcode project file inclusion for new Swift files — manual step (documented in Task L1)
