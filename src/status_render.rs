//! Terminal rendering of a status response.
use crate::{
    jobs::{Health, Observation},
    runs::{RunState, Timestamp},
    status::{HostOutcome, HostStatus, JobReport},
    targets::Target,
    terminal_text::clean,
    time_text,
};
use std::fmt::Write;

pub fn render(hosts: &[HostStatus], now: Timestamp, color: bool) -> String {
    let paint = Paint(color);
    let titled = hosts.len() > 1 || hosts.iter().any(|h| h.host != Target::LOCAL);
    let mut out = String::new();
    for (i, host) in hosts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if titled {
            writeln!(out, "{}", paint.bold(&clean(&host.host))).unwrap();
        }
        match &host.outcome {
            HostOutcome::Failed { error } => {
                writeln!(out, "{}", paint.red(error.message())).unwrap()
            }
            HostOutcome::Jobs { jobs } if jobs.is_empty() => {
                out.push_str("No launchd jobs in LaunchAgents.\n")
            }
            HostOutcome::Jobs { jobs } => table(&mut out, jobs, now, paint),
        }
    }
    out
}

fn table(out: &mut String, jobs: &[JobReport], now: Timestamp, paint: Paint) {
    let rows: Vec<[String; 4]> = jobs
        .iter()
        .map(|job| {
            [
                clean(&job.label),
                job.activation
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), ToString::to_string),
                job.health.name().to_owned(),
                detail(job, now),
            ]
        })
        .collect();
    let header = ["JOB", "SCHEDULE", "STATE", "LAST"].map(str::to_owned);
    let widths: Vec<usize> = (0..3)
        .map(|c| {
            std::iter::once(&header)
                .chain(&rows)
                .map(|r| r[c].chars().count())
                .max()
                .unwrap()
        })
        .collect();
    let line = |out: &mut String, marker: &str, cells: &[String; 4], health: Option<Health>| {
        let state = format!("{:w$}", cells[2], w = widths[2]);
        let state = match health {
            Some(h) if h.needs_attention() => paint.red(&state),
            Some(Health::Running) => paint.green(&state),
            Some(Health::Unloaded) => paint.dim(&state),
            _ => state,
        };
        let text = format!(
            "{marker} {:w0$}  {:w1$}  {state}  {}",
            cells[0],
            cells[1],
            cells[3],
            w0 = widths[0],
            w1 = widths[1],
        );
        out.push_str(text.trim_end());
        out.push('\n');
    };
    line(out, " ", &header, None);
    for (job, row) in jobs.iter().zip(&rows) {
        let marker = if job.health.needs_attention() {
            paint.red("!")
        } else {
            " ".to_owned()
        };
        line(out, &marker, row, Some(job.health));
    }
    let attention = jobs.iter().filter(|j| j.health.needs_attention()).count();
    let noun = if jobs.len() == 1 { "job" } else { "jobs" };
    match attention {
        0 => writeln!(out, "\n{} {noun}, none need attention", jobs.len()),
        n => writeln!(
            out,
            "\n{} {noun}, {}",
            jobs.len(),
            paint.red(&format!(
                "{n} {} attention",
                if n == 1 { "needs" } else { "need" }
            ))
        ),
    }
    .unwrap();
}

/// Recorded runs say when; launchd alone only knows the last exit. The newest
/// real run comes first, then a skip that followed it.
fn detail(job: &JobReport, now: Timestamp) -> String {
    let recorded = job
        .last_run
        .as_ref()
        .map(|run| (run.started_at, &run.state));
    let real = match (&job.observation, job.health, recorded) {
        (_, Health::Invalid, _) => return "unreadable definition".into(),
        (Observation::Unavailable, _, _) => return "status unavailable".into(),
        (Observation::Unloaded, _, _) => return "not loaded".into(),
        (Observation::Loaded { pid: Some(pid), .. }, _, Some((started, RunState::Running))) => {
            return format!("pid {pid}, for {}", time_text::duration(now.0 - started.0));
        }
        (Observation::Loaded { pid: Some(pid), .. }, _, _) => return format!("pid {pid}"),
        (
            _,
            _,
            Some((
                _,
                RunState::Finished {
                    ended_at, outcome, ..
                },
            )),
        ) => Some(format!("{outcome}, {}", time_text::ago(*ended_at, now))),
        (_, _, Some((started, RunState::Lost))) => {
            Some(format!("lost, started {}", time_text::ago(started, now)))
        }
        _ => None,
    };
    let skipped = job.last_skip.as_ref().map(|run| {
        let ended = match run.state {
            RunState::Finished { ended_at, .. } => ended_at,
            _ => run.started_at,
        };
        format!("skipped {}", time_text::ago(ended, now))
    });
    match (real, skipped) {
        (Some(real), Some(skipped)) => format!("{real} · {skipped}"),
        (Some(text), None) | (None, Some(text)) => text,
        (None, None) => match job.observation {
            Observation::Loaded {
                last_exit: Some(exit),
                ..
            } => exit.to_string(),
            _ => "never exited".into(),
        },
    }
}

#[derive(Clone, Copy)]
struct Paint(bool);
impl Paint {
    fn wrap(self, code: &str, text: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
    fn red(self, text: &str) -> String {
        self.wrap("31", text)
    }
    fn green(self, text: &str) -> String {
        self.wrap("32", text)
    }
    fn dim(self, text: &str) -> String {
        self.wrap("2", text)
    }
    fn bold(self, text: &str) -> String {
        self.wrap("1", text)
    }
}
