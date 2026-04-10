use rusqlite::Connection;

use crate::db::models::FileRecord;
use crate::HollowError;

pub struct FileStore;

impl FileStore {
    pub fn insert_file(conn: &Connection, record: FileRecord) -> Result<(), HollowError> {
        conn.execute(
            "INSERT INTO files (id, hash, quick_hash, inode, current_path, original_path, file_name, extension, mime_type, size_bytes, created_at, modified_at, ingested_at, status, detected_mime, extension_mismatch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                record.id,
                record.hash,
                record.quick_hash,
                record.inode,
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
                record.detected_mime,
                record.extension_mismatch as i64,
            ],
        )?;
        Ok(())
    }

    fn record_from_row(row: &rusqlite::Row) -> rusqlite::Result<FileRecord> {
        Ok(FileRecord {
            id: row.get(0)?,
            hash: row.get(1)?,
            quick_hash: row.get(2)?,
            inode: row.get(3)?,
            current_path: row.get(4)?,
            original_path: row.get(5)?,
            file_name: row.get(6)?,
            extension: row.get(7)?,
            mime_type: row.get(8)?,
            size_bytes: row.get(9)?,
            created_at: row.get(10)?,
            modified_at: row.get(11)?,
            ingested_at: row.get(12)?,
            status: row.get(13)?,
            detected_mime: row.get(14)?,
            extension_mismatch: row.get::<_, i64>(15)? != 0,
        })
    }

    const SELECT_COLS: &str = "id, hash, quick_hash, inode, current_path, original_path, file_name, extension, mime_type, size_bytes, created_at, modified_at, ingested_at, status, detected_mime, extension_mismatch";

    pub fn get_file(conn: &Connection, id: &str) -> Result<Option<FileRecord>, HollowError> {
        let sql = format!("SELECT {} FROM files WHERE id = ?1", Self::SELECT_COLS);
        let mut stmt = conn.prepare(&sql)?;

        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::record_from_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_files(
        conn: &Connection,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FileRecord>, HollowError> {
        let sql = format!(
            "SELECT {} FROM files ORDER BY ingested_at DESC LIMIT ?1 OFFSET ?2",
            Self::SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;

        let records = stmt
            .query_map(rusqlite::params![limit, offset], |row| Self::record_from_row(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    pub fn update_status(conn: &Connection, id: &str, status: &str) -> Result<(), HollowError> {
        let rows_updated = conn.execute(
            "UPDATE files SET status = ?1 WHERE id = ?2",
            rusqlite::params![status, id],
        )?;
        if rows_updated == 0 {
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

    pub fn path_exists(conn: &Connection, path: &str) -> Result<bool, HollowError> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE current_path = ?1",
            rusqlite::params![path],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn inode_exists(conn: &Connection, inode: i64) -> Result<bool, HollowError> {
        // Only count non-missing files — missing files' inodes are stale
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE inode = ?1 AND status != 'missing'",
            rusqlite::params![inode],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn path_status(conn: &Connection, path: &str) -> Result<Option<String>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT status FROM files WHERE current_path = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_by_path(conn: &Connection, path: &str) -> Result<(), HollowError> {
        conn.execute(
            "DELETE FROM files WHERE current_path = ?1",
            rusqlite::params![path],
        )?;
        Ok(())
    }

    pub fn mark_missing_by_path(conn: &Connection, path: &str) -> Result<(), HollowError> {
        conn.execute(
            "UPDATE files SET status = 'missing', inode = NULL WHERE current_path = ?1",
            rusqlite::params![path],
        )?;
        Ok(())
    }

    pub fn update_hash(conn: &Connection, id: &str, hash: &str) -> Result<(), HollowError> {
        let updated = conn.execute(
            "UPDATE files SET hash = ?1 WHERE id = ?2",
            rusqlite::params![hash, id],
        )?;
        if updated == 0 {
            return Err(HollowError::FileNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn get_ids_by_status(conn: &Connection, status: &str) -> Result<Vec<String>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT id FROM files WHERE status = ?1 ORDER BY ingested_at ASC",
        )?;
        let ids = stmt
            .query_map(rusqlite::params![status], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }

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
            quick_hash: "abcd1234".to_string(),
            inode: Some(12345),
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
            detected_mime: None,
            extension_mismatch: false,
        }
    }

    #[test]
    fn test_insert_and_get_file() {
        let db = test_db();
        let record = sample_record();
        FileStore::insert_file(&db.conn, record.clone()).unwrap();

        let fetched = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
        assert_eq!(fetched.id, record.id);
        assert_eq!(fetched.hash, record.hash);
        assert_eq!(fetched.inode, record.inode);
        assert_eq!(fetched.current_path, record.current_path);
        assert_eq!(fetched.file_name, record.file_name);
        assert_eq!(fetched.size_bytes, record.size_bytes);
    }

    #[test]
    fn test_get_nonexistent_file() {
        let db = test_db();
        let result = FileStore::get_file(&db.conn, "nonexistent-id").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_files_with_pagination() {
        let db = test_db();

        let record1 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000001".to_string(),
            ingested_at: "2026-04-09T11:00:00Z".to_string(),
            current_path: "/Users/test/Documents/file1.pdf".to_string(),
            inode: Some(111),
            ..sample_record()
        };
        let record2 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000002".to_string(),
            ingested_at: "2026-04-09T12:00:00Z".to_string(),
            current_path: "/Users/test/Documents/file2.pdf".to_string(),
            inode: Some(222),
            ..sample_record()
        };

        FileStore::insert_file(&db.conn, record1.clone()).unwrap();
        FileStore::insert_file(&db.conn, record2.clone()).unwrap();

        let all = FileStore::list_files(&db.conn, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, record2.id);
    }

    #[test]
    fn test_update_status() {
        let db = test_db();
        let record = sample_record();
        FileStore::insert_file(&db.conn, record.clone()).unwrap();

        FileStore::update_status(&db.conn, &record.id, "indexed").unwrap();

        let fetched = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
        assert_eq!(fetched.status, "indexed");
    }

    #[test]
    fn test_update_status_nonexistent() {
        let db = test_db();
        let result = FileStore::update_status(&db.conn, "nonexistent-id", "indexed");
        assert!(matches!(result, Err(HollowError::FileNotFound(_))));
    }

    #[test]
    fn test_check_duplicate() {
        let db = test_db();
        let record = sample_record();

        assert!(!FileStore::check_duplicate(&db.conn, &record.hash).unwrap());
        FileStore::insert_file(&db.conn, record.clone()).unwrap();
        assert!(FileStore::check_duplicate(&db.conn, &record.hash).unwrap());
    }

    #[test]
    fn test_inode_exists() {
        let db = test_db();
        let record = sample_record();

        assert!(!FileStore::inode_exists(&db.conn, 12345).unwrap());
        FileStore::insert_file(&db.conn, record).unwrap();
        assert!(FileStore::inode_exists(&db.conn, 12345).unwrap());
    }

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
    fn test_update_quick_hash_method() {
        let db = test_db();
        let record = sample_record();
        FileStore::insert_file(&db.conn, record.clone()).unwrap();

        FileStore::update_quick_hash(&db.conn, &record.id, "newhash").unwrap();

        let fetched = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
        assert_eq!(fetched.quick_hash, "newhash");
    }

    #[test]
    fn test_same_hash_different_paths() {
        let db = test_db();

        let record1 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000001".to_string(),
            current_path: "/Users/test/Documents/copy1.pdf".to_string(),
            inode: Some(111),
            ..sample_record()
        };
        let record2 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000002".to_string(),
            current_path: "/Users/test/Documents/copy2.pdf".to_string(),
            inode: Some(222),
            ..sample_record()
        };

        FileStore::insert_file(&db.conn, record1).unwrap();
        FileStore::insert_file(&db.conn, record2).unwrap();

        let all = FileStore::list_files(&db.conn, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
    }
}
