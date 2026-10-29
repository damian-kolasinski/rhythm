//! Report every job on the requested machines, most urgent first.
use crate::{
    jobs::{Activation, Health, Job, Observation, RunStore, Supervisor},
    ledger::Ledger,
    operation_error::{Error, Result},
    remote::{self, Ssh},
    runs::Run,
    targets::{self, Target},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobReport {
    pub label: String,
    pub health: Health,
    /// Absent when the definition cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<Activation>,
    pub observation: Observation,
    /// Runs through Rhythm's recorder.
    pub recorded: bool,
    /// The newest recorded run, for recorded jobs that have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<Run>,
    /// Where a local recorded job's runs are kept; never sent to other machines.
    #[serde(skip)]
    pub run_store: Option<RunStore>,
}
impl JobReport {
    fn new(job: Job, last_run: Option<Run>) -> Self {
        Self {
            health: Health::assess(job.activation.as_ref(), &job.observation),
            label: job.label,
            activation: job.activation,
            observation: job.observation,
            recorded: job.recorded.is_some(),
            run_store: job.recorded,
            last_run,
        }
    }
    /// A report from another machine is data: health follows from its facts.
    fn reassessed(self) -> Self {
        let job = Job {
            label: self.label,
            activation: self.activation,
            recorded: self.recorded.then_some(RunStore::Default),
            observation: self.observation,
        };
        Self {
            run_store: None,
            ..Self::new(job, self.last_run)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "status")]
pub struct StatusResponse {
    pub hosts: Vec<HostStatus>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStatus {
    pub host: String,
    #[serde(flatten)]
    pub outcome: HostOutcome,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostOutcome {
    Jobs { jobs: Vec<JobReport> },
    Failed { error: Error },
}

/// Recorded jobs carry their newest run from the ledger their recorder writes to.
pub fn jobs(
    supervisor: &impl Supervisor,
    ledgers: &dyn Fn(&RunStore) -> Option<Ledger>,
) -> Result<Vec<JobReport>> {
    let mut opened: HashMap<RunStore, Option<Ledger>> = HashMap::new();
    let mut reports = vec![];
    for job in supervisor.jobs()? {
        let last_run = match &job.recorded {
            Some(store) => {
                let ledger = opened
                    .entry(store.clone())
                    .or_insert_with(|| ledgers(store));
                match ledger {
                    Some(ledger) => ledger.latest(&job.label)?,
                    None => None,
                }
            }
            None => None,
        };
        reports.push(JobReport::new(job, last_run));
    }
    reports.sort_by(|a, b| (a.health, &a.label).cmp(&(b.health, &b.label)));
    Ok(reports)
}

/// Every target's jobs, reading other machines over ssh.
pub fn gather(
    targets: &[Target],
    local: impl Fn() -> Result<Vec<JobReport>> + Sync,
    ssh: &Ssh,
) -> StatusResponse {
    let remote = |host: &remote::Host| {
        let response: StatusResponse = ssh.call(host, &["status"])?;
        match response.hosts.into_iter().next().map(|h| h.outcome) {
            Some(HostOutcome::Jobs { jobs }) => {
                Ok(jobs.into_iter().map(JobReport::reassessed).collect())
            }
            Some(HostOutcome::Failed { error }) => Err(error),
            None => Err(Error::RemoteIncompatible),
        }
    };
    StatusResponse {
        hosts: targets::each(targets, local, remote)
            .into_iter()
            .map(|(host, result)| HostStatus {
                host,
                outcome: match result {
                    Ok(jobs) => HostOutcome::Jobs { jobs },
                    Err(error) => HostOutcome::Failed { error },
                },
            })
            .collect(),
    }
}
