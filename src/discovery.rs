//! Read-only discovery contract. No command arguments or environment values cross it.
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Service,
    Schedule,
    OnDemand,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ObservedState {
    Running {
        pid: u32,
        previous_exit_code: i32,
    },
    Idle {
        last_exit_code: i32,
    },
    /// Absent from the caller's bootstrap domain; may be loaded in another domain.
    NotObserved,
    Unknown,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Calendar {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weekday: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minute: Option<u8>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Interval { seconds: u64 },
    Calendar { components: Calendar },
    AtLoad,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    RestartAlways,
    ConditionalRestart,
    IntervalScheduling,
    CalendarScheduling,
    StartAtLoad,
    EnvironmentConfiguration,
    WorkingDirectory,
    OutputRouting,
    RestartThrottle,
    ExecutionContext,
    AdditionalLaunchdBehavior,
    VerifyWorkloadCompletion,
}

/// This record is deliberately insufficient to execute or import a registration.
#[derive(Debug, Serialize)]
pub struct Registration {
    pub label: String,
    pub source: PathBuf,
    pub kind: WorkKind,
    pub observed: ObservedState,
    pub triggers: Vec<Trigger>,
    pub environment_variable_count: usize,
    pub requirements: Vec<Requirement>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    DirectoryUnreadable,
    EntryUnreadable,
    InvalidRegistration,
    UnsupportedRegistration,
    ObservationUnavailable,
    DuplicateLabel,
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub source: Option<PathBuf>,
    pub code: DiagnosticCode,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub schema_version: u8,
    pub scope: &'static str,
    pub observation_scope: &'static str,
    pub ownership: &'static str,
    pub complete: bool,
    pub registrations: Vec<Registration>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Platform discovery is inherently provider-coupled; transports consume this port.
pub trait InventorySource {
    fn discover(&self) -> (Vec<Registration>, Vec<Diagnostic>);
}

pub fn discover(source: &impl InventorySource) -> Inventory {
    let (mut registrations, mut diagnostics) = source.discover();
    registrations.sort_by(|a, b| a.label.cmp(&b.label).then(a.source.cmp(&b.source)));
    for pair in registrations.windows(2) {
        if pair[0].label == pair[1].label {
            diagnostics.push(Diagnostic {
                source: Some(pair[1].source.clone()),
                code: DiagnosticCode::DuplicateLabel,
            });
        }
    }
    Inventory {
        schema_version: 1,
        scope: "current_user_launch_agents",
        observation_scope: "calling_launchd_context",
        ownership: "external_read_only",
        complete: diagnostics.is_empty(),
        registrations,
        diagnostics,
    }
}
