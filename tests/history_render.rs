//! Golden tests for terminal history output. Set UPDATE_GOLDEN=1 to rewrite them.
use rhythm::{
    history_render::render,
    run_history::HistoryResponse,
    runs::{Outcome, Run, RunState, Timestamp},
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
    RunState::Finished {
        ended_at: Timestamp(NOW.0 - ended_ago),
        outcome,
    }
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
    golden("history", &render(&history, NOW, &chrono::Utc));
}

#[test]
fn explains_an_empty_history() {
    let history = HistoryResponse {
        label: "com.example.scan".into(),
        runs: vec![],
    };
    golden("history-empty", &render(&history, NOW, &chrono::Utc));
}
