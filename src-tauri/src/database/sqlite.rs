use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderRow {
    pub id: i64,
    #[allow(dead_code)]
    pub path: String,
    pub enabled: bool,
    pub file_count: i64,
    pub last_indexed_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct FileMeta {
    #[allow(dead_code)]
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub status: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                last_indexed_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
                path TEXT NOT NULL UNIQUE,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                doc_id TEXT NOT NULL,
                indexed_at INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'ok'
            );

            CREATE INDEX IF NOT EXISTS idx_files_folder ON files(folder_id);
            CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);

            CREATE TABLE IF NOT EXISTS errors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                message TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn data_dir() -> PathBuf {
        let base = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
        base.join("file-searcher")
    }

    pub fn default_db_path() -> PathBuf {
        Self::data_dir().join("index.db")
    }

    pub fn default_index_path() -> PathBuf {
        Self::data_dir().join("tantivy")
    }

    pub fn add_folder(&self, path: &str) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO folders(path, enabled, created_at) VALUES (?1, 1, ?2)",
            params![path, now],
        )?;
        let id = conn.query_row(
            "SELECT id FROM folders WHERE path = ?1",
            params![path],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn remove_folder(&self, id: i64) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut pairs = Vec::new();
        {
            let mut stmt = conn.prepare("SELECT path, doc_id FROM files WHERE folder_id = ?1")?;
            let rows = stmt.query_map(params![id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                pairs.push(row?);
            }
        }
        conn.execute("DELETE FROM files WHERE folder_id = ?1", params![id])?;
        conn.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        Ok(pairs)
    }

    pub fn list_folders(&self) -> Result<Vec<FolderRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT f.id, f.path, f.enabled,
                   (SELECT COUNT(*) FROM files fi WHERE fi.folder_id = f.id AND fi.status = 'ok'),
                   f.last_indexed_at
            FROM folders f ORDER BY f.created_at
            "#,
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(FolderRow {
                id: r.get(0)?,
                path: r.get(1)?,
                enabled: r.get::<_, i64>(2)? != 0,
                file_count: r.get(3)?,
                last_indexed_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn set_folder_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE folders SET enabled = ?1 WHERE id = ?2",
            params![enabled as i64, id],
        )?;
        Ok(())
    }

    pub fn touch_folder(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE folders SET last_indexed_at = ?1 WHERE id = ?2",
            params![chrono::Utc::now().timestamp(), id],
        )?;
        Ok(())
    }

    pub fn get_file(&self, path: &str) -> Result<Option<FileMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path, size, mtime, status FROM files WHERE path = ?1")?;
        let mut rows = stmt.query_map(params![path], |r| {
            Ok(FileMeta {
                path: r.get(0)?,
                size: r.get::<_, i64>(1)? as u64,
                mtime: r.get(2)?,
                status: r.get(3)?,
            })
        })?;
        match rows.next() {
            Some(Ok(m)) => Ok(Some(m)),
            _ => Ok(None),
        }
    }

    pub fn upsert_file(&self, folder_id: i64, path: &str, size: u64, mtime: i64, doc_id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO files(folder_id, path, size, mtime, doc_id, indexed_at, status)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(path) DO UPDATE SET
                folder_id = excluded.folder_id,
                size = excluded.size,
                mtime = excluded.mtime,
                doc_id = excluded.doc_id,
                indexed_at = excluded.indexed_at,
                status = excluded.status
            "#,
            params![folder_id, path, size as i64, mtime, doc_id, chrono::Utc::now().timestamp(), status],
        )?;
        Ok(())
    }

    pub fn delete_file(&self, path: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let doc_id: Option<String> = conn
            .query_row("SELECT doc_id FROM files WHERE path = ?1", params![path], |r| r.get(0))
            .ok();
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(doc_id)
    }

    #[allow(dead_code)]
    pub fn delete_files_under(&self, folder_prefix: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path, doc_id FROM files WHERE path LIKE ?1")?;
        let pattern = format!("{folder_prefix}%");
        let rows = stmt.query_map(params![pattern], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let pairs: Vec<_> = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        conn.execute("DELETE FROM files WHERE path LIKE ?1", params![pattern])?;
        Ok(pairs)
    }

    pub fn doc_id_of(&self, path: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let doc_id: Option<String> = conn
            .query_row("SELECT doc_id FROM files WHERE path = ?1", params![path], |r| r.get(0))
            .ok();
        Ok(doc_id)
    }

    pub fn record_error(&self, path: &str, message: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO errors(path, message, created_at) VALUES (?1, ?2, ?3)",
            params![path, message, chrono::Utc::now().timestamp()],
        )?;
        Ok(())
    }

    pub fn recent_errors(&self, limit: usize) -> Result<Vec<(String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT path, message, created_at FROM errors ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn all_files(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path, doc_id FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}
