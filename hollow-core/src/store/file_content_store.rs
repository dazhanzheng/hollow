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
