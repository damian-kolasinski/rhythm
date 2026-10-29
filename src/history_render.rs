//! Terminal rendering of a job's recorded runs.
use crate::{
    run_history::HistoryResponse,
    runs::{RunState, Timestamp},
    time_text,
};
use chrono::TimeZone;
use std::fmt::Write;

pub fn render<Tz: TimeZone>(history: &HistoryResponse, now: Timestamp, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if history.runs.is_empty() {
        return "No recorded runs. Jobs record runs when launchd starts them through `rhythm run`.\n"
            .into();
    }
    let rows: Vec<[String; 4]> = history
        .runs
        .iter()
        .map(|run| {
            let (ended, result) = match &run.state {
                RunState::Running => (now, "running".to_owned()),
                RunState::Finished { ended_at, outcome } => (*ended_at, outcome.to_string()),
                RunState::Lost => (run.started_at, "lost".to_owned()),
            };
            let duration = match run.state {
                RunState::Lost => "-".to_owned(),
                _ => time_text::duration(ended.0 - run.started_at.0),
            };
            [
                run.id.to_string(),
                time_text::local(run.started_at, zone),
                duration,
                result,
            ]
        })
        .collect();
    let header = ["RUN", "STARTED", "DURATION", "RESULT"].map(str::to_owned);
    let widths: Vec<usize> = (0..3)
        .map(|c| {
            std::iter::once(&header)
                .chain(&rows)
                .map(|r| r[c].chars().count())
                .max()
                .unwrap()
        })
        .collect();
    let mut out = String::new();
    for (i, row) in std::iter::once(&header).chain(&rows).enumerate() {
        let marker = match history.runs.get(i.wrapping_sub(1)) {
            Some(run) if run.state.failed() => "!",
            _ => " ",
        };
        writeln!(
            out,
            "{marker} {:>w0$}  {:w1$}  {:>w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
        )
        .unwrap();
    }
    out
}
