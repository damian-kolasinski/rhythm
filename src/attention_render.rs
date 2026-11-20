//! Terminal rendering of attention: short lines an agent can act on.
use crate::{
    attention::{AttentionOutcome, AttentionResponse, Item},
    authoring::OWNED_PREFIX,
    jobs::Health,
    runs::{RunState, Timestamp},
    targets::Target,
    terminal_text::clean,
    time_text,
};
use std::fmt::Write;

/// Nothing at all when nothing needs attention, so session hooks stay silent.
pub fn render(response: &AttentionResponse, now: Timestamp) -> String {
    let mut lines = vec![];
    for host in &response.hosts {
        let prefix = if host.host == Target::LOCAL {
            String::new()
        } else {
            format!("{}: ", clean(&host.host))
        };
        match &host.outcome {
            AttentionOutcome::Failed { error } => {
                lines.push(format!("? {prefix}could not check: {}", error.message()))
            }
            AttentionOutcome::Items { items } => {
                for item in items {
                    for (marker, line) in item_lines(item, &host.host, now) {
                        lines.push(format!("{marker} {prefix}{line}"));
                    }
                }
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from("Background jobs need attention:\n");
    for line in lines {
        writeln!(out, "{line}").unwrap();
    }
    out
}

/// One line for the job's failure or recovery, and one for each kind of
/// change in its problems: `!` for what needs the operator, `~` for what
/// got better.
fn item_lines(item: &Item, host: &str, now: Timestamp) -> Vec<(&'static str, String)> {
    let label = clean(&item.label);
    let short = label.strip_prefix(OWNED_PREFIX).unwrap_or(&label);
    // Hints name the machine, so they work as written from here.
    let on = if host == Target::LOCAL {
        String::new()
    } else {
        format!(" --host {}", clean(host))
    };
    let see = format!("See `rhythm findings {short}{on}`.");
    let mut lines = vec![];
    if !item.new_findings.is_empty() {
        lines.push((
            "!",
            format!(
                "{label} found {}: {}. {see}",
                problems(item.new_findings.len(), "new "),
                titles(item.new_findings.iter().map(|f| f.problem.title.as_str())),
            ),
        ));
    }
    if !item.resolved_findings.is_empty() {
        lines.push((
            "~",
            format!(
                "{label} resolved {} since last check: {}.",
                problems(item.resolved_findings.len(), ""),
                titles(item.resolved_findings.iter().map(|r| r.title.as_str())),
            ),
        ));
    }
    if !item.findings.is_empty() {
        lines.push((
            "!",
            format!(
                "{label} has {}: {}. {see}",
                problems(item.findings.len(), "open "),
                titles(item.findings.iter().map(|f| f.problem.title.as_str())),
            ),
        ));
    }
    // An item from an older Rhythm, or a failure not yet reported, says why
    // the job itself needs attention.
    let failure = item.recovered_from.is_some()
        || (item.health.broken() && !item.failure_seen)
        || lines.is_empty();
    if failure {
        let marker = if item.recovered_from.is_none() && item.health.needs_attention() {
            "!"
        } else {
            "~"
        };
        lines.insert(0, (marker, failure_line(item, &label, short, &on, now)));
    }
    lines
}

/// `1 new problem`, `2 new problems`.
fn problems(count: usize, adjective: &str) -> String {
    format!(
        "{count} {adjective}problem{}",
        if count == 1 { "" } else { "s" }
    )
}

/// The first title, and how many more there are.
fn titles<'a>(mut titles: impl ExactSizeIterator<Item = &'a str>) -> String {
    let more = titles.len().saturating_sub(1);
    let first = titles.next().unwrap_or_default();
    match more {
        0 => first.to_owned(),
        more => format!("{first}, +{more} more"),
    }
}

fn failure_line(item: &Item, label: &str, short: &str, on: &str, now: Timestamp) -> String {
    if let Some(failures) = &item.recovered_from {
        let count = failures.count;
        let runs = if failures.first.id == failures.last.id {
            format!("run {}", failures.first.id)
        } else {
            format!("runs {}–{}", failures.first.id, failures.last.id)
        };
        return format!(
            "{label} recovered after {count} failed run{} since last check ({runs}). See `rhythm history {short}{on}`.",
            if count == 1 { "" } else { "s" },
        );
    }
    // Flagged without its problems in hand, as when they changed meanwhile
    // or came from a newer Rhythm in a shape this one does not read.
    if item.health == Health::Flagged {
        return format!("{label} reports open problems. See `rhythm findings {short}{on}`.");
    }
    let what = match (item.health, &item.last_run) {
        (Health::Invalid, _) => "has a definition launchd cannot load".to_owned(),
        (Health::Down, _) => "is a keep-alive job that is not running".to_owned(),
        // Recorded detail when it shows the failure; launchd's own exit otherwise.
        (_, Some(run)) if run.state.failed() => match &run.state {
            RunState::Finished {
                ended_at, outcome, ..
            } => {
                format!("failed: {outcome}, {}", time_text::ago(*ended_at, now))
            }
            RunState::Lost => format!(
                "lost its run started {}",
                time_text::ago(run.started_at, now)
            ),
            RunState::Running => "is failing".to_owned(),
        },
        _ => match item.last_exit {
            Some(exit) => format!("failed: {exit}"),
            None => "is failing".to_owned(),
        },
    };
    let next = if item.last_run.as_ref().is_some_and(|r| r.state.failed()) {
        format!(" See `rhythm logs {short}{on}`.")
    } else if item.last_run.is_some() {
        format!(" See `rhythm history {short}{on}`.")
    } else if item.health == Health::Failing {
        if on.is_empty() {
            format!(" Record its runs with `rhythm adopt {label}`.")
        } else {
            format!(" Record its runs by running `rhythm adopt {label}` there.")
        }
    } else {
        String::new()
    };
    format!("{label} {what}.{next}")
}
