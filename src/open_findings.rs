//! Read the problems a job reports as open now, for `rhythm findings`.
use crate::{
    findings::{Finding, Stated},
    jobs::Label,
    ledger::Ledger,
    operation_error::Result,
    runs::Timestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "findings")]
pub struct FindingsResponse {
    pub label: String,
    /// The run whose result stated the open set; absent when no run of the
    /// job has listed findings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<i64>,
    /// When that run ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<Timestamp>,
    pub findings: Vec<Finding>,
    /// Problems the job listed beyond the first 100, which are not shown.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u32,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
impl FindingsResponse {
    pub fn new(label: &Label, stated: Option<Stated>) -> Self {
        match stated {
            Some(stated) => Self {
                label: label.to_string(),
                run: Some(stated.run),
                reported_at: Some(stated.at),
                dropped: stated.set.dropped,
                findings: stated.set.open,
            },
            None => Self {
                label: label.to_string(),
                run: None,
                reported_at: None,
                findings: vec![],
                dropped: 0,
            },
        }
    }
}

/// A machine without a ledger has recorded nothing yet.
pub fn findings(ledger: Option<&Ledger>, label: &Label) -> Result<FindingsResponse> {
    let stated = match ledger {
        Some(ledger) => ledger.open_findings(label.as_str())?,
        None => None,
    };
    Ok(FindingsResponse::new(label, stated))
}
