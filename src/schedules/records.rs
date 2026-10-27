use super::{Definition, ScheduleId};
use crate::execution_records::{Execution, ExecutionId, Invocation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stored {
    pub definition: Definition,
    pub enabled: bool,
    pub next_due_ms: i64,
    pub pending: Option<Invocation>,
    pub last_decision: Option<Decision>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub scheduled_for_ms: i64,
    pub decided_at_ms: i64,
    pub action: Action,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Queued,
    SkippedMissed,
    SkippedOverlap,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Observed {
    Idle,
    Running { execution_id: ExecutionId },
    Recovering { execution_id: ExecutionId },
    NeedsAttention,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct View {
    pub id: ScheduleId,
    pub enabled: bool,
    pub next_run_at_ms: Option<i64>,
    pub pending: Option<Invocation>,
    pub last_decision: Option<Decision>,
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
        id: ScheduleId,
    },
    Pause {
        id: ScheduleId,
    },
    Resume {
        id: ScheduleId,
    },
    RunNow {
        id: ScheduleId,
    },
    Cancel {
        id: ScheduleId,
    },
    History {
        id: ScheduleId,
    },
    Acknowledge {
        id: ScheduleId,
        execution_id: ExecutionId,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Created {
        id: ScheduleId,
    },
    Schedules {
        schedules: Vec<View>,
    },
    Schedule {
        schedule: Box<View>,
        definition: Definition,
    },
    History {
        executions: Vec<Execution>,
    },
}
