//! Deterministic scenarios for snapshots and the demonstration.
use rhythm::{
    findings::{Finding, FindingKey, OpenSet, Problem, Said, Stated},
    jobs::{Activation, Calendar, Exit, Health, Observation, Trigger},
    routines::{CronEntry, CronTable, LaunchdSource, Routine, Snapshot, Source},
    runs::{Outcome, ResultError, Run, RunReport, RunState, Timestamp},
    status::JobReport,
};

use crate::{app::Window, routine::Inventory, updates::Offer};

type Scene = (&'static str, fn(&mut Window));

pub const SCENES: &[Scene] = &[
    ("loading", loading),
    ("empty", empty),
    ("routines", routines),
    ("partial", partial),
    ("stale", stale),
    ("unavailable", unavailable),
    ("release-available", release_available),
    ("release-offline", release_offline),
    ("release-failed", release_failed),
    ("flagged", flagged),
    ("skipped", skipped),
    ("problems", problems),
    ("problems-scroll", problems_scroll),
    ("problems-unreadable", problems_unreadable),
];

const NOW: Timestamp = Timestamp(2_000_000_000_000);

fn loading(window: &mut Window) {
    window.sample = true;
    window.now = NOW;
}

fn empty(window: &mut Window) {
    loading(window);
    window.freshness = Some(NOW);
    window.snapshot = Some(Inventory::default());
}

fn partial(window: &mut Window) {
    routines(window);
    window
        .snapshot
        .as_mut()
        .unwrap()
        .incomplete
        .insert(Source::SystemCrontabs);
}

fn stale(window: &mut Window) {
    routines(window);
    window.failed = true;
    window.freshness = Some(Timestamp(NOW.0 - 180_000));
}

fn unavailable(window: &mut Window) {
    loading(window);
    window.failed = true;
}

fn release_available(window: &mut Window) {
    release(window, false);
}

fn release_offline(window: &mut Window) {
    release(window, true);
}

fn release_failed(window: &mut Window) {
    release(window, false);
    window.updates.error = Some(rhythm::operation_error::Error::WindowUnavailable);
}

fn release(window: &mut Window, offline: bool) {
    routines(window);
    window.updates.sample = true;
    window.updates.offer = Some(Offer::Release {
        version: semver::Version::new(0, 3, 1),
        offline,
    });
}

fn routines(window: &mut Window) {
    show(window, |_| {});
}

/// A recorded job, among the others, with open problems and skipped runs
/// after its last real run.
fn flagged(window: &mut Window) {
    show(window, |jobs| {
        jobs.insert(
            1,
            Job {
                skipped: Some(240_000),
                findings: two_problems(),
                ..Job::new(
                    "dev.rhythm.check-links",
                    Health::Flagged,
                    Trigger::Interval { seconds: 900 },
                    Some(0),
                    10_800_000,
                )
            },
        );
    });
}

/// Recorded jobs whose newest runs had nothing to do.
fn skipped(window: &mut Window) {
    show(window, |jobs| {
        let poll = Job {
            skipped: Some(240_000),
            took: 41_000,
            ..Job::new(
                "dev.rhythm.poll-feed",
                Health::Ok,
                Trigger::Interval { seconds: 900 },
                Some(0),
                10_800_000,
            )
        };
        let watch = Job {
            skipped: Some(60_000),
            ..Job::new(
                "dev.rhythm.watch-inbox",
                Health::Ok,
                Trigger::Interval { seconds: 300 },
                Some(0),
                0,
            )
        };
        jobs.splice(2..2, [poll, watch]);
    });
}

fn problems(window: &mut Window) {
    flagged(window);
    window.problems = Some(FLAGGED.into());
}

fn problems_scroll(window: &mut Window) {
    flagged(window);
    let row = window
        .snapshot
        .as_mut()
        .unwrap()
        .rows
        .iter_mut()
        .find(|row| row.key == FLAGGED)
        .unwrap();
    // A job that listed more than the 100 Rhythm keeps.
    row.unlisted = 3;
    row.problems = (1..=100)
        .map(|n| {
            let mut finding = finding(
                &format!("broken-link/docs/page-{n}"),
                &format!("docs/page-{n} links to a removed page"),
                n * 3_600_000,
            );
            let problem = &mut finding.problem;
            if n % 3 == 1 {
                problem.detail = Some(format!("The link to /guides/step-{n} answers 404.").into());
            }
            if n == 2 {
                problem.link = Some(String::from("example-wiki/runbooks/broken-links").into());
                problem.handled_by = Some(String::from("the docs team").into());
            }
            finding
        })
        .collect();
    window.problems = Some(FLAGGED.into());
}

/// Rhythm's own problem for a result it could not read.
fn problems_unreadable(window: &mut Window) {
    show(window, |jobs| {
        let open = OpenSet::after(
            None,
            Said::Unreadable(ResultError::Invalid),
            Timestamp(NOW.0 - 1_500_000),
        )
        .unwrap();
        jobs.insert(
            1,
            Job {
                findings: open.open,
                ..Job::new(
                    "dev.rhythm.sync-notes",
                    Health::Flagged,
                    Trigger::Interval { seconds: 1800 },
                    Some(0),
                    1_500_000,
                )
            },
        );
    });
    window.problems = Some("UserAgent/0/dev.rhythm.sync-notes".into());
}

const FLAGGED: &str = "UserAgent/0/dev.rhythm.check-links";

fn two_problems() -> Vec<Finding> {
    let mut setup = finding(
        "broken-link/docs/setup",
        "docs/setup links to a removed page",
        187_200_000,
    );
    setup.problem.detail = Some(
        String::from("The install guide moved to /guides/install.\nThe old address answers 404.")
            .into(),
    );
    setup.problem.link = Some(String::from("https://example.com/guides/install").into());
    setup.problem.handled_by = Some(String::from("the docs team").into());
    vec![
        setup,
        finding(
            "slow-link/blog/archive",
            "blog/archive takes 14s to load",
            10_800_000,
        ),
    ]
}

/// A problem a job states, open since `open_for` before now.
fn finding(key: &str, title: &str, open_for: i64) -> Finding {
    Finding {
        problem: Problem {
            key: FindingKey::parse(key).unwrap(),
            title: title.to_owned().into(),
            detail: None,
            link: None,
            handled_by: None,
        },
        first_seen: Timestamp(NOW.0 - open_for),
    }
}

/// A recorded or external launchd job as the inventory reports it.
struct Job {
    label: &'static str,
    source: LaunchdSource,
    health: Health,
    trigger: Trigger,
    /// How the newest real run exited, or `None` while it runs.
    exit: Option<i32>,
    /// How long ago the newest real run ended; 0 when there is none.
    ended: i64,
    took: i64,
    /// How long ago the newest skipped run after it ended.
    skipped: Option<i64>,
    findings: Vec<Finding>,
}

impl Job {
    fn new(
        label: &'static str,
        health: Health,
        trigger: Trigger,
        exit: Option<i32>,
        ended: i64,
    ) -> Self {
        Self {
            label,
            source: LaunchdSource::UserAgent,
            health,
            trigger,
            exit,
            ended,
            took: if exit.is_some() { 12_000 } else { 0 },
            skipped: None,
            findings: Vec::new(),
        }
    }

    fn routine(self, id: i64) -> Routine {
        let run = |ended: i64, took: i64, exit: Option<i32>, skipped: bool| {
            let ended_at = Timestamp(NOW.0 - ended);
            Run {
                id,
                label: self.label.into(),
                started_at: Timestamp(ended_at.0 - took),
                state: exit.map_or(RunState::Running, |code| RunState::Finished {
                    ended_at,
                    outcome: Outcome::Code { code },
                    report: RunReport {
                        skipped,
                        ..RunReport::default()
                    },
                }),
            }
        };
        let report = JobReport {
            label: self.label.into(),
            health: self.health,
            activation: Some(Activation::Triggered {
                triggers: vec![self.trigger],
            }),
            observation: Observation::Loaded {
                pid: (self.health == Health::Running).then_some(1),
                runs: 1,
                last_exit: self.exit.map(|code| Exit::Code { code }),
            },
            recorded: self.source == LaunchdSource::UserAgent,
            last_run: (self.ended > 0).then(|| run(self.ended, self.took, self.exit, false)),
            last_skip: self.skipped.map(|ended| run(ended, 200, Some(0), true)),
            verdict: None,
            open_findings: self.findings.len(),
            run_store: None,
        };
        let stated = (!self.findings.is_empty()).then(|| Stated {
            run: id,
            at: NOW,
            set: OpenSet {
                open: self.findings,
                new: Vec::new(),
                resolved: 0,
                dropped: 0,
            },
        });
        Routine::Launchd {
            source: self.source,
            instance: 0,
            report,
            additional_timers: Vec::new(),
            findings: stated,
        }
    }
}

/// The everyday routines, changed by `change`, then a crontab entry.
fn show(window: &mut Window, change: impl FnOnce(&mut Vec<Job>)) {
    empty(window);
    let mut jobs = vec![
        Job {
            source: LaunchdSource::Daemon,
            took: 0,
            ..Job::new(
                "com.example.sync-catalog",
                Health::Failing,
                Trigger::Interval { seconds: 3600 },
                Some(1),
                0,
            )
        },
        Job::new(
            "com.example.index-samples",
            Health::Running,
            Trigger::Interval { seconds: 900 },
            None,
            120_000,
        ),
        Job::new(
            "dev.rhythm.archive-samples",
            Health::Ok,
            Trigger::Calendar(Calendar {
                hour: Some(8),
                minute: Some(0),
                ..Calendar::default()
            }),
            Some(0),
            840_000,
        ),
    ];
    change(&mut jobs);
    let routines = jobs
        .into_iter()
        .zip(0..)
        .map(|(job, id)| job.routine(id))
        .chain(std::iter::once(Routine::Cron(CronEntry {
            table: CronTable::User,
            line: 4,
            schedule: "Daily 03:15".into(),
        })))
        .collect();
    window.snapshot = Some(
        Snapshot {
            routines,
            incomplete: Default::default(),
        }
        .into(),
    );
}
