// Database schema for hollow-core.
//
// Policy (pre-v1.0): this file holds a single, always-current initial schema.
// Until hollow ships its first user-facing release, schema changes are made
// by editing MIGRATION_V1 directly and deleting any local dev databases.
// Once we ship v1.0, we will freeze this file and start writing ALTER-based
// migrations (v2, v3, ...) to protect real user data.

const MIGRATION_V1: &str = "
CREATE TABLE files (
    id                 TEXT    PRIMARY KEY,
    hash               TEXT    NOT NULL,
    quick_hash         TEXT    NOT NULL DEFAULT '',
    inode              INTEGER,
    current_path       TEXT    NOT NULL UNIQUE,
    original_path      TEXT    NOT NULL,
    file_name          TEXT    NOT NULL,
    extension          TEXT,
    mime_type          TEXT,
    detected_mime      TEXT,
    extension_mismatch INTEGER NOT NULL DEFAULT 0,
    size_bytes         INTEGER NOT NULL,
    created_at         TEXT    NOT NULL,
    modified_at        TEXT    NOT NULL,
    ingested_at        TEXT    NOT NULL,
    status             TEXT    NOT NULL DEFAULT 'pending'
);

CREATE INDEX idx_files_hash        ON files(hash);
CREATE INDEX idx_files_status      ON files(status);
CREATE INDEX idx_files_ingested_at ON files(ingested_at);
CREATE INDEX idx_files_inode       ON files(inode);
CREATE INDEX idx_files_quick_hash  ON files(quick_hash);

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
    file_id              TEXT    PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    body_text_compressed BLOB,
    body_text_bytes      INTEGER,
    encoding             TEXT,
    extractor_name       TEXT,
    extracted_at         TEXT,
    extract_error        TEXT
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

CREATE VIRTUAL TABLE file_content_fts USING fts5(
    file_id UNINDEXED,
    body_text,
    tokenize = 'trigram'
);

CREATE TABLE embeddings (
    file_id       TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    embedding     BLOB NOT NULL,
    dimensions    INTEGER NOT NULL,
    model_name    TEXT NOT NULL,
    embedded_at   TEXT NOT NULL
);
";

pub fn migrate(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let current_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current_version < 1 {
        conn.execute_batch(MIGRATION_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
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
        assert_eq!(version, 1);
    }

    #[test]
    fn test_migrate_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 1);
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
    fn test_files_table_has_all_columns() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(files)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for expected in [
            "id", "hash", "quick_hash", "inode", "current_path", "original_path",
            "file_name", "extension", "mime_type", "detected_mime", "extension_mismatch",
            "size_bytes", "created_at", "modified_at", "ingested_at", "status",
        ] {
            assert!(cols.contains(&expected.to_string()), "missing column: {}", expected);
        }
    }

    #[test]
    fn test_file_content_table_has_all_columns() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(file_content)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for expected in [
            "file_id", "body_text_compressed", "body_text_bytes", "encoding",
            "extractor_name", "extracted_at", "extract_error",
        ] {
            assert!(cols.contains(&expected.to_string()), "missing column: {}", expected);
        }
    }

    #[test]
    fn test_fts5_table_exists_after_migration() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='file_content_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_embeddings_table_exists_after_migration() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrate(&conn).unwrap();
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='embeddings'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
