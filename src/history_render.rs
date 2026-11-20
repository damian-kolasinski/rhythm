//! Terminal rendering of a job's recorded runs.
use crate::{
    run_history::HistoryResponse,
    runs::{Run, RunReport, RunState, Timestamp},
    time_text,
};
use chrono::TimeZone;
use std::fmt::Write;

/// How skipped runs appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skips {
    /// Consecutive skipped runs share one row, so real runs stay in view.
    Collapse,
    /// Every run has its own row.
    List,
}

pub fn render<Tz: TimeZone>(
    history: &HistoryResponse,
    now: Timestamp,
    zone: &Tz,
    skips: Skips,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if history.runs.is_empty() {
        return "No recorded runs. Jobs record runs when launchd starts them through `rhythm run`.\n"
            .into();
    }
    let rows: Vec<(bool, [String; 4])> = groups(&history.runs, skips)
        .map(|group| (group[0].state.failed(), row(group, now, zone)))
        .collect();
    let header = (
        false,
        ["RUN", "STARTED", "DURATION", "RESULT"].map(str::to_owned),
    );
    let widths: Vec<usize> = (0..3)
        .map(|c| {
            std::iter::once(&header)
                .chain(&rows)
                .map(|(_, r)| r[c].chars().count())
                .max()
                .unwrap()
        })
        .collect();
    let mut out = String::new();
    for (failed, row) in std::iter::once(&header).chain(&rows) {
        let marker = if *failed { "!" } else { " " };
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

/// Runs newest first, each alone except consecutive skips when collapsing.
fn groups(runs: &[Run], skips: Skips) -> impl Iterator<Item = &[Run]> {
    runs.chunk_by(move |a, b| skips == Skips::Collapse && a.state.skipped() && b.state.skipped())
}

/// A row for one run, or for consecutive skips described by the newest.
fn row<Tz: TimeZone>(group: &[Run], now: Timestamp, zone: &Tz) -> [String; 4]
where
    Tz::Offset: std::fmt::Display,
{
    let run = &group[0];
    let (ended, result) = match &run.state {
        RunState::Running => (now, "running".to_owned()),
        RunState::Finished {
            ended_at,
            outcome,
            report,
        } => {
            let mut result = match group {
                [_] if run.state.skipped() => "skipped".to_owned(),
                [.., oldest] if run.state.skipped() => format!(
                    "skipped ×{} since {}",
                    group.len(),
                    time_text::clock(oldest.started_at, now, zone)
                ),
                _ => outcome.to_string(),
            };
            describe(&mut result, report);
            (*ended_at, result)
        }
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
}

/// What the run said about itself, after how it ended.
fn describe(result: &mut String, report: &RunReport) {
    if let Some(error) = report.result_error {
        write!(result, ", result file {error}").unwrap();
    }
    if let Some(summary) = &report.summary {
        write!(result, ": {summary}").unwrap();
    }
}
