pub mod schema;
pub mod models;

use crate::HollowError;

pub struct Database {
    pub(crate) conn: rusqlite::Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self, HollowError> {
        let conn = if path == ":memory:" {
            rusqlite::Connection::open_in_memory()
        } else {
            rusqlite::Connection::open(path)
        }
        .map_err(HollowError::from)?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(HollowError::from)?;
        schema::migrate(&conn).map_err(HollowError::from)?;

        Ok(Database { conn })
    }
}
