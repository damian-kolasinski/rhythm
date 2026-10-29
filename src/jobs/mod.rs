//! Background jobs as the operator sees them: how they start, what their supervisor
//! last observed, and whether that needs attention.
mod activation;
mod health;
mod label;

pub use activation::{Activation, Calendar, Trigger};
pub use health::{Exit, Health, Observation};
pub use label::Label;

use crate::operation_error::Result;
use std::path::PathBuf;

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

/// The platform service that starts and restarts the operator's jobs.
pub trait Supervisor {
    fn jobs(&self) -> Result<Vec<Job>>;
}
