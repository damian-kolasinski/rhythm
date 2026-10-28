use serde::{Deserialize, Serialize};
pub type ExecutionId = i64;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Exited { code: i32 },
    Signalled { signal: i32 },
    Stopped,
    Interrupted,
    SpawnFailed,
    MonitorLost,
    OutputUnavailable,
}
impl Outcome {
    pub fn requires_attention(&self) -> bool {
        matches!(self, Self::MonitorLost | Self::OutputUnavailable)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: ExecutionId,
    pub schedule_id: crate::workload_id::WorkloadId,
    pub invocation: Invocation,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub outcome: Option<Outcome>,
    pub supervisor_pid: Option<u32>,
    pub recovery_acknowledged_at_ms: Option<i64>,
}
impl Execution {
    pub fn needs_attention(&self) -> bool {
        self.recovery_acknowledged_at_ms.is_none()
            && self
                .outcome
                .as_ref()
                .is_some_and(Outcome::requires_attention)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Invocation {
    Scheduled { scheduled_for_ms: i64 },
    Manual { requested_at_ms: i64 },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Completion {
    pub outcome: Outcome,
    pub finished_at_ms: i64,
}
pub enum Recovery {
    Waiting,
    NeverAttempted,
    Completed(Completion),
}

pub fn timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_millis() as i64
}
