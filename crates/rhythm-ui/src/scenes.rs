//! Deterministic scenarios for snapshots and the demonstration.
use rhythm::{
    jobs::{Activation, Calendar, Exit, Health, Observation, Trigger},
    routines::{CronEntry, CronTable, LaunchdSource, Routine, Snapshot, Source},
    runs::{Outcome, Run, RunReport, RunState, Timestamp},
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
    empty(window);
    let calendar = |hour, weekday| {
        Trigger::Calendar(Calendar {
            hour: Some(hour),
            minute: Some(0),
            weekday,
            ..Calendar::default()
        })
    };
    let entries = [
        (
            "com.example.sync-catalog",
            LaunchdSource::Daemon,
            Health::Failing,
            vec![Trigger::Interval { seconds: 3600 }],
            Some(1),
            0,
        ),
        (
            "com.example.index-samples",
            LaunchdSource::UserAgent,
            Health::Running,
            vec![Trigger::Interval { seconds: 900 }],
            None,
            120_000,
        ),
        (
            "dev.rhythm.archive-samples",
            LaunchdSource::UserAgent,
            Health::Ok,
            vec![calendar(8, None)],
            Some(0),
            840_000,
        ),
    ];
    let routines = entries
        .into_iter()
        .enumerate()
        .map(
            |(index, (label, source, health, triggers, exit, elapsed))| {
                let ended_at = Timestamp(NOW.0 - elapsed);
                let pid = (health == Health::Running).then_some(1);
                let last_run = (elapsed > 0).then(|| Run {
                    id: index as i64,
                    label: label.into(),
                    started_at: Timestamp(ended_at.0 - if exit.is_some() { 12_000 } else { 0 }),
                    state: exit.map_or(RunState::Running, |code| RunState::Finished {
                        ended_at,
                        outcome: Outcome::Code { code },
                        report: RunReport::default(),
                    }),
                });
                let report = JobReport {
                    label: label.into(),
                    health,
                    activation: Some(Activation::Triggered { triggers }),
                    observation: Observation::Loaded {
                        pid,
                        runs: 1,
                        last_exit: exit.map(|code| Exit::Code { code }),
                    },
                    recorded: source == LaunchdSource::UserAgent,
                    last_run,
                    last_skip: None,
                    verdict: None,
                    run_store: None,
                };
                Routine::Launchd {
                    source,
                    instance: 0,
                    report,
                    additional_timers: Vec::new(),
                }
            },
        )
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
