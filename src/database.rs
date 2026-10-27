//! Private database initialization and transactional schema migration.
use crate::{
    operation_error::{Error, Result},
    private_storage::{Root, private_file},
};
use rusqlite::{Connection, OptionalExtension};

const SERVICES: &str = "CREATE TABLE services(id TEXT PRIMARY KEY,definition TEXT NOT NULL,desired TEXT NOT NULL,pending_start INTEGER NOT NULL DEFAULT 0);";
const SCHEDULES: &str = "CREATE TABLE schedules(id TEXT PRIMARY KEY,definition TEXT NOT NULL,enabled INTEGER NOT NULL,next_due_ms INTEGER NOT NULL,pending TEXT,last_decision TEXT);";
const EXECUTIONS:&str="CREATE TABLE executions(id INTEGER PRIMARY KEY AUTOINCREMENT,service_id TEXT REFERENCES services(id),started_at_ms INTEGER NOT NULL,finished_at_ms INTEGER,outcome TEXT,supervisor_pid INTEGER,recovery_acknowledged_at_ms INTEGER,schedule_id TEXT REFERENCES schedules(id),invocation TEXT,
CHECK((service_id IS NOT NULL AND schedule_id IS NULL AND invocation IS NULL) OR (service_id IS NULL AND schedule_id IS NOT NULL AND invocation IS NOT NULL)));
CREATE UNIQUE INDEX one_active_execution ON executions(service_id) WHERE finished_at_ms IS NULL;
CREATE UNIQUE INDEX one_active_schedule_execution ON executions(schedule_id) WHERE finished_at_ms IS NULL;
CREATE INDEX service_execution_history ON executions(service_id,id DESC);
CREATE INDEX schedule_execution_history ON executions(schedule_id,id DESC);";

pub(crate) fn open(root: &Root) -> Result<Connection> {
    private_file(&root.path().join("state.sqlite"))?;
    let mut c = Connection::open(root.path().join("state.sqlite"))
        .map_err(|_| Error::StorageUnavailable)?;
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|_| Error::StorageUnavailable)?;
    if version > 2 {
        return Err(Error::StorageUnavailable);
    }
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
        .map_err(|_| Error::StorageUnavailable)?;
    if version < 2 {
        let tx = c.transaction().map_err(|_| Error::StorageUnavailable)?;
        if version == 0 {
            tx.execute_batch(SERVICES)
                .map_err(|_| Error::StorageUnavailable)?;
        }
        tx.execute_batch(SCHEDULES)
            .map_err(|_| Error::StorageUnavailable)?;
        if version == 1 {
            tx.execute_batch("ALTER TABLE executions RENAME TO previous_executions; DROP INDEX one_active_execution;").map_err(|_|Error::StorageUnavailable)?;
        }
        tx.execute_batch(EXECUTIONS)
            .map_err(|_| Error::StorageUnavailable)?;
        if version == 1 {
            tx.execute_batch("INSERT INTO executions(id,service_id,started_at_ms,finished_at_ms,outcome,supervisor_pid,recovery_acknowledged_at_ms) SELECT id,service_id,started_at_ms,finished_at_ms,outcome,supervisor_pid,recovery_acknowledged_at_ms FROM previous_executions;").map_err(|_|Error::StorageUnavailable)?;
            let sequence: i64 = tx
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='previous_executions'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| Error::StorageUnavailable)?
                .unwrap_or(0);
            tx.execute(
                "UPDATE sqlite_sequence SET seq=MAX(seq,?1) WHERE name='executions'",
                [sequence],
            )
            .map_err(|_| Error::StorageUnavailable)?;
            tx.execute_batch("DROP TABLE previous_executions;")
                .map_err(|_| Error::StorageUnavailable)?;
        }
        tx.execute_batch("PRAGMA user_version=2;")
            .map_err(|_| Error::StorageUnavailable)?;
        tx.commit().map_err(|_| Error::StorageUnavailable)?;
    }
    Ok(c)
}
