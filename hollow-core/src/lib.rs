// hollow-core/src/lib.rs
mod db;
mod error;
mod content;
mod logging;
mod store;

pub use db::models::FileRecord;
pub use error::HollowError;
pub use logging::{LogEntry, LogLevel};

use content::pipeline::ContentPipeline;
use content::registry::default_registry;
use db::Database;
use store::{FileContentStore, FileStore};

use sha2::{Sha256, Digest};
use std::fs;
use std::io::{BufReader, Read as _};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Mutex;
use tracing::{info, debug};
use uuid::Uuid;

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ExtractContentResult {
    pub file_id: String,
    pub status: String,
    pub extractor_name: Option<String>,
    pub detected_mime: String,
    pub extension_mismatch: bool,
    pub body_text_bytes: u64,
    pub error: Option<String>,
}

#[derive(uniffi::Object)]
pub struct HollowCore {
    db: Mutex<Database>,
}

#[uniffi::export]
impl HollowCore {
    #[uniffi::constructor]
    pub fn new(db_path: String) -> Result<Self, HollowError> {
        logging::init_logging();
        let db = Database::open(&db_path)?;
        info!("HollowCore initialized, db: {}", db_path);
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
                    debug!("Duplicate skipped: {}", file_path);
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
                    debug!("Duplicate skipped: {}", file_path);
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
            Ok(()) => {
                info!("Ingested: {} ({} bytes)", record.file_name, record.size_bytes);
                Ok(record)
            }
            Err(HollowError::Database(msg)) if msg.contains("UNIQUE constraint") => {
                Err(HollowError::DuplicateFile(record.current_path))
            }
            Err(e) => Err(e),
        }
    }

    /// Heavy operation: reads entire file in 8KB chunks, computes SHA-256,
    /// updates the DB record. Call this from a background thread.
    pub fn compute_hash(&self, file_id: String) -> Result<String, HollowError> {
        debug!("Computing full hash for {}", file_id);
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
        info!("Hash computed for {}", file_id);

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
        info!("Marked missing: {}", path);
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::mark_missing_by_path(&db.conn, &path)
    }

    pub fn get_logs(&self, since_id: u64) -> Vec<LogEntry> {
        logging::get_logs_since(since_id)
    }

    pub fn clear_logs(&self) {
        logging::clear_log_buffer();
    }

    /// Run content extraction for a file. Updates file_content table and files.status.
    pub fn extract_content(&self, file_id: String) -> Result<ExtractContentResult, HollowError> {
        // Fetch record to get path + extension
        let (current_path, original_extension) = {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            let record = FileStore::get_file(&db.conn, &file_id)?
                .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
            (record.current_path, record.extension)
        };

        let path = Path::new(&current_path);
        let pipeline = ContentPipeline::new(default_registry());
        let outcome = pipeline.process(path, original_extension.as_deref());

        let extracted_at = iso8601_now();
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;

        // Record detected mime + mismatch on files row
        FileStore::update_detected_mime(
            &db.conn,
            &file_id,
            &outcome.detected_mime,
            outcome.extension_mismatch,
        )?;

        let body_text_bytes: u64;
        if outcome.status == "indexed" {
            let body_text = outcome.body_text.clone().unwrap_or_default();
            body_text_bytes = body_text.len() as u64;
            let compressed = zstd::encode_all(body_text.as_bytes(), 3)
                .map_err(|e| HollowError::Database(format!("zstd encode: {}", e)))?;
            FileContentStore::upsert(
                &db.conn,
                &file_id,
                &compressed,
                body_text_bytes as i64,
                outcome.encoding.as_deref(),
                outcome.extractor_name.as_deref().unwrap_or("Unknown"),
                &extracted_at,
            )?;
            FileStore::update_status(&db.conn, &file_id, "indexed")?;
            info!(
                "Extracted content: {} ({} bytes, {})",
                file_id,
                body_text_bytes,
                outcome.extractor_name.as_deref().unwrap_or("?")
            );
        } else {
            body_text_bytes = 0;
            FileContentStore::upsert_error(
                &db.conn,
                &file_id,
                outcome.error.as_deref().unwrap_or("unknown error"),
                outcome.extractor_name.as_deref(),
                &extracted_at,
            )?;
            FileStore::update_status(&db.conn, &file_id, "extract_failed")?;
            info!(
                "Extraction failed: {} ({})",
                file_id,
                outcome.error.as_deref().unwrap_or("?")
            );
        }

        Ok(ExtractContentResult {
            file_id,
            status: outcome.status,
            extractor_name: outcome.extractor_name,
            detected_mime: outcome.detected_mime,
            extension_mismatch: outcome.extension_mismatch,
            body_text_bytes,
            error: outcome.error,
        })
    }

    /// Recompute quick_hash and compare with stored value.
    pub fn has_changed(&self, file_id: String) -> Result<bool, HollowError> {
        let (current_path, old_quick_hash) = {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            let record = FileStore::get_file(&db.conn, &file_id)?
                .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
            (record.current_path, record.quick_hash)
        };

        let path = Path::new(&current_path);
        if !path.exists() {
            return Err(HollowError::FileNotFound(current_path));
        }

        let metadata = fs::metadata(path)
            .map_err(|e| HollowError::InvalidInput(e.to_string()))?;
        let new_quick_hash = compute_quick_hash(path, metadata.len())?;

        if new_quick_hash != old_quick_hash {
            // Persist the new hash so subsequent calls don't keep reporting "changed"
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            FileStore::update_quick_hash(&db.conn, &file_id, &new_quick_hash)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Flip an indexed file back to pending so it will be re-extracted.
    pub fn mark_for_reextraction(&self, file_id: String) -> Result<(), HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        FileStore::mark_for_reextraction(&db.conn, &file_id)
    }

    /// Alias for get_pending_ids with a name that matches the new pipeline.
    pub fn get_pending_extraction_ids(&self) -> Result<Vec<String>, HollowError> {
        self.get_pending_ids()
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

    #[test]
    fn test_extract_content_plain_text() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_extract", "note.txt", b"hello from test");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(record.status, "pending");

        let result = core.extract_content(record.id.clone()).unwrap();
        assert_eq!(result.status, "indexed");
        assert!(result.extractor_name.is_some());
        assert_eq!(result.detected_mime, "text/plain");
        assert!(!result.extension_mismatch);
        assert_eq!(result.body_text_bytes, 15);

        let updated = core.get_file(record.id.clone()).unwrap().unwrap();
        assert_eq!(updated.status, "indexed");

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_extract_content_unknown_format_fails_gracefully() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_extract_bad", "blob.bin", &[0xFF, 0xFE, 0x00, 0x01]);

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        let result = core.extract_content(record.id.clone()).unwrap();

        assert_eq!(result.status, "extract_failed");
        assert!(result.error.is_some());

        let updated = core.get_file(record.id).unwrap().unwrap();
        assert_eq!(updated.status, "extract_failed");

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_has_changed_detects_content_change() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_changed", "file.txt", b"version one");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        assert!(!core.has_changed(record.id.clone()).unwrap());

        // Modify file
        std::fs::write(&path, b"version two is longer").unwrap();
        assert!(core.has_changed(record.id.clone()).unwrap());

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_mark_for_reextraction_flips_status_to_pending() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_reex", "file.txt", b"hello");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        core.extract_content(record.id.clone()).unwrap();

        let after_extract = core.get_file(record.id.clone()).unwrap().unwrap();
        assert_eq!(after_extract.status, "indexed");

        core.mark_for_reextraction(record.id.clone()).unwrap();
        let after_mark = core.get_file(record.id.clone()).unwrap().unwrap();
        assert_eq!(after_mark.status, "pending");

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_get_pending_extraction_ids_method() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let dir = std::env::temp_dir().join("hollow_t_pending_ex");
        fs::create_dir_all(&dir).unwrap();

        let f1 = dir.join("a.txt");
        fs::write(&f1, b"a").unwrap();
        let f2 = dir.join("b.txt");
        fs::write(&f2, b"b").unwrap();

        core.ingest_file(f1.to_string_lossy().to_string()).unwrap();
        core.ingest_file(f2.to_string_lossy().to_string()).unwrap();

        let pending = core.get_pending_extraction_ids().unwrap();
        assert_eq!(pending.len(), 2);

        cleanup(&[&dir]);
    }
}
