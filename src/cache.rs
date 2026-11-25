//! Persistent local metadata cache (SQLite, one JSON row per file).

use crate::model::DFile;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn open(path: &Path) -> Result<Cache> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA temp_store=MEMORY;
             CREATE TABLE IF NOT EXISTS files (id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS local_copies (
                 id TEXT PRIMARY KEY, path TEXT NOT NULL, modified TEXT NOT NULL DEFAULT '');",
        )?;
        Ok(Cache { conn })
    }

    pub fn load_all(&self) -> Result<Vec<DFile>> {
        let mut stmt = self.conn.prepare("SELECT data FROM files")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            if let Ok(f) = serde_json::from_str::<DFile>(&row?) {
                out.push(f);
            }
        }
        Ok(out)
    }

    pub fn upsert(&mut self, files: &[DFile]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("INSERT OR REPLACE INTO files (id, data) VALUES (?1, ?2)")?;
            for f in files {
                stmt.execute(params![f.id, serde_json::to_string(f)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove(&mut self, ids: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM files WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM files")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    /// Files downloaded to this machine: (file id, local path, Drive modifiedTime at download).
    pub fn local_copies(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, path, modified FROM local_copies")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_local_copy(&self, id: &str, path: &str, modified: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO local_copies (id, path, modified) VALUES (?1, ?2, ?3)",
            params![id, path, modified],
        )?;
        Ok(())
    }

    pub fn forget_local_copy(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM local_copies WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM files; DELETE FROM meta; DELETE FROM local_copies;")?;
        Ok(())
    }
}
