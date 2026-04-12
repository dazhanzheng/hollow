use rusqlite::Connection;
use crate::HollowError;

pub struct EmbeddingStore;

#[derive(Debug, Clone)]
pub struct EmbeddingSearchResult {
    pub file_id: String,
    pub score: f32,
}

impl EmbeddingStore {
    /// Insert or replace an embedding for a file.
    pub fn upsert(
        conn: &Connection,
        file_id: &str,
        embedding: &[f32],
        model_name: &str,
        embedded_at: &str,
    ) -> Result<(), HollowError> {
        let bytes = f32_slice_to_bytes(embedding);
        let dimensions = embedding.len() as i64;
        conn.execute(
            "INSERT INTO embeddings (file_id, embedding, dimensions, model_name, embedded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(file_id) DO UPDATE SET
                 embedding   = excluded.embedding,
                 dimensions  = excluded.dimensions,
                 model_name  = excluded.model_name,
                 embedded_at = excluded.embedded_at",
            rusqlite::params![file_id, bytes, dimensions, model_name, embedded_at],
        )?;
        Ok(())
    }

    /// Retrieve the embedding and model name for a file.
    /// Returns `None` if no embedding exists for the given file_id.
    pub fn get(conn: &Connection, file_id: &str) -> Result<Option<(Vec<f32>, String)>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT embedding, model_name FROM embeddings WHERE file_id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![file_id], |row| {
            let bytes: Vec<u8> = row.get(0)?;
            let model: String = row.get(1)?;
            Ok((bytes, model))
        })?;
        match rows.next() {
            Some(row) => {
                let (bytes, model) = row?;
                Ok(Some((bytes_to_f32_vec(&bytes), model)))
            }
            None => Ok(None),
        }
    }

    /// Brute-force cosine similarity search over all stored embeddings.
    /// Returns results sorted by descending similarity score.
    pub fn search(
        conn: &Connection,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<EmbeddingSearchResult>, HollowError> {
        let mut stmt = conn.prepare("SELECT file_id, embedding FROM embeddings")?;
        let rows = stmt.query_map([], |row| {
            let file_id: String = row.get(0)?;
            let bytes: Vec<u8> = row.get(1)?;
            Ok((file_id, bytes))
        })?;

        let mut results: Vec<EmbeddingSearchResult> = Vec::new();
        for row in rows {
            let (file_id, bytes) = row?;
            let vec = bytes_to_f32_vec(&bytes);
            let score = cosine_similarity(query_embedding, &vec);
            results.push(EmbeddingSearchResult { file_id, score });
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }

    /// Return file IDs that have status="indexed" but no corresponding row in the embeddings table.
    pub fn get_pending_ids(conn: &Connection) -> Result<Vec<String>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT id FROM files
             WHERE status = 'indexed'
               AND id NOT IN (SELECT file_id FROM embeddings)",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &f in v {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::store::FileStore;
    use crate::db::models::FileRecord;

    fn test_db() -> Database {
        Database::open(":memory:").unwrap()
    }

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
    fn test_upsert_and_get() {
        let db = test_db();
        insert_file(&db, "f1", 1);

        let vec = vec![0.1_f32, 0.2, 0.3, 0.4];
        EmbeddingStore::upsert(&db.conn, "f1", &vec, "text-embedding-3-small", "2026-01-01T00:00:00Z").unwrap();

        let result = EmbeddingStore::get(&db.conn, "f1").unwrap().expect("should have embedding");
        let (stored_vec, model) = result;

        assert_eq!(model, "text-embedding-3-small");
        assert_eq!(stored_vec.len(), 4);
        for (a, b) in stored_vec.iter().zip(vec.iter()) {
            assert!((a - b).abs() < 1e-6, "value mismatch: {} vs {}", a, b);
        }
    }

    #[test]
    fn test_cosine_search() {
        let db = test_db();
        insert_file(&db, "f1", 1);
        insert_file(&db, "f2", 2);
        insert_file(&db, "f3", 3);

        EmbeddingStore::upsert(&db.conn, "f1", &[0.9_f32, 0.1, 0.0, 0.0], "model", "2026-01-01T00:00:00Z").unwrap();
        EmbeddingStore::upsert(&db.conn, "f2", &[0.0_f32, 0.0, 1.0, 0.0], "model", "2026-01-01T00:00:00Z").unwrap();
        EmbeddingStore::upsert(&db.conn, "f3", &[-0.9_f32, -0.1, 0.0, 0.0], "model", "2026-01-01T00:00:00Z").unwrap();

        let query = vec![1.0_f32, 0.0, 0.0, 0.0];
        let results = EmbeddingStore::search(&db.conn, &query, 3).unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].file_id, "f1", "f1 should be the top result");
        assert!(results[0].score > results[1].score, "scores should be descending");
        assert!(results[0].score > 0.0);
        assert!(results[2].score < 0.0, "f3 should have negative similarity to [1,0,0,0]");
    }

    #[test]
    fn test_get_pending_ids() {
        let db = test_db();
        insert_file(&db, "f1", 1);
        insert_file(&db, "f2", 2);

        // Only embed f1
        EmbeddingStore::upsert(&db.conn, "f1", &[1.0_f32, 0.0], "model", "2026-01-01T00:00:00Z").unwrap();

        let pending = EmbeddingStore::get_pending_ids(&db.conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], "f2");
    }

    #[test]
    fn test_upsert_overwrites() {
        let db = test_db();
        insert_file(&db, "f1", 1);

        EmbeddingStore::upsert(&db.conn, "f1", &[1.0_f32, 0.0, 0.0], "model-v1", "2026-01-01T00:00:00Z").unwrap();
        EmbeddingStore::upsert(&db.conn, "f1", &[0.0_f32, 1.0, 0.0], "model-v2", "2026-02-01T00:00:00Z").unwrap();

        let (stored_vec, model) = EmbeddingStore::get(&db.conn, "f1").unwrap().expect("should exist");
        assert_eq!(model, "model-v2");
        assert!((stored_vec[0] - 0.0).abs() < 1e-6);
        assert!((stored_vec[1] - 1.0).abs() < 1e-6);
        assert!((stored_vec[2] - 0.0).abs() < 1e-6);

        // Verify only one row exists
        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM embeddings WHERE file_id = 'f1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
