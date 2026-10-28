//! Private schedule database initialization. Other schema versions require fresh state.
use crate::{
    operation_error::{Error, Result},
    private_storage::{Root, private_file},
};
use rusqlite::Connection;

const SCHEMA: &str = "CREATE TABLE schedules(id TEXT PRIMARY KEY,definition TEXT NOT NULL,enabled INTEGER NOT NULL,next_due_ms INTEGER NOT NULL,pending TEXT,last_decision TEXT);
CREATE TABLE executions(id INTEGER PRIMARY KEY AUTOINCREMENT,schedule_id TEXT NOT NULL REFERENCES schedules(id),invocation TEXT NOT NULL,started_at_ms INTEGER NOT NULL,finished_at_ms INTEGER,outcome TEXT,supervisor_pid INTEGER,recovery_acknowledged_at_ms INTEGER);
CREATE UNIQUE INDEX one_active_schedule_execution ON executions(schedule_id) WHERE finished_at_ms IS NULL;
CREATE INDEX schedule_execution_history ON executions(schedule_id,id DESC);
PRAGMA user_version=3;";

pub(crate) fn open(root: &Root) -> Result<Connection> {
    private_file(&root.path().join("state.sqlite"))?;
    let mut c = Connection::open(root.path().join("state.sqlite"))
        .map_err(|_| Error::StorageUnavailable)?;
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|_| Error::StorageUnavailable)?;
    if version != 0 && version != 3 {
        return Err(Error::UnsupportedSchema);
    }
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
        .map_err(|_| Error::StorageUnavailable)?;
    if version == 0 {
        let tx = c.transaction().map_err(|_| Error::StorageUnavailable)?;
        tx.execute_batch(SCHEMA)
            .map_err(|_| Error::StorageUnavailable)?;
        tx.commit().map_err(|_| Error::StorageUnavailable)?;
    }
    Ok(c)
}
