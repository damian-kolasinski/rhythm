//! SQLite adapter. Only the runtime opens the store while holding its singleton lock.
use crate::{private_storage::Root, services::*};
use rusqlite::{Connection, params};

pub struct SqliteStore(Connection);
impl SqliteStore {
    pub fn open(root: &Root) -> Result<Self> {
        Ok(Self(crate::database::open(root)?))
    }
    fn runs(&self, sql: &str, value: rusqlite::types::Value) -> Result<Vec<Execution>> {
        crate::execution_storage::read(&self.0, sql, value)
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
        crate::execution_storage::started(&self.0, id, pid)
    }
    fn finish(&mut self, id: i64, c: &Completion) -> Result<()> {
        crate::execution_storage::finish(&self.0, id, c)
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
            "SELECT * FROM executions WHERE service_id IS NOT NULL AND finished_at_ms IS NULL AND ?1=1",
            1.into(),
        )
    }
}
