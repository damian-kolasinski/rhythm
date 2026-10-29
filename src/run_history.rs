//! Read a job's recorded runs and their output.
use crate::{
    jobs::Label,
    ledger::Ledger,
    operation_error::{Error, Result},
    run_output,
    runs::Run,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "history")]
pub struct HistoryResponse {
    pub label: String,
    /// Newest first.
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "logs")]
pub struct LogsResponse {
    pub run: Run,
    /// Decoded as UTF-8, with invalid sequences replaced.
    pub output: String,
    /// Earlier output exists but is not included.
    pub truncated: bool,
}

pub const MAX_LIMIT: u32 = 1000;

/// A machine without a ledger has recorded nothing yet.
pub fn history(ledger: Option<&Ledger>, label: &Label, limit: u32) -> Result<HistoryResponse> {
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(Error::InvalidRequest);
    }
    Ok(HistoryResponse {
        label: label.to_string(),
        runs: match ledger {
            Some(ledger) => ledger.history(label.as_str(), limit)?,
            None => vec![],
        },
    })
}

/// The newest run's output, or the run `id` when it belongs to `label`.
pub fn logs(
    ledger: Option<&Ledger>,
    label: &Label,
    id: Option<i64>,
    bytes: u64,
) -> Result<LogsResponse> {
    if bytes == 0 || bytes > 2 * run_output::SEGMENT {
        return Err(Error::InvalidRequest);
    }
    let ledger = ledger.ok_or(Error::NotFound)?;
    let run = match id {
        Some(id) => ledger.run(id)?.filter(|r| r.label == label.as_str()),
        None => ledger.latest(label.as_str())?,
    }
    .ok_or(Error::NotFound)?;
    let tail = run_output::tail(&ledger.output_location(run.id), bytes)?;
    Ok(LogsResponse {
        run,
        output: String::from_utf8_lossy(&tail.bytes).into_owned(),
        truncated: tail.truncated,
    })
}
