//! SQLite adapter. Only the runtime opens the store while holding its singleton lock.
use crate::{
    private_storage::{Root, private_file},
    services::*,
};
use rusqlite::{Connection, params};

pub struct SqliteStore(Connection);
impl SqliteStore {
    pub fn open(root: &Root) -> Result<Self> {
        private_file(&root.path().join("state.sqlite"))?;
        let c = Connection::open(root.path().join("state.sqlite"))
            .map_err(|_| Error::StorageUnavailable)?;
        let version: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|_| Error::StorageUnavailable)?;
        if version > 1 {
            return Err(Error::StorageUnavailable);
        }
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS services(id TEXT PRIMARY KEY,definition TEXT NOT NULL,desired TEXT NOT NULL,pending_start INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS executions(id INTEGER PRIMARY KEY AUTOINCREMENT,service_id TEXT NOT NULL REFERENCES services(id),started_at_ms INTEGER NOT NULL,finished_at_ms INTEGER,outcome TEXT,supervisor_pid INTEGER,recovery_acknowledged_at_ms INTEGER);
        CREATE UNIQUE INDEX IF NOT EXISTS one_active_execution ON executions(service_id) WHERE finished_at_ms IS NULL;
        PRAGMA user_version=1;").map_err(|_|Error::StorageUnavailable)?;
        Ok(Self(c))
    }
    fn runs(&self, sql: &str, value: rusqlite::types::Value) -> Result<Vec<Execution>> {
        let mut stmt = self.0.prepare(sql).map_err(|_| Error::StorageUnavailable)?;
        let rows = stmt
            .query_map([value], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(|_| Error::StorageUnavailable)?;
        rows.map(|r| {
            let (
                id,
                service,
                started_at_ms,
                finished_at_ms,
                outcome,
                supervisor_pid,
                recovery_acknowledged_at_ms,
            ) = r.map_err(|_| Error::StorageUnavailable)?;
            Ok(Execution {
                id,
                service_id: ServiceId::try_from(service).map_err(|_| Error::StorageUnavailable)?,
                started_at_ms,
                finished_at_ms,
                outcome: outcome
                    .map(|s| serde_json::from_str(&s).map_err(|_| Error::StorageUnavailable))
                    .transpose()?,
                supervisor_pid,
                recovery_acknowledged_at_ms,
            })
        })
        .collect()
    }
}
impl ServiceStore for SqliteStore {
    fn create(&mut self, d: &Definition) -> Result<()> {
        let result = self.0.execute(
            "INSERT INTO services(id,definition,desired) VALUES(?1,?2,'stopped')",
            params![d.id().to_string(), serde_json::to_string(d).unwrap()],
        );
        match result {
            Ok(_) => Ok(()),
            Err(e) if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) => {
                Err(Error::AlreadyExists)
            }
            Err(_) => Err(Error::StorageUnavailable),
        }
    }
    fn list(&self) -> Result<Vec<StoredService>> {
        let mut s = self
            .0
            .prepare("SELECT definition,desired,pending_start FROM services ORDER BY id")
            .map_err(|_| Error::StorageUnavailable)?;
        let rows = s
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(|_| Error::StorageUnavailable)?;
        rows.map(|r| {
            let (d, state, pending_start) = r.map_err(|_| Error::StorageUnavailable)?;
            Ok(StoredService {
                definition: serde_json::from_str(&d).map_err(|_| Error::StorageUnavailable)?,
                desired: match state.as_str() {
                    "stopped" => Desired::Stopped,
                    "running" => Desired::Running,
                    _ => return Err(Error::StorageUnavailable),
                },
                pending_start,
            })
        })
        .collect()
    }
    fn get(&self, id: &ServiceId) -> Result<StoredService> {
        self.list()?
            .into_iter()
            .find(|s| s.definition.id() == id)
            .ok_or(Error::NotFound)
    }
    fn desire(&mut self, id: &ServiceId, d: Desired) -> Result<()> {
        let n = self
            .0
            .execute(
                "UPDATE services SET desired=?1,pending_start=CASE WHEN ?1='stopped' THEN 0 ELSE pending_start END WHERE id=?2",
                params![
                    if d == Desired::Running {
                        "running"
                    } else {
                        "stopped"
                    },
                    id.to_string()
                ],
            )
            .map_err(|_| Error::StorageUnavailable)?;
        if n == 0 { Err(Error::NotFound) } else { Ok(()) }
    }
    fn begin(&mut self, id: &ServiceId, now: i64) -> Result<i64> {
        let tx = self
            .0
            .transaction()
            .map_err(|_| Error::StorageUnavailable)?;
        tx.execute(
            "INSERT INTO executions(service_id,started_at_ms) VALUES(?1,?2)",
            params![id.to_string(), now],
        )
        .map_err(|_| Error::StorageUnavailable)?;
        let run = tx.last_insert_rowid();
        tx.execute(
            "UPDATE services SET pending_start=0 WHERE id=?1",
            [id.to_string()],
        )
        .map_err(|_| Error::StorageUnavailable)?;
        tx.commit().map_err(|_| Error::StorageUnavailable)?;
        Ok(run)
    }
    fn request_start(&mut self, id: &ServiceId) -> Result<()> {
        self.0
            .execute(
                "UPDATE services SET desired='running',pending_start=1 WHERE id=?1",
                [id.to_string()],
            )
            .map_err(|_| Error::StorageUnavailable)?;
        Ok(())
    }
    fn started(&mut self, id: i64, pid: u32) -> Result<()> {
        self.0
            .execute(
                "UPDATE executions SET supervisor_pid=?1 WHERE id=?2",
                params![pid, id],
            )
            .map_err(|_| Error::StorageUnavailable)?;
        Ok(())
    }
    fn finish(&mut self, id: i64, c: &Completion) -> Result<()> {
        self.0.execute("UPDATE executions SET outcome=?1,finished_at_ms=?2 WHERE id=?3 AND finished_at_ms IS NULL",params![serde_json::to_string(&c.outcome).unwrap(),c.finished_at_ms,id]).map_err(|_|Error::StorageUnavailable)?;
        Ok(())
    }
    fn history(&self, id: &ServiceId) -> Result<Vec<Execution>> {
        self.runs(
            "SELECT * FROM executions WHERE service_id=?1 ORDER BY id DESC LIMIT 100",
            id.to_string().into(),
        )
    }
    fn acknowledge(&mut self, id: ExecutionId, now: i64) -> Result<()> {
        let tx = self
            .0
            .transaction()
            .map_err(|_| Error::StorageUnavailable)?;
        tx.execute(
            "UPDATE executions SET recovery_acknowledged_at_ms=?1 WHERE id=?2",
            params![now, id],
        )
        .map_err(|_| Error::StorageUnavailable)?;
        tx.execute("UPDATE services SET desired='stopped',pending_start=0 WHERE id=(SELECT service_id FROM executions WHERE id=?1)",[id]).map_err(|_|Error::StorageUnavailable)?;
        tx.commit().map_err(|_| Error::StorageUnavailable)
    }
    fn active(&self) -> Result<Vec<Execution>> {
        self.runs(
            "SELECT * FROM executions WHERE finished_at_ms IS NULL AND ?1=1",
            1.into(),
        )
    }
    fn execution(&self, id: i64) -> Result<Execution> {
        self.runs("SELECT * FROM executions WHERE id=?1", id.into())?
            .into_iter()
            .next()
            .ok_or(Error::NotFound)
    }
}
