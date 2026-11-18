//! Deterministic scenarios for snapshots and the demonstration.
use rhythm::{
    jobs::{Activation, Calendar, Exit, Health, Observation, Trigger},
    runs::{Outcome, Run, RunState, Timestamp},
    status::JobReport,
};

use crate::app::Window;

pub const SCENES: &[(&str, fn(&mut Window))] = &[
    ("loading", loading),
    ("empty", empty),
    ("routines", routines),
    ("stale", stale),
    ("unavailable", unavailable),
];

const NOW: Timestamp = Timestamp(2_000_000_000_000);

fn loading(window: &mut Window) {
    window.sample = true;
    window.now = NOW;
}

fn empty(window: &mut Window) {
    loading(window);
    window.freshness = Some(NOW);
    window.snapshot = Some(Vec::new());
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
            "dev.rhythm.archive-samples",
            Health::Failing,
            vec![calendar(8, None)],
            Some(1),
            840_000,
        ),
        (
            "com.example.index-samples",
            Health::Running,
            vec![Trigger::Interval { seconds: 900 }],
            None,
            120_000,
        ),
        (
            "dev.rhythm.build-summary",
            Health::Ok,
            vec![calendar(9, Some(1))],
            Some(0),
            172_800_000,
        ),
        (
            "dev.rhythm.clean-samples",
            Health::Idle,
            Vec::new(),
            None,
            0,
        ),
    ];
    window.snapshot = Some(
        entries
            .into_iter()
            .enumerate()
            .map(|(index, (label, health, triggers, exit, elapsed))| {
                let ended_at = Timestamp(NOW.0 - elapsed);
                let pid = (health == Health::Running).then_some(1);
                let last_run = (elapsed > 0).then(|| Run {
                    id: index as i64,
                    label: label.into(),
                    started_at: Timestamp(ended_at.0 - if exit.is_some() { 12_000 } else { 0 }),
                    state: exit.map_or(RunState::Running, |code| RunState::Finished {
                        ended_at,
                        outcome: Outcome::Code { code },
                    }),
                });
                JobReport {
                    label: label.into(),
                    health,
                    activation: Some(Activation::Triggered { triggers }),
                    observation: Observation::Loaded {
                        pid,
                        runs: u64::from(last_run.is_some()),
                        last_exit: exit.map(|code| Exit::Code { code }),
                    },
                    recorded: true,
                    last_run,
                    run_store: None,
                }
                .into()
            })
            .collect(),
    );
}
