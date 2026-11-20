//! Golden tests for terminal history output. Set UPDATE_GOLDEN=1 to rewrite them.
use rhythm::{
    history_render::{Skips, render},
    run_history::HistoryResponse,
    runs::{Outcome, ResultError, Run, RunReport, RunState, Summary, Timestamp},
};
use std::path::Path;

const NOW: Timestamp = Timestamp(1_790_000_000_000);

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(&path).unwrap(), "{name}");
}

fn run(id: i64, started_ago: i64, state: RunState) -> Run {
    Run {
        id,
        label: "com.example.scan".into(),
        started_at: Timestamp(NOW.0 - started_ago),
        state,
    }
}

fn finished(ended_ago: i64, outcome: Outcome) -> RunState {
    reported(ended_ago, outcome, RunReport::default())
}

fn reported(ended_ago: i64, outcome: Outcome, report: RunReport) -> RunState {
    RunState::Finished {
        ended_at: Timestamp(NOW.0 - ended_ago),
        outcome,
        report,
    }
}

/// A clean exit that reported a skip, started `started_ago` and taking 200ms.
fn skip(id: i64, started_ago: i64, summary: Option<&str>) -> Run {
    let report = RunReport {
        skipped: true,
        summary: summary.and_then(Summary::new),
        result_error: None,
    };
    run(
        id,
        started_ago,
        reported(started_ago - 200, Outcome::Code { code: 0 }, report),
    )
}

#[test]
fn renders_runs_newest_first_with_failures_marked() {
    let history = HistoryResponse {
        label: "com.example.scan".into(),
        runs: vec![
            run(104, 90_000, RunState::Running),
            run(
                103,
                86_400_000,
                finished(86_396_800, Outcome::Code { code: 1 }),
            ),
            run(
                102,
                2 * 86_400_000,
                finished(2 * 86_400_000 - 850, Outcome::Code { code: 0 }),
            ),
            run(9, 3 * 86_400_000, RunState::Lost),
            run(
                8,
                4 * 86_400_000,
                finished(4 * 86_400_000 - 7_500_000, Outcome::Signal { signal: 15 }),
            ),
            run(
                7,
                5 * 86_400_000,
                finished(5 * 86_400_000, Outcome::SpawnFailed),
            ),
        ],
    };
    golden(
        "history",
        &render(&history, NOW, &chrono::Utc, Skips::Collapse),
    );
}

#[test]
fn explains_an_empty_history() {
    let history = HistoryResponse {
        label: "com.example.scan".into(),
        runs: vec![],
    };
    golden(
        "history-empty",
        &render(&history, NOW, &chrono::Utc, Skips::Collapse),
    );
}

/// A job polling every 15 minutes: runs of skips around real runs.
fn polling() -> HistoryResponse {
    const MINUTE: i64 = 60_000;
    let summary = |text: &str| RunReport {
        summary: Summary::new(text),
        ..RunReport::default()
    };
    let mut runs = vec![];
    // The newest 12 skips, every 15 minutes since late this morning.
    for i in 0..12 {
        let summary = (i == 0).then_some("nothing new since the last check");
        runs.push(skip(140 - i, (4 + 15 * i) * MINUTE, summary));
    }
    runs.push(run(
        128,
        3 * 60 * MINUTE,
        reported(
            3 * 60 * MINUTE - 41_000,
            Outcome::Code { code: 0 },
            summary("synced 3 new items"),
        ),
    ));
    runs.push(run(
        127,
        3 * 60 * MINUTE + 15 * MINUTE,
        reported(
            3 * 60 * MINUTE + 15 * MINUTE - 900,
            Outcome::Code { code: 0 },
            RunReport {
                result_error: Some(ResultError::Invalid),
                ..RunReport::default()
            },
        ),
    ));
    // One skip alone between real runs, then skips since yesterday.
    runs.push(skip(126, 3 * 60 * MINUTE + 30 * MINUTE, None));
    runs.push(run(
        125,
        3 * 60 * MINUTE + 45 * MINUTE,
        reported(
            3 * 60 * MINUTE + 45 * MINUTE - 2_000,
            Outcome::Code { code: 1 },
            summary("the source answered 503"),
        ),
    ));
    runs.push(skip(124, 15 * 60 * MINUTE, Some("still nothing")));
    runs.push(skip(123, 15 * 60 * MINUTE + 15 * MINUTE, None));
    runs.push(skip(122, 24 * 60 * MINUTE, None));
    HistoryResponse {
        label: "com.example.poll".into(),
        runs,
    }
}

#[test]
fn collapses_consecutive_skips_and_shows_what_runs_reported() {
    golden(
        "history-skips",
        &render(&polling(), NOW, &chrono::Utc, Skips::Collapse),
    );
}

#[test]
fn lists_every_skip_when_asked() {
    golden(
        "history-skips-all",
        &render(&polling(), NOW, &chrono::Utc, Skips::List),
    );
}
