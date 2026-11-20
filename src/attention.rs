//! What needs the operator now, and what is new since a consumer last looked:
//! jobs that started or kept failing, recorded failures that have since
//! recovered and would otherwise go unseen, and problems jobs opened or
//! resolved.
use crate::{
    findings::{Change, Finding, Resolved},
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
    /// The job's failure was reported in an earlier look; the item is here
    /// for its findings.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failure_seen: bool,
    /// Every problem open now, when listing everything that needs attention.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::runs::lenient_list"
    )]
    pub findings: Vec<Finding>,
    /// Problems opened since the last look.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::runs::lenient_list"
    )]
    pub new_findings: Vec<Finding>,
    /// Problems open at the last look that no longer are.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::runs::lenient_list"
    )]
    pub resolved_findings: Vec<Resolved>,
}

/// Each job's open problems, by label, for jobs that have any.
pub type OpenFindings = BTreeMap<String, Vec<Finding>>;

fn open_of<'a>(open: &'a OpenFindings, report: &JobReport) -> &'a [Finding] {
    open.get(&report.label).map_or(&[], Vec::as_slice)
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
    /// The job was broken: invalid, failing, or down.
    pub attention: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u64>,
    /// The newest recorded run that had ended and was not skipped; a run in
    /// progress is not seen yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_finished: Option<i64>,
    /// The problems open then, with their titles so a resolved one can be named.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Resolved>,
}
impl Snapshot {
    /// What a look at `reports` and their `open` findings saw, carrying
    /// forward what `previous` saw of runs still in progress.
    pub fn after(previous: &Snapshot, reports: &[JobReport], open: &OpenFindings) -> Self {
        Self {
            jobs: reports
                .iter()
                .map(|r| {
                    let seen = Seen {
                        attention: r.health.broken(),
                        runs: runs(r),
                        last_finished: finished(r)
                            .or_else(|| previous.jobs.get(&r.label).and_then(|s| s.last_finished)),
                        findings: open_of(open, r).iter().map(Resolved::from).collect(),
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
        Observation::Unloaded | Observation::Unavailable => None,
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

/// Everything needing attention now, with each job's open problems.
pub fn current(reports: &[JobReport], open: &OpenFindings) -> Vec<Item> {
    reports
        .iter()
        .filter(|r| r.health.needs_attention())
        .map(|r| Item {
            findings: open_of(open, r).to_vec(),
            ..item(r, None)
        })
        .collect()
}

/// Only what changed since `previous`. A job is reported again when it fails
/// again, which a newer finished run shows, or launchd's run count for a job
/// without one. Skipped runs change neither. Problems are reported when they
/// open and when they are resolved, never again while they stay open.
pub fn since(
    previous: &Snapshot,
    reports: &[JobReport],
    open: &OpenFindings,
    failures_after: &dyn Fn(&JobReport, i64) -> Result<Vec<Run>>,
) -> Result<Vec<Item>> {
    let mut items = vec![];
    for report in reports {
        let seen = previous.jobs.get(&report.label);
        let seen_findings = seen.map_or(&[][..], |s| &s.findings[..]);
        let change = Change::since(seen_findings, |r| &r.key, open_of(open, report));
        let mut news = failure_news(report, seen, failures_after)?;
        if !change.is_empty() {
            let news = news.get_or_insert_with(|| Item {
                failure_seen: report.health.broken(),
                ..item(report, None)
            });
            news.new_findings = change.new.into_iter().cloned().collect();
            news.resolved_findings = change.resolved.into_iter().cloned().collect();
        }
        items.extend(news);
    }
    Ok(items)
}

/// A broken job's failure the consumer has not seen, or failures a working
/// job recovered from since its last look.
fn failure_news(
    report: &JobReport,
    seen: Option<&Seen>,
    failures_after: &dyn Fn(&JobReport, i64) -> Result<Vec<Run>>,
) -> Result<Option<Item>> {
    if report.health.broken() {
        let changed = match seen {
            None => true,
            // launchd counts skips as runs; recorded runs tell them apart,
            // except for failures the recorder could not record.
            Some(seen) => {
                !seen.attention
                    || match finished(report) {
                        Some(run) if !report.unrecorded_failure() => {
                            seen.last_finished != Some(run)
                        }
                        _ => seen.runs != runs(report),
                    }
            }
        };
        return Ok(changed.then(|| item(report, None)));
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
            return Ok(Some(item(report, Some(failures))));
        }
    }
    Ok(None)
}

/// One machine's look: everything needing attention, or only what is new since
/// `previous`, with the snapshot to save once the result has been delivered.
pub fn look(
    supervisor: &impl Supervisor,
    ledgers: &dyn Fn(&RunStore) -> Option<Ledger>,
    previous: Option<&Snapshot>,
) -> Result<(Vec<Item>, Option<Snapshot>)> {
    let reports = status::jobs(supervisor, ledgers)?;
    let mut open = OpenFindings::new();
    for report in reports.iter().filter(|r| r.open_findings > 0) {
        if let Some(stated) = report
            .run_store
            .as_ref()
            .and_then(ledgers)
            .map(|ledger| ledger.open_findings(&report.label))
            .transpose()?
            .flatten()
        {
            open.insert(report.label.clone(), stated.set.open);
        }
    }
    let Some(previous) = previous else {
        return Ok((current(&reports, &open), None));
    };
    let items = since(previous, &reports, &open, &|report, after| match report
        .run_store
        .as_ref()
        .and_then(ledgers)
    {
        Some(ledger) => ledger.failures_after(&report.label, after),
        None => Ok(vec![]),
    })?;
    Ok((items, Some(Snapshot::after(previous, &reports, &open))))
}

fn item(report: &JobReport, recovered_from: Option<Failures>) -> Item {
    Item {
        label: report.label.clone(),
        health: report.health,
        last_exit: match report.observation {
            Observation::Loaded { last_exit, .. } => last_exit,
            Observation::Unloaded | Observation::Unavailable => None,
        },
        last_run: report.last_run.clone(),
        recovered_from,
        failure_seen: false,
        findings: vec![],
        new_findings: vec![],
        resolved_findings: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        jobs::{Activation, Job, Observation},
        runs::{Outcome, RunReport, RunState, Timestamp},
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
                report: RunReport::default(),
            },
        });
        status_report(job, run)
    }

    fn status_report(job: Job, run: Option<Run>) -> JobReport {
        let json = serde_json::json!({
            "label": job.label,
            "health": crate::jobs::Health::assess(job.activation.as_ref(), &job.observation, None, 0),
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
        assert_eq!(labels(&current(&reports, &OpenFindings::new())), ["a"]);
    }

    #[test]
    fn reports_a_failure_once_until_it_happens_again() {
        let first = [report("scan", 5, 1, None), report("ok", 5, 0, None)];
        assert_eq!(
            labels(
                &since(
                    &Snapshot::default(),
                    &first,
                    &OpenFindings::new(),
                    &no_failures
                )
                .unwrap()
            ),
            ["scan"]
        );
        let seen = Snapshot::after(&Snapshot::default(), &first, &OpenFindings::new());
        assert!(
            since(&seen, &first, &OpenFindings::new(), &no_failures)
                .unwrap()
                .is_empty()
        );
        let failed_again = [report("scan", 6, 1, None), report("ok", 6, 0, None)];
        assert_eq!(
            labels(&since(&seen, &failed_again, &OpenFindings::new(), &no_failures).unwrap()),
            ["scan"]
        );
        let newly_failing = [report("scan", 5, 1, None), report("ok", 6, 2, None)];
        assert_eq!(
            labels(&since(&seen, &newly_failing, &OpenFindings::new(), &no_failures).unwrap()),
            ["ok"]
        );
    }

    #[test]
    fn reports_recorded_failures_that_recovered_unseen() {
        let seen = Snapshot::after(
            &Snapshot::default(),
            &[report("job", 3, 0, Some(10))],
            &OpenFindings::new(),
        );
        let now = [report("job", 5, 0, Some(12))];
        let failures = |r: &JobReport, after: i64| -> Result<Vec<Run>> {
            assert_eq!((r.label.as_str(), after), ("job", 10));
            let mut failed = r.last_run.clone().unwrap();
            failed.id = 11;
            failed.state = RunState::Lost;
            Ok(vec![failed])
        };
        let items = since(&seen, &now, &OpenFindings::new(), &failures).unwrap();
        assert_eq!(labels(&items), ["job"]);
        let recovered = items[0].recovered_from.as_ref().unwrap();
        assert_eq!((recovered.count, recovered.first.id), (1, 11));
        assert!(
            since(
                &Snapshot::after(&seen, &now, &OpenFindings::new()),
                &now,
                &OpenFindings::new(),
                &failures
            )
            .unwrap()
            .is_empty()
        );
    }

    fn finding(key: &str) -> Finding {
        serde_json::from_value(serde_json::json!({
            "key": key, "title": format!("{key} broke"), "first_seen": "2026-09-21T14:13:20.000Z"
        }))
        .unwrap()
    }
    /// A recorded job whose newest run exited `exit` with `keys` open.
    fn with_findings(label: &str, exit: i32, keys: &[&str]) -> (JobReport, OpenFindings) {
        let mut report = report(label, 1, exit, Some(1));
        report.open_findings = keys.len();
        report.health = Health::assess(
            report.activation.as_ref(),
            &report.observation,
            None,
            keys.len(),
        );
        let open = match keys {
            [] => OpenFindings::new(),
            keys => [(label.to_owned(), keys.iter().map(|k| finding(k)).collect())].into(),
        };
        (report, open)
    }
    fn keys<T>(list: &[T], key: impl Fn(&T) -> &str) -> Vec<&str> {
        list.iter().map(key).collect()
    }

    #[test]
    fn current_lists_flagged_jobs_with_their_open_findings() {
        let (flagged, open) = with_findings("check", 0, &["a", "b"]);
        assert_eq!(flagged.health, Health::Flagged);
        let items = current(&[flagged, report("ok", 1, 0, None)], &open);
        assert_eq!(labels(&items), ["check"]);
        assert_eq!(
            keys(&items[0].findings, |f| f.problem.key.as_str()),
            ["a", "b"]
        );
    }

    #[test]
    fn reports_problems_when_they_open_and_when_they_are_resolved() {
        let (first, open) = with_findings("check", 0, &["a", "b"]);
        let items = since(
            &Snapshot::default(),
            std::slice::from_ref(&first),
            &open,
            &no_failures,
        )
        .unwrap();
        assert_eq!(
            keys(&items[0].new_findings, |f| f.problem.key.as_str()),
            ["a", "b"]
        );
        assert!(items[0].resolved_findings.is_empty());
        let seen = Snapshot::after(&Snapshot::default(), std::slice::from_ref(&first), &open);
        // Reported again by later runs: nothing new.
        let (again, open_again) = with_findings("check", 0, &["b", "a"]);
        assert!(
            since(&seen, &[again], &open_again, &no_failures)
                .unwrap()
                .is_empty()
        );
        let (changed, open_changed) = with_findings("check", 0, &["b", "c"]);
        let items = since(
            &seen,
            std::slice::from_ref(&changed),
            &open_changed,
            &no_failures,
        )
        .unwrap();
        assert_eq!(
            keys(&items[0].new_findings, |f| f.problem.key.as_str()),
            ["c"]
        );
        assert_eq!(keys(&items[0].resolved_findings, |r| r.key.as_str()), ["a"]);
        assert_eq!(items[0].resolved_findings[0].title.as_str(), "a broke");
        let seen = Snapshot::after(&seen, &[changed], &open_changed);
        let (clear, none) = with_findings("check", 0, &[]);
        assert_eq!(clear.health, Health::Ok);
        let items = since(&seen, &[clear], &none, &no_failures).unwrap();
        assert_eq!(items[0].health, Health::Ok);
        assert_eq!(
            keys(&items[0].resolved_findings, |r| r.key.as_str()),
            ["b", "c"]
        );
    }

    #[test]
    fn a_seen_failure_is_not_repeated_for_its_findings() {
        let (failing, open) = with_findings("check", 1, &["a"]);
        let seen = Snapshot::after(&Snapshot::default(), &[failing], &OpenFindings::new());
        let items = since(
            &seen,
            &[with_findings("check", 1, &[]).0],
            &open,
            &no_failures,
        )
        .unwrap();
        assert!(items[0].failure_seen);
        assert_eq!(
            keys(&items[0].new_findings, |f| f.problem.key.as_str()),
            ["a"]
        );
        // An unseen failure is news of its own.
        let (failing, open) = with_findings("other", 1, &["a"]);
        let items = since(&Snapshot::default(), &[failing], &open, &no_failures).unwrap();
        assert!(!items[0].failure_seen);
    }

    #[test]
    fn a_snapshot_from_before_findings_sees_every_open_one_as_new() {
        let old: Snapshot = serde_json::from_value(serde_json::json!({
            "jobs": {"check": {"attention": false, "runs": 1, "last_finished": 1}}
        }))
        .unwrap();
        let (flagged, open) = with_findings("check", 0, &["a"]);
        let items = since(&old, &[flagged], &open, &no_failures).unwrap();
        assert_eq!(items[0].new_findings.len(), 1);
    }

    #[test]
    fn findings_a_newer_rhythm_sends_differently_are_left_out() {
        let item: Item = serde_json::from_value(serde_json::json!({
            "label": "check",
            "health": "flagged",
            "new_findings": [
                {"key": "a", "title": "a broke", "first_seen": "2026-09-21T14:13:20.000Z"},
                {"key": "bad\nkey", "title": "t", "first_seen": "2026-09-21T14:13:20.000Z"}
            ],
            "resolved_findings": "not a list"
        }))
        .unwrap();
        assert_eq!(keys(&item.new_findings, |f| f.problem.key.as_str()), ["a"]);
        assert!(item.resolved_findings.is_empty());
        let report: crate::runs::RunReport = serde_json::from_value(
            serde_json::json!({"skipped": true, "findings": {"open": "many"}}),
        )
        .unwrap();
        assert_eq!((report.skipped, report.findings), (true, None));
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
        runs::{Outcome, RunReport, Timestamp},
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
            "health": crate::jobs::Health::assess(job.activation.as_ref(), &job.observation, None, 0),
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
            report: RunReport::default(),
        }
    }

    #[test]
    fn a_look_during_a_run_does_not_skip_that_runs_failure() {
        let before = [report(1, (9, finished(0)))];
        let seen = Snapshot::after(&Snapshot::default(), &before, &OpenFindings::new());
        let during = [report(2, (10, RunState::Running))];
        assert!(
            since(&seen, &during, &OpenFindings::new(), &|_, _| Ok(vec![]))
                .unwrap()
                .is_empty()
        );
        let seen = Snapshot::after(&seen, &during, &OpenFindings::new());
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
        let items = since(&seen, &after, &OpenFindings::new(), &failures).unwrap();
        assert_eq!(items[0].recovered_from.as_ref().unwrap().first.id, 10);
    }

    #[test]
    fn a_job_still_running_after_a_failure_has_not_recovered() {
        let seen = Snapshot::after(
            &Snapshot::default(),
            &[report(1, (9, finished(0)))],
            &OpenFindings::new(),
        );
        let running = [report(3, (11, RunState::Running))];
        let failures = |_: &JobReport, _: i64| -> Result<Vec<Run>> {
            Ok(vec![Run {
                id: 10,
                label: "job".into(),
                started_at: Timestamp(0),
                state: finished(1),
            }])
        };
        assert!(
            since(&seen, &running, &OpenFindings::new(), &failures)
                .unwrap()
                .is_empty()
        );
    }
}
