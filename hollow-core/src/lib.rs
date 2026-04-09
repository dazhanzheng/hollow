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
use std::io::{BufReader, Read as _};
use std::os::unix::fs::MetadataExt;
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

    /// Fast intake: only reads fs metadata, no file content read.
    /// Returns immediately with hash="", status="pending".
    pub fn ingest_file(&self, file_path: String) -> Result<FileRecord, HollowError> {
        let path = Path::new(&file_path);
        if !path.exists() {
            return Err(HollowError::FileNotFound(file_path.clone()));
        }

        let fs_metadata = fs::metadata(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;

        let inode = Some(fs_metadata.ino() as i64);

        // Dedup: inode check (survives rename/move), then path check
        {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            if let Some(ino) = inode {
                if FileStore::inode_exists(&db.conn, ino)? {
                    return Err(HollowError::DuplicateFile(file_path.clone()));
                }
            }
            // If path exists with status "missing", remove old record to make room
            // (handles: user deleted file, then dropped a new file with same name)
            match FileStore::path_status(&db.conn, &file_path)? {
                Some(status) if status == "missing" => {
                    FileStore::delete_by_path(&db.conn, &file_path)?;
                }
                Some(_) => {
                    return Err(HollowError::DuplicateFile(file_path.clone()));
                }
                None => {} // path not in DB, proceed
            }
        }

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_string());

        let mime_type = extension.as_deref().and_then(|ext| {
            mime_guess::from_ext(ext)
                .first()
                .map(|m| m.to_string())
        });

        let created_at = system_time_to_rfc3339(fs_metadata.created().ok());
        let modified_at = system_time_to_rfc3339(fs_metadata.modified().ok());
        let ingested_at = iso8601_now();

        // Quick hash: sample 5 points across the file (< 1ms for any file size)
        let quick_hash = compute_quick_hash(path, fs_metadata.len())?;

        let record = FileRecord {
            id: Uuid::now_v7().to_string(),
            hash: String::new(), // full hash only on demand
            quick_hash,
            inode,
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
        match FileStore::insert_file(&db.conn, record.clone()) {
            Ok(()) => Ok(record),
            Err(HollowError::Database(msg)) if msg.contains("UNIQUE constraint") => {
                Err(HollowError::DuplicateFile(record.current_path))
            }
            Err(e) => Err(e),
        }
    }

    /// Heavy operation: reads entire file in 8KB chunks, computes SHA-256,
    /// updates the DB record. Call this from a background thread.
    pub fn compute_hash(&self, file_id: String) -> Result<String, HollowError> {
        // Get the file record to find its path
        let current_path = {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            let record = FileStore::get_file(&db.conn, &file_id)?
                .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
            record.current_path
        };

        let path = Path::new(&current_path);
        if !path.exists() {
            return Err(HollowError::FileNotFound(current_path));
        }

        // Stream-based SHA-256
        let file = fs::File::open(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
        let mut reader = BufReader::new(file);
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = reader.read(&mut buf)
                .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
        let hash = format!("{:x}", hasher.finalize());

        // Update the record in DB
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::update_hash(&db.conn, &file_id, &hash)?;

        Ok(hash)
    }

    /// Returns IDs of all files with status="pending" (not yet fully processed).
    pub fn get_pending_ids(&self) -> Result<Vec<String>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::get_ids_by_status(&db.conn, "pending")
    }

    /// Mark a file as fully processed.
    pub fn mark_indexed(&self, file_id: String) -> Result<(), HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::update_status(&db.conn, &file_id, "indexed")
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

    pub fn path_exists(&self, path: String) -> Result<bool, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::path_exists(&db.conn, &path)
    }

    /// Mark a file as missing (deleted from filesystem).
    /// Clears inode so it won't block a new file with the same inode.
    pub fn mark_missing(&self, path: String) -> Result<(), HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::mark_missing_by_path(&db.conn, &path)
    }
}

/// Quick hash: SHA-256 of file_size + 5 sampled 4KB blocks.
/// Deterministic — same file always produces same hash.
/// Covers head, 25%, 50%, 75%, tail. For files <20KB, reads entire file.
fn compute_quick_hash(path: &Path, file_size: u64) -> Result<String, HollowError> {
    use std::io::{Seek, SeekFrom};

    const BLOCK_SIZE: u64 = 4096;
    const SMALL_FILE_THRESHOLD: u64 = BLOCK_SIZE * 5; // 20KB

    let mut file = fs::File::open(path)
        .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
    let mut hasher = Sha256::new();

    // Include file size in hash so different-sized files always differ
    hasher.update(file_size.to_le_bytes());

    if file_size <= SMALL_FILE_THRESHOLD {
        // Small file: read everything
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
        hasher.update(&buf);
    } else {
        // Sample 5 positions: 0, 25%, 50%, 75%, end-4KB
        let positions = [
            0,
            file_size / 4,
            file_size / 2,
            file_size * 3 / 4,
            file_size.saturating_sub(BLOCK_SIZE),
        ];

        let mut buf = [0u8; BLOCK_SIZE as usize];
        for pos in positions {
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
            let n = file.read(&mut buf)
                .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
            hasher.update(&buf[..n]);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
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
    fn test_ingest_is_instant_no_hash() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_quick", "test.txt", b"hello hollow");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.file_name, "test.txt");
        assert_eq!(record.extension, Some("txt".to_string()));
        assert_eq!(record.status, "pending");
        assert_eq!(record.size_bytes, 12);
        assert_eq!(record.hash, ""); // hash not computed yet

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_compute_hash_updates_record() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_hash", "hashme.txt", b"hello hollow");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.hash, "");

        let hash = core.compute_hash(record.id.clone()).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 hex

        // Verify DB was updated
        let updated = core.get_file(record.id).unwrap().unwrap();
        assert_eq!(updated.hash, hash);

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_get_pending_ids() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let dir = std::env::temp_dir().join("hollow_t_pending");
        fs::create_dir_all(&dir).unwrap();

        let f1 = dir.join("a.txt");
        fs::write(&f1, b"aaa").unwrap();
        let f2 = dir.join("b.txt");
        fs::write(&f2, b"bbb").unwrap();

        let r1 = core.ingest_file(f1.to_string_lossy().to_string()).unwrap();
        let r2 = core.ingest_file(f2.to_string_lossy().to_string()).unwrap();

        let pending = core.get_pending_ids().unwrap();
        assert_eq!(pending.len(), 2);

        // Process one, check pending drops to 1
        core.compute_hash(r1.id.clone()).unwrap();
        core.mark_indexed(r1.id).unwrap();

        let pending = core.get_pending_ids().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], r2.id);

        cleanup(&[&dir]);
    }

    #[test]
    fn test_mime_type_detection() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();

        let pdf_path = make_temp_file("hollow_t_mime2", "doc.pdf", b"fake pdf");
        let record = core.ingest_file(pdf_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.mime_type, Some("application/pdf".to_string()));

        let txt_path = make_temp_file("hollow_t_mime2", "note.txt", b"plain text");
        let record = core.ingest_file(txt_path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.mime_type, Some("text/plain".to_string()));

        cleanup(&[&pdf_path.parent().unwrap()]);
    }

    #[test]
    fn test_path_dedup() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_pathdup", "same.txt", b"content");

        core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // Same path again → DuplicateFile
        let result = core.ingest_file(path.to_string_lossy().to_string());
        assert!(matches!(result, Err(HollowError::DuplicateFile(_))));

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_same_content_different_paths_both_accepted() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let dir = std::env::temp_dir().join("hollow_t_samecontent");
        fs::create_dir_all(&dir).unwrap();

        let f1 = dir.join("copy1.txt");
        fs::write(&f1, b"identical content").unwrap();
        let f2 = dir.join("copy2.txt");
        fs::write(&f2, b"identical content").unwrap();

        // Both should be accepted (no hash-based rejection)
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
