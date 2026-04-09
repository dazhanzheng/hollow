use rusqlite::Connection;

use crate::db::models::FileRecord;
use crate::HollowError;

pub struct FileStore;

impl FileStore {
    pub fn insert_file(conn: &Connection, record: FileRecord) -> Result<(), HollowError> {
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

        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(FileRecord {
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
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_files(
        conn: &Connection,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<FileRecord>, HollowError> {
        let mut stmt = conn.prepare(
            "SELECT id, hash, current_path, original_path, file_name, extension, mime_type, size_bytes, created_at, modified_at, ingested_at, status
             FROM files ORDER BY ingested_at DESC LIMIT ?1 OFFSET ?2",
        )?;

        let records = stmt.query_map(rusqlite::params![limit, offset], |row| {
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
        })?
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

    pub fn delete_file(conn: &Connection, id: &str) -> Result<(), HollowError> {
        let rows_deleted = conn.execute("DELETE FROM files WHERE id = ?1", rusqlite::params![id])?;
        if rows_deleted == 0 {
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
        FileStore::insert_file(&db.conn, record.clone()).unwrap();

        let fetched = FileStore::get_file(&db.conn, &record.id).unwrap().unwrap();
        assert_eq!(fetched.id, record.id);
        assert_eq!(fetched.hash, record.hash);
        assert_eq!(fetched.current_path, record.current_path);
        assert_eq!(fetched.original_path, record.original_path);
        assert_eq!(fetched.file_name, record.file_name);
        assert_eq!(fetched.extension, record.extension);
        assert_eq!(fetched.mime_type, record.mime_type);
        assert_eq!(fetched.size_bytes, record.size_bytes);
        assert_eq!(fetched.created_at, record.created_at);
        assert_eq!(fetched.modified_at, record.modified_at);
        assert_eq!(fetched.ingested_at, record.ingested_at);
        assert_eq!(fetched.status, record.status);
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
            ..sample_record()
        };
        let record2 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000002".to_string(),
            ingested_at: "2026-04-09T12:00:00Z".to_string(),
            current_path: "/Users/test/Documents/file2.pdf".to_string(),
            ..sample_record()
        };

        FileStore::insert_file(&db.conn, record1.clone()).unwrap();
        FileStore::insert_file(&db.conn, record2.clone()).unwrap();

        // Most recent first
        let all = FileStore::list_files(&db.conn, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, record2.id);
        assert_eq!(all[1].id, record1.id);

        // Limit 1
        let limited = FileStore::list_files(&db.conn, 1, 0).unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].id, record2.id);

        // Offset 1
        let offset = FileStore::list_files(&db.conn, 10, 1).unwrap();
        assert_eq!(offset.len(), 1);
        assert_eq!(offset[0].id, record1.id);
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
    fn test_delete_file() {
        let db = test_db();
        let record = sample_record();
        FileStore::insert_file(&db.conn, record.clone()).unwrap();

        FileStore::delete_file(&db.conn, &record.id).unwrap();

        let fetched = FileStore::get_file(&db.conn, &record.id).unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_check_duplicate() {
        let db = test_db();
        let record = sample_record();

        let before = FileStore::check_duplicate(&db.conn, &record.hash).unwrap();
        assert!(!before);

        FileStore::insert_file(&db.conn, record.clone()).unwrap();

        let after = FileStore::check_duplicate(&db.conn, &record.hash).unwrap();
        assert!(after);
    }

    #[test]
    fn test_same_hash_different_paths() {
        let db = test_db();

        let record1 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000001".to_string(),
            current_path: "/Users/test/Documents/copy1.pdf".to_string(),
            ..sample_record()
        };
        let record2 = FileRecord {
            id: "01961234-5678-7abc-def0-000000000002".to_string(),
            current_path: "/Users/test/Documents/copy2.pdf".to_string(),
            ..sample_record()
        };

        FileStore::insert_file(&db.conn, record1).unwrap();
        FileStore::insert_file(&db.conn, record2).unwrap();

        let all = FileStore::list_files(&db.conn, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
    }
}
