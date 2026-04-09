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
        FileStore::insert_file(&db.conn, record.clone())?;
        Ok(record)
    }

    pub fn get_file(&self, id: String) -> Result<Option<FileRecord>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::get_file(&db.conn, &id)
    }

    pub fn list_files(&self, limit: u32, offset: u32) -> Result<Vec<FileRecord>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::list_files(&db.conn, limit as i64, offset as i64)
    }

    pub fn check_duplicate(&self, hash: String) -> Result<bool, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::check_duplicate(&db.conn, &hash)
    }
}

fn iso8601_now() -> String {
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
