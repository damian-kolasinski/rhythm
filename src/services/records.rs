use super::{Definition, ServiceId};
use serde::{Deserialize, Serialize};

pub type ExecutionId = i64;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Desired {
    Stopped,
    Running,
}
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
    pub service_id: ServiceId,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredService {
    pub definition: Definition,
    pub desired: Desired,
    pub pending_start: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Observed {
    Stopped,
    Running {
        execution_id: ExecutionId,
        supervisor_pid: u32,
    },
    Recovering {
        execution_id: ExecutionId,
    },
    Waiting {
        retry_at_ms: i64,
    },
    Exited,
    NeedsAttention,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceView {
    pub id: ServiceId,
    pub desired: Desired,
    pub observed: Observed,
    pub latest_execution: Option<Execution>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Create {
        definition: Definition,
    },
    List,
    Inspect {
        id: ServiceId,
    },
    Start {
        id: ServiceId,
    },
    Stop {
        id: ServiceId,
    },
    Restart {
        id: ServiceId,
    },
    Acknowledge {
        id: ServiceId,
        execution_id: ExecutionId,
    },
    History {
        id: ServiceId,
    },
    Logs {
        execution_id: ExecutionId,
    },
    Status,
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Created {
        id: ServiceId,
    },
    Services {
        services: Vec<ServiceView>,
    },
    Service {
        service: ServiceView,
        definition: Definition,
    },
    History {
        executions: Vec<Execution>,
    },
    Logs {
        stdout: String,
        stderr: String,
    },
    Runtime {
        version: String,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    InvalidRequest,
    NotFound,
    AlreadyExists,
    StorageUnavailable,
    ExecutionUnavailable,
    RecoveryRequired,
    RuntimeUnavailable,
    RuntimeAlreadyRunning,
    InsecureStorage,
    Unauthorized,
    UnsupportedPlatform,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Serialize, Deserialize)]
pub struct Completion {
    pub outcome: Outcome,
    pub finished_at_ms: i64,
}

pub fn timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_millis() as i64
}
