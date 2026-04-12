use crate::store::{FtsStore, EmbeddingStore};
use crate::HollowError;
use rusqlite::Connection;
use std::collections::HashMap;

pub struct HybridSearcher;

#[derive(Debug, Clone)]
pub struct HybridResult {
    pub file_id: String,
    pub score: f32,
    pub snippet: Option<String>,
    pub sources: Vec<String>,
}

impl HybridSearcher {
    /// Hybrid search with Reciprocal Rank Fusion.
    /// Falls back to FTS5-only if query_embedding is None.
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
            let vec_results = EmbeddingStore::search(conn, embedding, (limit * 2) as usize)?;
            for (rank, result) in vec_results.iter().enumerate() {
                if result.score < 0.5 { continue; }
                let rrf_score = 1.0 / (k + rank as f32 + 1.0);
                let entry = scores.entry(result.file_id.clone()).or_insert((0.0, None, Vec::new()));
                entry.0 += rrf_score;
                if !entry.2.contains(&"embedding".to_string()) {
                    entry.2.push("embedding".to_string());
                }
            }
        }

        let mut results: Vec<HybridResult> = scores
            .into_iter()
            .map(|(file_id, (score, snippet, sources))| HybridResult {
                file_id, score, snippet, sources,
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

    fn test_db() -> Database { Database::open(":memory:").unwrap() }

    fn insert_file(db: &Database, id: &str, inode: i64) {
        let record = FileRecord {
            id: id.to_string(),
            hash: "".to_string(),
            quick_hash: "qh".to_string(),
            inode: Some(inode),
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
        insert_file(&db, "f1", 1);
        insert_file(&db, "f2", 2);
        FtsStore::index(&db.conn, "f1", "quarterly revenue report").unwrap();
        FtsStore::index(&db.conn, "f2", "meeting notes from Tuesday").unwrap();

        let results = HybridSearcher::search(&db.conn, "revenue report", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, "f1");
        assert!(results[0].sources.contains(&"fts".to_string()));
    }

    #[test]
    fn test_hybrid_both_sources() {
        let db = test_db();
        insert_file(&db, "f1", 1);
        insert_file(&db, "f2", 2);
        FtsStore::index(&db.conn, "f1", "quarterly revenue report").unwrap();
        FtsStore::index(&db.conn, "f2", "meeting notes").unwrap();

        EmbeddingStore::upsert(&db.conn, "f1", &[0.9, 0.1, 0.0], "m", "t").unwrap();
        EmbeddingStore::upsert(&db.conn, "f2", &[0.0, 0.0, 1.0], "m", "t").unwrap();

        let query_emb = vec![1.0_f32, 0.0, 0.0];
        let results = HybridSearcher::search(&db.conn, "revenue report", Some(&query_emb), 10).unwrap();

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
