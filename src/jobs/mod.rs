//! Background jobs as the operator sees them: how they start, what their supervisor
//! last observed, and whether that needs attention.
mod activation;
mod health;

pub use activation::{Activation, Calendar, Trigger};
pub use health::{Exit, Health, Observation};

use crate::operation_error::Result;

/// One job definition known to the supervisor, identified by its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub label: String,
    /// `None` when the definition exists but cannot be read.
    pub activation: Option<Activation>,
    pub observation: Observation,
}

/// The platform service that starts and restarts the operator's jobs.
pub trait Supervisor {
    fn jobs(&self) -> Result<Vec<Job>>;
}
