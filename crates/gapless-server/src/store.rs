//! Incidents persist in SQLite, so the console's history survives a restart.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::dto::IncidentDto;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS sessions (
                 id INTEGER PRIMARY KEY,
                 started_at INTEGER NOT NULL,
                 mode TEXT NOT NULL,
                 program TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS incidents (
                 id INTEGER PRIMARY KEY,
                 session_id INTEGER NOT NULL REFERENCES sessions(id),
                 session_incident INTEGER NOT NULL,
                 opened_at INTEGER NOT NULL,
                 data TEXT NOT NULL,
                 UNIQUE (session_id, session_incident)
             );",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn start_session(&self, started_at: u64, mode: &str, program: &str) -> Result<i64> {
        let (mode, program) = (mode.to_owned(), program.to_owned());
        self.blocking(move |c| {
            c.execute(
                "INSERT INTO sessions (started_at, mode, program) VALUES (?1, ?2, ?3)",
                params![started_at as i64, mode, program],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await
    }

    /// Reserve an id for a new incident.
    pub async fn open_incident(
        &self,
        session: i64,
        session_incident: u64,
        opened_at: u64,
    ) -> Result<i64> {
        self.blocking(move |c| {
            c.execute(
                "INSERT INTO incidents (session_id, session_incident, opened_at, data) VALUES (?1, ?2, ?3, '{}')",
                params![session, session_incident as i64, opened_at as i64],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await
    }

    pub async fn save(&self, incident: &IncidentDto) -> Result<()> {
        let (id, data) = (incident.id, serde_json::to_string(incident)?);
        self.blocking(move |c| {
            c.execute(
                "UPDATE incidents SET data = ?2 WHERE id = ?1",
                params![id, data],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        self.blocking(move |c| {
            let mut stmt = c.prepare(
                "SELECT data FROM incidents WHERE data != '{}' ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit as i64], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(serde_json::from_str(&row?)?);
            }
            Ok(out)
        })
        .await
    }

    pub async fn get(&self, id: i64) -> Result<Option<serde_json::Value>> {
        self.blocking(move |c| {
            let data: Option<String> = c
                .query_row("SELECT data FROM incidents WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(data.map(|d| serde_json::from_str(&d)).transpose()?)
        })
        .await
    }

    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || f(&conn.lock().expect("store lock"))).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Reason;

    fn incident(id: i64) -> IncidentDto {
        IncidentDto {
            id,
            session_incident: 1,
            status: "open",
            reason: Reason {
                code: "killed".into(),
                text: "killed".into(),
            },
            detail: "stream terminated by user".into(),
            opened_at: 1,
            recovered_at: None,
            duration_ms: None,
            last_complete_slot: Some(10),
            resume_from: Some(11),
            gap: None,
            unrecoverable: None,
            steps: Vec::new(),
            replayed: 0,
            duplicates: 0,
            naive_double_counts: 0,
            patch: None,
            verification: None,
            chaos: Some("test".into()),
        }
    }

    #[tokio::test]
    async fn incidents_round_trip() {
        let store = Store::in_memory().unwrap();
        let session = store.start_session(1, "offline", "P").await.unwrap();
        let id = store.open_incident(session, 1, 1).await.unwrap();
        assert!(
            store.list(10).await.unwrap().is_empty(),
            "reserved rows stay hidden until saved"
        );
        let mut dto = incident(id);
        store.save(&dto).await.unwrap();
        dto.status = "verified";
        store.save(&dto).await.unwrap();
        let listed = store.list(10).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["status"], "verified");
        assert_eq!(store.get(id).await.unwrap().unwrap()["resumeFrom"], 11);
        assert!(store.get(id + 1).await.unwrap().is_none());
    }
}
