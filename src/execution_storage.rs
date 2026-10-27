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
                r.get(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
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
            schedule,
            invocation,
        ) = r.map_err(|_| Error::StorageUnavailable)?;
        let parse_id = |s: String| WorkloadId::try_from(s).map_err(|_| Error::StorageUnavailable);
        let owner = match (service, schedule, invocation) {
            (Some(id), None, None) => ExecutionOwner::Service {
                service_id: parse_id(id)?,
            },
            (None, Some(id), Some(invocation)) => ExecutionOwner::Schedule {
                schedule_id: parse_id(id)?,
                invocation: serde_json::from_str(&invocation)
                    .map_err(|_| Error::StorageUnavailable)?,
            },
            _ => return Err(Error::StorageUnavailable),
        };
        Ok(Execution {
            id,
            owner,
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
