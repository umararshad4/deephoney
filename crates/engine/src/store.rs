//! Saved sessions in SQLite. The whole report is one JSON column; there is nothing to query inside it yet.
use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::Report;

pub struct Store(Connection);

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub id: i64,
    pub topic: String,
    pub mode: String,
    pub created: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(c: Connection) -> Result<Self> {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY,
                topic TEXT NOT NULL,
                mode TEXT NOT NULL,
                created TEXT NOT NULL DEFAULT (datetime('now')),
                report TEXT NOT NULL
            );",
        )?;
        Ok(Self(c))
    }

    pub fn save(&self, r: &Report) -> Result<i64> {
        self.0.execute(
            "INSERT INTO sessions (topic, mode, report) VALUES (?1, ?2, ?3)",
            params![r.topic, r.mode.label(), serde_json::to_string(r)?],
        )?;
        Ok(self.0.last_insert_rowid())
    }

    pub fn list(&self) -> Result<Vec<SessionRow>> {
        let mut st = self.0.prepare("SELECT id, topic, mode, created FROM sessions ORDER BY id DESC")?;
        let rows = st.query_map([], |r| Ok(SessionRow { id: r.get(0)?, topic: r.get(1)?, mode: r.get(2)?, created: r.get(3)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn load(&self, id: i64) -> Result<Report> {
        let json: String = self.0.query_row("SELECT report FROM sessions WHERE id = ?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.0.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let s = super::Store::memory().unwrap();
        let r = crate::Report { topic: "lithium".into(), markdown: "# hi".into(), ..Default::default() };
        let id = s.save(&r).unwrap();
        assert_eq!(s.list().unwrap()[0].topic, "lithium");
        assert_eq!(s.load(id).unwrap().markdown, "# hi");
        s.delete(id).unwrap();
        assert!(s.list().unwrap().is_empty());
    }
}
