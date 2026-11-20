//! Report every job on the requested machines, most urgent first.
use crate::{
    jobs::{Activation, Health, Job, Observation, RunStore, Supervisor},
    ledger::Ledger,
    operation_error::{Error, Result},
    remote::{self, Ssh},
    runs::{RecentRuns, Run, RunState, Verdict},
    targets::{self, Target},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

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
    /// The newest recorded run that was not skipped, for recorded jobs that have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<Run>,
    /// The newest skipped run, when one followed `last_run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_skip: Option<Run>,
    /// What the newest recorded run that was neither skipped nor stopped says
    /// about the job; health follows it rather than launchd's last exit.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::runs::lenient"
    )]
    pub verdict: Option<Verdict>,
    /// Where a local recorded job's runs are kept; never sent to other machines.
    #[serde(skip)]
    pub run_store: Option<RunStore>,
}
impl JobReport {
    fn new(job: Job, runs: RecentRuns) -> Self {
        Self {
            health: Health::assess(
                job.activation.as_ref(),
                &job.observation,
                runs.verdict.filter(|_| {
                    !unrecorded_failure(&job.observation, &runs.last_run, &runs.last_skip)
                }),
            ),
            label: job.label,
            activation: job.activation,
            observation: job.observation,
            recorded: job.recorded.is_some(),
            run_store: job.recorded,
            last_run: runs.last_run,
            last_skip: runs.last_skip,
            verdict: runs.verdict,
        }
    }
    /// launchd saw the job fail after its newest recorded run exited 0: a run
    /// the ledger missed, as when the recorder could not record and ran the
    /// command alone. Health then follows launchd, whatever `verdict` says.
    pub fn unrecorded_failure(&self) -> bool {
        unrecorded_failure(&self.observation, &self.last_run, &self.last_skip)
    }
    /// A report from another machine is data: health follows from its facts.
    /// Only a recorded job has recorded runs to judge it by.
    fn reassessed(self) -> Self {
        let job = Job {
            label: self.label,
            activation: self.activation,
            recorded: self.recorded.then_some(RunStore::Default),
            observation: self.observation,
        };
        let runs = RecentRuns {
            last_run: self.last_run,
            last_skip: self.last_skip,
            verdict: self.verdict.filter(|_| self.recorded),
        };
        Self {
            run_store: None,
            ..Self::new(job, runs)
        }
    }
}

/// See [`JobReport::unrecorded_failure`].
fn unrecorded_failure(
    observation: &Observation,
    last_run: &Option<Run>,
    last_skip: &Option<Run>,
) -> bool {
    let launchd_failed = matches!(
        observation,
        Observation::Loaded { last_exit: Some(exit), .. } if !exit.succeeded()
    );
    let newest = last_skip.as_ref().or(last_run.as_ref());
    launchd_failed
        && newest.is_some_and(
            |run| matches!(&run.state, RunState::Finished { outcome, .. } if outcome.succeeded()),
        )
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
    reports(supervisor.jobs()?, ledgers)
}

pub fn local_ledgers(data_dir: Option<PathBuf>) -> impl Fn(&RunStore) -> Option<Ledger> {
    move |store| {
        let dir = match store {
            RunStore::Default => data_dir.clone(),
            RunStore::Directory(dir) => Some(dir.clone()),
        };
        let root = crate::state_directory::existing(dir).ok().flatten()?;
        Ledger::open_existing(root).ok().flatten()
    }
}

fn reports(
    jobs: Vec<Job>,
    ledgers: &dyn Fn(&RunStore) -> Option<Ledger>,
) -> Result<Vec<JobReport>> {
    let mut reader = ReportReader::new(ledgers);
    let mut reports = jobs
        .into_iter()
        .map(|job| reader.read(job))
        .collect::<Result<Vec<_>>>()?;
    reports.sort_by(|a, b| (a.health, &a.label).cmp(&(b.health, &b.label)));
    Ok(reports)
}

pub(crate) struct ReportReader<'a> {
    opened: HashMap<RunStore, Option<Ledger>>,
    ledgers: &'a dyn Fn(&RunStore) -> Option<Ledger>,
}

impl<'a> ReportReader<'a> {
    pub(crate) fn new(ledgers: &'a dyn Fn(&RunStore) -> Option<Ledger>) -> Self {
        Self {
            opened: HashMap::new(),
            ledgers,
        }
    }

    pub(crate) fn read(&mut self, job: Job) -> Result<JobReport> {
        let runs = match &job.recorded {
            Some(store) => {
                let ledger = self
                    .opened
                    .entry(store.clone())
                    .or_insert_with(|| (self.ledgers)(store));
                match ledger {
                    Some(ledger) => ledger.recent(&job.label)?,
                    None => RecentRuns::default(),
                }
            }
            None => RecentRuns::default(),
        };
        Ok(JobReport::new(job, runs))
    }
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
