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

        let content = fs::read(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = format!("{:x}", hasher.finalize());

        // Check for duplicate before inserting
        {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            if FileStore::check_duplicate(&db.conn, &hash)? {
                return Err(HollowError::DuplicateFile(hash));
            }
        }

        let fs_metadata = fs::metadata(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_string());

        // MIME type from extension
        let mime_type = extension.as_deref().and_then(|ext| {
            mime_guess::from_ext(ext)
                .first()
                .map(|m| m.to_string())
        });

        let created_at = system_time_to_rfc3339(fs_metadata.created().ok());
        let modified_at = system_time_to_rfc3339(fs_metadata.modified().ok());
        let ingested_at = iso8601_now();

        let record = FileRecord {
            id: Uuid::now_v7().to_string(),
            hash,
            current_path: file_path.clone(),
            original_path: file_path,
            file_name,
            extension,
            mime_type,
            size_bytes: fs_metadata.len() as i64,
            created_at,
            modified_at,
            ingested_at,
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

fn system_time_to_rfc3339(time: Option<std::time::SystemTime>) -> String {
    match time {
        Some(t) => {
            let offset_dt = time::OffsetDateTime::from(t);
            offset_dt
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| iso8601_now())
        }
        None => iso8601_now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_file(dir_name: &str, file_name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join(file_name);
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(content).unwrap();
        file_path
    }

    fn cleanup(paths: &[&std::path::Path]) {
        for p in paths {
            if p.is_file() {
                fs::remove_file(p).ok();
            } else if p.is_dir() {
                fs::remove_dir_all(p).ok();
            }
        }
    }

    #[test]
    fn test_ingest_and_get() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t1", "test.txt", b"hello hollow");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.file_name, "test.txt");
        assert_eq!(record.extension, Some("txt".to_string()));
        assert_eq!(record.status, "pending");
        assert_eq!(record.size_bytes, 12);

        let retrieved = core.get_file(record.id.clone()).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().hash, record.hash);

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_mime_type_detection() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();

        let pdf_path = make_temp_file("hollow_t_mime", "doc.pdf", b"fake pdf");
        let record = core.ingest_file(pdf_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.mime_type, Some("application/pdf".to_string()));

        let txt_path = make_temp_file("hollow_t_mime", "note.txt", b"plain text");
        let record = core.ingest_file(txt_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.mime_type, Some("text/plain".to_string()));

        let unknown_path = make_temp_file("hollow_t_mime", "data.xyzabc", b"unknown");
        let record = core.ingest_file(unknown_path.to_string_lossy().to_string()).unwrap();
        assert!(record.mime_type.is_none());

        cleanup(&[&pdf_path.parent().unwrap()]);
    }

    #[test]
    fn test_real_timestamps() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_time", "ts.txt", b"timestamp test");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // created_at and modified_at should be valid RFC3339 strings
        assert!(record.created_at.contains("T"));
        assert!(record.modified_at.contains("T"));
        assert!(record.ingested_at.contains("T"));

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_duplicate_detection() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_dup", "dup.txt", b"duplicate content");

        // First ingest succeeds
        core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // Second ingest of same file returns DuplicateFile error
        let result = core.ingest_file(path.to_string_lossy().to_string());
        assert!(result.is_err());
        match result {
            Err(HollowError::DuplicateFile(_)) => {} // expected
            other => panic!("expected DuplicateFile, got {:?}", other),
        }

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_list_files() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let dir = std::env::temp_dir().join("hollow_t_list");
        fs::create_dir_all(&dir).unwrap();

        let f1 = dir.join("a.txt");
        fs::write(&f1, b"aaa").unwrap();
        let f2 = dir.join("b.txt");
        fs::write(&f2, b"bbb").unwrap();

        core.ingest_file(f1.to_string_lossy().to_string()).unwrap();
        core.ingest_file(f2.to_string_lossy().to_string()).unwrap();

        let files = core.list_files(10, 0).unwrap();
        assert_eq!(files.len(), 2);

        cleanup(&[&dir]);
    }

    #[test]
    fn test_ingest_nonexistent_file() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let result = core.ingest_file("/nonexistent/path/file.txt".to_string());
        assert!(result.is_err());
    }
}
