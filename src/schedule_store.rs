//! Schedule persistence adapter. Cursor/pending updates precede dispatch durably.
use crate::{
    execution_records::*,
    private_storage::Root,
    schedules::*,
    services::{Error, Result},
};
use rusqlite::{Connection, params};
pub struct SqliteScheduleStore(Connection);
impl SqliteScheduleStore {
    pub fn open(root: &Root) -> Result<Self> {
        Ok(Self(crate::database::open(root)?))
    }
}
impl ScheduleStore for SqliteScheduleStore {
    fn create(&mut self, s: &Stored) -> Result<()> {
        match self.0.execute(
            "INSERT INTO schedules(id,definition,enabled,next_due_ms) VALUES(?1,?2,?3,?4)",
            params![
                s.definition.id().to_string(),
                serde_json::to_string(&s.definition).unwrap(),
                s.enabled,
                s.next_due_ms
            ],
        ) {
            Ok(_) => Ok(()),
            Err(e) if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) => {
                Err(Error::AlreadyExists)
            }
            Err(_) => Err(Error::StorageUnavailable),
        }
    }
    fn list(&self) -> Result<Vec<Stored>> {
        let mut statement=self.0.prepare("SELECT definition,enabled,next_due_ms,pending,last_decision FROM schedules ORDER BY id").map_err(|_|Error::StorageUnavailable)?;
        let rows = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(|_| Error::StorageUnavailable)?;
        rows.map(|r| {
            let (definition, enabled, next_due_ms, pending, last_decision) =
                r.map_err(|_| Error::StorageUnavailable)?;
            Ok(Stored {
                definition: serde_json::from_str(&definition)
                    .map_err(|_| Error::StorageUnavailable)?,
                enabled,
                next_due_ms,
                pending: pending
                    .map(|s| serde_json::from_str(&s).map_err(|_| Error::StorageUnavailable))
                    .transpose()?,
                last_decision: last_decision
                    .map(|s| serde_json::from_str(&s).map_err(|_| Error::StorageUnavailable))
                    .transpose()?,
            })
        })
        .collect()
    }
    fn get(&self, id: &ScheduleId) -> Result<Stored> {
        self.list()?
            .into_iter()
            .find(|s| s.definition.id() == id)
            .ok_or(Error::NotFound)
    }
    fn save(&mut self, s: &Stored) -> Result<()> {
        self.0.execute("UPDATE schedules SET enabled=?1,next_due_ms=?2,pending=?3,last_decision=?4 WHERE id=?5",params![s.enabled,s.next_due_ms,s.pending.map(|p|serde_json::to_string(&p).unwrap()),s.last_decision.as_ref().map(|d|serde_json::to_string(d).unwrap()),s.definition.id().to_string()]).map_err(|_|Error::StorageUnavailable)?;
        Ok(())
    }
    fn begin(&mut self, id: &ScheduleId, now: i64) -> Result<ExecutionId> {
        let tx = self
            .0
            .transaction()
            .map_err(|_| Error::StorageUnavailable)?;
        tx.execute("INSERT INTO executions(schedule_id,invocation,started_at_ms) SELECT id,pending,?1 FROM schedules WHERE id=?2 AND pending IS NOT NULL",params![now,id.to_string()]).map_err(|_|Error::StorageUnavailable)?;
        let run = tx.last_insert_rowid();
        tx.execute(
            "UPDATE schedules SET pending=NULL WHERE id=?1",
            [id.to_string()],
        )
        .map_err(|_| Error::StorageUnavailable)?;
        tx.commit().map_err(|_| Error::StorageUnavailable)?;
        Ok(run)
    }
    fn started(&mut self, id: ExecutionId, pid: u32) -> Result<()> {
        crate::execution_storage::started(&self.0, id, pid)
    }
    fn finish(&mut self, id: ExecutionId, completion: &Completion) -> Result<()> {
        crate::execution_storage::finish(&self.0, id, completion)
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
        tx.execute("UPDATE schedules SET enabled=0,pending=NULL WHERE id=(SELECT schedule_id FROM executions WHERE id=?1)",[id]).map_err(|_|Error::StorageUnavailable)?;
        tx.commit().map_err(|_| Error::StorageUnavailable)
    }
    fn history(&self, id: &ScheduleId) -> Result<Vec<Execution>> {
        crate::execution_storage::read(
            &self.0,
            "SELECT * FROM executions WHERE schedule_id=?1 ORDER BY id DESC LIMIT 100",
            id.to_string().into(),
        )
    }
    fn active(&self) -> Result<Vec<Execution>> {
        crate::execution_storage::read(
            &self.0,
            "SELECT * FROM executions WHERE schedule_id IS NOT NULL AND finished_at_ms IS NULL AND ?1=1",
            1.into(),
        )
    }
}
