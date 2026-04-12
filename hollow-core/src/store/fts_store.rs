use rusqlite::Connection;
use crate::HollowError;

pub struct FtsStore;

#[derive(Debug, Clone)]
pub struct FtsSearchResult {
    pub file_id: String,
    pub snippet: String,
    pub rank: f64,
}

impl FtsStore {
    /// Insert or replace body text into the FTS5 index for a file.
    /// Idempotent — safe to re-call on re-extraction.
    pub fn index(conn: &Connection, file_id: &str, body_text: &str) -> Result<(), HollowError> {
        // Delete existing entry first (FTS5 doesn't have ON CONFLICT)
        conn.execute("DELETE FROM file_content_fts WHERE file_id = ?1", rusqlite::params![file_id])?;
        conn.execute(
            "INSERT INTO file_content_fts (file_id, body_text) VALUES (?1, ?2)",
            rusqlite::params![file_id, body_text],
        )?;
        Ok(())
    }

    /// Remove a file from the FTS5 index.
    pub fn remove(conn: &Connection, file_id: &str) -> Result<(), HollowError> {
        conn.execute("DELETE FROM file_content_fts WHERE file_id = ?1", rusqlite::params![file_id])?;
        Ok(())
    }

    /// Full-text search. Returns results ranked by FTS5 relevance.
    /// Snippet highlights matches with <b> tags.
    /// For queries shorter than 3 chars (trigram minimum), falls back to
    /// a LIKE scan on the FTS5 content.
    pub fn search(conn: &Connection, query: &str, limit: u32) -> Result<Vec<FtsSearchResult>, HollowError> {
        let trimmed = query.trim();
        if trimmed.is_empty() || trimmed.chars().count() < 3 {
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(
            "SELECT file_id, snippet(file_content_fts, 1, '<b>', '</b>', '…', 32), rank
             FROM file_content_fts
             WHERE body_text MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![trimmed, limit], |row| {
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

        // FTS5 trigram tokenizer requires at least 3 Unicode characters per term;
        // "合同文" is a 3-char substring that includes "合同" and is present in the text.
        let results = FtsStore::search(&db.conn, "合同文", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, "f1");
        assert!(results[0].snippet.contains("合同"));
    }

    #[test]
    fn test_search_english() {
        let db = test_db();
        insert_file(&db, "f2");
        FtsStore::index(&db.conn, "f2", "The quick brown fox jumps over the lazy dog").unwrap();

        let results = FtsStore::search(&db.conn, "brown fox", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, "f2");
    }

    #[test]
    fn test_search_no_match() {
        let db = test_db();
        insert_file(&db, "f3");
        FtsStore::index(&db.conn, "f3", "hello world").unwrap();

        let results = FtsStore::search(&db.conn, "zzzznotfound", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_multiple_results_ranked() {
        let db = test_db();

        let file_ids = ["inv1", "inv2", "other"];
        let texts = [
            "invoice number 1001 for services rendered",
            "invoice number 1002 for consulting work",
            "receipt for office supplies purchased",
        ];

        for (i, (id, text)) in file_ids.iter().zip(texts.iter()).enumerate() {
            let record = FileRecord {
                id: id.to_string(),
                hash: "".to_string(),
                quick_hash: "qh".to_string(),
                inode: Some(i as i64 + 1),
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
            FtsStore::index(&db.conn, id, text).unwrap();
        }

        let results = FtsStore::search(&db.conn, "invoice", 10).unwrap();
        assert_eq!(results.len(), 2);
        let ids: Vec<&str> = results.iter().map(|r| r.file_id.as_str()).collect();
        assert!(ids.contains(&"inv1"));
        assert!(ids.contains(&"inv2"));
    }

    #[test]
    fn test_index_idempotent() {
        let db = test_db();
        insert_file(&db, "f4");

        FtsStore::index(&db.conn, "f4", "version one content here").unwrap();
        FtsStore::index(&db.conn, "f4", "version two content here").unwrap();

        let results_v2 = FtsStore::search(&db.conn, "\"version two\"", 10).unwrap();
        assert_eq!(results_v2.len(), 1);

        let results_v1 = FtsStore::search(&db.conn, "\"version one\"", 10).unwrap();
        assert_eq!(results_v1.len(), 0);
    }

    #[test]
    fn test_remove() {
        let db = test_db();
        insert_file(&db, "f5");
        FtsStore::index(&db.conn, "f5", "document about contracts and agreements").unwrap();

        let before = FtsStore::search(&db.conn, "contracts", 10).unwrap();
        assert_eq!(before.len(), 1);

        FtsStore::remove(&db.conn, "f5").unwrap();

        let after = FtsStore::search(&db.conn, "contracts", 10).unwrap();
        assert!(after.is_empty());
    }
}
