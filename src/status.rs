//! Report every job on the requested machines, most urgent first.
use crate::{
    jobs::{Activation, Health, Job, Observation, RunStore, Supervisor},
    ledger::Ledger,
    operation_error::{Error, Result},
    remote::{self, Ssh},
    runs::Run,
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
}
impl JobReport {
    fn new(job: Job, last_run: Option<Run>) -> Self {
        Self {
            health: Health::assess(job.activation.as_ref(), &job.observation),
            label: job.label,
            activation: job.activation,
            observation: job.observation,
            recorded: job.recorded.is_some(),
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
        Self::new(job, self.last_run)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local,
    Remote(remote::Host),
}
impl Target {
    pub const LOCAL: &str = "local";
    pub fn parse(name: &str) -> Result<Self> {
        if name == Self::LOCAL {
            Ok(Self::Local)
        } else {
            name.parse().map(Self::Remote)
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Local => Self::LOCAL,
            Self::Remote(host) => host.as_str(),
        }
    }
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

/// Queries every target concurrently; one unreachable host does not hide the others.
pub fn gather(
    targets: &[Target],
    local: impl Fn() -> Result<Vec<JobReport>> + Sync,
    ssh: &Ssh,
) -> StatusResponse {
    let hosts = std::thread::scope(|scope| {
        let pending: Vec<_> =
            targets
                .iter()
                .map(|target| {
                    let local = &local;
                    scope.spawn(move || match target {
                        Target::Local => local(),
                        Target::Remote(host) => ssh
                            .call::<StatusResponse>(host, &["status"])
                            .and_then(|r| match r.hosts.into_iter().next() {
                                Some(HostStatus {
                                    outcome: HostOutcome::Jobs { jobs },
                                    ..
                                }) => Ok(jobs.into_iter().map(JobReport::reassessed).collect()),
                                Some(HostStatus {
                                    outcome: HostOutcome::Failed { error },
                                    ..
                                }) => Err(error),
                                None => Err(Error::RemoteIncompatible),
                            }),
                    })
                })
                .collect();
        targets
            .iter()
            .zip(pending)
            .map(|(target, pending)| HostStatus {
                host: target.name().to_owned(),
                outcome: match pending.join().unwrap_or(Err(Error::RemoteUnavailable)) {
                    Ok(jobs) => HostOutcome::Jobs { jobs },
                    Err(error) => HostOutcome::Failed { error },
                },
            })
            .collect()
    });
    StatusResponse { hosts }
}
