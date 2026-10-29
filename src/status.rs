//! Report every job on the requested machines, most urgent first.
use crate::{
    jobs::{Activation, Health, Job, Observation, Supervisor},
    operation_error::{Error, Result},
    remote::{self, Ssh},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobReport {
    pub label: String,
    pub health: Health,
    /// Absent when the definition cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<Activation>,
    pub observation: Observation,
}
impl From<Job> for JobReport {
    fn from(job: Job) -> Self {
        Self {
            health: Health::assess(job.activation.as_ref(), &job.observation),
            label: job.label,
            activation: job.activation,
            observation: job.observation,
        }
    }
}
impl JobReport {
    /// A report from another machine is data: health follows from its facts.
    fn reassessed(self) -> Self {
        Job {
            label: self.label,
            activation: self.activation,
            observation: self.observation,
        }
        .into()
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

pub fn jobs(supervisor: &impl Supervisor) -> Result<Vec<JobReport>> {
    let mut reports: Vec<JobReport> = supervisor.jobs()?.into_iter().map(Into::into).collect();
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
