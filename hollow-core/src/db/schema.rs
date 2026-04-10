pub const SCHEMA_VERSION: u32 = 5;

const MIGRATION_V1: &str = "
CREATE TABLE files (
    id           TEXT PRIMARY KEY,
    hash         TEXT NOT NULL,
    current_path TEXT NOT NULL UNIQUE,
    original_path TEXT NOT NULL,
    file_name    TEXT NOT NULL,
    extension    TEXT,
    mime_type    TEXT,
    size_bytes   INTEGER NOT NULL,
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL,
    ingested_at  TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX idx_files_hash ON files(hash);
CREATE INDEX idx_files_status ON files(status);
CREATE INDEX idx_files_ingested_at ON files(ingested_at);

CREATE TABLE file_metadata (
    file_id        TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    summary        TEXT,
    tags           TEXT,
    category       TEXT,
    sensitivity    TEXT DEFAULT 'normal',
    suggested_name TEXT,
    suggested_path TEXT
);

CREATE TABLE file_content (
    file_id   TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    body_text TEXT,
    ocr_text  TEXT,
    source    TEXT
);

CREATE TABLE operations_log (
    id           TEXT PRIMARY KEY,
    file_id      TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    op_type      TEXT NOT NULL,
    before_state TEXT,
    after_state  TEXT,
    performed_at TEXT NOT NULL
);

CREATE INDEX idx_operations_log_file_time ON operations_log(file_id, performed_at);
";

const MIGRATION_V2: &str = "
ALTER TABLE files ADD COLUMN inode INTEGER;
CREATE INDEX idx_files_inode ON files(inode);
";

const MIGRATION_V3: &str = "
ALTER TABLE files ADD COLUMN quick_hash TEXT NOT NULL DEFAULT '';
CREATE INDEX idx_files_quick_hash ON files(quick_hash);
";

const MIGRATION_V4: &str = "
ALTER TABLE file_content ADD COLUMN body_text_compressed BLOB;
ALTER TABLE file_content ADD COLUMN body_text_bytes INTEGER;
ALTER TABLE file_content ADD COLUMN encoding TEXT;
ALTER TABLE file_content ADD COLUMN extracted_at TEXT;
ALTER TABLE file_content ADD COLUMN extractor_name TEXT;
ALTER TABLE file_content ADD COLUMN extract_error TEXT;

ALTER TABLE files ADD COLUMN detected_mime TEXT;
ALTER TABLE files ADD COLUMN extension_mismatch INTEGER NOT NULL DEFAULT 0;

UPDATE files SET status = 'pending' WHERE status = 'indexed';
";

// V5: re-queue all extract_failed files for re-extraction. Before v5 the
// pipeline conflated "no extractor available" with real failures; now that
// `unsupported` is a distinct status, flip failed rows back to pending so
// the next startup scan classifies them correctly.
const MIGRATION_V5: &str = "
UPDATE files SET status = 'pending' WHERE status = 'extract_failed';
";

pub fn migrate(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let current_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current_version < 1 {
        conn.execute_batch(MIGRATION_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    if current_version < 2 {
        conn.execute_batch(MIGRATION_V2)?;
        conn.pragma_update(None, "user_version", 2)?;
    }

    if current_version < 3 {
        conn.execute_batch(MIGRATION_V3)?;
        conn.pragma_update(None, "user_version", 3)?;
    }

    if current_version < 4 {
        conn.execute_batch(MIGRATION_V4)?;
        conn.pragma_update(None, "user_version", 4)?;
    }

    if current_version < 5 {
        conn.execute_batch(MIGRATION_V5)?;
        conn.pragma_update(None, "user_version", 5)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_fresh_database() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_migrate_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_tables_exist_after_migration() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"file_metadata".to_string()));
        assert!(tables.contains(&"file_content".to_string()));
        assert!(tables.contains(&"operations_log".to_string()));
    }

    #[test]
    fn test_migration_v4_adds_content_columns() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();

        // file_content new columns
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(file_content)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(cols.contains(&"body_text_compressed".to_string()));
        assert!(cols.contains(&"body_text_bytes".to_string()));
        assert!(cols.contains(&"encoding".to_string()));
        assert!(cols.contains(&"extracted_at".to_string()));
        assert!(cols.contains(&"extractor_name".to_string()));
        assert!(cols.contains(&"extract_error".to_string()));

        // files new columns
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(files)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(cols.contains(&"detected_mime".to_string()));
        assert!(cols.contains(&"extension_mismatch".to_string()));
    }

    #[test]
    fn test_migration_v4_resets_indexed_to_pending() {
        // Simulate an existing v3 DB with indexed files, then run v4
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        // Apply v1..v3 manually
        conn.execute_batch(MIGRATION_V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(MIGRATION_V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(MIGRATION_V3).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();

        // Insert a file with status indexed
        conn.execute(
            "INSERT INTO files (id, hash, quick_hash, current_path, original_path, file_name, size_bytes, created_at, modified_at, ingested_at, status) VALUES ('a', '', '', '/a.txt', '/a.txt', 'a.txt', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'indexed')",
            [],
        ).unwrap();

        // Run full migrate (should apply v4)
        migrate(&conn).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM files WHERE id = 'a'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_migration_v5_requeues_extract_failed() {
        // Simulate a v4 DB with an extract_failed row
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(MIGRATION_V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(MIGRATION_V3).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute_batch(MIGRATION_V4).unwrap();
        conn.pragma_update(None, "user_version", 4).unwrap();

        conn.execute(
            "INSERT INTO files (id, hash, quick_hash, current_path, original_path, file_name, size_bytes, created_at, modified_at, ingested_at, status) VALUES ('ef', '', '', '/ef.jpg', '/ef.jpg', 'ef.jpg', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'extract_failed')",
            [],
        ).unwrap();

        migrate(&conn).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM files WHERE id = 'ef'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }
}
