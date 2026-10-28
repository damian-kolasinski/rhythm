//! Shared SQL execution ledger. Owners control allocation and acknowledgment transactions.
use crate::{
    execution_records::*,
    operation_error::{Error, Result},
    workload_id::WorkloadId,
};
use rusqlite::{Connection, params};

pub(crate) fn read(
    c: &Connection,
    sql: &str,
    value: rusqlite::types::Value,
) -> Result<Vec<Execution>> {
    let mut statement = c.prepare(sql).map_err(|_| Error::StorageUnavailable)?;
    let rows = statement
        .query_map([value], |r| {
            Ok((
                r.get("id")?,
                r.get::<_, String>("schedule_id")?,
                r.get::<_, String>("invocation")?,
                r.get("started_at_ms")?,
                r.get("finished_at_ms")?,
                r.get::<_, Option<String>>("outcome")?,
                r.get("supervisor_pid")?,
                r.get("recovery_acknowledged_at_ms")?,
            ))
        })
        .map_err(|_| Error::StorageUnavailable)?;
    rows.map(|r| {
        let (
            id,
            schedule,
            invocation,
            started_at_ms,
            finished_at_ms,
            outcome,
            supervisor_pid,
            recovery_acknowledged_at_ms,
        ) = r.map_err(|_| Error::StorageUnavailable)?;
        Ok(Execution {
            id,
            schedule_id: WorkloadId::try_from(schedule).map_err(|_| Error::StorageUnavailable)?,
            invocation: serde_json::from_str(&invocation).map_err(|_| Error::StorageUnavailable)?,
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
pub(crate) fn started(c: &Connection, id: i64, pid: u32) -> Result<()> {
    c.execute(
        "UPDATE executions SET supervisor_pid=?1 WHERE id=?2",
        params![pid, id],
    )
    .map_err(|_| Error::StorageUnavailable)?;
    Ok(())
}
pub(crate) fn finish(c: &Connection, id: i64, completion: &Completion) -> Result<()> {
    c.execute(
        "UPDATE executions SET outcome=?1,finished_at_ms=?2 WHERE id=?3 AND finished_at_ms IS NULL",
        params![
            serde_json::to_string(&completion.outcome).unwrap(),
            completion.finished_at_ms,
            id
        ],
    )
    .map_err(|_| Error::StorageUnavailable)?;
    Ok(())
}
