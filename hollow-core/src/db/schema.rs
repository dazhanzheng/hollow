pub const SCHEMA_VERSION: u32 = 1;

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
}
