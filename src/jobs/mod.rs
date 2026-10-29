//! Background jobs as the operator sees them: how they start, what their supervisor
//! last observed, and whether that needs attention.
mod activation;
mod health;
mod label;

pub use activation::{Activation, Calendar, Trigger};
pub use health::{Exit, Health, Observation};
pub use label::Label;

use crate::operation_error::Result;
use std::{collections::BTreeMap, path::PathBuf};

/// The state directory a recorded job's runs are kept in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RunStore {
    Default,
    Directory(PathBuf),
}

/// One job definition known to the supervisor, identified by its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub label: String,
    /// `None` when the definition exists but cannot be read.
    pub activation: Option<Activation>,
    /// Where the recorder keeps its runs, for jobs that run through it.
    pub recorded: Option<RunStore>,
    pub observation: Observation,
}

/// A job Rhythm creates: its command already includes the recorder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub label: Label,
    pub schedule: Schedule,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<String, String>,
    /// Where the supervisor writes the recorder's own diagnostics.
    pub diagnostics: PathBuf,
}

/// The schedules Rhythm creates: a narrower form of [`Activation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    KeepAlive,
    /// No triggers at all means the job only runs when started explicitly.
    Triggered {
        at_load: bool,
        every_seconds: Option<u64>,
        calendar: Vec<Calendar>,
    },
}
impl From<&Schedule> for Activation {
    fn from(schedule: &Schedule) -> Self {
        match schedule {
            Schedule::KeepAlive => Self::Service { always: true },
            Schedule::Triggered {
                at_load,
                every_seconds,
                calendar,
            } => Self::Triggered {
                triggers: at_load
                    .then_some(Trigger::AtLoad)
                    .into_iter()
                    .chain(every_seconds.map(|seconds| Trigger::Interval { seconds }))
                    .chain(calendar.iter().cloned().map(Trigger::Calendar))
                    .collect(),
            },
        }
    }
}

/// How an existing job starts its command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub arguments: Vec<String>,
    /// The definition routes output to its own log files.
    pub has_log_files: bool,
}

/// The platform service that starts and restarts the operator's jobs.
pub trait Supervisor {
    fn jobs(&self) -> Result<Vec<Job>>;
    fn observe(&self, label: &Label) -> Result<Observation>;
    fn program(&self, label: &Label) -> Result<Program>;
    /// Creates and loads a job whose label is not defined or loaded yet.
    fn create(&self, spec: &JobSpec) -> Result<()>;
    /// Overwrites the job's existing definition and reloads it.
    fn replace(&self, spec: &JobSpec) -> Result<()>;
    /// Changes only the command, reloading the job if it was loaded.
    fn set_program(&self, label: &Label, arguments: &[String]) -> Result<()>;
    fn uninstall(&self, label: &Label) -> Result<()>;
    /// Starts a loaded job now unless it is already running.
    fn start(&self, label: &Label) -> Result<()>;
}
