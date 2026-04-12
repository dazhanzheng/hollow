# Batch 3: 语义理解 + 全文检索 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give hollow full-text search (FTS5) and local embedding-based semantic search, making it a usable "semantic entry point" — completing Phase 1 of the roadmap.

**Architecture:** Three sub-batches, each independently shippable. 3a adds FTS5 trigram full-text search over extracted content. 3b adds local ONNX embedding inference (Qwen3-Embedding-0.6B INT8 default, 4B optional) with vector storage in SQLite BLOBs. 3c combines both into a hybrid search API + unified Swift search UI.

**Tech Stack:** Rust (rusqlite FTS5, `ort` crate for ONNX Runtime, cosine similarity), Swift/SwiftUI (search UI, model download management), UniFFI (FFI bridge)

**Key Decisions:**
- FTS5 tokenizer: `trigram` — handles CJK + English without dictionary, good substring matching
- Embedding model: Qwen3-Embedding-0.6B INT8 ONNX (default, ~600MB), Qwen3-Embedding-4B INT8 (optional, ~4GB)
- Embedding runtime: `ort` crate with CoreML Execution Provider for Apple Silicon acceleration
- Vector storage: f32 BLOBs in SQLite, brute-force cosine similarity in Rust (sufficient for <100k personal files)
- No hollow-server dependency — embedding is fully local

---

## File Structure

### New Rust files (hollow-core)

| File | Responsibility |
|------|---------------|
| `src/store/fts_store.rs` | FTS5 virtual table population + search queries |
| `src/store/embedding_store.rs` | Embedding BLOB storage + retrieval for similarity search |
| `src/embedding/mod.rs` | Module exports |
| `src/embedding/model_manager.rs` | Model download, path resolution, model listing |
| `src/embedding/inference.rs` | ONNX Runtime session management + text → f32 vector |
| `src/search/mod.rs` | Module exports |
| `src/search/hybrid.rs` | Combine FTS5 + vector scores into unified ranked results |

### Modified Rust files

| File | Changes |
|------|---------|
| `src/db/schema.rs` | Add FTS5 virtual table + embeddings table to MIGRATION_V1 |
| `src/store/mod.rs` | Export FtsStore, EmbeddingStore |
| `src/lib.rs` | Add search/embedding FFI methods, new result types |
| `Cargo.toml` | Add `ort`, `ndarray`, `tokenizers` dependencies |

### New Swift files

| File | Responsibility |
|------|---------------|
| `hollow/SearchView.swift` | Search bar + results list + snippet highlighting |
| `hollow/EmbeddingService.swift` | Background embedding queue, model lifecycle |
| `hollow/ModelDownloadView.swift` | Model selection, download progress, size/RAM warnings |

### Modified Swift files

| File | Changes |
|------|---------|
| `hollow/SettingsView.swift` | Add "Models" tab for embedding model management |
| `hollow/HollowBridge.swift` | Add search + embedding FFI wrappers |
| `hollow/ContentView.swift` | Add search button/entry point |
| `hollow/hollowApp.swift` | Add search window scene |
| `hollow/IngestionService.swift` | Trigger FTS5 + embedding after extraction |
| `hollow/Logging/HollowLogger.swift` | Add `search` and `embedding` logger categories |

---

## Batch 3a: FTS5 Full-Text Search

### Task 1: Schema — Add FTS5 virtual table

**Files:**
- Modify: `hollow-core/src/db/schema.rs`

- [ ] **Step 1: Write failing test — FTS5 table exists after migration**

```rust
#[test]
fn test_fts5_table_exists_after_migration() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    migrate(&conn).unwrap();
    // FTS5 virtual tables appear as type='table' in sqlite_master
    let count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='file_content_fts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hollow-core test_fts5_table_exists_after_migration`
Expected: FAIL — table does not exist

- [ ] **Step 3: Add FTS5 virtual table to MIGRATION_V1**

In `schema.rs`, append to `MIGRATION_V1` string, after the `operations_log` index:

```sql
CREATE VIRTUAL TABLE file_content_fts USING fts5(
    file_id UNINDEXED,
    body_text,
    tokenize = 'trigram'
);
```

Note: `file_id UNINDEXED` stores the ID for joins but doesn't index it for search. `trigram` tokenizer handles CJK characters by creating overlapping 3-character substrings.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hollow-core test_fts5_table_exists_after_migration`
Expected: PASS

- [ ] **Step 5: Run all schema tests to verify no regressions**

Run: `cargo test -p hollow-core schema`
Expected: all PASS. Note: existing tests that check table counts may need updating.

- [ ] **Step 6: Commit**

```bash
git add hollow-core/src/db/schema.rs
git commit -m "feat(schema): add FTS5 virtual table for full-text search (trigram tokenizer)"
```

---

### Task 2: FtsStore — populate + search

**Files:**
- Create: `hollow-core/src/store/fts_store.rs`
- Modify: `hollow-core/src/store/mod.rs`

- [ ] **Step 1: Write failing test — FtsStore::index populates FTS5**

Create `hollow-core/src/store/fts_store.rs`:

```rust
use rusqlite::Connection;
use crate::HollowError;

pub struct FtsStore;

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
            size_bytes: 100,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            ingested_at: "2026-01-01T00:00:00Z".to_string(),
            status: "indexed".to_string(),
            detected_mime: None,
            extension_mismatch: false,
        };
        FileStore::insert_file(&db.conn, record).unwrap();
    }

    #[test]
    fn test_index_and_search() {
        let db = test_db();
        insert_file(&db, "f1");

        FtsStore::index(&db.conn, "f1", "这是一份合同文件，关于房屋租赁的协议").unwrap();

        let results = FtsStore::search(&db.conn, "合同", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, "f1");
        assert!(results[0].snippet.contains("合同"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hollow-core test_index_and_search`
Expected: FAIL — methods don't exist

- [ ] **Step 3: Implement FtsStore**

```rust
use rusqlite::Connection;
use crate::HollowError;

pub struct FtsStore;

/// A single FTS5 search hit.
#[derive(Debug, Clone)]
pub struct FtsSearchResult {
    pub file_id: String,
    pub snippet: String,
    pub rank: f64,
}

impl FtsStore {
    /// Insert or replace body text into the FTS5 index for a file.
    /// Call this after successful text extraction. Idempotent — safe to re-call
    /// on re-extraction.
    pub fn index(conn: &Connection, file_id: &str, body_text: &str) -> Result<(), HollowError> {
        // Delete any existing entry first (FTS5 doesn't have ON CONFLICT)
        conn.execute(
            "DELETE FROM file_content_fts WHERE file_id = ?1",
            rusqlite::params![file_id],
        )?;
        conn.execute(
            "INSERT INTO file_content_fts (file_id, body_text) VALUES (?1, ?2)",
            rusqlite::params![file_id, body_text],
        )?;
        Ok(())
    }

    /// Remove a file from the FTS5 index (e.g. when file is deleted).
    pub fn remove(conn: &Connection, file_id: &str) -> Result<(), HollowError> {
        conn.execute(
            "DELETE FROM file_content_fts WHERE file_id = ?1",
            rusqlite::params![file_id],
        )?;
        Ok(())
    }

    /// Full-text search. Returns results ranked by FTS5 relevance.
    /// The snippet function highlights matches with `<b>` tags.
    pub fn search(
        conn: &Connection,
        query: &str,
        limit: u32,
    ) -> Result<Vec<FtsSearchResult>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT file_id, snippet(file_content_fts, 1, '<b>', '</b>', '…', 32), rank
             FROM file_content_fts
             WHERE body_text MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![query, limit], |row| {
            Ok(FtsSearchResult {
                file_id: row.get(0)?,
                snippet: row.get(1)?,
                rank: row.get(2)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}
```

- [ ] **Step 4: Export FtsStore from store/mod.rs**

Add to `hollow-core/src/store/mod.rs`:

```rust
mod fts_store;
pub use fts_store::{FtsStore, FtsSearchResult};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p hollow-core test_index_and_search`
Expected: PASS

- [ ] **Step 6: Write additional tests — English search, multi-result, no-match**

Append to the test module in `fts_store.rs`:

```rust
#[test]
fn test_search_english() {
    let db = test_db();
    insert_file(&db, "f1");
    FtsStore::index(&db.conn, "f1", "The quick brown fox jumps over the lazy dog").unwrap();
    let results = FtsStore::search(&db.conn, "brown fox", 10).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_no_match() {
    let db = test_db();
    insert_file(&db, "f1");
    FtsStore::index(&db.conn, "f1", "hello world").unwrap();
    let results = FtsStore::search(&db.conn, "zzzznotfound", 10).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_search_multiple_results_ranked() {
    let db = test_db();
    for i in 1..=3 {
        let id = format!("f{}", i);
        let mut record = FileRecord {
            id: id.clone(),
            hash: "".to_string(),
            quick_hash: "qh".to_string(),
            inode: Some(i as i64),
            current_path: format!("/tmp/{}.txt", id),
            original_path: format!("/tmp/{}.txt", id),
            file_name: format!("{}.txt", id),
            extension: Some("txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            size_bytes: 100,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            ingested_at: "2026-01-01T00:00:00Z".to_string(),
            status: "indexed".to_string(),
            detected_mime: None,
            extension_mismatch: false,
        };
        FileStore::insert_file(&db.conn, record).unwrap();
    }
    FtsStore::index(&db.conn, "f1", "invoice for consulting services rendered").unwrap();
    FtsStore::index(&db.conn, "f2", "invoice summary: total invoices this quarter").unwrap();
    FtsStore::index(&db.conn, "f3", "meeting notes from last Tuesday").unwrap();

    let results = FtsStore::search(&db.conn, "invoice", 10).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn test_index_idempotent() {
    let db = test_db();
    insert_file(&db, "f1");
    FtsStore::index(&db.conn, "f1", "version one").unwrap();
    FtsStore::index(&db.conn, "f1", "version two").unwrap();
    let results = FtsStore::search(&db.conn, "version two", 10).unwrap();
    assert_eq!(results.len(), 1);
    // Old content should not match
    let old = FtsStore::search(&db.conn, "version one", 10).unwrap();
    assert!(old.is_empty());
}

#[test]
fn test_remove() {
    let db = test_db();
    insert_file(&db, "f1");
    FtsStore::index(&db.conn, "f1", "searchable content").unwrap();
    FtsStore::remove(&db.conn, "f1").unwrap();
    let results = FtsStore::search(&db.conn, "searchable", 10).unwrap();
    assert!(results.is_empty());
}
```

- [ ] **Step 7: Run all FTS tests**

Run: `cargo test -p hollow-core fts_store`
Expected: all PASS

- [ ] **Step 8: Commit**

```bash
git add hollow-core/src/store/fts_store.rs hollow-core/src/store/mod.rs
git commit -m "feat(fts): add FtsStore with trigram FTS5 index, search, and remove"
```

---

### Task 3: Integrate FTS5 indexing into extraction pipeline

**Files:**
- Modify: `hollow-core/src/lib.rs`

FTS5 indexing happens automatically after a successful extraction (status = "indexed"). Both `extract_content` and `extract_content_external` need this.

- [ ] **Step 1: Add FtsStore import to lib.rs**

Add to the imports at the top of `lib.rs`:

```rust
use store::{FileContentStore, FileStore, FtsStore};
```

- [ ] **Step 2: Add FTS5 indexing to `extract_content` — after the "indexed" branch**

In `extract_content()`, in the `"indexed"` match arm, after `FileStore::update_status(&db.conn, &file_id, "indexed")?;` and before the `info!()` call, add:

```rust
// Populate FTS5 index with the extracted text
let body_text_for_fts = outcome.body_text.as_deref().unwrap_or_default();
if !body_text_for_fts.is_empty() {
    FtsStore::index(&db.conn, &file_id, body_text_for_fts)?;
}
```

Note: `outcome.body_text` is still available here because we only moved it into `compressed_body` via `.clone()` earlier.

- [ ] **Step 3: Add FTS5 indexing to `extract_content_external` — after the "indexed" branch**

In `extract_content_external()`, the body_text was consumed by `zstd::encode_all`. We need to keep a reference. Change the `"indexed"` block at step 2 to retain the text:

Find the line `let body = body_text.unwrap_or_default();` in the `if status == "indexed"` block and change so we also keep the plain text available. Then in the match arm for `"indexed"`, after `FileStore::update_status`, add:

```rust
// Populate FTS5 index
// body_text was consumed above; decompress to get it back for FTS
let fts_text = zstd::decode_all(&compressed[..])
    .map_err(|e| HollowError::Database(format!("zstd decode for FTS: {}", e)))?;
let fts_text = String::from_utf8(fts_text)
    .map_err(|e| HollowError::Database(format!("utf8 for FTS: {}", e)))?;
if !fts_text.is_empty() {
    FtsStore::index(&db.conn, &file_id, &fts_text)?;
}
```

Alternative (cleaner): refactor to keep the body text string available. Before the lockless compression block in `extract_content_external`, clone the body:

```rust
let body_text_plain = if status == "indexed" {
    body_text.clone()
} else {
    None
};
```

Then in the "indexed" match arm:
```rust
if let Some(ref text) = body_text_plain {
    if !text.is_empty() {
        FtsStore::index(&db.conn, &file_id, text)?;
    }
}
```

Use whichever approach is cleaner in context.

- [ ] **Step 4: Run existing extraction tests to verify no regressions**

Run: `cargo test -p hollow-core -- --test-threads=1`
Expected: all PASS (136+ tests)

- [ ] **Step 5: Write integration test — extraction auto-populates FTS5**

Add to the tests module in `lib.rs`:

```rust
#[test]
fn test_extract_content_populates_fts5() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_fts_int", "note.txt", b"searchable content here");
    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

    core.extract_content(record.id.clone()).unwrap();

    // Verify FTS5 was populated
    let db = core.db.lock().unwrap();
    let results = store::FtsStore::search(&db.conn, "searchable", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_id, record.id);

    cleanup(&[&path, &path.parent().unwrap()]);
}
```

- [ ] **Step 6: Run the integration test**

Run: `cargo test -p hollow-core test_extract_content_populates_fts5`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add hollow-core/src/lib.rs
git commit -m "feat(fts): auto-populate FTS5 index on successful extraction"
```

---

### Task 4: Search FFI — expose search to Swift

**Files:**
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Add SearchResult UniFFI record**

Add near the other `uniffi::Record` structs in `lib.rs`:

```rust
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResult {
    pub file_id: String,
    pub file_name: String,
    pub current_path: String,
    pub snippet: String,
    pub rank: f64,
}
```

- [ ] **Step 2: Add search method to HollowCore**

Add to the `#[uniffi::export] impl HollowCore` block:

```rust
/// Full-text search across all indexed file content.
/// Returns results ranked by FTS5 relevance, enriched with file metadata.
pub fn search(&self, query: String, limit: u32) -> Result<Vec<SearchResult>, HollowError> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
    let fts_results = FtsStore::search(&db.conn, &query, limit)?;
    let mut results = Vec::with_capacity(fts_results.len());
    for fts in fts_results {
        if let Some(record) = FileStore::get_file(&db.conn, &fts.file_id)? {
            results.push(SearchResult {
                file_id: fts.file_id,
                file_name: record.file_name,
                current_path: record.current_path,
                snippet: fts.snippet,
                rank: fts.rank,
            });
        }
    }
    Ok(results)
}
```

- [ ] **Step 3: Write test for search FFI**

Add to tests in `lib.rs`:

```rust
#[test]
fn test_search_ffi() {
    let core = HollowCore::new(":memory:".to_string()).unwrap();
    let path = make_temp_file("hollow_t_search_ffi", "report.txt", b"quarterly revenue report for Q4 2025");
    let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
    core.extract_content(record.id.clone()).unwrap();

    let results = core.search("revenue".to_string(), 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "report.txt");
    assert!(!results[0].snippet.is_empty());

    // Empty query returns empty
    let empty = core.search("".to_string(), 10).unwrap();
    assert!(empty.is_empty());

    cleanup(&[&path, &path.parent().unwrap()]);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p hollow-core test_search_ffi`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/lib.rs
git commit -m "feat(ffi): add search() method for full-text search via FTS5"
```

---

### Task 5: Swift — HollowBridge search wrapper + SearchView

**Files:**
- Modify: `hollow/HollowBridge.swift`
- Create: `hollow/SearchView.swift`
- Modify: `hollow/hollowApp.swift`
- Modify: `hollow/ContentView.swift`
- Modify: `hollow/Logging/HollowLogger.swift`

- [ ] **Step 1: Regenerate UniFFI bindings**

After the Rust changes, rebuild and regenerate:

```bash
cargo build -p hollow-core
cd hollow-core && cargo run --bin uniffi-bindgen generate --library ../target/debug/libhollow_core.a --language swift --out-dir ../hollow/Generated
```

Verify `SearchResult` appears in `hollow/Generated/hollow_core.swift`.

- [ ] **Step 2: Add search wrapper to HollowBridge**

Add to `hollow/HollowBridge.swift`:

```swift
/// Full-text search across all indexed content.
nonisolated func search(query: String, limit: UInt32 = 50) -> [SearchResult] {
    guard let core else { return [] }
    do {
        return try core.search(query: query, limit: limit)
    } catch {
        HollowLogger.search.error("Search failed: \(error)")
        return []
    }
}
```

- [ ] **Step 3: Add search logger category**

Add to `hollow/Logging/HollowLogger.swift`:

```swift
static let search = Logger(subsystem: "com.syncpulse.hollow", category: "Search")
```

- [ ] **Step 4: Create SearchView**

Create `hollow/SearchView.swift`:

```swift
import SwiftUI

struct SearchView: View {
    @State private var query = ""
    @State private var results: [SearchResult] = []
    @State private var isSearching = false

    var body: some View {
        VStack(spacing: 0) {
            // Search bar
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Search files…", text: $query)
                    .textFieldStyle(.plain)
                    .onSubmit { performSearch() }
                if !query.isEmpty {
                    Button {
                        query = ""
                        results = []
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(12)
            .background(.bar)

            Divider()

            // Results
            if results.isEmpty && !query.isEmpty && !isSearching {
                ContentUnavailableView.search(text: query)
            } else {
                List(results, id: \.fileId) { result in
                    SearchResultRow(result: result)
                }
                .listStyle(.plain)
            }
        }
        .frame(minWidth: 500, minHeight: 400)
        .onChange(of: query) {
            // Debounced search: search as you type with small delay
            if query.count >= 2 {
                performSearch()
            } else if query.isEmpty {
                results = []
            }
        }
    }

    private func performSearch() {
        isSearching = true
        let currentQuery = query
        DispatchQueue.global(qos: .userInitiated).async {
            let searchResults = HollowBridge.shared.search(
                query: currentQuery,
                limit: 50
            )
            DispatchQueue.main.async {
                if query == currentQuery {
                    results = searchResults
                    isSearching = false
                }
            }
        }
    }
}

private struct SearchResultRow: View {
    let result: SearchResult

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Image(systemName: "doc.text")
                    .foregroundStyle(.tint)
                Text(result.fileName)
                    .font(.body.weight(.medium))
                Spacer()
            }
            Text(result.currentPath)
                .font(.caption)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .truncationMode(.middle)
            Text(snippetPlainText(result.snippet))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(3)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .onTapGesture {
            NSWorkspace.shared.activateFileViewerSelecting(
                [URL(fileURLWithPath: result.currentPath)]
            )
        }
    }

    /// Strip `<b>` tags from FTS5 snippet for display.
    /// A future enhancement could use AttributedString for bold highlighting.
    private func snippetPlainText(_ snippet: String) -> String {
        snippet.replacingOccurrences(of: "<b>", with: "")
               .replacingOccurrences(of: "</b>", with: "")
    }
}
```

- [ ] **Step 5: Add Search window to hollowApp.swift**

Add a new `Window` scene to the app:

```swift
Window("Search", id: "search") {
    SearchView()
}
.defaultSize(width: 600, height: 500)
.keyboardShortcut("f", modifiers: [.command, .shift])
```

- [ ] **Step 6: Add search button to ContentView**

Add a search button in `ContentView`, e.g. in the stats area or as a prominent action:

```swift
Button {
    openWindow(id: "search")
} label: {
    Label("Search Files", systemImage: "magnifyingglass")
}
.keyboardShortcut("f", modifiers: [.command, .shift])
```

Add `@Environment(\.openWindow) private var openWindow` to ContentView.

- [ ] **Step 7: Build and test manually**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build, no warnings

- [ ] **Step 8: Commit**

```bash
git add hollow/HollowBridge.swift hollow/SearchView.swift hollow/hollowApp.swift hollow/ContentView.swift hollow/Logging/HollowLogger.swift hollow/Generated/
git commit -m "feat(search): add SearchView with FTS5 full-text search UI"
```

---

### Task 6: Trigger FTS5 indexing from IngestionService

**Files:**
- Modify: `hollow/IngestionService.swift`

Currently, FTS5 indexing happens inside Rust's `extract_content`. But `extract_content_external` (Swift OCR path) also needs it. Since we added FTS5 indexing to both Rust methods in Task 3, this task just verifies the integration works end-to-end.

- [ ] **Step 1: Verify OCR extraction path also indexes FTS5**

The FTS5 indexing was added to `extract_content_external` in Task 3. No Swift changes needed — the Rust side handles it automatically.

Write a manual test: drop a `.txt` file into the Inbox folder, wait for extraction, then search for text from that file in the Search window.

- [ ] **Step 2: Commit (if any changes needed)**

If no changes: skip. If fixes needed, commit them.

---

## Batch 3b: Local Embedding Pipeline

### Task 7: Schema — Add embeddings table

**Files:**
- Modify: `hollow-core/src/db/schema.rs`

- [ ] **Step 1: Write failing test — embeddings table exists**

```rust
#[test]
fn test_embeddings_table_exists_after_migration() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    migrate(&conn).unwrap();
    let count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='embeddings'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
```

- [ ] **Step 2: Run test — should fail**

Run: `cargo test -p hollow-core test_embeddings_table_exists`
Expected: FAIL

- [ ] **Step 3: Add embeddings table to MIGRATION_V1**

Append to `MIGRATION_V1`:

```sql
CREATE TABLE embeddings (
    file_id       TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    embedding     BLOB NOT NULL,
    dimensions    INTEGER NOT NULL,
    model_name    TEXT NOT NULL,
    embedded_at   TEXT NOT NULL
);
```

- [ ] **Step 4: Run test — should pass**

Run: `cargo test -p hollow-core test_embeddings_table_exists`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo test -p hollow-core`
Expected: all PASS

- [ ] **Step 6: Commit**

```bash
git add hollow-core/src/db/schema.rs
git commit -m "feat(schema): add embeddings table for vector storage"
```

---

### Task 8: EmbeddingStore — BLOB storage + cosine similarity search

**Files:**
- Create: `hollow-core/src/store/embedding_store.rs`
- Modify: `hollow-core/src/store/mod.rs`

- [ ] **Step 1: Write failing tests**

Create `hollow-core/src/store/embedding_store.rs`:

```rust
use rusqlite::Connection;
use crate::HollowError;

pub struct EmbeddingStore;

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
            size_bytes: 100,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            ingested_at: "2026-01-01T00:00:00Z".to_string(),
            status: "indexed".to_string(),
            detected_mime: None,
            extension_mismatch: false,
        };
        FileStore::insert_file(&db.conn, record).unwrap();
    }

    #[test]
    fn test_upsert_and_get() {
        let db = test_db();
        insert_file(&db, "f1");
        let vec = vec![0.1_f32, 0.2, 0.3, 0.4];
        EmbeddingStore::upsert(&db.conn, "f1", &vec, "qwen3-0.6b", "2026-04-12T00:00:00Z").unwrap();
        let got = EmbeddingStore::get(&db.conn, "f1").unwrap();
        assert!(got.is_some());
        let (emb, model) = got.unwrap();
        assert_eq!(emb.len(), 4);
        assert!((emb[0] - 0.1).abs() < 1e-6);
        assert_eq!(model, "qwen3-0.6b");
    }

    #[test]
    fn test_cosine_search() {
        let db = test_db();
        for i in 1..=3 {
            let id = format!("f{}", i);
            let mut record = FileRecord {
                id: id.clone(),
                hash: "".to_string(),
                quick_hash: "qh".to_string(),
                inode: Some(i as i64),
                current_path: format!("/tmp/{}.txt", id),
                original_path: format!("/tmp/{}.txt", id),
                file_name: format!("{}.txt", id),
                extension: Some("txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                size_bytes: 100,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                modified_at: "2026-01-01T00:00:00Z".to_string(),
                ingested_at: "2026-01-01T00:00:00Z".to_string(),
                status: "indexed".to_string(),
                detected_mime: None,
                extension_mismatch: false,
            };
            FileStore::insert_file(&db.conn, record).unwrap();
        }

        // f1: similar to query, f2: orthogonal, f3: opposite
        EmbeddingStore::upsert(&db.conn, "f1", &[0.9, 0.1, 0.0, 0.0], "m", "t").unwrap();
        EmbeddingStore::upsert(&db.conn, "f2", &[0.0, 0.0, 1.0, 0.0], "m", "t").unwrap();
        EmbeddingStore::upsert(&db.conn, "f3", &[-0.9, -0.1, 0.0, 0.0], "m", "t").unwrap();

        let query = vec![1.0_f32, 0.0, 0.0, 0.0];
        let results = EmbeddingStore::search(&db.conn, &query, 10).unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].file_id, "f1"); // most similar
        assert!(results[0].score > results[1].score);
    }
}
```

- [ ] **Step 2: Run tests — should fail**

Run: `cargo test -p hollow-core embedding_store`
Expected: FAIL

- [ ] **Step 3: Implement EmbeddingStore**

```rust
use rusqlite::Connection;
use crate::HollowError;

pub struct EmbeddingStore;

#[derive(Debug, Clone)]
pub struct EmbeddingSearchResult {
    pub file_id: String,
    pub score: f32, // cosine similarity, higher = more similar
}

impl EmbeddingStore {
    /// Store or replace an embedding vector for a file.
    /// The vector is stored as a raw f32 BLOB.
    pub fn upsert(
        conn: &Connection,
        file_id: &str,
        embedding: &[f32],
        model_name: &str,
        embedded_at: &str,
    ) -> Result<(), HollowError> {
        let blob = f32_slice_to_bytes(embedding);
        conn.execute(
            "INSERT INTO embeddings (file_id, embedding, dimensions, model_name, embedded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(file_id) DO UPDATE SET
                embedding = excluded.embedding,
                dimensions = excluded.dimensions,
                model_name = excluded.model_name,
                embedded_at = excluded.embedded_at",
            rusqlite::params![file_id, blob, embedding.len() as i32, model_name, embedded_at],
        )?;
        Ok(())
    }

    /// Get the stored embedding for a file.
    pub fn get(
        conn: &Connection,
        file_id: &str,
    ) -> Result<Option<(Vec<f32>, String)>, HollowError> {
        let result = conn.query_row(
            "SELECT embedding, model_name FROM embeddings WHERE file_id = ?1",
            rusqlite::params![file_id],
            |row| {
                let blob: Vec<u8> = row.get(0)?;
                let model: String = row.get(1)?;
                Ok((blob, model))
            },
        );
        match result {
            Ok((blob, model)) => Ok(Some((bytes_to_f32_vec(&blob), model))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(HollowError::Database(e.to_string())),
        }
    }

    /// Brute-force cosine similarity search across all stored embeddings.
    /// Returns results sorted by descending similarity score.
    pub fn search(
        conn: &Connection,
        query_embedding: &[f32],
        limit: u32,
    ) -> Result<Vec<EmbeddingSearchResult>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT file_id, embedding FROM embeddings"
        )?;
        let rows = stmt.query_map([], |row| {
            let file_id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((file_id, blob))
        })?;

        let mut scored: Vec<EmbeddingSearchResult> = Vec::new();
        for row in rows {
            let (file_id, blob) = row?;
            let embedding = bytes_to_f32_vec(&blob);
            let score = cosine_similarity(query_embedding, &embedding);
            scored.push(EmbeddingSearchResult { file_id, score });
        }

        // Sort by score descending
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit as usize);
        Ok(scored)
    }

    /// Get file IDs that have no embedding yet (status = "indexed" but no embeddings row).
    pub fn get_pending_ids(conn: &Connection) -> Result<Vec<String>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT f.id FROM files f
             LEFT JOIN embeddings e ON f.id = e.file_id
             WHERE f.status = 'indexed' AND e.file_id IS NULL"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }
}

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 {
        0.0
    } else {
        dot / denom
    }
}
```

- [ ] **Step 4: Export from store/mod.rs**

Add to `hollow-core/src/store/mod.rs`:

```rust
mod embedding_store;
pub use embedding_store::{EmbeddingStore, EmbeddingSearchResult};
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p hollow-core embedding_store`
Expected: all PASS

- [ ] **Step 6: Commit**

```bash
git add hollow-core/src/store/embedding_store.rs hollow-core/src/store/mod.rs
git commit -m "feat(embedding): add EmbeddingStore with BLOB storage and cosine similarity search"
```

---

### Task 9: Embedding inference module — ONNX Runtime integration

**Files:**
- Modify: `hollow-core/Cargo.toml`
- Create: `hollow-core/src/embedding/mod.rs`
- Create: `hollow-core/src/embedding/inference.rs`
- Create: `hollow-core/src/embedding/model_manager.rs`
- Modify: `hollow-core/src/lib.rs`

This is the most complex task. It integrates the `ort` crate for ONNX Runtime inference.

- [ ] **Step 1: Add dependencies to Cargo.toml**

Add to `[dependencies]` in `hollow-core/Cargo.toml`:

```toml
ort = { version = "2", default-features = false, features = ["load-dynamic"] }
ndarray = "0.16"
tokenizers = { version = "0.21", default-features = false }
```

Notes:
- `ort` with `load-dynamic` defers ONNX Runtime library loading — the app ships the .dylib separately or bundles it.
- `tokenizers` is the HuggingFace tokenizer library for BPE/WordPiece tokenization.
- `ndarray` for tensor manipulation.

- [ ] **Step 2: Create embedding module structure**

Create `hollow-core/src/embedding/mod.rs`:

```rust
pub mod inference;
pub mod model_manager;

pub use inference::EmbeddingModel;
pub use model_manager::{ModelInfo, ModelManager, ModelVariant};
```

- [ ] **Step 3: Implement ModelManager — model download and path resolution**

Create `hollow-core/src/embedding/model_manager.rs`:

```rust
use crate::HollowError;
use std::path::{Path, PathBuf};
use std::fs;

/// Available embedding model variants.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelVariant {
    /// Qwen3-Embedding-0.6B INT8 ONNX (~600MB)
    Qwen3Small,
    /// Qwen3-Embedding-4B INT8 ONNX (~4GB)
    Qwen3Large,
}

/// Metadata about an available model.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub variant: ModelVariant,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub download_size_mb: u64,
    pub ram_usage_mb: u64,
    pub dimensions: u32,
}

pub struct ModelManager {
    models_dir: PathBuf,
}

impl ModelManager {
    /// Create a new ModelManager. `models_dir` is where ONNX models are stored
    /// (typically `~/Library/Application Support/com.syncpulse.hollow/models/`).
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    /// List all available model variants with metadata.
    pub fn available_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                variant: ModelVariant::Qwen3Small,
                name: "qwen3-embedding-0.6b-int8".to_string(),
                display_name: "Qwen3 Embedding 0.6B (INT8)".to_string(),
                description: "Default model. Good balance of quality and speed for Chinese + English.".to_string(),
                download_size_mb: 600,
                ram_usage_mb: 400,
                dimensions: 1024,
            },
            ModelInfo {
                variant: ModelVariant::Qwen3Large,
                name: "qwen3-embedding-4b-int8".to_string(),
                display_name: "Qwen3 Embedding 4B (INT8)".to_string(),
                description: "High quality model. Better accuracy but uses more memory and CPU.".to_string(),
                download_size_mb: 4000,
                ram_usage_mb: 3000,
                dimensions: 1024,
            },
        ]
    }

    /// Check if a model is downloaded and ready to use.
    pub fn is_downloaded(&self, variant: &ModelVariant) -> bool {
        let model_dir = self.model_dir(variant);
        model_dir.join("model.onnx").exists() && model_dir.join("tokenizer.json").exists()
    }

    /// Get the directory path for a model variant.
    pub fn model_dir(&self, variant: &ModelVariant) -> PathBuf {
        let name = match variant {
            ModelVariant::Qwen3Small => "qwen3-embedding-0.6b-int8",
            ModelVariant::Qwen3Large => "qwen3-embedding-4b-int8",
        };
        self.models_dir.join(name)
    }

    /// Get the ONNX model file path.
    pub fn model_path(&self, variant: &ModelVariant) -> PathBuf {
        self.model_dir(variant).join("model.onnx")
    }

    /// Get the tokenizer file path.
    pub fn tokenizer_path(&self, variant: &ModelVariant) -> PathBuf {
        self.model_dir(variant).join("tokenizer.json")
    }

    /// Delete a downloaded model to free disk space.
    pub fn delete_model(&self, variant: &ModelVariant) -> Result<(), HollowError> {
        let dir = self.model_dir(variant);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| HollowError::InvalidInput(format!("Failed to delete model: {}", e)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_models_returns_two() {
        let models = ModelManager::available_models();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].variant, ModelVariant::Qwen3Small);
        assert_eq!(models[1].variant, ModelVariant::Qwen3Large);
    }

    #[test]
    fn test_is_downloaded_false_when_missing() {
        let tmp = std::env::temp_dir().join("hollow_test_mm");
        let mgr = ModelManager::new(tmp.clone());
        assert!(!mgr.is_downloaded(&ModelVariant::Qwen3Small));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_model_paths() {
        let mgr = ModelManager::new(PathBuf::from("/models"));
        assert_eq!(
            mgr.model_path(&ModelVariant::Qwen3Small),
            PathBuf::from("/models/qwen3-embedding-0.6b-int8/model.onnx")
        );
        assert_eq!(
            mgr.tokenizer_path(&ModelVariant::Qwen3Small),
            PathBuf::from("/models/qwen3-embedding-0.6b-int8/tokenizer.json")
        );
    }
}
```

- [ ] **Step 4: Implement EmbeddingModel — ONNX inference**

Create `hollow-core/src/embedding/inference.rs`:

```rust
use crate::HollowError;
use std::path::Path;

/// Wraps an ONNX Runtime session for embedding inference.
/// Thread-safe — can be shared across workers.
pub struct EmbeddingModel {
    session: ort::Session,
    tokenizer: tokenizers::Tokenizer,
    dimensions: usize,
}

impl EmbeddingModel {
    /// Load an ONNX model and tokenizer from disk.
    /// This is expensive (~2-5s) — call once and reuse.
    pub fn load(model_path: &Path, tokenizer_path: &Path) -> Result<Self, HollowError> {
        let session = ort::Session::builder()
            .map_err(|e| HollowError::InvalidInput(format!("ONNX session builder: {}", e)))?
            .with_optimization_level(ort::GraphOptimizationLevel::Level3)
            .map_err(|e| HollowError::InvalidInput(format!("ONNX opt level: {}", e)))?
            .commit_from_file(model_path)
            .map_err(|e| HollowError::InvalidInput(format!("ONNX load model: {}", e)))?;

        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| HollowError::InvalidInput(format!("Load tokenizer: {}", e)))?;

        // Determine output dimensions from first output shape
        let dimensions = session
            .outputs
            .first()
            .and_then(|o| o.output_type.tensor_dimensions().map(|d| d.last().copied()))
            .flatten()
            .unwrap_or(Some(1024))
            .unwrap_or(1024) as usize;

        Ok(Self {
            session,
            tokenizer,
            dimensions,
        })
    }

    /// Generate an embedding vector for a text input.
    /// Truncates to the model's max sequence length.
    /// Returns a normalized f32 vector.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, HollowError> {
        let encoding = self.tokenizer.encode(text, true)
            .map_err(|e| HollowError::InvalidInput(format!("Tokenize: {}", e)))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
        let seq_len = input_ids.len();

        let input_ids_array = ndarray::Array2::from_shape_vec(
            (1, seq_len),
            input_ids,
        ).map_err(|e| HollowError::InvalidInput(format!("input_ids shape: {}", e)))?;

        let attention_mask_array = ndarray::Array2::from_shape_vec(
            (1, seq_len),
            attention_mask,
        ).map_err(|e| HollowError::InvalidInput(format!("attention_mask shape: {}", e)))?;

        let outputs = self.session.run(
            ort::inputs![
                "input_ids" => input_ids_array,
                "attention_mask" => attention_mask_array,
            ].map_err(|e| HollowError::InvalidInput(format!("ONNX inputs: {}", e)))?,
        ).map_err(|e| HollowError::InvalidInput(format!("ONNX run: {}", e)))?;

        // Extract the embedding from the output tensor.
        // Most embedding models output shape [1, seq_len, dim] — we mean-pool.
        // Some output [1, dim] directly (sentence-level pooling built-in).
        let output = outputs.first()
            .ok_or_else(|| HollowError::InvalidInput("No output from ONNX model".to_string()))?;
        let tensor = output.try_extract_tensor::<f32>()
            .map_err(|e| HollowError::InvalidInput(format!("Extract tensor: {}", e)))?;

        let shape = tensor.shape();
        let embedding = if shape.len() == 3 {
            // [1, seq_len, dim] — mean pooling over seq_len
            let dim = shape[2];
            let seq = shape[1];
            let mut pooled = vec![0.0_f32; dim];
            for s in 0..seq {
                for d in 0..dim {
                    pooled[d] += tensor[[0, s, d]];
                }
            }
            for d in 0..dim {
                pooled[d] /= seq as f32;
            }
            pooled
        } else if shape.len() == 2 {
            // [1, dim] — already pooled
            tensor.slice(ndarray::s![0, ..]).to_vec()
        } else {
            return Err(HollowError::InvalidInput(
                format!("Unexpected output shape: {:?}", shape)
            ));
        };

        // L2 normalize
        Ok(l2_normalize(&embedding))
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-10 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l2_normalize() {
        let v = vec![3.0, 4.0];
        let n = l2_normalize(&v);
        assert!((n[0] - 0.6).abs() < 1e-6);
        assert!((n[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_l2_normalize_zero_vector() {
        let v = vec![0.0, 0.0, 0.0];
        let n = l2_normalize(&v);
        assert!(n.iter().all(|x| *x == 0.0));
    }
}
```

- [ ] **Step 5: Add embedding module to lib.rs**

Add `mod embedding;` to the top of `lib.rs`:

```rust
mod embedding;
```

- [ ] **Step 6: Run unit tests (l2_normalize, model_manager)**

Run: `cargo test -p hollow-core embedding`
Expected: unit tests PASS (inference tests that need a model file will be integration tests)

- [ ] **Step 7: Commit**

```bash
git add hollow-core/Cargo.toml hollow-core/src/embedding/
git commit -m "feat(embedding): add ONNX inference module and model manager"
```

---

### Task 10: Embedding FFI — expose to Swift

**Files:**
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Add embedding-related types and imports**

Add to `lib.rs` imports:

```rust
use embedding::{ModelManager, ModelVariant, EmbeddingModel};
use store::EmbeddingStore;
```

Add UniFFI records:

```rust
#[derive(Debug, Clone, uniffi::Record)]
pub struct EmbeddingModelInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub download_size_mb: u64,
    pub ram_usage_mb: u64,
    pub is_downloaded: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EmbeddingStatus {
    pub total_indexed: u32,
    pub total_embedded: u32,
    pub pending_embedding: u32,
}
```

- [ ] **Step 2: Add model_manager and embedding_model fields to HollowCore**

Modify the `HollowCore` struct to hold model state:

```rust
#[derive(uniffi::Object)]
pub struct HollowCore {
    db: Mutex<Database>,
    model_manager: ModelManager,
    embedding_model: Mutex<Option<EmbeddingModel>>,
}
```

Update the constructor to initialize `model_manager` using a `models_dir` derived from the `db_path` parent:

```rust
#[uniffi::constructor]
pub fn new(db_path: String) -> Result<Self, HollowError> {
    logging::init_logging();
    let db = Database::open(&db_path)?;

    // Models directory: sibling to the database file
    let db_parent = Path::new(&db_path)
        .parent()
        .unwrap_or(Path::new("."));
    let models_dir = db_parent.join("models");

    info!("HollowCore initialized, db: {}", db_path);
    Ok(HollowCore {
        db: Mutex::new(db),
        model_manager: ModelManager::new(models_dir),
        embedding_model: Mutex::new(None),
    })
}
```

- [ ] **Step 3: Add embedding FFI methods**

Add to the `#[uniffi::export] impl HollowCore` block:

```rust
/// List available embedding models and their download status.
pub fn list_embedding_models(&self) -> Vec<EmbeddingModelInfo> {
    ModelManager::available_models()
        .into_iter()
        .map(|m| EmbeddingModelInfo {
            name: m.name,
            display_name: m.display_name,
            description: m.description,
            download_size_mb: m.download_size_mb,
            ram_usage_mb: m.ram_usage_mb,
            is_downloaded: self.model_manager.is_downloaded(&m.variant),
        })
        .collect()
}

/// Check if the default embedding model is ready.
pub fn is_embedding_ready(&self) -> bool {
    self.model_manager.is_downloaded(&ModelVariant::Qwen3Small)
}

/// Get embedding statistics.
pub fn get_embedding_status(&self) -> Result<EmbeddingStatus, HollowError> {
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;

    let total_indexed: u32 = db.conn.query_row(
        "SELECT COUNT(*) FROM files WHERE status = 'indexed'",
        [],
        |r| r.get(0),
    )?;
    let total_embedded: u32 = db.conn.query_row(
        "SELECT COUNT(*) FROM embeddings",
        [],
        |r| r.get(0),
    )?;

    Ok(EmbeddingStatus {
        total_indexed,
        total_embedded,
        pending_embedding: total_indexed.saturating_sub(total_embedded),
    })
}

/// Get file IDs that need embedding.
pub fn get_pending_embedding_ids(&self) -> Result<Vec<String>, HollowError> {
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
    EmbeddingStore::get_pending_ids(&db.conn)
}

/// Generate and store an embedding for a file.
/// Loads the model lazily on first call.
pub fn embed_file(&self, file_id: String) -> Result<bool, HollowError> {
    // Get body text
    let body_text = self.get_body_text(file_id.clone())?;
    let Some(text) = body_text else {
        return Ok(false); // No text to embed
    };
    if text.is_empty() {
        return Ok(false);
    }

    // Ensure model is loaded
    {
        let mut model_lock = self.embedding_model.lock()
            .map_err(|e| HollowError::InvalidInput(format!("Model lock: {}", e)))?;
        if model_lock.is_none() {
            let model_path = self.model_manager.model_path(&ModelVariant::Qwen3Small);
            let tokenizer_path = self.model_manager.tokenizer_path(&ModelVariant::Qwen3Small);
            if !model_path.exists() {
                return Err(HollowError::InvalidInput(
                    "Embedding model not downloaded. Download it from Settings → Models.".to_string()
                ));
            }
            info!("Loading embedding model...");
            let model = EmbeddingModel::load(&model_path, &tokenizer_path)?;
            *model_lock = Some(model);
            info!("Embedding model loaded");
        }
    }

    // Run inference (outside db lock)
    let embedding = {
        let model_lock = self.embedding_model.lock()
            .map_err(|e| HollowError::InvalidInput(format!("Model lock: {}", e)))?;
        let model = model_lock.as_ref().unwrap();
        // Truncate very long texts to first ~2000 chars for embedding
        let truncated = if text.len() > 8000 { &text[..8000] } else { &text };
        model.embed(truncated)?
    };

    // Store embedding
    let embedded_at = iso8601_now();
    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
    EmbeddingStore::upsert(
        &db.conn,
        &file_id,
        &embedding,
        "qwen3-embedding-0.6b-int8",
        &embedded_at,
    )?;

    info!("Embedded file: {} ({} dims)", file_id, embedding.len());
    Ok(true)
}
```

- [ ] **Step 4: Run build to verify compilation**

Run: `cargo build -p hollow-core`
Expected: compiles (some warnings OK)

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/lib.rs
git commit -m "feat(ffi): add embedding model management and embed_file FFI methods"
```

---

### Task 11: Swift — Model download UI + EmbeddingService

**Files:**
- Create: `hollow/ModelDownloadView.swift`
- Modify: `hollow/SettingsView.swift`
- Create: `hollow/EmbeddingService.swift`
- Modify: `hollow/HollowBridge.swift`
- Modify: `hollow/IngestionService.swift`
- Modify: `hollow/Logging/HollowLogger.swift`

- [ ] **Step 1: Regenerate UniFFI bindings**

```bash
cargo build -p hollow-core
cd hollow-core && cargo run --bin uniffi-bindgen generate --library ../target/debug/libhollow_core.a --language swift --out-dir ../hollow/Generated
```

- [ ] **Step 2: Add embedding logger category**

Add to `hollow/Logging/HollowLogger.swift`:

```swift
static let embedding = Logger(subsystem: "com.syncpulse.hollow", category: "Embedding")
```

- [ ] **Step 3: Add embedding wrappers to HollowBridge**

Add to `hollow/HollowBridge.swift`:

```swift
nonisolated func listEmbeddingModels() -> [EmbeddingModelInfo] {
    guard let core else { return [] }
    return core.listEmbeddingModels()
}

nonisolated func isEmbeddingReady() -> Bool {
    guard let core else { return false }
    return core.isEmbeddingReady()
}

nonisolated func getEmbeddingStatus() -> EmbeddingStatus? {
    guard let core else { return nil }
    return try? core.getEmbeddingStatus()
}

nonisolated func getPendingEmbeddingIds() -> [String] {
    guard let core else { return [] }
    return (try? core.getPendingEmbeddingIds()) ?? []
}

nonisolated func embedFile(fileId: String) -> Bool {
    guard let core else { return false }
    return (try? core.embedFile(fileId: fileId)) ?? false
}
```

- [ ] **Step 4: Create EmbeddingService**

Create `hollow/EmbeddingService.swift`:

```swift
import Foundation
import Observation
import os

@MainActor @Observable
final class EmbeddingService {
    var isProcessing = false
    var processedCount = 0
    var totalPending = 0

    private let embeddingQueue: OperationQueue = {
        let q = OperationQueue()
        q.name = "com.syncpulse.hollow.embedding"
        q.maxConcurrentOperationCount = 1 // One at a time — model is memory-heavy
        q.qualityOfService = .utility
        return q
    }()

    /// Process all files that need embedding. Call after extraction completes.
    func processAllPending() {
        guard !isProcessing else { return }
        guard HollowBridge.shared.isEmbeddingReady() else {
            HollowLogger.embedding.info("Embedding model not downloaded, skipping")
            return
        }

        isProcessing = true
        processedCount = 0

        let bridge = HollowBridge.shared
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let ids = bridge.getPendingEmbeddingIds()
            let total = ids.count

            Task { @MainActor in
                self?.totalPending = total
            }

            for (index, fileId) in ids.enumerated() {
                _ = bridge.embedFile(fileId: fileId)
                Task { @MainActor in
                    self?.processedCount = index + 1
                }
            }

            Task { @MainActor in
                self?.isProcessing = false
                HollowLogger.embedding.info("Embedding complete: \(total) files")
            }
        }
    }
}
```

- [ ] **Step 5: Add Models tab to SettingsView**

Add to the `TabView` in `SettingsView`:

```swift
ModelsSettingsView()
    .tabItem {
        Label("Models", systemImage: "cpu")
    }
```

Create the `ModelsSettingsView`:

```swift
private struct ModelsSettingsView: View {
    @State private var models: [EmbeddingModelInfo] = []
    @State private var embeddingStatus: EmbeddingStatus?

    var body: some View {
        Form {
            Section {
                ForEach(models, id: \.name) { model in
                    ModelRow(model: model)
                }
            } header: {
                Text("Embedding Models")
            } footer: {
                Text("Embedding models enable semantic search — finding files by meaning, not just keywords. Models run locally on your Mac.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if let status = embeddingStatus {
                Section("Embedding Status") {
                    LabeledContent("Files indexed") {
                        Text("\(status.totalIndexed)")
                            .monospacedDigit()
                    }
                    LabeledContent("Files embedded") {
                        Text("\(status.totalEmbedded)")
                            .monospacedDigit()
                    }
                    if status.pendingEmbedding > 0 {
                        LabeledContent("Pending") {
                            Text("\(status.pendingEmbedding)")
                                .monospacedDigit()
                                .foregroundStyle(.orange)
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .task {
            models = HollowBridge.shared.listEmbeddingModels()
            embeddingStatus = HollowBridge.shared.getEmbeddingStatus()
        }
    }
}

private struct ModelRow: View {
    let model: EmbeddingModelInfo

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(model.displayName)
                    .font(.body.weight(.medium))
                Text(model.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack(spacing: 8) {
                    Text("Download: \(model.downloadSizeMb) MB")
                    Text("RAM: ~\(model.ramUsageMb) MB")
                }
                .font(.caption2)
                .foregroundStyle(.tertiary)
            }

            Spacer()

            if model.isDownloaded {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            } else {
                Button("Download") {
                    // TODO: Implement model download (Task 12 or separate)
                    // This will be a background download from HuggingFace
                }
                .buttonStyle(.bordered)
            }
        }
    }
}
```

- [ ] **Step 6: Widen settings window for new tab**

Update the frame in `SettingsView`:

```swift
.frame(width: 560, height: 460)
```

- [ ] **Step 7: Build and verify**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build

- [ ] **Step 8: Commit**

```bash
git add hollow/EmbeddingService.swift hollow/HollowBridge.swift hollow/SettingsView.swift hollow/Logging/HollowLogger.swift hollow/Generated/
git commit -m "feat(embedding): add EmbeddingService, model download UI, and Settings Models tab"
```

---

### Task 12: First-launch onboarding dialog + model download

When hollow launches for the first time (no model downloaded yet, checked via `UserDefaults` flag `hasShownModelOnboarding`), present a sheet/dialog that:

1. Explains what embedding models do ("enable semantic search — find files by meaning")
2. Shows two model options side-by-side with clear hardware guidance
3. Lets the user pick one and starts download immediately with a progress bar
4. Can be dismissed ("Skip for now") — user can always download later from Settings → Models

**Hardware guidance:**
- 0.6B: "Recommended for all Macs. ~600 MB download, uses ~400 MB RAM."
- 4B: "For Macs with 32 GB+ RAM. Better accuracy. ~4 GB download, uses ~3 GB RAM."
  - If the Mac has <32 GB RAM, show a warning: "Your Mac has X GB RAM — this model may slow down other apps."

**Files:**
- Create: `hollow/OnboardingModelView.swift`
- Modify: `hollow/hollowApp.swift` (show sheet on first launch)
- Modify: `hollow/SettingsView.swift` (reuse download logic)

**Files:**
- Modify: `hollow/ModelDownloadView.swift` or inline in SettingsView

Model download is done from Swift since it needs UI progress feedback and network access. The models are downloaded from HuggingFace.

- [ ] **Step 1: Create ModelDownloader — URLSession background download with progress**

Create a shared `ModelDownloader` class that handles downloading from HuggingFace:

```swift
import Foundation
import Observation
import os

@Observable
final class ModelDownloader: @unchecked Sendable {
    var isDownloading = false
    var progress: Double = 0.0  // 0..1
    var downloadedBytes: Int64 = 0
    var totalBytes: Int64 = 0
    var error: String?
    var currentModelName: String?

    /// Download model files (model.onnx + tokenizer.json) from HuggingFace.
    /// Files are saved to the model directory under Application Support.
    func download(modelName: String, to destinationDir: URL) async throws {
        await MainActor.run {
            isDownloading = true
            progress = 0
            error = nil
            currentModelName = modelName
        }

        do {
            try FileManager.default.createDirectory(
                at: destinationDir,
                withIntermediateDirectories: true
            )

            // HuggingFace direct file download URLs
            let baseURL = "https://huggingface.co/onnx-community/Qwen3-Embedding-\(modelName)/resolve/main"
            let files = ["model.onnx", "tokenizer.json"]

            for (index, fileName) in files.enumerated() {
                let url = URL(string: "\(baseURL)/\(fileName)")!
                let dest = destinationDir.appendingPathComponent(fileName)

                let (tempURL, response) = try await URLSession.shared.download(from: url, delegate: nil)

                try FileManager.default.moveItem(at: tempURL, to: dest)

                await MainActor.run {
                    progress = Double(index + 1) / Double(files.count)
                }
            }

            await MainActor.run {
                isDownloading = false
                progress = 1.0
                currentModelName = nil
            }
        } catch {
            await MainActor.run {
                self.isDownloading = false
                self.error = error.localizedDescription
                self.currentModelName = nil
            }
            throw error
        }
    }
}
```

Note: The exact HuggingFace URLs and file names will need to be verified at implementation time. The ONNX model may be a single file or require multiple shards. For proper download progress, use a `URLSessionDownloadDelegate` instead of `URLSession.shared.download(from:)`.

- [ ] **Step 2: Create OnboardingModelView — first-launch dialog**

Create `hollow/OnboardingModelView.swift`:

```swift
import SwiftUI

struct OnboardingModelView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var downloader = ModelDownloader()
    @State private var selectedModel: String?

    private let systemRAM: UInt64 = ProcessInfo.processInfo.physicalMemory / (1024 * 1024 * 1024) // GB

    var body: some View {
        VStack(spacing: 24) {
            // Header
            VStack(spacing: 8) {
                Image(systemName: "brain")
                    .font(.system(size: 40))
                    .foregroundStyle(.tint)
                Text("Set Up Semantic Search")
                    .font(.title2.weight(.semibold))
                Text("Download an embedding model to search files by meaning, not just keywords. Models run entirely on your Mac — nothing leaves your device.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            // Model cards
            VStack(spacing: 12) {
                modelCard(
                    name: "0.6B-int8",
                    title: "Standard",
                    subtitle: "Recommended for all Macs",
                    size: "~600 MB download",
                    ram: "~400 MB RAM",
                    recommended: true,
                    warning: nil
                )

                modelCard(
                    name: "4B-int8",
                    title: "High Quality",
                    subtitle: "Better accuracy, higher resource usage",
                    size: "~4 GB download",
                    ram: "~3 GB RAM",
                    recommended: false,
                    warning: systemRAM < 32
                        ? "Your Mac has \(systemRAM) GB RAM — this model may slow down other apps while embedding."
                        : nil
                )
            }

            // Download progress
            if downloader.isDownloading {
                VStack(spacing: 8) {
                    ProgressView(value: downloader.progress)
                    Text("Downloading \(downloader.currentModelName ?? "model")…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            if let error = downloader.error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            // Skip button
            if !downloader.isDownloading {
                Button("Skip for now") {
                    markOnboardingDone()
                    dismiss()
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .font(.caption)
            }
        }
        .padding(32)
        .frame(width: 480)
    }

    private func modelCard(
        name: String,
        title: String,
        subtitle: String,
        size: String,
        ram: String,
        recommended: Bool,
        warning: String?
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(title).font(.body.weight(.medium))
                        if recommended {
                            Text("RECOMMENDED")
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 2)
                                .background(.tint, in: Capsule())
                        }
                    }
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    HStack(spacing: 12) {
                        Text(size)
                        Text(ram)
                    }
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                }

                Spacer()

                Button("Download") {
                    selectedModel = name
                    startDownload(name: name)
                }
                .buttonStyle(.bordered)
                .tint(recommended ? .accentColor : nil)
                .disabled(downloader.isDownloading)
            }

            if let warning {
                HStack(spacing: 4) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text(warning)
                        .font(.caption2)
                        .foregroundStyle(.orange)
                }
            }
        }
        .padding(12)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
    }

    private func startDownload(name: String) {
        let modelsDir = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
            .appendingPathComponent("com.syncpulse.hollow/models/qwen3-embedding-\(name)")

        Task {
            try? await downloader.download(modelName: name, to: modelsDir)
            if downloader.error == nil {
                markOnboardingDone()
                dismiss()
            }
        }
    }

    private func markOnboardingDone() {
        UserDefaults.standard.set(true, forKey: "hasShownModelOnboarding")
    }
}
```

- [ ] **Step 3: Show onboarding sheet on first launch**

In `hollowApp.swift`, add state and `.sheet` modifier to the main window:

```swift
@AppStorage("hasShownModelOnboarding") private var hasShownOnboarding = false
@State private var showOnboarding = false

// In the main Window content:
ContentView()
    .sheet(isPresented: $showOnboarding) {
        OnboardingModelView()
    }
    .task {
        if !hasShownOnboarding {
            showOnboarding = true
        }
    }
```

- [ ] **Step 4: Update ModelRow in Settings to reuse ModelDownloader**

In `ModelsSettingsView`, use the same `ModelDownloader` for the Settings download buttons, showing progress inline:

```swift
if downloader.isDownloading && downloader.currentModelName == model.name {
    ProgressView(value: downloader.progress)
        .frame(width: 80)
} else if model.isDownloaded {
    Image(systemName: "checkmark.circle.fill")
        .foregroundStyle(.green)
} else {
    Button("Download") { startDownload(model) }
        .buttonStyle(.bordered)
}
```

- [ ] **Step 5: Add RAM warning for 4B model in Settings**

```swift
if model.ramUsageMb >= 3000 && !model.isDownloaded {
    let ram = ProcessInfo.processInfo.physicalMemory / (1024 * 1024 * 1024)
    if ram < 32 {
        HStack(spacing: 4) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
            Text("Your Mac has \(ram) GB RAM. This model may slow down other apps.")
                .font(.caption2)
                .foregroundStyle(.orange)
        }
    }
}
```

- [ ] **Step 6: Build and verify**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build

- [ ] **Step 7: Commit**

```bash
git add hollow/OnboardingModelView.swift hollow/SettingsView.swift hollow/hollowApp.swift
git commit -m "feat(onboarding): first-launch model download dialog with progress and hardware guidance"
```

---

## Batch 3c: Hybrid Search

### Task 13: Hybrid search combiner

**Files:**
- Create: `hollow-core/src/search/mod.rs`
- Create: `hollow-core/src/search/hybrid.rs`
- Modify: `hollow-core/src/lib.rs`

- [ ] **Step 1: Create search module**

Create `hollow-core/src/search/mod.rs`:

```rust
pub mod hybrid;
pub use hybrid::HybridSearcher;
```

Add `mod search;` to `lib.rs`.

- [ ] **Step 2: Write failing test for hybrid search**

Create `hollow-core/src/search/hybrid.rs`:

```rust
use crate::store::{FtsStore, FtsSearchResult, EmbeddingStore, EmbeddingSearchResult};
use crate::HollowError;
use rusqlite::Connection;
use std::collections::HashMap;

/// Combines FTS5 full-text and embedding vector search results
/// into a unified ranked list.
pub struct HybridSearcher;

#[derive(Debug, Clone)]
pub struct HybridResult {
    pub file_id: String,
    /// Combined score (higher = better match). Range 0..1.
    pub score: f32,
    /// FTS5 snippet if available.
    pub snippet: Option<String>,
    /// Whether this result came from FTS5, embedding, or both.
    pub sources: Vec<String>,
}

impl HybridSearcher {
    /// Run hybrid search: FTS5 for keyword matching + vector similarity for semantic matching.
    /// Uses Reciprocal Rank Fusion (RRF) to combine scores.
    ///
    /// If no embedding model is loaded (no query_embedding), falls back to FTS5 only.
    pub fn search(
        conn: &Connection,
        text_query: &str,
        query_embedding: Option<&[f32]>,
        limit: u32,
    ) -> Result<Vec<HybridResult>, HollowError> {
        let k: f32 = 60.0; // RRF constant

        let mut scores: HashMap<String, (f32, Option<String>, Vec<String>)> = HashMap::new();

        // FTS5 results
        if !text_query.is_empty() {
            let fts_results = FtsStore::search(conn, text_query, limit * 2)?;
            for (rank, result) in fts_results.iter().enumerate() {
                let rrf_score = 1.0 / (k + rank as f32 + 1.0);
                let entry = scores.entry(result.file_id.clone()).or_insert((0.0, None, Vec::new()));
                entry.0 += rrf_score;
                entry.1 = Some(result.snippet.clone());
                entry.2.push("fts".to_string());
            }
        }

        // Vector results
        if let Some(embedding) = query_embedding {
            let vec_results = EmbeddingStore::search(conn, embedding, limit * 2)?;
            for (rank, result) in vec_results.iter().enumerate() {
                if result.score < 0.3 {
                    continue; // Skip very low similarity results
                }
                let rrf_score = 1.0 / (k + rank as f32 + 1.0);
                let entry = scores.entry(result.file_id.clone()).or_insert((0.0, None, Vec::new()));
                entry.0 += rrf_score;
                if !entry.2.contains(&"embedding".to_string()) {
                    entry.2.push("embedding".to_string());
                }
            }
        }

        // Sort by combined score
        let mut results: Vec<HybridResult> = scores
            .into_iter()
            .map(|(file_id, (score, snippet, sources))| HybridResult {
                file_id,
                score,
                snippet,
                sources,
            })
            .collect();
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        Ok(results)
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
            inode: Some(id.as_bytes()[0] as i64),
            current_path: format!("/tmp/{}.txt", id),
            original_path: format!("/tmp/{}.txt", id),
            file_name: format!("{}.txt", id),
            extension: Some("txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            size_bytes: 100,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            modified_at: "2026-01-01T00:00:00Z".to_string(),
            ingested_at: "2026-01-01T00:00:00Z".to_string(),
            status: "indexed".to_string(),
            detected_mime: None,
            extension_mismatch: false,
        };
        FileStore::insert_file(&db.conn, record).unwrap();
    }

    #[test]
    fn test_hybrid_fts_only() {
        let db = test_db();
        insert_file(&db, "f1");
        insert_file(&db, "f2");
        FtsStore::index(&db.conn, "f1", "quarterly revenue report").unwrap();
        FtsStore::index(&db.conn, "f2", "meeting notes from Tuesday").unwrap();

        let results = HybridSearcher::search(&db.conn, "revenue", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, "f1");
        assert!(results[0].sources.contains(&"fts".to_string()));
    }

    #[test]
    fn test_hybrid_both_sources() {
        let db = test_db();
        insert_file(&db, "f1");
        insert_file(&db, "f2");
        FtsStore::index(&db.conn, "f1", "quarterly revenue report").unwrap();
        FtsStore::index(&db.conn, "f2", "meeting notes").unwrap();

        // f1 matches FTS; give f1 a high vector score too
        EmbeddingStore::upsert(&db.conn, "f1", &[0.9, 0.1, 0.0], "m", "t").unwrap();
        EmbeddingStore::upsert(&db.conn, "f2", &[0.0, 0.0, 1.0], "m", "t").unwrap();

        let query_emb = vec![1.0_f32, 0.0, 0.0];
        let results = HybridSearcher::search(&db.conn, "revenue", Some(&query_emb), 10).unwrap();

        // f1 should be top (matches both FTS and embedding)
        assert_eq!(results[0].file_id, "f1");
        assert!(results[0].sources.contains(&"fts".to_string()));
        assert!(results[0].sources.contains(&"embedding".to_string()));
    }

    #[test]
    fn test_hybrid_empty_query() {
        let db = test_db();
        let results = HybridSearcher::search(&db.conn, "", None, 10).unwrap();
        assert!(results.is_empty());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p hollow-core hybrid`
Expected: all PASS

- [ ] **Step 4: Commit**

```bash
git add hollow-core/src/search/
git commit -m "feat(search): add hybrid search combiner with Reciprocal Rank Fusion"
```

---

### Task 14: Hybrid search FFI + Swift UI update

**Files:**
- Modify: `hollow-core/src/lib.rs`
- Modify: `hollow/SearchView.swift`
- Modify: `hollow/HollowBridge.swift`

- [ ] **Step 1: Add hybrid_search FFI method**

Add to `#[uniffi::export] impl HollowCore`:

```rust
/// Hybrid search: combines full-text (FTS5) and semantic (embedding) search.
/// If embedding model is loaded, the query text is also embedded for vector search.
/// Falls back to FTS5-only if no model is available.
pub fn hybrid_search(&self, query: String, limit: u32) -> Result<Vec<SearchResult>, HollowError> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    // Try to embed the query for vector search
    let query_embedding: Option<Vec<f32>> = {
        let model_lock = self.embedding_model.lock()
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
        if let Some(ref model) = *model_lock {
            model.embed(&query).ok()
        } else {
            None
        }
    };

    let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;

    let hybrid_results = search::HybridSearcher::search(
        &db.conn,
        &query,
        query_embedding.as_deref(),
        limit,
    )?;

    let mut results = Vec::with_capacity(hybrid_results.len());
    for hr in hybrid_results {
        if let Some(record) = FileStore::get_file(&db.conn, &hr.file_id)? {
            results.push(SearchResult {
                file_id: hr.file_id,
                file_name: record.file_name,
                current_path: record.current_path,
                snippet: hr.snippet.unwrap_or_default(),
                rank: hr.score as f64,
            });
        }
    }
    Ok(results)
}
```

- [ ] **Step 2: Add hybrid_search to HollowBridge**

```swift
nonisolated func hybridSearch(query: String, limit: UInt32 = 50) -> [SearchResult] {
    guard let core else { return [] }
    do {
        return try core.hybridSearch(query: query, limit: limit)
    } catch {
        HollowLogger.search.error("Hybrid search failed: \(error)")
        return []
    }
}
```

- [ ] **Step 3: Update SearchView to use hybrid_search**

In `SearchView.swift`, change `performSearch()` to call `hybridSearch` instead of `search`:

```swift
private func performSearch() {
    isSearching = true
    let currentQuery = query
    DispatchQueue.global(qos: .userInitiated).async {
        let searchResults = HollowBridge.shared.hybridSearch(
            query: currentQuery,
            limit: 50
        )
        DispatchQueue.main.async {
            if query == currentQuery {
                results = searchResults
                isSearching = false
            }
        }
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build`
Expected: clean build

- [ ] **Step 5: Commit**

```bash
git add hollow-core/src/lib.rs hollow/SearchView.swift hollow/HollowBridge.swift hollow/Generated/
git commit -m "feat(search): hybrid search combining FTS5 and vector similarity via FFI"
```

---

### Task 15: Integration — trigger embedding after extraction

**Files:**
- Modify: `hollow/IngestionService.swift`

- [ ] **Step 1: Add EmbeddingService to IngestionService flow**

After a successful content extraction, queue the file for embedding. In the `ContentExtractionOperation`'s completion handler (or in `handleExtractionResult`), add:

```swift
// After extraction succeeds (status == "indexed"), queue for embedding
if result.status == "indexed" {
    embeddingService?.processAllPending()
}
```

The exact integration point depends on how IngestionService reports extraction results. The key pattern: after `extractContent` or `extractContentExternal` returns `"indexed"`, call `embeddingService.processAllPending()`.

- [ ] **Step 2: Wire EmbeddingService into the app**

Add `EmbeddingService` as an environment object in `hollowApp.swift`, similar to `IngestionService`.

- [ ] **Step 3: Manual end-to-end test**

1. Download the default embedding model from Settings → Models
2. Drop a text file into the Hollow Inbox
3. Wait for extraction + embedding (check logs)
4. Search for semantic concepts in the Search window
5. Verify results appear

- [ ] **Step 4: Commit**

```bash
git add hollow/IngestionService.swift hollow/hollowApp.swift
git commit -m "feat(embedding): trigger embedding after successful content extraction"
```

---

## Batch 3 Completion: Update engineering-status.md

### Task 16: Update docs

**Files:**
- Modify: `docs/engineering-status.md`

- [ ] **Step 1: Update engineering status**

Mark Batch 3 items as complete, update current status table, add key technical decisions (trigram tokenizer, Qwen3 default model, brute-force cosine, RRF hybrid scoring).

- [ ] **Step 2: Commit**

```bash
git add docs/engineering-status.md
git commit -m "docs: update engineering status for Batch 3 completion"
```
