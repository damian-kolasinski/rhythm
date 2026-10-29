//! What needs the operator now, and what is new since a consumer last looked:
//! jobs that started or kept failing, and recorded failures that have since
//! recovered and would otherwise go unseen.
use crate::{
    jobs::{Exit, Health, Observation, RunStore, Supervisor},
    ledger::Ledger,
    operation_error::{Error, Result},
    runs::{Run, RunState},
    status::{self, JobReport},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "attention")]
pub struct AttentionResponse {
    pub hosts: Vec<AttentionHost>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionHost {
    pub host: String,
    #[serde(flatten)]
    pub outcome: AttentionOutcome,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttentionOutcome {
    Items {
        items: Vec<Item>,
    },
    Failed {
        error: crate::operation_error::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub label: String,
    pub health: Health,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_exit: Option<Exit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<Run>,
    /// Failures since the last look of a job whose latest run succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_from: Option<Failures>,
}

/// Failed or lost runs, summarized so a long outage stays one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failures {
    pub count: u64,
    pub first: Run,
    pub last: Run,
}

/// What a consumer saw of each job when it last looked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub jobs: BTreeMap<String, Seen>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub attention: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u64>,
    /// The newest recorded run that had ended; a run in progress is not seen yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_finished: Option<i64>,
}
impl Snapshot {
    /// What a look at `reports` saw, carrying forward what `previous` saw of
    /// runs still in progress.
    pub fn after(previous: &Snapshot, reports: &[JobReport]) -> Self {
        Self {
            jobs: reports
                .iter()
                .map(|r| {
                    let seen = Seen {
                        attention: r.health.needs_attention(),
                        runs: runs(r),
                        last_finished: finished(r)
                            .or_else(|| previous.jobs.get(&r.label).and_then(|s| s.last_finished)),
                    };
                    (r.label.clone(), seen)
                })
                .collect(),
        }
    }
}

fn runs(report: &JobReport) -> Option<u64> {
    match report.observation {
        Observation::Loaded { runs, .. } => Some(runs),
        Observation::Unloaded => None,
    }
}

fn finished(report: &JobReport) -> Option<i64> {
    report
        .last_run
        .as_ref()
        .filter(|run| run.state != RunState::Running)
        .map(|run| run.id)
}

/// A named reader of attention, such as one agent tool's session hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer(String);
impl Consumer {
    pub fn parse(name: &str) -> Result<Self> {
        let valid = (1..=40).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        valid
            .then(|| Self(name.to_owned()))
            .ok_or(Error::InvalidRequest)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything needing attention now.
pub fn current(reports: &[JobReport]) -> Vec<Item> {
    reports
        .iter()
        .filter(|r| r.health.needs_attention())
        .map(|r| item(r, None))
        .collect()
}

/// Only what changed since `previous`. A job is reported again when it fails
/// again, which launchd's run count or a newer finished run shows.
pub fn since(
    previous: &Snapshot,
    reports: &[JobReport],
    failures_after: &dyn Fn(&JobReport, i64) -> Result<Vec<Run>>,
) -> Result<Vec<Item>> {
    let mut items = vec![];
    for report in reports {
        let seen = previous.jobs.get(&report.label);
        if report.health.needs_attention() {
            let changed = match seen {
                None => true,
                Some(seen) => {
                    !seen.attention
                        || seen.runs != runs(report)
                        || (finished(report).is_some() && seen.last_finished != finished(report))
                }
            };
            if changed {
                items.push(item(report, None));
            }
            continue;
        }
        let succeeded = report.last_run.as_ref().is_some_and(
            |run| matches!(&run.state, RunState::Finished { outcome, .. } if outcome.succeeded()),
        );
        if let Some(seen_run) = seen.and_then(|s| s.last_finished)
            && report.recorded
            && succeeded
            && finished(report) != Some(seen_run)
        {
            let failed = failures_after(report, seen_run)?;
            if let (Some(first), Some(last)) = (failed.first(), failed.last()) {
                let failures = Failures {
                    count: failed.len() as u64,
                    first: first.clone(),
                    last: last.clone(),
                };
                items.push(item(report, Some(failures)));
            }
        }
    }
    Ok(items)
}

/// One machine's look: everything needing attention, or only what is new since
/// `previous`, with the snapshot to save once the result has been delivered.
pub fn look(
    supervisor: &impl Supervisor,
    ledgers: &dyn Fn(&RunStore) -> Option<Ledger>,
    previous: Option<&Snapshot>,
) -> Result<(Vec<Item>, Option<Snapshot>)> {
    let reports = status::jobs(supervisor, ledgers)?;
    let Some(previous) = previous else {
        return Ok((current(&reports), None));
    };
    let items = since(previous, &reports, &|report, after| match report
        .run_store
        .as_ref()
        .and_then(ledgers)
    {
        Some(ledger) => ledger.failures_after(&report.label, after),
        None => Ok(vec![]),
    })?;
    Ok((items, Some(Snapshot::after(previous, &reports))))
}

fn item(report: &JobReport, recovered_from: Option<Failures>) -> Item {
    Item {
        label: report.label.clone(),
        health: report.health,
        last_exit: match report.observation {
            Observation::Loaded { last_exit, .. } => last_exit,
            Observation::Unloaded => None,
        },
        last_run: report.last_run.clone(),
        recovered_from,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        jobs::{Activation, Job, Observation},
        runs::{Outcome, RunState, Timestamp},
    };

    fn report(label: &str, runs: u64, exit: i32, last_run: Option<i64>) -> JobReport {
        let job = Job {
            label: label.into(),
            activation: Some(Activation::Triggered { triggers: vec![] }),
            recorded: last_run.map(|_| crate::jobs::RunStore::Default),
            observation: Observation::Loaded {
                pid: None,
                runs,
                last_exit: Some(Exit::Code { code: exit }),
            },
        };
        let run = last_run.map(|id| Run {
            id,
            label: label.into(),
            started_at: Timestamp(0),
            state: RunState::Finished {
                ended_at: Timestamp(1),
                outcome: Outcome::Code { code: exit },
            },
        });
        status_report(job, run)
    }

    fn status_report(job: Job, run: Option<Run>) -> JobReport {
        let json = serde_json::json!({
            "label": job.label,
            "health": crate::jobs::Health::assess(job.activation.as_ref(), &job.observation),
            "activation": job.activation,
            "observation": job.observation,
            "recorded": job.recorded.is_some(),
            "last_run": run,
        });
        serde_json::from_value(json).unwrap()
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn no_failures(_: &JobReport, _: i64) -> Result<Vec<Run>> {
        Ok(vec![])
    }

    #[test]
    fn current_lists_every_job_needing_attention() {
        let reports = [report("a", 1, 1, None), report("b", 1, 0, None)];
        assert_eq!(labels(&current(&reports)), ["a"]);
    }

    #[test]
    fn reports_a_failure_once_until_it_happens_again() {
        let first = [report("scan", 5, 1, None), report("ok", 5, 0, None)];
        assert_eq!(
            labels(&since(&Snapshot::default(), &first, &no_failures).unwrap()),
            ["scan"]
        );
        let seen = Snapshot::after(&Snapshot::default(), &first);
        assert!(since(&seen, &first, &no_failures).unwrap().is_empty());
        let failed_again = [report("scan", 6, 1, None), report("ok", 6, 0, None)];
        assert_eq!(
            labels(&since(&seen, &failed_again, &no_failures).unwrap()),
            ["scan"]
        );
        let newly_failing = [report("scan", 5, 1, None), report("ok", 6, 2, None)];
        assert_eq!(
            labels(&since(&seen, &newly_failing, &no_failures).unwrap()),
            ["ok"]
        );
    }

    #[test]
    fn reports_recorded_failures_that_recovered_unseen() {
        let seen = Snapshot::after(&Snapshot::default(), &[report("job", 3, 0, Some(10))]);
        let now = [report("job", 5, 0, Some(12))];
        let failures = |r: &JobReport, after: i64| -> Result<Vec<Run>> {
            assert_eq!((r.label.as_str(), after), ("job", 10));
            let mut failed = r.last_run.clone().unwrap();
            failed.id = 11;
            failed.state = RunState::Lost;
            Ok(vec![failed])
        };
        let items = since(&seen, &now, &failures).unwrap();
        assert_eq!(labels(&items), ["job"]);
        let recovered = items[0].recovered_from.as_ref().unwrap();
        assert_eq!((recovered.count, recovered.first.id), (1, 11));
        assert!(
            since(&Snapshot::after(&seen, &now), &now, &failures)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn consumers_are_simple_names() {
        assert!(Consumer::parse("claude-code").is_ok());
        for bad in ["", "Claude", "a/b", "../x", &"x".repeat(41)] {
            assert!(Consumer::parse(bad).is_err(), "{bad:?}");
        }
    }
}

#[cfg(test)]
mod in_progress {
    use super::*;
    use crate::{
        jobs::{Activation, Job, RunStore},
        runs::{Outcome, Timestamp},
    };

    fn report(runs: u64, last: (i64, RunState)) -> JobReport {
        let job = Job {
            label: "job".into(),
            activation: Some(Activation::Triggered { triggers: vec![] }),
            recorded: Some(RunStore::Default),
            observation: Observation::Loaded {
                pid: (last.1 == RunState::Running).then_some(7),
                runs,
                last_exit: Some(Exit::Code { code: 0 }),
            },
        };
        let json = serde_json::json!({
            "label": "job",
            "health": crate::jobs::Health::assess(job.activation.as_ref(), &job.observation),
            "activation": job.activation,
            "observation": job.observation,
            "recorded": true,
            "last_run": Run { id: last.0, label: "job".into(), started_at: Timestamp(0), state: last.1 },
        });
        serde_json::from_value(json).unwrap()
    }

    fn finished(code: i32) -> RunState {
        RunState::Finished {
            ended_at: Timestamp(1),
            outcome: Outcome::Code { code },
        }
    }

    #[test]
    fn a_look_during_a_run_does_not_skip_that_runs_failure() {
        let before = [report(1, (9, finished(0)))];
        let seen = Snapshot::after(&Snapshot::default(), &before);
        let during = [report(2, (10, RunState::Running))];
        assert!(
            since(&seen, &during, &|_, _| Ok(vec![]))
                .unwrap()
                .is_empty()
        );
        let seen = Snapshot::after(&seen, &during);
        assert_eq!(seen.jobs["job"].last_finished, Some(9));
        // Run 10 failed, run 11 succeeded: run 10 is still news.
        let after = [report(3, (11, finished(0)))];
        let failures = |_: &JobReport, after: i64| -> Result<Vec<Run>> {
            assert_eq!(after, 9);
            Ok(vec![Run {
                id: 10,
                label: "job".into(),
                started_at: Timestamp(0),
                state: finished(1),
            }])
        };
        let items = since(&seen, &after, &failures).unwrap();
        assert_eq!(items[0].recovered_from.as_ref().unwrap().first.id, 10);
    }

    #[test]
    fn a_job_still_running_after_a_failure_has_not_recovered() {
        let seen = Snapshot::after(&Snapshot::default(), &[report(1, (9, finished(0)))]);
        let running = [report(3, (11, RunState::Running))];
        let failures = |_: &JobReport, _: i64| -> Result<Vec<Run>> {
            Ok(vec![Run {
                id: 10,
                label: "job".into(),
                started_at: Timestamp(0),
                state: finished(1),
            }])
        };
        assert!(since(&seen, &running, &failures).unwrap().is_empty());
    }
}
