use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct FileRecord {
    pub id: String,
    pub hash: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: String,
    pub summary: Option<String>,
    pub tags: Option<Vec<String>>,
    pub category: Option<String>,
    pub sensitivity: String,
    pub suggested_name: Option<String>,
    pub suggested_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub file_id: String,
    pub body_text: Option<String>,
    pub ocr_text: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLog {
    pub id: String,
    pub file_id: String,
    pub op_type: String,
    pub before_state: Option<String>,
    pub after_state: Option<String>,
    pub performed_at: String,
}
