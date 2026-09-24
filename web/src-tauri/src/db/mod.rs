use rusqlite::{Connection, Result as SqlResult};
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn new(db_path: &Path) -> SqlResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)?;
        let db = Database {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS collections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                parent_id TEXT REFERENCES collections(id),
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS requests (
                id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL REFERENCES collections(id),
                name TEXT NOT NULL DEFAULT 'Untitled',
                protocol TEXT NOT NULL DEFAULT 'http',
                method TEXT NOT NULL DEFAULT 'GET',
                url TEXT NOT NULL DEFAULT '',
                headers TEXT NOT NULL DEFAULT '{}',
                body TEXT NOT NULL DEFAULT '',
                content_type TEXT NOT NULL DEFAULT '',
                cookies TEXT NOT NULL DEFAULT '[]',
                request_body_model_id TEXT,
                responses TEXT NOT NULL DEFAULT '[]',
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS environments (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                variables TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS history (
                id TEXT PRIMARY KEY,
                method TEXT NOT NULL,
                url TEXT NOT NULL,
                status INTEGER,
                duration_ms INTEGER,
                size_bytes INTEGER,
                request_headers TEXT,
                request_body TEXT,
                response_headers TEXT,
                response_body TEXT,
                timestamp TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS models (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                fields TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mock_rules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                method TEXT NOT NULL DEFAULT 'ANY',
                path TEXT NOT NULL DEFAULT '/',
                status INTEGER NOT NULL DEFAULT 200,
                headers TEXT NOT NULL DEFAULT '{}',
                body TEXT NOT NULL DEFAULT '',
                delay_ms INTEGER NOT NULL DEFAULT 0,
                model_id TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }
}
