use super::{Definition, ServiceId};
use serde::{Deserialize, Serialize};

pub use crate::execution_records::*;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Desired {
    Stopped,
    Running,
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
        service: Box<ServiceView>,
        definition: Definition,
    },
    History {
        executions: Vec<Execution>,
    },
}
pub use crate::operation_error::{Error, Result};
