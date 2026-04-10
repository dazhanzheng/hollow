use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct FileRecord {
    pub id: String,
    pub hash: String,
    pub quick_hash: String,
    pub inode: Option<i64>,
    pub current_path: String,
    pub original_path: String,
    pub file_name: String,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub created_at: String,
    pub modified_at: String,
    pub ingested_at: String,
    pub status: String,
    pub detected_mime: Option<String>,
    pub extension_mismatch: bool,
}
