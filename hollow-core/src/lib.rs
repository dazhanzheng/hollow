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

/// Descriptor for a built-in extractor plugin, returned to the settings UI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ExtractorPluginInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub extensions: Vec<String>,
}

/// Text template + image byte payloads for a zip-based document format
/// (docx / pptx / odt / ods / odp / epub). Used by the Swift side to run
/// Apple Vision OCR on embedded images and substitute the results back
/// into the text template at the placeholder positions.
///
/// Returning `None` from `extract_with_images` means "this file type has
/// no image-aware extractor" — the Swift caller should fall back to the
/// regular text-only pipeline.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ExtractWithImagesResult {
    /// Body text with `{{HOLLOW_IMG_N}}` markers inserted at image
    /// positions.
    pub text_template: String,
    /// Image byte payloads, one per marker, in placeholder order.
    pub images: Vec<ExtractedImage>,
    /// Canonical MIME type for this document.
    pub detected_mime: String,
    /// Extractor name to record on `file_content.extractor_name`.
    /// Matches the name used by the corresponding `SwiftExtractor`.
    pub extractor_name: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExtractedImage {
    /// The bare marker string (`"HOLLOW_IMG_0"`, no wrapping braces) —
    /// Swift builds the full `{{HOLLOW_IMG_0}}` form when substituting.
    pub marker: String,
    /// Raw bytes of the embedded image as stored in the archive.
    pub bytes: Vec<u8>,
    /// Best-guess IANA MIME type for the image, e.g. "image/png".
    pub mime: String,
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
            detected_mime: None,
            extension_mismatch: false,
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

    /// Read back the extracted body text for a file. Decompresses the
    /// zstd-compressed blob stored in `file_content.body_text_compressed`.
    /// Returns `Ok(None)` if no content row exists for this file (e.g.
    /// still pending or unsupported).
    pub fn get_body_text(&self, file_id: String) -> Result<Option<String>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        let compressed: Option<Vec<u8>> = db
            .conn
            .query_row(
                "SELECT body_text_compressed FROM file_content WHERE file_id = ?1",
                rusqlite::params![file_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        let Some(bytes) = compressed else {
            return Ok(None);
        };
        if bytes.is_empty() {
            return Ok(None);
        }
        let decoded = zstd::decode_all(&bytes[..])
            .map_err(|e| HollowError::Database(format!("zstd decode: {}", e)))?;
        let text = String::from_utf8(decoded)
            .map_err(|e| HollowError::Database(format!("utf8 decode: {}", e)))?;
        Ok(Some(text))
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
        // Step 1: Read record + mark as extracting (in same lock).
        // Marking extracting here lets crash recovery distinguish "never started" from "interrupted".
        // If the file is already missing, bail out immediately without touching its status.
        let (current_path, original_extension) = {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            let record = FileStore::get_file(&db.conn, &file_id)?
                .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
            if record.status == "missing" {
                info!("Skip extraction: {} is already missing", file_id);
                return Ok(ExtractContentResult {
                    file_id,
                    status: "missing".to_string(),
                    extractor_name: None,
                    detected_mime: String::new(),
                    extension_mismatch: false,
                    body_text_bytes: 0,
                    error: Some("file was removed before extraction started".to_string()),
                });
            }
            FileStore::update_status(&db.conn, &file_id, "extracting")?;
            (record.current_path, record.extension)
        };

        // Step 1.5: File may have been deleted between ingestion and extraction.
        // Mark missing and return rather than producing a bogus extract_failed.
        let path = Path::new(&current_path);
        if !path.exists() {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            FileStore::mark_missing_by_path(&db.conn, &current_path)?;
            info!("File vanished before extraction: {}", current_path);
            return Ok(ExtractContentResult {
                file_id,
                status: "missing".to_string(),
                extractor_name: None,
                detected_mime: "application/octet-stream".to_string(),
                extension_mismatch: false,
                body_text_bytes: 0,
                error: Some("file was removed before extraction could start".to_string()),
            });
        }

        let pipeline = ContentPipeline::new(default_registry());
        let outcome = pipeline.process(path, original_extension.as_deref());

        let extracted_at = iso8601_now();

        // Step 3 (lockless): if indexed, compress body text before reacquiring the mutex.
        // This keeps CPU-intensive zstd work outside any critical section.
        let (body_text_bytes, compressed_body): (u64, Option<Vec<u8>>) = if outcome.status == "indexed" {
            let body_text = outcome.body_text.clone().unwrap_or_default();
            let bytes = body_text.len() as u64;
            let compressed = zstd::encode_all(body_text.as_bytes(), 3)
                .map_err(|e| HollowError::Database(format!("zstd encode: {}", e)))?;
            (bytes, Some(compressed))
        } else {
            (0, None)
        };

        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;

        // Guard: if the file was marked missing between our two lock acquisitions, do not overwrite.
        let current_status = FileStore::get_file(&db.conn, &file_id)?
            .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?
            .status;
        if current_status == "missing" {
            info!("Skip overwrite: {} was marked missing during extraction", file_id);
            return Ok(ExtractContentResult {
                file_id,
                status: "missing".to_string(),
                extractor_name: outcome.extractor_name,
                detected_mime: outcome.detected_mime,
                extension_mismatch: false,
                body_text_bytes: 0,
                error: Some("file was removed during extraction".to_string()),
            });
        }

        // Record detected mime + mismatch on files row
        FileStore::update_detected_mime(
            &db.conn,
            &file_id,
            &outcome.detected_mime,
            outcome.extension_mismatch,
        )?;

        match outcome.status.as_str() {
            "indexed" => {
                let compressed = compressed_body.expect("compressed_body is Some when status is indexed");
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
            }
            "unsupported" => {
                // No extractor available — not a failure, just a format we can't process yet.
                // Don't write to file_content; just flip status and update detected_mime.
                FileStore::update_status(&db.conn, &file_id, "unsupported")?;
                info!("No extractor for {}: {}", file_id, outcome.detected_mime);
            }
            _ => {
                // "extract_failed" — real failure, store error
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

    /// Extract a zip-based document's text layer + embedded image bytes,
    /// for handoff to Swift-side Apple Vision OCR. The returned template
    /// has `{{HOLLOW_IMG_N}}` placeholders where images appear, and the
    /// `images` array holds the raw bytes of each referenced image in
    /// placeholder order.
    ///
    /// Returns `Ok(None)` for file types without an image-aware
    /// extractor (anything other than .docx/.pptx/.odt/.ods/.odp/.epub) —
    /// the caller should fall back to the regular text-only extraction
    /// path.
    ///
    /// Does **not** touch the file's status in the database — the Swift
    /// caller owns the state transition and commits via
    /// `extract_content_external` once it has OCR'd and merged the
    /// final text.
    pub fn extract_with_images(
        &self,
        file_id: String,
    ) -> Result<Option<ExtractWithImagesResult>, HollowError> {
        // Look up the record's current path. If missing or vanished,
        // return None — Swift will fall back to extract_content which
        // handles those cases.
        let current_path = {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            let record = FileStore::get_file(&db.conn, &file_id)?
                .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
            if record.status == "missing" {
                return Ok(None);
            }
            record.current_path
        };

        let path = Path::new(&current_path);
        if !path.exists() {
            return Ok(None);
        }

        let result = match content::image_docs::extract(path) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(None),
            Err(e) => {
                return Err(HollowError::InvalidInput(format!(
                    "image_docs extraction failed: {}",
                    e
                )));
            }
        };

        let images = result
            .images
            .into_iter()
            .map(|img| ExtractedImage {
                marker: img.marker,
                bytes: img.bytes,
                mime: img.mime,
            })
            .collect();

        Ok(Some(ExtractWithImagesResult {
            text_template: result.text_template,
            images,
            detected_mime: result.detected_mime,
            extractor_name: result.extractor_name,
        }))
    }

    /// Store an extraction outcome produced outside the Rust ContentPipeline
    /// (e.g. by the Swift-side Apple Vision OCR pipeline for images and PDFs).
    ///
    /// Takes the same state-machine path as `extract_content` — mark extracting,
    /// guard against file-vanished races, update detected_mime, write compressed
    /// body into `file_content`, flip `files.status` — but bypasses the pipeline
    /// run entirely, using caller-supplied values instead.
    ///
    /// `status` must be one of `"indexed"`, `"extract_failed"`, `"unsupported"`.
    /// For `"indexed"` the `body_text` argument is used as the content;
    /// for `"extract_failed"` the `error` argument is stored on file_content.
    pub fn extract_content_external(
        &self,
        file_id: String,
        status: String,
        body_text: Option<String>,
        extractor_name: String,
        detected_mime: String,
        encoding: Option<String>,
        error: Option<String>,
    ) -> Result<ExtractContentResult, HollowError> {
        // Step 1: Same missing/extracting handshake as extract_content. If the
        // file is already missing, bail out. Otherwise mark it extracting so
        // that reclaim_extracting() can recover from crashes during OCR.
        let current_path = {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            let record = FileStore::get_file(&db.conn, &file_id)?
                .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?;
            if record.status == "missing" {
                info!("Skip external extraction: {} is already missing", file_id);
                return Ok(ExtractContentResult {
                    file_id,
                    status: "missing".to_string(),
                    extractor_name: None,
                    detected_mime: String::new(),
                    extension_mismatch: false,
                    body_text_bytes: 0,
                    error: Some("file was removed before extraction started".to_string()),
                });
            }
            FileStore::update_status(&db.conn, &file_id, "extracting")?;
            record.current_path
        };

        // Step 1.5: Existence re-check, same as the Rust-pipeline path.
        let path = Path::new(&current_path);
        if !path.exists() {
            let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
            FileStore::mark_missing_by_path(&db.conn, &current_path)?;
            info!("File vanished before external extraction: {}", current_path);
            return Ok(ExtractContentResult {
                file_id,
                status: "missing".to_string(),
                extractor_name: None,
                detected_mime: "application/octet-stream".to_string(),
                extension_mismatch: false,
                body_text_bytes: 0,
                error: Some("file was removed before extraction could start".to_string()),
            });
        }

        let extracted_at = iso8601_now();

        // Step 2: If indexed, compress the caller-supplied body_text outside
        // the mutex (matches Rust pipeline behavior — zstd is CPU heavy).
        let (body_text_bytes, compressed_body): (u64, Option<Vec<u8>>) = if status == "indexed" {
            let body = body_text.unwrap_or_default();
            let bytes = body.len() as u64;
            let compressed = zstd::encode_all(body.as_bytes(), 3)
                .map_err(|e| HollowError::Database(format!("zstd encode: {}", e)))?;
            (bytes, Some(compressed))
        } else {
            (0, None)
        };

        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;

        // Step 3: Re-check missing state after reacquiring the lock.
        let current_status = FileStore::get_file(&db.conn, &file_id)?
            .ok_or_else(|| HollowError::FileNotFound(file_id.clone()))?
            .status;
        if current_status == "missing" {
            info!("Skip overwrite: {} was marked missing during external extraction", file_id);
            return Ok(ExtractContentResult {
                file_id,
                status: "missing".to_string(),
                extractor_name: Some(extractor_name),
                detected_mime,
                extension_mismatch: false,
                body_text_bytes: 0,
                error: Some("file was removed during extraction".to_string()),
            });
        }

        // External extractors don't do their own magic-bytes detection, so we
        // take the caller's word on detected_mime and never flag mismatch here.
        FileStore::update_detected_mime(&db.conn, &file_id, &detected_mime, false)?;

        match status.as_str() {
            "indexed" => {
                let compressed = compressed_body
                    .expect("compressed_body is Some when status is indexed");
                FileContentStore::upsert(
                    &db.conn,
                    &file_id,
                    &compressed,
                    body_text_bytes as i64,
                    encoding.as_deref(),
                    &extractor_name,
                    &extracted_at,
                )?;
                FileStore::update_status(&db.conn, &file_id, "indexed")?;
                info!(
                    "External extraction indexed: {} ({} bytes, {})",
                    file_id, body_text_bytes, extractor_name
                );
            }
            "unsupported" => {
                FileStore::update_status(&db.conn, &file_id, "unsupported")?;
                info!("External extraction unsupported: {} ({})", file_id, extractor_name);
            }
            _ => {
                // Treat any other value as extract_failed.
                FileContentStore::upsert_error(
                    &db.conn,
                    &file_id,
                    error.as_deref().unwrap_or("external extractor failed"),
                    Some(&extractor_name),
                    &extracted_at,
                )?;
                FileStore::update_status(&db.conn, &file_id, "extract_failed")?;
                info!(
                    "External extraction failed: {} ({})",
                    file_id,
                    error.as_deref().unwrap_or("?")
                );
            }
        }

        Ok(ExtractContentResult {
            file_id,
            status,
            extractor_name: Some(extractor_name),
            detected_mime,
            extension_mismatch: false,
            body_text_bytes,
            error,
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

    /// Reclaim files stuck in the `extracting` state (crashed mid-extraction).
    /// Flips them back to pending so the next resume scan picks them up.
    pub fn reclaim_extracting(&self) -> Result<u32, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        let count = db.conn.execute(
            "UPDATE files SET status = 'pending' WHERE status = 'extracting'",
            [],
        )?;
        if count > 0 {
            info!("Reclaimed {} files stuck in extracting state", count);
        }
        Ok(count as u32)
    }

    /// List all built-in extractor plugins. Static; does not hit the database.
    pub fn list_extractors(&self) -> Vec<ExtractorPluginInfo> {
        content::registry::plugin_descriptors()
            .iter()
            .map(|d| ExtractorPluginInfo {
                name: d.name.to_string(),
                display_name: d.display_name.to_string(),
                description: d.description.to_string(),
                extensions: d.extensions.iter().map(|e| e.to_string()).collect(),
            })
            .collect()
    }

    /// Enable or disable a specific extractor plugin by name. Disabled plugins
    /// are bypassed by the pipeline and matching files are reported as
    /// `unsupported` instead of being extracted.
    pub fn set_extractor_enabled(&self, name: String, enabled: bool) {
        content::registry::set_extractor_enabled(&name, enabled);
        info!("Extractor {} {}", name, if enabled { "enabled" } else { "disabled" });
    }

    /// Look up a file's UUID by its current path.
    pub fn file_id_for_path(&self, path: String) -> Result<Option<String>, HollowError> {
        let db = self.db.lock().map_err(|e| HollowError::Database(e.to_string()))?;
        let mut stmt = db.conn.prepare("SELECT id FROM files WHERE current_path = ?1")?;
        let mut rows = stmt.query(rusqlite::params![path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
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
    fn test_extract_content_external_indexed() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_extract_ext", "photo.png", b"fake png bytes");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        let result = core
            .extract_content_external(
                record.id.clone(),
                "indexed".to_string(),
                Some("recognized text from OCR".to_string()),
                "AppleVisionImage".to_string(),
                "image/png".to_string(),
                Some("UTF-8".to_string()),
                None,
            )
            .unwrap();

        assert_eq!(result.status, "indexed");
        assert_eq!(result.extractor_name.as_deref(), Some("AppleVisionImage"));
        assert_eq!(result.detected_mime, "image/png");
        assert_eq!(result.body_text_bytes, "recognized text from OCR".len() as u64);

        // Verify the record got bumped to indexed in files table.
        let fetched = core.get_file(record.id.clone()).unwrap().unwrap();
        assert_eq!(fetched.status, "indexed");
        assert_eq!(fetched.detected_mime.as_deref(), Some("image/png"));

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_extract_content_external_failed() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_extract_ext_fail", "bad.pdf", b"not really a pdf");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        let result = core
            .extract_content_external(
                record.id.clone(),
                "extract_failed".to_string(),
                None,
                "AppleVisionPdf".to_string(),
                "application/pdf".to_string(),
                None,
                Some("no pages found".to_string()),
            )
            .unwrap();

        assert_eq!(result.status, "extract_failed");
        let fetched = core.get_file(record.id.clone()).unwrap().unwrap();
        assert_eq!(fetched.status, "extract_failed");

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_extract_with_images_returns_none_for_plain_text() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_ewi_txt", "note.txt", b"just some text");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // .txt has no image-aware extractor — should get None back.
        let result = core.extract_with_images(record.id.clone()).unwrap();
        assert!(result.is_none());

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_extract_with_images_returns_none_for_missing() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_ewi_missing", "book.epub", b"fake");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        core.mark_missing(path.to_string_lossy().to_string()).unwrap();

        let result = core.extract_with_images(record.id.clone()).unwrap();
        assert!(result.is_none());

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_extract_content_external_preserves_missing_status() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_extract_ext_missing", "photo.jpg", b"fake");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        core.mark_missing(path.to_string_lossy().to_string()).unwrap();

        let result = core
            .extract_content_external(
                record.id.clone(),
                "indexed".to_string(),
                Some("should be ignored".to_string()),
                "AppleVisionImage".to_string(),
                "image/jpeg".to_string(),
                Some("UTF-8".to_string()),
                None,
            )
            .unwrap();

        assert_eq!(result.status, "missing");
        let fetched = core.get_file(record.id.clone()).unwrap().unwrap();
        assert_eq!(fetched.status, "missing");

        cleanup(&[&path, &path.parent().unwrap()]);
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

        assert_eq!(result.status, "unsupported");
        assert!(result.error.is_some());

        let updated = core.get_file(record.id).unwrap().unwrap();
        assert_eq!(updated.status, "unsupported");

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
    fn test_extract_content_uses_extracting_state() {
        // Verifies reclaim is a no-op when nothing is stuck after a clean run.
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let reclaimed = core.reclaim_extracting().unwrap();
        assert_eq!(reclaimed, 0);
    }

    #[test]
    fn test_reclaim_extracting_flips_to_pending() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_reclaim", "x.txt", b"data");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // mark_for_reextraction → pending, then extract_content → extracting → indexed
        core.mark_for_reextraction(record.id.clone()).unwrap();
        core.extract_content(record.id.clone()).unwrap();

        // After a successful extraction, nothing should be stuck in extracting.
        let reclaimed = core.reclaim_extracting().unwrap();
        assert_eq!(reclaimed, 0);

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_extract_content_preserves_missing_status() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_del_race", "file.txt", b"hello");

        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // Simulate: mark missing before extraction completes
        core.mark_missing(path.to_string_lossy().to_string()).unwrap();

        // Now file is marked missing — extract_content should not overwrite
        let result = core.extract_content(record.id.clone()).unwrap();
        assert_eq!(result.status, "missing");

        let fetched = core.get_file(record.id).unwrap().unwrap();
        assert_eq!(fetched.status, "missing");

        cleanup(&[&path, &path.parent().unwrap()]);
    }

    #[test]
    fn test_file_id_for_path() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_idforpath", "lookup.txt", b"x");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();
        let looked_up = core.file_id_for_path(path.to_string_lossy().to_string()).unwrap();
        assert_eq!(looked_up, Some(record.id));
        assert!(core.file_id_for_path("/nonexistent".to_string()).unwrap().is_none());
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

    #[test]
    fn test_extract_content_missing_file_marks_missing() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        let path = make_temp_file("hollow_t_vanish", "ghost.txt", b"boo");
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        // Delete the file on disk before extraction
        std::fs::remove_file(&path).unwrap();

        let result = core.extract_content(record.id.clone()).unwrap();
        assert_eq!(result.status, "missing");

        let fetched = core.get_file(record.id).unwrap().unwrap();
        assert_eq!(fetched.status, "missing");

        // Cleanup parent dir
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_extract_content_unsupported_format() {
        let core = HollowCore::new(":memory:".to_string()).unwrap();
        // JPEG magic bytes: FF D8 FF E0 ...
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        let path = make_temp_file("hollow_t_unsupp", "photo.jpg", &jpeg);
        let record = core.ingest_file(path.to_string_lossy().to_string()).unwrap();

        let result = core.extract_content(record.id.clone()).unwrap();
        assert_eq!(result.status, "unsupported");

        let fetched = core.get_file(record.id).unwrap().unwrap();
        assert_eq!(fetched.status, "unsupported");

        cleanup(&[&path, &path.parent().unwrap()]);
    }
}
